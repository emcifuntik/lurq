use super::*;
impl Renderer {
  pub(super) fn begin_presentation(
    &mut self,
    device: &Device,
    queue: &Queue,
    canvas: &CanvasHandle,
    token: u64,
    front_revision: u64,
  ) {
    let id = canvas.surface_id();
    self.abort_for_resize(queue, canvas);
    self.rejected.remove(&id);
    let metrics = canvas.metrics();
    if metrics.pixel_width > device.limits().max_texture_dimension_2d
      || metrics.pixel_height > device.limits().max_texture_dimension_2d
    {
      self.rejected.insert(id);
      if let Some(front) = self.surfaces.get(&id) {
        canvas.retain_presentation_revision(front.revision);
      }
      canvas.set_gpu_error(CanvasError::SurfaceTooLarge);
      return;
    }
    let Some(mut back) = self
      .take_spare(id, metrics.pixel_width, metrics.pixel_height)
      .inspect(|_| {
        canvas.presentation_event(PresentationEvent::Reuse);
      })
      .or_else(|| {
        if metrics.pixel_width > device.limits().max_texture_dimension_2d
          || metrics.pixel_height > device.limits().max_texture_dimension_2d
        {
          return None;
        }
        if self.spares.iter().filter(|lease| lease.id == id).count() >= 3
          || !self.allocation_admitted(metrics.pixel_width as usize * metrics.pixel_height as usize * 4)
        {
          return None;
        }
        canvas.presentation_event(PresentationEvent::Allocate);
        Some(Backing {
          revision: 0,
          artwork: None,
          owner: canvas.downgrade(),
          image: Texture::new(
            device,
            &self.image_layout,
            &self.nearest,
            &self.linear,
            metrics.pixel_width,
            metrics.pixel_height,
          ),
          generation: 0,
        })
      })
    else {
      canvas.presentation_event(PresentationEvent::Refuse);
      self.rejected.insert(id);
      if let Some(front) = self.surfaces.get(&id) {
        canvas.retain_presentation_revision(front.revision);
      }
      canvas.set_gpu_error(CanvasError::PresentationBusy);
      return;
    };
    let Some(mut front) = self.surfaces.remove(&id) else {
      self.retire_back(queue, id, back);
      return;
    };
    front.revision = front_revision;
    back.artwork = front.artwork.clone();
    self.surfaces.insert(id, back);
    self.fronts.insert(id, (token, front));
    self.account_presentation(canvas);
  }
  pub(super) fn end_presentation(
    &mut self,
    queue: &Queue,
    canvas: &CanvasHandle,
    token: u64,
    commit: bool,
    revision: u64,
  ) {
    let id = canvas.surface_id();
    if !canvas.presentation_current(token) {
      return;
    }
    // A rejected transaction must keep rejecting its later draw commands.
    // Only the next Begin clears rejection and admits a new replacement.
    if self.rejected.contains(&id) {
      return;
    }
    if self.fronts.get(&id).is_some_and(|(current, _)| *current == token) {
      let (_, front) = self.frames.end_replacement(&mut self.fronts, id).unwrap();
      if !commit || canvas.status().error.is_some() {
        if let Some(back) = self.surfaces.insert(id, front) {
          self.retire_back(queue, id, back);
        }
      } else {
        canvas.presentation_event(PresentationEvent::Commit(token, revision));
        self.generation += 1;
        if let Some(back) = self.surfaces.get_mut(&id) {
          back.generation = self.generation;
          back.revision = revision;
        }
        self.retire_back(queue, id, front);
      }
      self.account_presentation(canvas);
    }
  }
  pub(super) fn abort_for_resize(&mut self, queue: &Queue, canvas: &CanvasHandle) {
    if let Some((_, front)) = self.frames.end_replacement(&mut self.fronts, canvas.surface_id())
      && let Some(back) = self.surfaces.insert(canvas.surface_id(), front)
    {
      self.retire_back(queue, canvas.surface_id(), back);
    }
  }
  pub(super) fn account_presentation(&self, canvas: &CanvasHandle) {
    canvas.set_gpu_bytes(self.target_bytes());
    canvas.presentation_workspace(self.workspace_bytes());
  }
}
