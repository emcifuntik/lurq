//! Offscreen targets for flattened group opacity and layer blur (see
//! [`crate::layout::opacity_layer`]) and the pipelines that composite them.
//!
//! A blurred layer takes one or two more textures from the same pool for the
//! passes between painting it and compositing it ([`BlurSlots`]). A backdrop
//! blur (see [`crate::layout::backdrop`]) takes those and one for the copy of
//! its target ([`BackdropSlots`]).
//!
//! Layer textures are pooled across frames: a frame takes the smallest free
//! texture that fits each layer, creating one (rounded up to
//! [`LAYER_SIZE_STEP`]) only when none does, and textures unused for
//! [`LAYER_IDLE_FRAMES`] frames are released. Nothing is created until a
//! frame has a layer.

use super::vertex::QuadVertex;
use crate::layout::{
  backdrop::BackdropRegion,
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
}

/// The textures a blurred layer's passes paint, beside the one it is painted
/// into: the reduced copy of a wide blur, and the horizontal pass's result,
/// which the pass compositing the layer reads.
#[derive(Clone, Copy)]
pub(super) struct BlurSlots {
  blur: LayerBlur,
  plan: BlurPlan,
  reduced: Option<usize>,
  blurred: usize,
}

/// The textures of a backdrop blur: the copy of its target's pixels, and the
/// textures of the blur run on it.
#[derive(Clone, Copy)]
pub(super) struct BackdropSlots {
  region: BackdropRegion,
  copied: usize,
  blur: BlurSlots,
}

impl BackdropSlots {
  /// The window pixels the blur changes.
  pub(super) fn bounds(&self) -> crate::layout::opacity_layer::LayerBounds {
    self.region.layer.bounds
  }
}

struct LayerSlot {
  texture: wgpu::Texture,
  view: wgpu::TextureView,
  /// Holds the uniforms of the one pass that reads this texture in a frame:
  /// a `LayerComposite` or a `LayerBlurPass`.
  uniform: wgpu::Buffer,
  bind_group: wgpu::BindGroup,
  blur_bind_group: wgpu::BindGroup,
  width: u32,
  height: u32,
  in_use: bool,
  last_used: u64,
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
    }
  }

  pub(super) fn format(&self) -> wgpu::TextureFormat {
    self.format
  }

  pub(super) fn begin_frame(&mut self) {
    self.frame += 1;
    for slot in &mut self.slots {
      slot.in_use = false;
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
    let index = fitting.unwrap_or_else(|| {
      let limit = device.limits().max_texture_dimension_2d;
      let size = |value: u32| {
        value
          .div_ceil(LAYER_SIZE_STEP)
          .saturating_mul(LAYER_SIZE_STEP)
          .min(limit)
      };
      self.slots.push(self.create_slot(device, size(width), size(height)));
      self.slots.len() - 1
    });
    let slot = &mut self.slots[index];
    slot.in_use = true;
    slot.last_used = self.frame;
    index
  }

  fn create_slot(&self, device: &wgpu::Device, width: u32, height: u32) -> LayerSlot {
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
      texture,
      view,
      uniform,
      bind_group,
      blur_bind_group,
      width,
      height,
      in_use: false,
      last_used: 0,
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
    let blurred = self.acquire(device, plan.size[0], plan.size[1]);
    BlurSlots {
      blur: *blur,
      plan,
      reduced,
      blurred,
    }
  }

  pub(super) fn texture(&self, slot: usize) -> &wgpu::Texture {
    &self.slots[slot].texture
  }

  /// The textures the backdrop blur `region` uses this frame.
  pub(super) fn acquire_backdrop(&mut self, device: &wgpu::Device, region: BackdropRegion) -> Option<BackdropSlots> {
    let blur = region.layer.blur?;
    let copied = self.acquire(device, blur.width, blur.height);
    Some(BackdropSlots {
      region,
      copied,
      blur: self.acquire_blur(device, &blur),
    })
  }

  /// Runs a backdrop blur up to compositing, between two passes of its
  /// target: copies the pixels it reads out of `target` (whose texel (0, 0)
  /// is window pixel `target_origin`), then reduces and blurs the copy
  /// along x.
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
  ) {
    let blur = slots.blur.blur;
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
        texture: &self.slots[slots.copied].texture,
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
    self.prepare_blur(queue, encoder, vertices, indices, slots.copied, &slots.blur);
  }

  /// Puts a blurred backdrop back into the pass painting `parent`, inside
  /// the node's box: the vertical Gaussian over what
  /// [`Self::prepare_backdrop`] left in `slots`.
  pub(super) fn composite_backdrop(
    &self,
    queue: &wgpu::Queue,
    pass: &mut wgpu::RenderPass<'_>,
    slots: &BackdropSlots,
    parent: &TargetSpace,
  ) {
    let blurred = slots.blur.blurred;
    let uniforms = LayerBlurPass::backdrop(&slots.region, &slots.blur.plan, parent, self.size(blurred));
    let slot = &self.slots[blurred];
    queue.write_buffer(&slot.uniform, 0, bytemuck::bytes_of(&uniforms));
    pass.set_bind_group(0, &slot.blur_bind_group, &[]);
    for pipeline in [&self.backdrop_erase_pipeline, &self.backdrop_add_pipeline] {
      pass.set_pipeline(pipeline);
      pass.draw_indexed(0..QuadVertex::INDICES.len() as u32, 0, 0..1);
    }
  }

  /// Runs the passes of a blur that come before compositing, on the layer
  /// just painted into `painted`: the reduction of a wide blur, then the
  /// horizontal Gaussian. `vertices` and `indices` are the unit quad's.
  pub(super) fn prepare_blur(
    &self,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    vertices: &wgpu::Buffer,
    indices: &wgpu::Buffer,
    painted: usize,
    slots: &BlurSlots,
  ) {
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
    let uniforms = LayerBlurPass::horizontal(&slots.plan, self.size(source), self.size(slots.blurred));
    self.run_blur_pass(
      queue,
      encoder,
      vertices,
      indices,
      source,
      slots.blurred,
      &uniforms,
      slots.plan.size,
    );
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

  /// Draws a blurred `layer` into the pass painting `parent`: the vertical
  /// Gaussian over what [`Self::prepare_blur`] left in `slots`.
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
    queue.write_buffer(&slot.uniform, 0, bytemuck::bytes_of(uniforms));
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
    queue.write_buffer(
      &slot.uniform,
      0,
      bytemuck::bytes_of(&LayerComposite::new(layer, parent)),
    );
    pass.set_pipeline(&self.pipeline);
    pass.set_bind_group(0, &slot.bind_group, &[]);
    pass.draw_indexed(0..QuadVertex::INDICES.len() as u32, 0, 0..1);
  }

  /// Releases textures no frame has used for a while.
  pub(super) fn end_frame(&mut self) {
    let frame = self.frame;
    self
      .slots
      .retain(|slot| slot.in_use || frame.saturating_sub(slot.last_used) <= LAYER_IDLE_FRAMES);
  }

  #[cfg(all(test, feature = "screenshot"))]
  pub(super) fn texture_count(&self) -> usize {
    self.slots.len()
  }
}
