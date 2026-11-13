//! Offscreen targets for flattened group opacity and layer blur (see
//! [`crate::layout::opacity_layer`]) and the pipelines that composite them.
//!
//! A blurred layer takes two or three more textures from the same pool for the
//! passes between painting it and compositing it ([`BlurSlots`]). A backdrop
//! blur (see [`crate::layout::backdrop`]) takes those and one for the copy of
//! its target ([`BackdropSlots`]).
//!
//! Layer textures are pooled across frames: a frame takes the smallest free
//! texture that fits each layer, creating one (rounded up to
//! [`LAYER_SIZE_STEP`]) only when none does, and textures unused for
//! [`LAYER_IDLE_FRAMES`] frames are released. Nothing is created until a
//! frame has a layer.
//!
//! The finished blur of a backdrop is kept out of the pool for as long as
//! the pixels it was blurred from stay the same (see
//! [`crate::layout::backdrop_reuse`]): a frame that finds it kept puts it
//! back with two draws in the target's pass, and takes no other texture for
//! that blur.

use super::vertex::QuadVertex;
use crate::layout::{
  backdrop::BackdropRegion,
  backdrop_reuse::{BackdropCache, BackdropInputs, BackdropUse},
  opacity_layer::{BlurPlan, LayerBlur, LayerBlurPass, LayerCmd, LayerComposite, TargetSpace},
};

/// Layer textures grow in steps of this many pixels, so a layer that changes
/// size a little (an animation) keeps its texture.
const LAYER_SIZE_STEP: u32 = 64;
/// Frames a pooled layer texture may stay unused before it is released.
const LAYER_IDLE_FRAMES: u64 = 120;

pub(super) struct LayerCompositor {
  pipeline: wgpu::RenderPipeline,
  bind_group_layout: wgpu::BindGroupLayout,
  blur_pipeline: wgpu::RenderPipeline,
  /// The two draws that put a blurred backdrop back: remove what the box
  /// covers, then add the blurred pixels.
  backdrop_erase_pipeline: wgpu::RenderPipeline,
  backdrop_add_pipeline: wgpu::RenderPipeline,
  blur_bind_group_layout: wgpu::BindGroupLayout,
  blur_sampler: wgpu::Sampler,
  format: wgpu::TextureFormat,
  slots: Vec<LayerSlot>,
  frame: u64,
  /// The backdrop blurs whose finished texture is kept, by the textures'
  /// ids.
  backdrops: BackdropCache,
  /// The id of the texture created last.
  last_texture: u64,
  /// What the frame does for each of its backdrop blurs, and the textures
  /// the cache gave back: scratch of [`Self::acquire_backdrops`].
  backdrop_uses: Vec<Option<BackdropUse>>,
  released: Vec<u64>,
  /// Backdrop blurs the frame put back from a kept texture.
  backdrops_reused: usize,
}

/// The textures a blurred layer's passes paint, beside the one it is painted
/// into: the reduced copy of a wide blur, the horizontal pass's result, and
/// the vertical pass's, which is the finished blur the pass compositing the
/// layer reads.
#[derive(Clone, Copy)]
pub(super) struct BlurSlots {
  blur: LayerBlur,
  plan: BlurPlan,
  reduced: Option<usize>,
  horizontal: usize,
  blurred: usize,
}

/// The textures of a backdrop blur: the finished blur, kept from an earlier
/// frame or painted by this one, and for a blur that runs the copy of its
/// target's pixels and the textures of the passes between.
#[derive(Clone, Copy)]
pub(super) struct BackdropSlots {
  region: BackdropRegion,
  plan: BlurPlan,
  blurred: usize,
  /// `None` when the kept blur is put back as it is.
  work: Option<BackdropWork>,
}

/// The textures a backdrop blur paints on its way to the finished one.
#[derive(Clone, Copy)]
struct BackdropWork {
  blur: LayerBlur,
  copied: usize,
  reduced: Option<usize>,
  horizontal: usize,
}

impl BackdropSlots {
  /// The window pixels the blur changes.
  pub(super) fn bounds(&self) -> crate::layout::opacity_layer::LayerBounds {
    self.region.layer.bounds
  }

  /// Whether the frame runs the blur: it stops the target's pass for it.
  /// Otherwise the kept result is put back inside the pass.
  pub(super) fn runs(&self) -> bool {
    self.work.is_some()
  }
}

struct LayerSlot {
  /// Names the texture to what outlives a frame: slots move as the pool
  /// releases others.
  id: u64,
  texture: wgpu::Texture,
  view: wgpu::TextureView,
  /// Holds the uniforms of the one pass that reads this texture in a frame:
  /// a `LayerComposite` or a `LayerBlurPass`.
  uniform: wgpu::Buffer,
  /// The bytes `uniform` holds. A pass whose uniforms are the ones of the
  /// frame before writes nothing: every write is a staging buffer and a copy
  /// when the frame is submitted, and a layer or a glass that holds still
  /// has the same passes frame after frame.
  uniforms: std::cell::RefCell<Vec<u8>>,
  bind_group: wgpu::BindGroup,
  blur_bind_group: wgpu::BindGroup,
  width: u32,
  height: u32,
  in_use: bool,
  last_used: u64,
  /// Holds a kept backdrop blur: no frame takes it for anything else, and
  /// it is not released for being idle.
  kept: bool,
}

impl LayerSlot {
  /// The bytes the texture holds, at four a texel.
  fn bytes(&self) -> usize {
    self.width as usize * self.height as usize * 4
  }

  /// Puts the uniforms of the pass that reads the texture this frame in its
  /// buffer, unless it holds them already.
  fn write_uniforms(&self, queue: &wgpu::Queue, uniforms: &[u8]) {
    let mut held = self.uniforms.borrow_mut();
    if held.as_slice() == uniforms {
      return;
    }
    queue.write_buffer(&self.uniform, 0, uniforms);
    held.clear();
    held.extend_from_slice(uniforms);
  }
}

impl LayerCompositor {
  /// `format` is the render target format the other pipelines draw in; the
  /// layers use it too, so every pipeline draws into them unchanged.
  pub(super) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("lurq_layer_bgl"),
      entries: &[
        wgpu::BindGroupLayoutEntry {
          binding: 0,
          visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
          },
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
          },
          count: None,
        },
      ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("lurq_layer_shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("shaders/layer.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("lurq_layer_pl"),
      bind_group_layouts: &[Some(&bind_group_layout)],
      immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("lurq_layer_pipeline"),
      layout: Some(&layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: Some("vs_main"),
        buffers: &[QuadVertex::desc()],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: Some("fs_main"),
        targets: &[Some(wgpu::ColorTargetState {
          format,
          blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        ..Default::default()
      },
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      multiview_mask: None,
      cache: None,
    });
    let blur_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("lurq_layer_blur_bgl"),
      entries: &[
        wgpu::BindGroupLayoutEntry {
          binding: 0,
          visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
          },
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
          },
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 2,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
          count: None,
        },
      ],
    });
    let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("lurq_layer_blur_shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("shaders/layer_blur.wgsl").into()),
    });
    let blur_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("lurq_layer_blur_pl"),
      bind_group_layouts: &[Some(&blur_bind_group_layout)],
      immediate_size: 0,
    });
    let blur_pipeline_of = |label: &str, fragment: &str, blend: wgpu::BlendState| {
      device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&blur_layout),
        vertex: wgpu::VertexState {
          module: &blur_shader,
          entry_point: Some("vs_main"),
          buffers: &[QuadVertex::desc()],
          compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
          module: &blur_shader,
          entry_point: Some(fragment),
          targets: &[Some(wgpu::ColorTargetState {
            format,
            blend: Some(blend),
            write_mask: wgpu::ColorWrites::ALL,
          })],
          compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
          topology: wgpu::PrimitiveTopology::TriangleList,
          ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
      })
    };
    let blur_pipeline = blur_pipeline_of(
      "lurq_layer_blur_pipeline",
      "fs_main",
      wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
    );
    let erase = wgpu::BlendComponent {
      src_factor: wgpu::BlendFactor::Zero,
      dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
      operation: wgpu::BlendOperation::Add,
    };
    let backdrop_erase_pipeline = blur_pipeline_of(
      "lurq_backdrop_erase_pipeline",
      "fs_backdrop_erase",
      wgpu::BlendState {
        color: erase,
        alpha: erase,
      },
    );
    let add = wgpu::BlendComponent {
      src_factor: wgpu::BlendFactor::One,
      dst_factor: wgpu::BlendFactor::One,
      operation: wgpu::BlendOperation::Add,
    };
    let backdrop_add_pipeline = blur_pipeline_of(
      "lurq_backdrop_add_pipeline",
      "fs_backdrop_add",
      wgpu::BlendState { color: add, alpha: add },
    );
    // Bilinear, so one tap reads two texels of a Gaussian or four of a box;
    // past the texture's edge it repeats the edge, which a pass never shows
    // (see `layout::opacity_layer`).
    let blur_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("lurq_layer_blur_sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      ..Default::default()
    });
    Self {
      pipeline,
      bind_group_layout,
      blur_pipeline,
      backdrop_erase_pipeline,
      backdrop_add_pipeline,
      blur_bind_group_layout,
      blur_sampler,
      format,
      slots: Vec::new(),
      frame: 0,
      backdrops: BackdropCache::default(),
      last_texture: 0,
      backdrop_uses: Vec::new(),
      released: Vec::new(),
      backdrops_reused: 0,
    }
  }

  pub(super) fn format(&self) -> wgpu::TextureFormat {
    self.format
  }

  pub(super) fn begin_frame(&mut self) {
    self.frame += 1;
    for slot in &mut self.slots {
      // A kept blur is not the frame's to paint over, and is in use for as
      // long as it is kept.
      slot.in_use = slot.kept;
      if slot.kept {
        slot.last_used = self.frame;
      }
    }
  }

  /// A free texture of at least `width` x `height` for this frame.
  pub(super) fn acquire(&mut self, device: &wgpu::Device, width: u32, height: u32) -> usize {
    let fitting = self
      .slots
      .iter()
      .enumerate()
      .filter(|(_, slot)| !slot.in_use && slot.width >= width && slot.height >= height)
      .min_by_key(|(_, slot)| u64::from(slot.width) * u64::from(slot.height))
      .map(|(index, _)| index);
    self.take(device, fitting, width, height)
  }

  /// A free texture for a `width` x `height` blur that may be kept: one of
  /// the size a new one would have, so that a small blur does not keep a
  /// large texture out of the pool.
  fn acquire_to_keep(&mut self, device: &wgpu::Device, width: u32, height: u32) -> usize {
    let size = Self::slot_size(device, width, height);
    let fitting = self
      .slots
      .iter()
      .position(|slot| !slot.in_use && [slot.width, slot.height] == size);
    self.take(device, fitting, width, height)
  }

  /// The size of a new texture for a `width` x `height` layer.
  fn slot_size(device: &wgpu::Device, width: u32, height: u32) -> [u32; 2] {
    let limit = device.limits().max_texture_dimension_2d;
    [width, height].map(|value| {
      value
        .div_ceil(LAYER_SIZE_STEP)
        .saturating_mul(LAYER_SIZE_STEP)
        .min(limit)
    })
  }

  /// Takes the free texture `fitting` for this frame, or a new one for a
  /// `width` x `height` layer.
  fn take(&mut self, device: &wgpu::Device, fitting: Option<usize>, width: u32, height: u32) -> usize {
    let index = fitting.unwrap_or_else(|| {
      let [width, height] = Self::slot_size(device, width, height);
      self.last_texture += 1;
      self
        .slots
        .push(self.create_slot(device, self.last_texture, width, height));
      self.slots.len() - 1
    });
    let slot = &mut self.slots[index];
    slot.in_use = true;
    slot.last_used = self.frame;
    index
  }

  fn create_slot(&self, device: &wgpu::Device, id: u64, width: u32, height: u32) -> LayerSlot {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("lurq_opacity_layer"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: self.format,
      // Copied from and to by backdrop blurs.
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_SRC
        | wgpu::TextureUsages::COPY_DST,
      view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("lurq_layer_composite"),
      size: std::mem::size_of::<LayerComposite>().max(std::mem::size_of::<LayerBlurPass>()) as wgpu::BufferAddress,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("lurq_layer_bg"),
      layout: &self.bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: uniform.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::TextureView(&view),
        },
      ],
    });
    let blur_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("lurq_layer_blur_bg"),
      layout: &self.blur_bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: uniform.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::TextureView(&view),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: wgpu::BindingResource::Sampler(&self.blur_sampler),
        },
      ],
    });
    LayerSlot {
      id,
      texture,
      view,
      uniform,
      uniforms: Default::default(),
      bind_group,
      blur_bind_group,
      width,
      height,
      in_use: false,
      last_used: 0,
      kept: false,
    }
  }

  /// The pixel space of `layer` painted into `slot`.
  pub(super) fn space(&self, slot: usize, layer: &LayerCmd) -> TargetSpace {
    let slot = &self.slots[slot];
    TargetSpace::layer(layer, slot.width, slot.height)
  }

  fn size(&self, slot: usize) -> [u32; 2] {
    [self.slots[slot].width, self.slots[slot].height]
  }

  /// The textures the passes of `blur` paint this frame.
  pub(super) fn acquire_blur(&mut self, device: &wgpu::Device, blur: &LayerBlur) -> BlurSlots {
    let plan = BlurPlan::new(blur);
    let reduced = (plan.reduction > 1).then(|| self.acquire(device, plan.size[0], plan.size[1]));
    let horizontal = self.acquire(device, plan.size[0], plan.size[1]);
    let blurred = self.acquire(device, plan.size[0], plan.size[1]);
    BlurSlots {
      blur: *blur,
      plan,
      reduced,
      horizontal,
      blurred,
    }
  }

  pub(super) fn texture(&self, slot: usize) -> &wgpu::Texture {
    &self.slots[slot].texture
  }

  /// The textures of the frame's backdrop blurs (`slots`, by the blurs'
  /// index; `None` for one without a region): `regions` are the blurs'
  /// regions and `inputs` what each of them reads, in a window of `window`
  /// pixels. A blur whose kept result is still the blur of what it reads
  /// takes that texture alone; any other takes the textures of its passes,
  /// and the texture kept for its place or a new one for the result, which
  /// is kept from then on if the cache has room for it.
  pub(super) fn acquire_backdrops(
    &mut self,
    device: &wgpu::Device,
    regions: &[Option<BackdropRegion>],
    inputs: &BackdropInputs,
    window: [u32; 2],
    slots: &mut Vec<Option<BackdropSlots>>,
  ) {
    let mut uses = std::mem::take(&mut self.backdrop_uses);
    let mut released = std::mem::take(&mut self.released);
    self.backdrops.begin_frame(
      regions,
      inputs,
      BackdropCache::budget_of(window),
      &mut uses,
      &mut released,
    );
    self.release_kept(&released);
    slots.clear();
    self.backdrops_reused = 0;
    for (index, (region, used)) in regions.iter().zip(&uses).enumerate() {
      let (Some(region), Some(used)) = (region, used) else {
        slots.push(None);
        continue;
      };
      let Some(blur) = region.layer.blur else {
        slots.push(None);
        continue;
      };
      let plan = BlurPlan::new(&blur);
      let kept = match used {
        BackdropUse::Reuse { texture } | BackdropUse::Refresh { texture } => {
          self.slots.iter().position(|slot| slot.id == *texture)
        }
        BackdropUse::Blur { .. } => None,
      };
      let work = match (used, kept) {
        (BackdropUse::Reuse { .. }, Some(_)) => None,
        _ => Some(BackdropWork {
          blur,
          copied: self.acquire(device, blur.width, blur.height),
          reduced: (plan.reduction > 1).then(|| self.acquire(device, plan.size[0], plan.size[1])),
          horizontal: self.acquire(device, plan.size[0], plan.size[1]),
        }),
      };
      let blurred = kept.unwrap_or_else(|| {
        let slot = self.acquire_to_keep(device, plan.size[0], plan.size[1]);
        let (id, bytes) = (self.slots[slot].id, self.slots[slot].bytes());
        if matches!(used, BackdropUse::Blur { keep: true }) && self.backdrops.keep(inputs, index, id, bytes) {
          self.slots[slot].kept = true;
        }
        slot
      });
      self.backdrops_reused += usize::from(work.is_none());
      slots.push(Some(BackdropSlots {
        region: *region,
        plan,
        blurred,
        work,
      }));
    }
    self.backdrop_uses = uses;
    self.released = released;
  }

  /// Gives the kept textures `released` back to the pool, free for this
  /// frame.
  fn release_kept(&mut self, released: &[u64]) {
    for slot in &mut self.slots {
      if slot.kept && released.contains(&slot.id) {
        slot.kept = false;
        slot.in_use = false;
      }
    }
  }

  /// The frame's commands were handed to the GPU: the backdrop blurs it ran
  /// into kept textures can be put back from them.
  pub(super) fn frame_submitted(&mut self) {
    self.backdrops.commit();
  }

  /// Keeps no backdrop blur any longer: their textures go back to the pool.
  pub(super) fn forget_backdrops(&mut self) {
    let mut released = std::mem::take(&mut self.released);
    released.clear();
    self.backdrops.clear(&mut released);
    self.release_kept(&released);
    self.released = released;
  }

  /// Backdrop blurs the frame put back from a kept texture, then the kept
  /// textures and their bytes.
  #[cfg_attr(not(any(test, feature = "perf_profile")), allow(dead_code))]
  pub(super) fn backdrop_stats(&self) -> (usize, usize, usize) {
    (self.backdrops_reused, self.backdrops.len(), self.backdrops.bytes())
  }

  /// Runs a backdrop blur up to compositing, between two passes of its
  /// target: copies the pixels it reads out of `target` (whose texel (0, 0)
  /// is window pixel `target_origin`), then reduces and blurs the copy.
  #[allow(clippy::too_many_arguments)]
  pub(super) fn prepare_backdrop(
    &self,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    vertices: &wgpu::Buffer,
    indices: &wgpu::Buffer,
    target: &wgpu::Texture,
    target_origin: [i32; 2],
    slots: &BackdropSlots,
  ) -> usize {
    let Some(work) = slots.work else {
      return 0;
    };
    let blur = work.blur;
    encoder.copy_texture_to_texture(
      wgpu::TexelCopyTextureInfo {
        texture: target,
        mip_level: 0,
        origin: wgpu::Origin3d {
          x: (blur.x - target_origin[0]).max(0) as u32,
          y: (blur.y - target_origin[1]).max(0) as u32,
          z: 0,
        },
        aspect: wgpu::TextureAspect::All,
      },
      wgpu::TexelCopyTextureInfo {
        texture: &self.slots[work.copied].texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      wgpu::Extent3d {
        width: blur.width,
        height: blur.height,
        depth_or_array_layers: 1,
      },
    );
    let passes = BlurSlots {
      blur,
      plan: slots.plan,
      reduced: work.reduced,
      horizontal: work.horizontal,
      blurred: slots.blurred,
    };
    self.prepare_blur(queue, encoder, vertices, indices, work.copied, &passes)
  }

  /// Puts a blurred backdrop back into the pass painting `parent`, inside
  /// the node's box: what [`Self::prepare_backdrop`] left in `slots`, or
  /// what an earlier frame's did.
  pub(super) fn composite_backdrop(
    &self,
    queue: &wgpu::Queue,
    pass: &mut wgpu::RenderPass<'_>,
    slots: &BackdropSlots,
    parent: &TargetSpace,
  ) {
    let blurred = slots.blurred;
    let uniforms = LayerBlurPass::backdrop(&slots.region, &slots.plan, parent, self.size(blurred));
    let slot = &self.slots[blurred];
    slot.write_uniforms(queue, bytemuck::bytes_of(&uniforms));
    pass.set_bind_group(0, &slot.blur_bind_group, &[]);
    for pipeline in [&self.backdrop_erase_pipeline, &self.backdrop_add_pipeline] {
      pass.set_pipeline(pipeline);
      pass.draw_indexed(0..QuadVertex::INDICES.len() as u32, 0, 0..1);
    }
  }

  /// Runs the passes of a blur that come before compositing, on the layer
  /// just painted into `painted`: the reduction of a wide blur, then the
  /// horizontal Gaussian and the vertical one. `vertices` and `indices` are
  /// the unit quad's. Returns the number of passes it ran.
  pub(super) fn prepare_blur(
    &self,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    vertices: &wgpu::Buffer,
    indices: &wgpu::Buffer,
    painted: usize,
    slots: &BlurSlots,
  ) -> usize {
    let mut source = painted;
    if let Some(reduced) = slots.reduced {
      let uniforms = LayerBlurPass::reduce(&slots.blur, &slots.plan, self.size(source), self.size(reduced));
      self.run_blur_pass(
        queue,
        encoder,
        vertices,
        indices,
        source,
        reduced,
        &uniforms,
        slots.plan.size,
      );
      source = reduced;
    }
    let uniforms = LayerBlurPass::horizontal(&slots.plan, self.size(source), self.size(slots.horizontal));
    self.run_blur_pass(
      queue,
      encoder,
      vertices,
      indices,
      source,
      slots.horizontal,
      &uniforms,
      slots.plan.size,
    );
    let uniforms = LayerBlurPass::vertical(&slots.plan, self.size(slots.horizontal), self.size(slots.blurred));
    self.run_blur_pass(
      queue,
      encoder,
      vertices,
      indices,
      slots.horizontal,
      slots.blurred,
      &uniforms,
      slots.plan.size,
    );
    2 + usize::from(slots.reduced.is_some())
  }

  #[allow(clippy::too_many_arguments)]
  fn run_blur_pass(
    &self,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    vertices: &wgpu::Buffer,
    indices: &wgpu::Buffer,
    source: usize,
    target: usize,
    uniforms: &LayerBlurPass,
    size: [u32; 2],
  ) {
    let [width, height] = self.size(target);
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("lurq_layer_blur_pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: &self.slots[target].view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
          store: wgpu::StoreOp::Store,
        },
      })],
      ..Default::default()
    });
    pass.set_scissor_rect(0, 0, size[0].min(width), size[1].min(height));
    pass.set_vertex_buffer(0, vertices.slice(..));
    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint16);
    self.draw_blur(queue, &mut pass, source, uniforms);
  }

  /// Draws a blurred `layer` into the pass painting `parent`: what
  /// [`Self::prepare_blur`] left in `slots`.
  pub(super) fn composite_blurred(
    &self,
    queue: &wgpu::Queue,
    pass: &mut wgpu::RenderPass<'_>,
    slots: &BlurSlots,
    layer: &LayerCmd,
    parent: &TargetSpace,
  ) {
    let uniforms = LayerBlurPass::composite(layer, &slots.plan, parent, self.size(slots.blurred));
    self.draw_blur(queue, pass, slots.blurred, &uniforms);
  }

  fn draw_blur(&self, queue: &wgpu::Queue, pass: &mut wgpu::RenderPass<'_>, source: usize, uniforms: &LayerBlurPass) {
    let slot = &self.slots[source];
    // Each texture is read by one pass per frame, so its own buffer holds
    // that pass's values until the frame's submit.
    slot.write_uniforms(queue, bytemuck::bytes_of(uniforms));
    pass.set_pipeline(&self.blur_pipeline);
    pass.set_bind_group(0, &slot.blur_bind_group, &[]);
    pass.draw_indexed(0..QuadVertex::INDICES.len() as u32, 0, 0..1);
  }

  pub(super) fn view(&self, slot: usize) -> &wgpu::TextureView {
    &self.slots[slot].view
  }

  /// Draws `slot`, holding `layer`, into the pass painting `parent`.
  pub(super) fn composite(
    &self,
    queue: &wgpu::Queue,
    pass: &mut wgpu::RenderPass<'_>,
    slot: usize,
    layer: &LayerCmd,
    parent: &TargetSpace,
  ) {
    let slot = &self.slots[slot];
    // Each layer is composited once per frame, so its own buffer holds the
    // values until the frame's submit.
    slot.write_uniforms(queue, bytemuck::bytes_of(&LayerComposite::new(layer, parent)));
    pass.set_pipeline(&self.pipeline);
    pass.set_bind_group(0, &slot.bind_group, &[]);
    pass.draw_indexed(0..QuadVertex::INDICES.len() as u32, 0, 0..1);
  }

  /// Releases textures no frame has used for a while.
  pub(super) fn end_frame(&mut self) {
    let frame = self.frame;
    self
      .slots
      .retain(|slot| slot.in_use || slot.kept || frame.saturating_sub(slot.last_used) <= LAYER_IDLE_FRAMES);
  }

  #[cfg(all(test, feature = "screenshot"))]
  pub(super) fn texture_count(&self) -> usize {
    self.slots.len()
  }

  /// The pool's textures and the bytes they hold, at four bytes a texel.
  #[cfg(feature = "perf_profile")]
  pub(super) fn texture_stats(&self) -> (usize, usize) {
    let bytes = self
      .slots
      .iter()
      .map(|slot| slot.width as usize * slot.height as usize * 4)
      .sum();
    (self.slots.len(), bytes)
  }
}
