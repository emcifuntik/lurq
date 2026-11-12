use super::*;
impl Renderer {
  pub(super) unsafe fn begin_presentation(
    &mut self,
    state: &mut Dx12State,
    canvas: &CanvasHandle,
    token: u64,
    front_revision: u64,
  ) -> Result<()> {
    self.abort_for_resize(state, canvas);
    let id = canvas.surface_id();
    self.rejected.remove(&id);
    let metrics = canvas.metrics();
    let mut back = if let Some(back) = self.take_spare(state, id, metrics.pixel_width, metrics.pixel_height) {
      canvas.presentation_event(PresentationEvent::Reuse);
      back
    } else {
      if self.spares.iter().filter(|lease| lease.id == id).count() >= 3
        || !self.allocation_admitted(
          state,
          resources::target_allocation_bytes(&state.device, metrics.pixel_width, metrics.pixel_height, false),
        )
      {
        canvas.presentation_event(PresentationEvent::Refuse);
        self.rejected.insert(id);
        if let Some(front) = self.surfaces.get(&id) {
          canvas.retain_presentation_revision(front.revision);
        }
        canvas.set_gpu_error(CanvasError::PresentationBusy);
        return Ok(());
      }
      canvas.presentation_event(PresentationEvent::Allocate);
      Backing {
        bytes: resources::target_allocation_bytes(&state.device, metrics.pixel_width, metrics.pixel_height, false),
        revision: 0,
        artwork: None,
        owner: canvas.downgrade(),
        width: metrics.pixel_width,
        height: metrics.pixel_height,
        texture: match resources::texture(
          &state.device,
          metrics.pixel_width,
          metrics.pixel_height,
          1,
          false,
          D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        ) {
          Ok(texture) => texture,
          Err(_) => {
            canvas.presentation_event(PresentationEvent::Refuse);
            self.rejected.insert(id);
            if let Some(front) = self.surfaces.get(&id) {
              canvas.retain_presentation_revision(front.revision);
            }
            canvas.set_gpu_error(CanvasError::SurfaceTooLarge);
            return Ok(());
          }
        },
      }
    };
    let Some(mut front) = self.surfaces.remove(&id) else {
      self.retire_back(state, id, back);
      return Ok(());
    };
    front.revision = front_revision;
    back.artwork = front.artwork.clone();
    self.surfaces.insert(id, back);
    self.fronts.insert(id, (token, front));
    self.account_presentation(state, canvas);
    Ok(())
  }
  pub(super) unsafe fn end_presentation(
    &mut self,
    state: &mut Dx12State,
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
          self.retire_back(state, id, back);
        }
      } else {
        if let Some(back) = self.surfaces.get_mut(&id) {
          back.revision = revision;
        }
        canvas.presentation_event(PresentationEvent::Commit(token, revision));
        self.retire_back(state, id, front);
      }
      self.account_presentation(state, canvas);
    }
  }
  pub(super) unsafe fn abort_for_resize(&mut self, state: &mut Dx12State, canvas: &CanvasHandle) {
    if let Some((_, front)) = self.frames.end_replacement(&mut self.fronts, canvas.surface_id())
      && let Some(back) = self.surfaces.insert(canvas.surface_id(), front)
    {
      self.retire_back(state, canvas.surface_id(), back);
    }
  }
  pub(super) unsafe fn account_presentation(&self, state: &Dx12State, canvas: &CanvasHandle) {
    canvas.set_gpu_bytes(self.target_bytes());
    canvas.presentation_workspace(self.workspace_bytes(state));
  }
}
