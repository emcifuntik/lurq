use std::sync::Arc;

use parking_lot::Mutex;

#[cfg(feature = "devtools")]
use crate::core::signal::Subscription;
use crate::core::{
  signal::{Signal, SignalValue},
  tracking,
};

pub struct Memo<T: SignalValue + Clone + PartialEq + Send + Sync + 'static> {
  output: Signal<T>,
  _subscriptions: Arc<Mutex<Vec<Box<dyn Send + Sync>>>>,
}

impl<T: SignalValue + Clone + PartialEq + Send + Sync + 'static> Memo<T> {
  pub fn new(f: impl Fn() -> T + Send + Sync + 'static) -> Self {
    tracking::start_tracking();
    let initial = f();
    let deps = tracking::stop_tracking();

    let output = Signal::memo_output(initial);
    let subscriptions: Arc<Mutex<Vec<Box<dyn Send + Sync>>>> = Arc::new(Mutex::new(Vec::new()));

    let output_clone = output.clone();
    let compute = Arc::new(f);
    let recompute: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
      let new_val = compute();
      let changed = output_clone.with_untracked(|current| *current != new_val);
      if changed {
        output_clone.set(new_val);
      }
    });

    let mut subs = subscriptions.lock();
    for entry in deps {
      // A detailed recording names what made the memo recompute in the
      // causes its new value sends.
      #[cfg(feature = "perf_profile")]
      let recompute: Arc<dyn Fn() + Send + Sync> = {
        let recompute = recompute.clone();
        let trigger = entry.source;
        Arc::new(move || {
          let _recompute = crate::app::profiler::detail::memo_recompute(trigger);
          recompute();
        })
      };
      let guard = (entry.subscribe_fn)(recompute.clone());
      subs.push(guard);
    }
    drop(subs);

    Self {
      output,
      _subscriptions: subscriptions,
    }
  }

  pub fn get(&self) -> T {
    self.output.get()
  }

  pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
    self.output.with(f)
  }

  pub fn id(&self) -> usize {
    self.output.id()
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn subscribe(&self, sub: impl Fn(&T) + Send + Sync + 'static) -> Subscription<T> {
    self.output.subscribe_debug(sub)
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn devtools_subscriber_count(&self) -> Arc<std::sync::atomic::AtomicUsize> {
    self.output.devtools_subscriber_count()
  }
}

impl<T: SignalValue + Clone + PartialEq + Send + Sync + 'static> Clone for Memo<T> {
  fn clone(&self) -> Self {
    Self {
      output: self.output.clone(),
      _subscriptions: self._subscriptions.clone(),
    }
  }
}
