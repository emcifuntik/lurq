//! Submitted texture leases stay charged until the queue reports completion.
use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};

use super::*;
pub(super) struct Spare {
  pub(super) id: CanvasId,
  pub(super) ready: Arc<AtomicBool>,
  back: Backing,
}
pub(super) struct ArtworkLease {
  pub(super) ready: Arc<AtomicBool>,
  pub(super) art: Arc<Artwork>,
}
pub(super) fn completion(queue: &Queue) -> Arc<AtomicBool> {
  let ready = Arc::new(AtomicBool::new(false));
  let signal = ready.clone();
  queue.on_submitted_work_done(move || signal.store(true, Ordering::Release));
  ready
}
impl Renderer {
  pub(super) fn reap(&mut self, live: &HashSet<CanvasId>) {
    self
      .retired_artwork
      .retain(|lease| !lease.ready.load(Ordering::Acquire));
    self
      .spares
      .retain(|lease| live.contains(&lease.id) || !lease.ready.load(Ordering::Acquire));
  }
  pub(super) fn retire_back(&mut self, queue: &Queue, id: CanvasId, mut back: Backing) {
    let ready = completion(queue);
    let owner = back.owner.clone();
    queue.on_submitted_work_done(move || {
      if let Some(canvas) = owner.upgrade() {
        canvas.request_paint();
      }
    });
    if let Some(art) = back.artwork.take() {
      self.retired_artwork.push(ArtworkLease {
        ready: ready.clone(),
        art,
      });
    }
    self.spares.push(Spare { id, ready, back });
  }
  pub(super) fn take_spare(&mut self, id: CanvasId, width: u32, height: u32) -> Option<Backing> {
    self
      .retired_artwork
      .retain(|lease| !lease.ready.load(Ordering::Acquire));
    self.spares.retain(|lease| {
      lease.id != id
        || !lease.ready.load(Ordering::Acquire)
        || (lease.back.image.texture.width() == width && lease.back.image.texture.height() == height)
    });
    let position = self.spares.iter().position(|lease| {
      lease.id == id
        && lease.ready.load(Ordering::Acquire)
        && lease.back.image.texture.width() == width
        && lease.back.image.texture.height() == height
    })?;
    Some(self.spares.swap_remove(position).back)
  }
  pub(super) fn allocation_admitted(&self, bytes: usize) -> bool {
    replacement_admitted(self.target_bytes(), bytes, 32 * 1024 * 1024)
  }
  pub(super) fn workspace_bytes(&self) -> usize {
    let size = |texture: &wgpu::Texture| {
      texture.width() as usize * texture.height() as usize * texture.sample_count() as usize * 4
    };
    size(&self._scratch)
      .saturating_add(size(&self.resolve))
      .saturating_add(size(&self._stencil))
      .saturating_add(self.saved.iter().map(|texture| size(&texture.texture)).sum::<usize>())
      .saturating_add(self.layer.as_ref().map_or(0, |texture| size(&texture.texture)))
  }
  pub(super) fn target_bytes(&self) -> usize {
    let size = |image: &Texture| image.texture.width() as usize * image.texture.height() as usize * 4;
    let mut charge = TargetCharge::default();
    for back in self
      .surfaces
      .values()
      .chain(self.fronts.values().map(|(_, b)| b))
      .chain(self.spares.iter().map(|s| &s.back))
    {
      charge.add(size(&back.image));
      if let Some(art) = &back.artwork {
        charge.shared(Arc::as_ptr(art) as usize, size(&art.image));
      }
    }
    for lease in &self.retired_artwork {
      charge.shared(Arc::as_ptr(&lease.art) as usize, size(&lease.art.image));
    }
    charge.bytes()
  }
}
