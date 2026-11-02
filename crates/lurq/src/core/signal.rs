use std::{
  fmt,
  sync::{
    Arc, Weak,
    atomic::{AtomicU64, AtomicUsize, Ordering},
  },
};

use parking_lot::{Mutex, RwLock};

use crate::core::{
  notify::{NotifyLoop, ObserverList},
  tracking,
};

static NEXT_SIGNAL_ID: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "devtools")]
pub trait SignalValue: crate::app::component::DevtoolsInspectable {}

#[cfg(feature = "devtools")]
impl<T: crate::app::component::DevtoolsInspectable> SignalValue for T {}

#[cfg(not(feature = "devtools"))]
pub trait SignalValue {}

#[cfg(not(feature = "devtools"))]
impl<T> SignalValue for T {}

pub type SignalSubscriber<T> = dyn Fn(&T) + Send + Sync + 'static;

type Watcher = Arc<dyn Fn() + Send + Sync>;

type WatcherFn = dyn Fn() + Send + Sync;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SignalObserverKind {
  External,
  Runtime,
  Reactive,
  #[cfg(feature = "devtools")]
  Debug,
}

struct SignalInner<T: SignalValue> {
  id: usize,
  /// Shared so a notification pass can lend the value to subscribers without
  /// holding the lock while they run.
  value: RwLock<Arc<T>>,
  /// Bumped by every write, so a tracked read can tell that the signal was
  /// written before its watcher was subscribed.
  version: AtomicU64,
  /// Held while subscribers borrow the value, so an `update` from another
  /// thread can wait for them to release it.
  delivery: Mutex<()>,
  notify_loop: NotifyLoop,
  next_subscriber_id: AtomicUsize,
  subscribers: ObserverList<SignalObserverKind, SignalSubscriber<T>>,
  watchers: ObserverList<SignalObserverKind, WatcherFn>,
  #[cfg(feature = "devtools")]
  devtools_subscriber_count: Arc<AtomicUsize>,
}

/// A reactive value.
///
/// # Writes from observers
///
/// A notification pass holds no lock while observers run, so a `Ctx::watch`
/// callback, watcher, effect, or memo may read and write the signal it
/// observes and subscribe or unsubscribe observers. A write is applied
/// immediately. Its notification does not start a nested pass: a pass that is
/// already running (from an observer or another thread) first delivers its
/// value to every observer and then repeats with the latest value, so every
/// observer sees the values in the same order. Several writes during one pass
/// coalesce into one repeat. A subscriber added during a pass first hears the
/// next pass; one removed during a pass is not called for the rest of it.
///
/// `set` does not compare values, so an observer that writes on every
/// notification never settles: after 100 consecutive passes re-triggered by
/// the signal's own observers, the write panics instead of looping forever.
/// A `Ctx::watch` callback still borrows the value it was given, so calling
/// [`Signal::update`] on the same signal from it panics; use [`Signal::set`].
pub struct Signal<T: SignalValue> {
  inner: Arc<SignalInner<T>>,
}

impl<T: SignalValue> fmt::Debug for Signal<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_tuple("Signal").field(&self.inner.id).finish()
  }
}

#[must_use = "dropping the subscription immediately unsubscribes it"]
pub struct Subscription<T: SignalValue> {
  id: usize,
  inner: Weak<SignalInner<T>>,
}

#[must_use = "dropping the handle immediately unsubscribes it"]
pub struct WatchHandle<T: SignalValue> {
  id: usize,
  inner: Weak<SignalInner<T>>,
}

impl<T: SignalValue> Clone for Signal<T> {
  fn clone(&self) -> Self {
    Self {
      inner: Arc::clone(&self.inner),
    }
  }
}

impl<T: SignalValue> Signal<T> {
  pub fn new(value: T) -> Self {
    Self {
      inner: Arc::new(SignalInner {
        id: NEXT_SIGNAL_ID.fetch_add(1, Ordering::Relaxed),
        value: RwLock::new(Arc::new(value)),
        version: AtomicU64::new(0),
        delivery: Mutex::new(()),
        notify_loop: NotifyLoop::default(),
        next_subscriber_id: AtomicUsize::new(0),
        subscribers: ObserverList::new(),
        watchers: ObserverList::new(),
        #[cfg(feature = "devtools")]
        devtools_subscriber_count: Arc::new(AtomicUsize::new(0)),
      }),
    }
  }

  pub fn id(&self) -> usize {
    self.inner.id
  }

  pub fn get(&self) -> T
  where
    T: Clone + Send + Sync + 'static,
  {
    self.track_access();
    T::clone(&self.inner.value.read())
  }

  pub fn get_untracked(&self) -> T
  where
    T: Clone + Send + Sync + 'static,
  {
    T::clone(&self.inner.value.read())
  }

  pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R
  where
    T: Send + Sync + 'static,
  {
    self.track_access();
    let value = self.inner.value.read();
    f(&value)
  }

  /// How many times the value was replaced or mutated.
  #[cfg_attr(not(feature = "markdown"), allow(dead_code))]
  pub(crate) fn version(&self) -> u64 {
    self.inner.version.load(Ordering::Acquire)
  }

  pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R
  where
    T: Send + Sync + 'static,
  {
    let value = self.inner.value.read();
    f(&value)
  }

  /// Replaces the value and notifies observers.
  ///
  /// Observers of this signal may call `set` too; see
  /// [writes from observers](Signal#writes-from-observers).
  pub fn set(&self, value: T) {
    {
      let mut current = self.inner.value.write();
      match Arc::get_mut(&mut current) {
        Some(slot) => *slot = value,
        // Subscribers of a running notification pass still borrow the old value.
        None => *current = Arc::new(value),
      }
    }
    self.inner.version.fetch_add(1, Ordering::AcqRel);
    self.notify();
  }

  /// Mutates the value in place and notifies observers.
  ///
  /// Works from watchers, effects, and `Ctx::watch` callbacks of other
  /// signals. A `Ctx::watch` callback of this same signal still borrows the
  /// value it was given, so calling `update` there panics; call `set` with a
  /// value derived from the callback argument instead.
  pub fn update(&self, f: impl FnOnce(&mut T)) {
    let mut current = self.inner.value.write();
    if let Some(value) = Arc::get_mut(&mut current) {
      f(value);
      drop(current);
    } else {
      drop(current);
      assert!(
        !self.inner.notify_loop.is_running_on_this_thread(),
        "Signal::update was called on signal {} from one of its own `Ctx::watch` callbacks, which still borrows the \
         value being delivered; call `set` with a value derived from the callback argument instead",
        self.inner.id
      );
      // Another thread is delivering this signal. Its subscribers release the
      // value before `delivery` is unlocked, and no new pass can lend it while
      // this thread holds `delivery`.
      let _delivery = self.inner.delivery.lock();
      let mut current = self.inner.value.write();
      f(Arc::get_mut(&mut current).expect("only a pass holding `delivery` lends the value"));
    }
    self.inner.version.fetch_add(1, Ordering::AcqRel);
    self.notify();
  }

  pub(crate) fn subscribe(&self, sub: impl Fn(&T) + Send + Sync + 'static) -> Subscription<T> {
    self.subscribe_with_kind(sub, SignalObserverKind::External)
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn subscribe_debug(&self, sub: impl Fn(&T) + Send + Sync + 'static) -> Subscription<T> {
    self.subscribe_with_kind(sub, SignalObserverKind::Debug)
  }

  fn subscribe_with_kind(&self, sub: impl Fn(&T) + Send + Sync + 'static, kind: SignalObserverKind) -> Subscription<T> {
    let id = self.inner.next_subscriber_id.fetch_add(1, Ordering::Relaxed);
    self.inner.subscribers.add(id, kind, Arc::new(sub));
    self.inner.refresh_devtools_subscriber_count();
    Subscription {
      id,
      inner: Arc::downgrade(&self.inner),
    }
  }

  pub(crate) fn watch(&self, f: impl Fn() + Send + Sync + 'static) -> WatchHandle<T> {
    let id = self.inner.next_subscriber_id.fetch_add(1, Ordering::Relaxed);
    self.inner.watchers.add(id, SignalObserverKind::Runtime, Arc::new(f));
    self.inner.refresh_devtools_subscriber_count();
    WatchHandle {
      id,
      inner: Arc::downgrade(&self.inner),
    }
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn devtools_subscriber_count(&self) -> Arc<AtomicUsize> {
    self.inner.devtools_subscriber_count.clone()
  }

  fn track_access(&self)
  where
    T: Send + Sync + 'static,
  {
    if tracking::is_tracking() {
      let weak = Arc::downgrade(&self.inner);
      let signal_id = self.inner.id;
      let version_read = self.inner.version.load(Ordering::Acquire);
      tracking::track(
        signal_id,
        Box::new(move |watcher: Watcher| {
          if let Some(inner) = weak.upgrade() {
            let id = inner.next_subscriber_id.fetch_add(1, Ordering::Relaxed);
            inner
              .watchers
              .add(id, SignalObserverKind::Reactive, Arc::clone(&watcher));
            inner.refresh_devtools_subscriber_count();
            // The reader saw an older value if the signal was written before
            // this watcher existed, for example by the reader itself. A pending
            // repeat pass will reach the watcher; otherwise catch it up now.
            if inner.version.load(Ordering::Acquire) != version_read && !inner.notify_loop.has_pending_pass() {
              watcher();
            }
            Box::new(DropGuard { id, inner: weak })
          } else {
            Box::new(())
          }
        }),
      );
    }
  }

  /// Delivers the current value to subscribers, then calls watchers; see
  /// [writes from observers](Signal#writes-from-observers).
  fn notify(&self) {
    let inner = &*self.inner;
    inner.notify_loop.run("signal", inner.id, || inner.notify_pass());
  }
}

impl<T: SignalValue> SignalInner<T> {
  fn notify_pass(&self) {
    if self.subscribers.has_observers() {
      let _delivery = self.delivery.lock();
      let value = Arc::clone(&self.value.read());
      self.subscribers.for_each(|subscriber| subscriber(&value));
    }
    self.watchers.for_each(|watcher| watcher());
  }

  #[cfg(feature = "devtools")]
  fn refresh_devtools_subscriber_count(&self) {
    let external_subscribers = self.subscribers.count(|kind| kind == SignalObserverKind::External);
    let reactive_watchers = self.watchers.count(|kind| kind == SignalObserverKind::Reactive);
    self
      .devtools_subscriber_count
      .store(external_subscribers + reactive_watchers, Ordering::Relaxed);
  }

  #[cfg(not(feature = "devtools"))]
  fn refresh_devtools_subscriber_count(&self) {}
}

struct DropGuard<T: SignalValue> {
  id: usize,
  inner: Weak<SignalInner<T>>,
}

impl<T: SignalValue> Drop for DropGuard<T> {
  fn drop(&mut self) {
    if let Some(inner) = self.inner.upgrade() {
      inner.watchers.remove(self.id);
      inner.refresh_devtools_subscriber_count();
    }
  }
}

impl<T: SignalValue> Drop for Subscription<T> {
  fn drop(&mut self) {
    if let Some(inner) = self.inner.upgrade() {
      inner.subscribers.remove(self.id);
      inner.refresh_devtools_subscriber_count();
    }
  }
}

impl<T: SignalValue> Drop for WatchHandle<T> {
  fn drop(&mut self) {
    if let Some(inner) = self.inner.upgrade() {
      inner.watchers.remove(self.id);
      inner.refresh_devtools_subscriber_count();
    }
  }
}

impl<T: SignalValue> From<T> for Signal<T> {
  fn from(value: T) -> Self {
    Self::new(value)
  }
}
