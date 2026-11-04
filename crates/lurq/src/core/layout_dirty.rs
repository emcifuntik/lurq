use std::sync::{
  Arc, Weak,
  atomic::{AtomicU64, Ordering},
};

use parking_lot::Mutex;

static GENERATION: AtomicU64 = AtomicU64::new(1);

/// A node's subtree revision. Mutations notify its ancestors immediately;
/// inspecting or laying out a subtree never consumes an ancestor's revision.
pub(crate) struct LayoutWatch {
  generation: AtomicU64,
  local_generation: AtomicU64,
  parent: Mutex<Weak<LayoutWatch>>,
}

impl LayoutWatch {
  pub(crate) fn new() -> Arc<Self> {
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    Arc::new(Self {
      generation: AtomicU64::new(generation),
      local_generation: AtomicU64::new(generation),
      parent: Mutex::new(Weak::new()),
    })
  }

  pub(crate) fn generation(&self) -> u64 {
    self.generation.load(Ordering::Relaxed)
  }

  pub(crate) fn local_changed_since(&self, generation: u64) -> bool {
    self.local_generation.load(Ordering::Relaxed) > generation
  }

  pub(crate) fn mark(&self) {
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    self.local_generation.fetch_max(generation, Ordering::Relaxed);
    self.mark_generation(generation);
  }

  pub(crate) fn mark_descendant(&self) {
    self.mark_generation(GENERATION.fetch_add(1, Ordering::Relaxed));
  }

  fn mark_generation(&self, generation: u64) {
    #[cfg(test)]
    NOTIFIED.with(|notified| notified.set(notified.get() + 1));
    self.generation.fetch_max(generation, Ordering::Relaxed);
    let parent = self.parent.lock().upgrade();
    if let Some(parent) = parent {
      parent.mark_generation(generation);
    }
  }

  /// Makes `parent` the watch this one notifies. Returns whether it is a new
  /// parent: the caller then marks `parent` with
  /// [`Self::mark_descendant`], once for all the children it attaches, so a
  /// first layout does not notify the ancestors once per child.
  #[must_use]
  pub(crate) fn set_parent(&self, parent: &Arc<Self>) -> bool {
    let mut current = self.parent.lock();
    if std::ptr::eq(current.as_ptr(), Arc::as_ptr(parent)) {
      return false;
    }
    *current = Arc::downgrade(parent);
    true
  }

  /// Forgets the parent, as a new watch has none: for a watch a rebuilt node
  /// takes over, so attaching it to its new parent notifies that parent as
  /// attaching a new one would.
  pub(crate) fn detach_parent(&self) {
    *self.parent.lock() = Weak::new();
  }
}

/// Shared handles may be bound to more than one live node during a rebuild
/// or overlay measurement. Notify every live binding; a clone must not steal
/// the original node's subscription.
#[derive(Default)]
pub(crate) struct LayoutSubscribers {
  watches: Mutex<Vec<Weak<LayoutWatch>>>,
}

impl LayoutSubscribers {
  pub(crate) fn bind(&self, watch: &Arc<LayoutWatch>) {
    let mut watches = self.watches.lock();
    watches.retain(|current| current.strong_count() != 0);
    let weak = Arc::downgrade(watch);
    if !watches.iter().any(|current| current.ptr_eq(&weak)) {
      watches.push(weak);
    }
  }

  pub(crate) fn mark(&self) {
    let watches = self.watches.lock();
    for watch in watches.iter().filter_map(Weak::upgrade) {
      watch.mark();
    }
  }
}

#[cfg(test)]
thread_local! {
  pub(crate) static VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
  pub(crate) static LAYOUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
  /// Watches notified of a mark, each ancestor a mark reaches counted.
  pub(crate) static NOTIFIED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn visit() {
  VISITS.with(|visits| visits.set(visits.get() + 1));
}
