use std::{cell::RefCell, sync::Arc};

pub type SubscribeFn = Box<dyn FnOnce(Arc<dyn Fn() + Send + Sync>) -> Box<dyn Send + Sync>>;

pub struct TrackingEntry {
  pub signal_id: usize,
  pub subscribe_fn: SubscribeFn,
  /// What a detailed recording names a notification of this dependency.
  #[cfg(feature = "perf_profile")]
  pub(crate) source: crate::app::profiler::RenderSource,
}

thread_local! {
  static TRACKING: RefCell<Vec<Vec<TrackingEntry>>> = const { RefCell::new(Vec::new()) };
}

pub fn start_tracking() {
  TRACKING.with(|t| {
    t.borrow_mut().push(Vec::new());
  });
}

pub fn stop_tracking() -> Vec<TrackingEntry> {
  TRACKING.with(|t| t.borrow_mut().pop().unwrap_or_default())
}

pub fn is_tracking() -> bool {
  TRACKING.with(|t| !t.borrow().is_empty())
}

pub fn track(signal_id: usize, subscribe_fn: SubscribeFn) {
  track_entry(TrackingEntry {
    signal_id,
    subscribe_fn,
    #[cfg(feature = "perf_profile")]
    source: crate::app::profiler::RenderSource {
      trigger: crate::app::profiler::RenderTrigger::Signal,
      value_type: None,
    },
  });
}

pub(crate) fn track_entry(entry: TrackingEntry) {
  TRACKING.with(|t| {
    let mut borrow = t.borrow_mut();
    if let Some(entries) = borrow.last_mut()
      && !entries.iter().any(|e| e.signal_id == entry.signal_id)
    {
      entries.push(entry);
    }
  });
}
