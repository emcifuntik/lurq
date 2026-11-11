//! Artwork is captured before editor overlays; pending camera draws reuse only this texture.
use super::*;
impl Renderer {
  pub(super) fn forget_artwork(&mut self, queue: &Queue, canvas: &CanvasHandle) {
    let id = canvas.surface_id();
    if let Some(back) = self.surfaces.get_mut(&id) {
      if let Some(art) = back.artwork.take() {
        self.retired_artwork.push(ArtworkLease {
          ready: presentation_pool::completion(queue),
          art,
        });
      }
    }
    if let Some((_, front)) = self.fronts.get_mut(&id) {
      if let Some(art) = front.artwork.take() {
        self.retired_artwork.push(ArtworkLease {
          ready: presentation_pool::completion(queue),
          art,
        });
      }
    }
    self.account_presentation(canvas);
  }

  pub(super) fn capture_artwork(&mut self, device: &Device, queue: &Queue, canvas: &CanvasHandle) {
    let id = canvas.surface_id();
    let bytes = self.surfaces.get(&id).map_or(0, |back| {
      back.image.texture.width() as usize * back.image.texture.height() as usize * 4
    });
    if !self.allocation_admitted(bytes) {
      canvas.set_gpu_error(CanvasError::PresentationBusy);
      return;
    }
    let Some(back) = self.surfaces.get_mut(&id) else {
      return;
    };
    let image = Texture::new(
      device,
      &self.image_layout,
      &self.nearest,
      &self.linear,
      back.image.texture.width(),
      back.image.texture.height(),
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    #[cfg(feature = "perf_profile")]
    gpu_timing::begin_canvas(&mut self.gpu_marks, &mut encoder);
    encoder.copy_texture_to_texture(
      back.image.texture.as_image_copy(),
      image.texture.as_image_copy(),
      Extent3d {
        width: image.texture.width(),
        height: image.texture.height(),
        depth_or_array_layers: 1,
      },
    );
    #[cfg(feature = "perf_profile")]
    gpu_timing::end_canvas(&mut self.gpu_marks, &mut encoder);
    queue.submit([encoder.finish()]);
    if let Some(old) = back.artwork.replace(std::sync::Arc::new(Artwork { image })) {
      self.retired_artwork.push(ArtworkLease {
        ready: presentation_pool::completion(queue),
        art: old,
      });
    }
    self.account_presentation(canvas);
  }
  pub(super) fn draw_artwork(
    &mut self,
    device: &Device,
    queue: &Queue,
    canvas: &CanvasHandle,
    matrix: crate::node::transform::Transform2D,
  ) {
    let Some(back) = self.surfaces.get(&canvas.surface_id()) else {
      return;
    };
    let Some(artwork) = &back.artwork else {
      canvas.set_gpu_error(CanvasError::StateLimit);
      return;
    };
    let width = back.image.texture.width();
    let height = back.image.texture.height();
    let data = artwork_vertices(artwork.image.texture.width(), artwork.image.texture.height(), matrix);
    let vertices = self.vertices.write(device, queue, &data).unwrap();
    let globals = global_group(
      device,
      &self.globals_layout,
      self
        .globals
        .write(
          device,
          queue,
          &[0., 0., width as f32, height as f32, width as f32, height as f32, 0., 0.],
        )
        .unwrap(),
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    #[cfg(feature = "perf_profile")]
    gpu_timing::begin_canvas(&mut self.gpu_marks, &mut encoder);
    {
      let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("canvas retained artwork"),
        color_attachments: &[Some(RenderPassColorAttachment {
          view: &back.image.view,
          depth_slice: None,
          resolve_target: None,
          ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
          },
        })],
        ..Default::default()
      });
      pass.set_pipeline(&self.retained);
      pass.set_bind_group(0, &globals, &[0]);
      pass.set_bind_group(1, &artwork.image.linear, &[]);
      pass.set_vertex_buffer(0, vertices.slice(..));
      pass.draw(0..6, 0..1);
    }
    #[cfg(feature = "perf_profile")]
    gpu_timing::end_canvas(&mut self.gpu_marks, &mut encoder);
    canvas.presentation_event(PresentationEvent::Quad);
    queue.submit([encoder.finish()]);
  }
}
