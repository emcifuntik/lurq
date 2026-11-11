mod artwork;
mod pipeline;
mod presentation_pool;
use pipeline::pipeline;
use presentation_pool::{ArtworkLease, Spare};
mod drawing;
mod initialization;
mod presentation;
use std::collections::{HashMap, HashSet, VecDeque};

use wgpu::*;

use super::DynamicBuffer;
#[cfg(feature = "perf_profile")]
use super::gpu_timing;
use crate::{
  app::profile_support::{profile_elapsed, profile_if, profile_scope},
  canvas::{
    AssetCache, BlendMode, CanvasAssetBudget, CanvasError, CanvasHandle, CanvasId, CanvasSnapshot, CanvasWeak,
    FrameBoundary, GradientKind, PresentationEvent, TargetCharge, gpu::*, replacement_admitted,
  },
};

struct Texture {
  texture: wgpu::Texture,
  view: TextureView,
  nearest: BindGroup,
  linear: BindGroup,
}
struct Artwork {
  image: Texture,
}
struct Backing {
  revision: u64,
  artwork: Option<std::sync::Arc<Artwork>>,
  owner: CanvasWeak,
  image: Texture,
  generation: u64,
}
pub(super) struct Renderer {
  #[cfg(feature = "perf_profile")]
  pub(super) profile: crate::app::profile_types::CanvasProfile,
  #[cfg(feature = "perf_profile")]
  pub(super) profile_context: Option<crate::app::profiler::ProfileContext>,
  /// Brackets each command buffer with GPU timestamps while a frame is measured.
  #[cfg(feature = "perf_profile")]
  pub(super) gpu_marks: Option<gpu_timing::CanvasMarks>,
  meshes: MeshCache,
  surfaces: HashMap<CanvasId, Backing>,
  fronts: HashMap<CanvasId, (u64, Backing)>,
  spares: Vec<Spare>,
  retired_artwork: Vec<ArtworkLease>,
  rejected: HashSet<CanvasId>,
  assets: AssetCache<Texture>,
  frames: FrameBoundary,
  generation: u64,
  globals_layout: BindGroupLayout,
  image_layout: BindGroupLayout,
  blend_layout: BindGroupLayout,
  nearest: Sampler,
  linear: Sampler,
  _scratch: wgpu::Texture,
  scratch_view: TextureView,
  resolve: wgpu::Texture,
  resolve_view: TextureView,
  _stencil: wgpu::Texture,
  stencil_view: TextureView,
  white: Texture,
  seed: RenderPipeline,
  resample: RenderPipeline,
  reset_stencil: RenderPipeline,
  clip: RenderPipeline,
  solid: RenderPipeline,

  premul: RenderPipeline,
  retained: RenderPipeline,
  erase: RenderPipeline,
  gradient_linear: RenderPipeline,
  gradient_radial: RenderPipeline,
  gradient_angular: RenderPipeline,
  /// Composites a finished isolated layer onto what was under it, source-over.
  compose: RenderPipeline,
  /// Puts a saved tile back as the base of the pass that composites over it.
  restore: RenderPipeline,
  /// The same composite for the other seventeen blend modes, which read the
  /// backdrop instead of relying on fixed-function blending.
  blend: RenderPipeline,
  /// Tile-sized copies for isolated layers, allocated on first use.
  saved: Vec<Texture>,
  layer: Option<Texture>,
  vertices: DynamicBuffer,
  globals: DynamicBuffer,
}
impl Renderer {
  pub fn snapshot(&self, canvas: &CanvasHandle) -> Option<crate::images::WgpuExternalImageSnapshot> {
    let id = canvas.surface_id();
    let b = self
      .fronts
      .get(&id)
      .map(|(_, b)| b)
      .or_else(|| self.surfaces.get(&id))?;
    Some(crate::images::WgpuExternalImageSnapshot {
      view: b.image.view.clone(),
      width: b.image.texture.width(),
      height: b.image.texture.height(),
      version: b.generation,
    })
  }
  pub fn process(&mut self, device: &Device, queue: &Queue, canvases: &[CanvasHandle]) {
    profile_if! { self.profile = Default::default(); }
    let _process_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _phase = self
      .profile_context
      .as_ref()
      .map(|context| context.phase(crate::app::profiler::Phase::CanvasBackend));
    self.frames.begin_encode();
    let live: HashSet<_> = canvases
      .iter()
      .filter(|c| c.is_attached())
      .map(CanvasHandle::surface_id)
      .collect();
    let closed: Vec<_> = self
      .surfaces
      .keys()
      .chain(self.fronts.keys())
      .copied()
      .filter(|id| !live.contains(id))
      .collect();
    for id in closed {
      if let Some((_, front)) = self.frames.end_replacement(&mut self.fronts, id) {
        self.retire_back(queue, id, front);
      }
      if let Some(back) = self.surfaces.remove(&id) {
        if let Some(owner) = back.owner.upgrade() {
          owner.set_gpu_bytes(0);
        }
        self.retire_back(queue, id, back);
      }
      self.rejected.remove(&id);
    }
    self.reap(&live);
    for canvas in canvases {
      let Some(mut batch) = canvas.take_batch() else {
        continue;
      };
      self.frames.encodes(canvas);
      profile_if! { self.profile.batches += 1; }
      let mut commands: VecDeque<_> = batch.take_commands().into();
      if !self.surfaces.contains_key(&canvas.surface_id()) && !matches!(commands.front(), Some(Command::Resize { .. }))
      {
        let metrics = canvas.metrics();
        self.resize(device, queue, canvas, metrics.pixel_width, metrics.pixel_height, false);
      }
      while let Some(command) = commands.pop_front() {
        if self.rejected.contains(&canvas.surface_id())
          && !matches!(
            command,
            Command::BeginPresentation(..)
              | Command::CommitPresentation(..)
              | Command::AbortPresentation(_)
              | Command::Readback(..)
              | Command::ForgetArtwork
          )
        {
          continue;
        }
        match command {
          Command::BeginPresentation(token, front_revision) => {
            if canvas.presentation_current(token) {
              self.begin_presentation(device, queue, canvas, token, front_revision);
            } else {
              self.rejected.insert(canvas.surface_id());
            }
          }
          Command::CommitPresentation(token, revision) => self.end_presentation(queue, canvas, token, true, revision),
          Command::AbortPresentation(token) => self.end_presentation(queue, canvas, token, false, 0),
          Command::ForgetArtwork => self.forget_artwork(queue, canvas),
          Command::CaptureArtwork => self.capture_artwork(device, queue, canvas),
          Command::RetainedArtwork(matrix) => self.draw_artwork(device, queue, canvas, matrix),
          Command::Resize {
            width,
            height,
            preserve,
          } => {
            self.abort_for_resize(queue, canvas);
            if !self.surfaces.contains_key(&canvas.surface_id()) && commands.is_empty() {
              continue;
            }
            self.resize(device, queue, canvas, width, height, preserve);
          }
          Command::Readback(done, metrics, revision, complete) => {
            let id = canvas.surface_id();
            if let Some(backing) = self.fronts.get(&id).map(|(_, b)| b).or_else(|| self.surfaces.get(&id)) {
              readback(
                device,
                queue,
                &backing.image.texture,
                done,
                if complete || self.fronts.contains_key(&id) || self.rejected.contains(&id) {
                  backing.revision
                } else {
                  revision
                },
              );
            } else if metrics.pixel_width != 0 && metrics.pixel_height != 0 {
              done.finish(Err(canvas.status().error.unwrap_or(CanvasError::RendererLost)));
            } else {
              done.finish(Ok(CanvasSnapshot {
                width: metrics.pixel_width,
                height: metrics.pixel_height,
                rgba: Vec::new(),
                revision,
              }));
            }
          }
          first => {
            let mut group = vec![first];
            while commands.front().is_some_and(|c| {
              !matches!(
                c,
                Command::Resize { .. }
                  | Command::Readback(..)
                  | Command::BeginPresentation(..)
                  | Command::CommitPresentation(..)
                  | Command::AbortPresentation(_)
                  | Command::ForgetArtwork
                  | Command::CaptureArtwork
                  | Command::RetainedArtwork(_)
              )
            }) {
              group.push(commands.pop_front().unwrap());
            }
            let before = self.meshes.stats();
            let _tessellation_start = profile_scope!();
            #[cfg(feature = "perf_profile")]
            let _tessellation_phase = self
              .profile_context
              .as_ref()
              .and_then(|context| context.detail_phase(crate::app::profiler::Phase::CanvasTessellation));
            let prepared = Prepared::new(&group, &mut self.meshes);
            profile_if! {
              self.profile.tessellation += profile_elapsed!(_tessellation_start);
              self.profile.command_groups += 1;
              drop(_tessellation_phase);
            }
            canvas.record_mesh_cache(before, self.meshes.stats());
            match prepared {
              Ok(prepared) => self.draw(device, queue, canvas.surface_id(), &prepared),
              Err(error) => canvas.set_gpu_error(error),
            }
          }
        }
      }
      batch.submit();
    }
    // WGPU keeps a dropped texture alive while submitted work still uses it.
    let frame_ends = self.frames.end_encode(&self.fronts);
    let _frame = self.assets.finish_encode(frame_ends, drop);
    profile_if! {
      self.profile.asset_cache_budget_bytes = self.assets.budget();
      self.profile.asset_cache_bytes = self.assets.bytes();
      self.profile.asset_cache_stretch_bytes = _frame.stretch_bytes;
      self.profile.asset_cache_uncached = _frame.uncached;
      self.profile.asset_cache_uncached_bytes = _frame.uncached_bytes;
    }
    profile_if! { self.profile.total = profile_elapsed!(_process_start); }
  }
  fn resize(&mut self, device: &Device, queue: &Queue, canvas: &CanvasHandle, width: u32, height: u32, preserve: bool) {
    let id = canvas.surface_id();
    if width == 0 || height == 0 {
      if let Some(old) = self.surfaces.remove(&id) {
        self.retire_back(queue, id, old);
      }
      canvas.set_gpu_bytes(0);
      return;
    }
    if width > device.limits().max_texture_dimension_2d || height > device.limits().max_texture_dimension_2d {
      canvas.set_gpu_error(CanvasError::SurfaceTooLarge);
      return;
    }
    if !self.allocation_admitted(width as usize * height as usize * 4) {
      canvas.set_gpu_error(CanvasError::PresentationBusy);
      return;
    }
    let old = self.surfaces.remove(&id);
    let image = Texture::new(device, &self.image_layout, &self.nearest, &self.linear, width, height);
    let mut encoder = device.create_command_encoder(&Default::default());
    #[cfg(feature = "perf_profile")]
    gpu_timing::begin_canvas(&mut self.gpu_marks, &mut encoder);
    let constants = [0., 0., width as f32, height as f32, width as f32, height as f32, 0., 0.];
    let buffer = self.globals.write(device, queue, &constants).unwrap();
    let globals = global_group(device, &self.globals_layout, buffer);
    {
      let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("canvas resize"),
        color_attachments: &[Some(RenderPassColorAttachment {
          view: &image.view,
          depth_slice: None,
          resolve_target: None,
          ops: Operations {
            load: LoadOp::Clear(Color::TRANSPARENT),
            store: StoreOp::Store,
          },
        })],
        ..Default::default()
      });
      if preserve && let Some(old) = &old {
        pass.set_pipeline(&self.resample);
        pass.set_bind_group(0, &globals, &[0]);
        pass.set_bind_group(1, &old.image.linear, &[]);
        pass.draw(0..3, 0..1);
      }
    }
    #[cfg(feature = "perf_profile")]
    gpu_timing::end_canvas(&mut self.gpu_marks, &mut encoder);
    let _submit_start = profile_scope!();
    queue.submit([encoder.finish()]);
    profile_if! { self.profile.submit += profile_elapsed!(_submit_start); }
    self.generation += 1;
    self.surfaces.insert(
      id,
      Backing {
        revision: 0,
        artwork: old.as_ref().and_then(|b| b.artwork.clone()),
        owner: canvas.downgrade(),
        image,
        generation: self.generation,
      },
    );
    if let Some(old) = old {
      self.retire_back(queue, id, old);
    }
    self.account_presentation(canvas);
  }
  /// Tile-sized copies an isolated layer needs: `saved[d]` is what was under the
  /// layer opened at depth `d`, and one shared `layer` holds the finished layer
  /// being composited. Allocated on first use, so a canvas that never opens a
  /// layer pays nothing; each is TILE × TILE × 4 bytes.
  fn reserve_layers(&mut self, device: &Device, depth: usize) {
    while self.saved.len() < depth {
      self.saved.push(Texture::new(
        device,
        &self.image_layout,
        &self.nearest,
        &self.linear,
        TILE,
        TILE,
      ));
    }
    if depth > 0 && self.layer.is_none() {
      self.layer = Some(Texture::new(
        device,
        &self.image_layout,
        &self.nearest,
        &self.linear,
        TILE,
        TILE,
      ));
    }
  }
}

fn copy_tile(encoder: &mut CommandEncoder, source: &wgpu::Texture, target: &wgpu::Texture, tile: [u32; 4]) {
  encoder.copy_texture_to_texture(
    source.as_image_copy(),
    target.as_image_copy(),
    Extent3d {
      width: tile[2],
      height: tile[3],
      depth_or_array_layers: 1,
    },
  );
}

impl Texture {
  fn new(
    device: &Device,
    layout: &BindGroupLayout,
    nearest: &Sampler,
    linear: &Sampler,
    width: u32,
    height: u32,
  ) -> Self {
    let texture = device.create_texture(&TextureDescriptor {
      label: Some("canvas pixels"),
      size: Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: TextureDimension::D2,
      format: TextureFormat::Rgba8Unorm,
      usage: TextureUsages::TEXTURE_BINDING
        | TextureUsages::RENDER_ATTACHMENT
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC,
      view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let bind = |sampler| {
      device.create_bind_group(&BindGroupDescriptor {
        label: Some("canvas pixels"),
        layout,
        entries: &[
          BindGroupEntry {
            binding: 0,
            resource: BindingResource::TextureView(&view),
          },
          BindGroupEntry {
            binding: 1,
            resource: BindingResource::Sampler(sampler),
          },
        ],
      })
    };
    let nearest = bind(nearest);
    let linear = bind(linear);
    Self {
      texture,
      view,
      nearest,
      linear,
    }
  }
}
fn blend_group(
  device: &Device,
  layout: &BindGroupLayout,
  source: &TextureView,
  backdrop: &TextureView,
  sampler: &Sampler,
) -> BindGroup {
  device.create_bind_group(&BindGroupDescriptor {
    label: Some("canvas blend"),
    layout,
    entries: &[
      BindGroupEntry {
        binding: 0,
        resource: BindingResource::TextureView(source),
      },
      BindGroupEntry {
        binding: 1,
        resource: BindingResource::Sampler(sampler),
      },
      BindGroupEntry {
        binding: 2,
        resource: BindingResource::TextureView(backdrop),
      },
    ],
  })
}
fn global_group(device: &Device, layout: &BindGroupLayout, buffer: &Buffer) -> BindGroup {
  device.create_bind_group(&BindGroupDescriptor {
    label: Some("canvas globals"),
    layout,
    entries: &[BindGroupEntry {
      binding: 0,
      resource: BindingResource::Buffer(BufferBinding {
        buffer,
        offset: 0,
        size: BufferSize::new(32),
      }),
    }],
  })
}

fn readback(device: &Device, queue: &Queue, texture: &wgpu::Texture, done: Completion, revision: u64) {
  let (width, height) = (texture.width(), texture.height());
  let pitch = (width * 4).div_ceil(256) * 256;
  let buffer = device.create_buffer(&BufferDescriptor {
    label: Some("canvas explicit readback"),
    size: u64::from(pitch) * u64::from(height),
    usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
    mapped_at_creation: false,
  });
  let mut encoder = device.create_command_encoder(&Default::default());
  encoder.copy_texture_to_buffer(
    texture.as_image_copy(),
    TexelCopyBufferInfo {
      buffer: &buffer,
      layout: TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(pitch),
        rows_per_image: Some(height),
      },
    },
    Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
  );
  queue.submit([encoder.finish()]);
  let device = device.clone();
  std::thread::spawn(move || {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    buffer.slice(..).map_async(MapMode::Read, move |result| {
      let _ = tx.send(result);
    });
    if device.poll(PollType::wait_indefinitely()).is_err() || !matches!(rx.recv(), Ok(Ok(()))) {
      done.finish(Err(CanvasError::RendererLost));
      return;
    }
    let mapped = buffer.slice(..).get_mapped_range();
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for row in mapped.chunks_exact(pitch as usize) {
      rgba.extend_from_slice(&row[..width as usize * 4]);
    }
    drop(mapped);
    buffer.unmap();
    done.finish(Ok(CanvasSnapshot {
      width,
      height,
      rgba: unpremultiply(rgba),
      revision,
    }));
  });
}

impl Drop for Renderer {
  fn drop(&mut self) {
    for backing in self.surfaces.values() {
      if let Some(c) = backing.owner.upgrade() {
        c.set_gpu_bytes(0);
        c.set_gpu_error(CanvasError::RendererLost);
      }
    }
  }
}

#[cfg(test)]
mod camera_tests;
#[cfg(test)]
mod replacement_residency_tests;
#[cfg(test)]
mod residency_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod presentation_tests;
