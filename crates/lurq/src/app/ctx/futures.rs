//! Futures, streams and future actions of a component: their states, handles,
//! tasks, and the `Ctx` methods that create them.

#[cfg(feature = "tokio")]
use std::sync::mpsc::{self, TryRecvError};
use std::{
  any::Any,
  future::Future,
  pin::Pin,
  sync::Arc,
  task::{Context as TaskContext, Poll, Wake, Waker},
};

use parking_lot::Mutex;

use super::Ctx;
use crate::{
  app::window::WindowWaker,
  core::signal::{Signal, SignalValue},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, crate::DevtoolsInspectable)]
pub enum FutureStatus {
  Idle,
  Pending,
  Fulfilled,
  Rejected,
}

/// The state of a future, stream or future action.
///
/// `data` is the last value that succeeded, not the result of the latest run:
/// it is kept while a new run is `Pending` and after that run is `Rejected`, so
/// a view can keep showing it. "Has data and is not pending" therefore does not
/// mean the latest run succeeded. Ask [`outcome`](Self::outcome) or
/// [`fulfilled_data`](Self::fulfilled_data) for that, or match on `status`.
#[derive(Clone, Debug, PartialEq)]
pub struct FutureState<T, E> {
  pub status: FutureStatus,
  /// The last value that succeeded: set when a run is `Fulfilled` (or a stream
  /// emits), kept while a later run is `Pending` and after it is `Rejected`.
  pub data: Option<T>,
  /// The error of the latest run while `status` is `Rejected`; `None` otherwise.
  pub error: Option<E>,
}

impl<T, E> crate::app::component::DevtoolsInspectable for FutureState<T, E>
where
  T: crate::app::component::DevtoolsInspectable,
  E: crate::app::component::DevtoolsInspectable,
{
  fn write_info(&self, buffer: &mut Vec<crate::app::component::ComponentInfo>) {
    let mut children = Vec::new();
    crate::app::component::DevtoolsInspectable::write_info(&self.status, &mut children);
    crate::app::component::DevtoolsInspectable::write_info(&self.data, &mut children);
    crate::app::component::DevtoolsInspectable::write_info(&self.error, &mut children);
    buffer.push(crate::app::component::ComponentInfo::with_children(
      "FutureState",
      std::any::type_name::<Self>(),
      children,
    ));
  }
}

impl<T, E> FutureState<T, E> {
  pub fn idle() -> Self {
    Self {
      status: FutureStatus::Idle,
      data: None,
      error: None,
    }
  }

  pub fn pending(data: Option<T>) -> Self {
    Self {
      status: FutureStatus::Pending,
      data,
      error: None,
    }
  }

  pub fn fulfilled(data: T) -> Self {
    Self {
      status: FutureStatus::Fulfilled,
      data: Some(data),
      error: None,
    }
  }

  /// A rejected state; `data` is the last value that succeeded, which it keeps.
  pub fn rejected(error: E, data: Option<T>) -> Self {
    Self {
      status: FutureStatus::Rejected,
      data,
      error: Some(error),
    }
  }

  pub fn is_idle(&self) -> bool {
    self.status == FutureStatus::Idle
  }

  pub fn is_pending(&self) -> bool {
    self.status == FutureStatus::Pending
  }

  pub fn is_fulfilled(&self) -> bool {
    self.status == FutureStatus::Fulfilled
  }

  pub fn is_rejected(&self) -> bool {
    self.status == FutureStatus::Rejected
  }

  /// How the latest run ended: `Ok` with its value when it was fulfilled,
  /// `Err` with its error when it was rejected, and `None` while nothing has
  /// run (`Idle`) or a run is `Pending`. Unlike `data`, a rejection after an
  /// earlier success is an `Err`.
  pub fn outcome(&self) -> Option<Result<&T, &E>> {
    match self.status {
      FutureStatus::Fulfilled => self.data.as_ref().map(Ok),
      FutureStatus::Rejected => self.error.as_ref().map(Err),
      FutureStatus::Idle | FutureStatus::Pending => None,
    }
  }

  /// The value of the latest run if it was fulfilled; `None` while idle or
  /// pending and after a rejection, even when `data` still holds an earlier
  /// value.
  pub fn fulfilled_data(&self) -> Option<&T> {
    self.outcome().and_then(Result::ok)
  }
}

pub struct FutureHandle<T: SignalValue, E: SignalValue> {
  state: Signal<FutureState<T, E>>,
  task: AsyncTask,
}

pub struct StreamHandle<T: SignalValue, E: SignalValue> {
  state: Signal<FutureState<T, E>>,
  task: AsyncTask,
}

#[derive(Clone)]
pub struct StreamEmitter<T: SignalValue, E: SignalValue> {
  state: Signal<FutureState<T, E>>,
  #[cfg(feature = "tokio")]
  sender: Option<CompletionSender>,
}

pub struct FutureAction<A, T: SignalValue, E: SignalValue> {
  state: Signal<FutureState<T, E>>,
  task: AsyncTask,
  runner: Arc<Mutex<ActionRunner<A, T, E>>>,
  runtime_handle: RuntimeFutureHandle,
}

type BoxFutureResult<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;
type ActionRunner<A, T, E> = Arc<dyn Fn(A) -> BoxFutureResult<T, E> + Send + Sync>;
#[cfg(feature = "tokio")]
type FutureCompletion = Box<dyn FnOnce() + Send>;

/// Hands completions of work on a Tokio runtime to the event loop, and wakes the loop for each, so that it can
/// wait instead of polling the channel.
#[cfg(feature = "tokio")]
#[derive(Clone)]
struct CompletionSender {
  sender: mpsc::Sender<FutureCompletion>,
  waker: Waker,
}

#[cfg(feature = "tokio")]
impl CompletionSender {
  /// Whether the completion was handed over: `false` once the task's receiving end is gone.
  fn send(&self, completion: FutureCompletion) -> bool {
    let sent = self.sender.send(completion).is_ok();
    if sent {
      self.waker.wake_by_ref();
    }
    sent
  }
}
#[cfg(feature = "tokio")]
pub(super) type RuntimeFutureHandle = Option<tokio::runtime::Handle>;
#[cfg(not(feature = "tokio"))]
pub(super) type RuntimeFutureHandle = ();

#[derive(Clone)]
pub(super) struct AsyncTask {
  inner: Arc<Mutex<AsyncTaskInner>>,
  /// Wakes the event loop when the task gets work to poll, and when work on a Tokio runtime hands over a
  /// result. A future polled by `tick_futures` gets the loop's waker from there.
  waker: Waker,
}

#[derive(Default)]
struct AsyncTaskInner {
  work: AsyncWork,
  /// Set when the slot that owns the task is dropped: the task never starts
  /// work again, even through a handle that outlived its component.
  closed: Option<TaskClosed>,
}

/// Why a task's slot was dropped.
#[derive(Clone, Copy, Debug)]
enum TaskClosed {
  /// Its component unmounted.
  Unmounted,
  /// A render of `component` no longer reached the slot's position, or made a
  /// different call there, while the component stayed mounted.
  DroppedByRender { component: &'static str },
}

/// The work a task is running. Dropping it cancels the work.
#[derive(Default)]
struct AsyncWork {
  future: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
  #[cfg(feature = "tokio")]
  tokio_task: Option<TokioAsyncTask>,
}

/// A `ctx.future`, `ctx.stream` or `ctx.future_action` slot of a component.
/// Its task lives as long as the slot: dropping the slot (its component
/// unmounts, or a render no longer reaches it) cancels the task.
pub(super) struct FutureSlot {
  deps: Option<Box<dyn Any + Send + Sync>>,
  handle: Box<dyn Any + Send + Sync>,
  pub(super) task: AsyncTask,
}

impl FutureSlot {
  /// Drops a positional slot that a render of the still mounted `component` no longer reached.
  fn drop_while_mounted(self, component: &'static str) {
    self.task.close(TaskClosed::DroppedByRender { component });
  }
}

impl Drop for FutureSlot {
  fn drop(&mut self) {
    // A slot dropped in any other way goes with its component.
    self.task.close(TaskClosed::Unmounted);
  }
}

#[cfg(feature = "tokio")]
struct TokioAsyncTask {
  join: tokio::task::JoinHandle<()>,
  receiver: mpsc::Receiver<FutureCompletion>,
  finish_on_message: bool,
}

/// Dropping a `JoinHandle` only detaches its task, so the task is aborted
/// explicitly. `abort` only marks the task cancelled and schedules it: the
/// runtime drops the future on its own threads, and on a runtime that is
/// shutting down or gone the call does nothing (shutdown drops the future).
#[cfg(feature = "tokio")]
impl Drop for TokioAsyncTask {
  fn drop(&mut self) {
    self.join.abort();
  }
}

struct NoopWake;

impl Wake for NoopWake {
  fn wake(self: Arc<Self>) {}
}

/// Wakes the event loop of a window.
struct LoopWake(WindowWaker);

impl Wake for LoopWake {
  fn wake(self: Arc<Self>) {
    (self.0)();
  }

  fn wake_by_ref(self: &Arc<Self>) {
    (self.0)();
  }
}

impl<T: SignalValue, E: SignalValue> Clone for FutureHandle<T, E> {
  fn clone(&self) -> Self {
    Self {
      state: self.state.clone(),
      task: self.task.clone(),
    }
  }
}

impl<T: SignalValue, E: SignalValue> Clone for StreamHandle<T, E> {
  fn clone(&self) -> Self {
    Self {
      state: self.state.clone(),
      task: self.task.clone(),
    }
  }
}

impl<A, T: SignalValue, E: SignalValue> Clone for FutureAction<A, T, E> {
  fn clone(&self) -> Self {
    Self {
      state: self.state.clone(),
      task: self.task.clone(),
      runner: self.runner.clone(),
      runtime_handle: self.runtime_handle.clone(),
    }
  }
}

impl<T, E> StreamEmitter<T, E>
where
  T: SignalValue + Clone + PartialEq + Send + Sync + 'static,
  E: SignalValue + Clone + PartialEq + Send + Sync + 'static,
{
  pub fn emit(&self, data: T) -> bool {
    #[cfg(feature = "tokio")]
    if let Some(sender) = &self.sender {
      let state = self.state.clone();
      return sender.send(Box::new(move || state.set(FutureState::fulfilled(data))));
    }

    self.state.set(FutureState::fulfilled(data));
    true
  }

  pub fn reject(&self, error: E) -> bool {
    #[cfg(feature = "tokio")]
    if let Some(sender) = &self.sender {
      let state = self.state.clone();
      return sender.send(Box::new(move || {
        let previous_data = state.get_untracked().data;
        state.set(FutureState::rejected(error, previous_data));
      }));
    }

    let previous_data = self.state.get_untracked().data;
    self.state.set(FutureState::rejected(error, previous_data));
    true
  }
}

impl<T: SignalValue, E: SignalValue> FutureHandle<T, E> {
  pub fn state(&self) -> Signal<FutureState<T, E>> {
    self.state.clone()
  }

  pub fn cancel(&self) {
    self.task.cancel();
  }

  pub fn is_active(&self) -> bool {
    self.task.is_active()
  }
}

impl<T: SignalValue, E: SignalValue> StreamHandle<T, E> {
  pub fn state(&self) -> Signal<FutureState<T, E>> {
    self.state.clone()
  }

  pub fn cancel(&self) {
    self.task.cancel();
  }

  pub fn is_active(&self) -> bool {
    self.task.is_active()
  }
}

impl<A: Send + Sync + 'static, T, E> FutureAction<A, T, E>
where
  T: SignalValue + Clone + PartialEq + Send + Sync + 'static,
  E: SignalValue + Clone + PartialEq + Send + Sync + 'static,
{
  pub fn state(&self) -> Signal<FutureState<T, E>> {
    self.state.clone()
  }

  /// Starts the action with `args`. A run still in flight is cancelled and
  /// replaced: its result is never applied, so a second click on a button that
  /// runs a save aborts the first save. Use [`run_if_idle`](Self::run_if_idle)
  /// to ignore a run while one is in flight.
  ///
  /// Does nothing once the component that created the action has unmounted,
  /// and logs an error when a render dropped the action (see
  /// [`Ctx::future_action`]).
  pub fn run(&self, args: A) {
    if self.startable() {
      self.start(args);
    }
  }

  /// Starts the action with `args` unless a run is in flight, and returns
  /// whether it started. The run in flight is left alone, so a double click
  /// starts one run. Returns `false`, and starts nothing, after the action's
  /// component has unmounted or a render dropped the action, as [`run`](Self::run).
  pub fn run_if_idle(&self, args: A) -> bool {
    if self.task.is_active() || !self.startable() {
      return false;
    }
    self.start(args);
    true
  }

  fn start(&self, args: A) {
    let runner = self.runner.lock().clone();
    let future = runner(args);
    start_future_task(
      self.state.clone(),
      self.task.clone(),
      self.runtime_handle.clone(),
      future,
    );
  }

  pub fn cancel(&self) {
    self.task.cancel();
  }

  /// Whether a run is in flight: started, and its result not applied yet.
  pub fn is_active(&self) -> bool {
    self.task.is_active()
  }

  /// Whether a run may start. An action dropped by a render of its still
  /// mounted component is a bug in that component, so running it is logged.
  fn startable(&self) -> bool {
    match self.task.closed() {
      None => true,
      Some(TaskClosed::Unmounted) => false,
      Some(TaskClosed::DroppedByRender { component }) => {
        tracing::error!(
          component,
          "FutureAction::run on an action that a render of {component} dropped: the render no longer made the            same ctx.future/ctx.stream/ctx.future_action call at its position. The run does nothing. Create the            action in `create`, or make these calls unconditionally and in the same order in every render."
        );
        false
      }
    }
  }
}

impl AsyncTask {
  fn new(waker: Waker) -> Self {
    Self {
      inner: Arc::new(Mutex::new(AsyncTaskInner::default())),
      waker,
    }
  }

  /// A channel for completions of work on a Tokio runtime that wakes the event loop for each.
  #[cfg(feature = "tokio")]
  fn completion_channel(&self) -> (CompletionSender, mpsc::Receiver<FutureCompletion>) {
    let (sender, receiver) = mpsc::channel();
    let sender = CompletionSender {
      sender,
      waker: self.waker.clone(),
    };
    (sender, receiver)
  }

  /// Replaces the running work with a future polled by `tick_futures`, and wakes the event loop to poll it: the
  /// work may be started from another thread, while the loop waits.
  fn set(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
    let work = AsyncWork {
      future: Some(future),
      #[cfg(feature = "tokio")]
      tokio_task: None,
    };
    self.replace_work(work);
    self.waker.wake_by_ref();
  }

  /// Replaces the running work with `future` spawned on `runtime`.
  ///
  /// The task is spawned before the lock is taken: on a runtime that is gone,
  /// `spawn` drops the future on this thread, and its destructors may use a
  /// handle of this task. A `close` that lands between the spawn and the lock
  /// is seen by `replace_work`, which then aborts the new task.
  #[cfg(feature = "tokio")]
  fn spawn(
    &self,
    runtime: &tokio::runtime::Handle,
    future: impl Future<Output = ()> + Send + 'static,
    receiver: mpsc::Receiver<FutureCompletion>,
    finish_on_message: bool,
  ) {
    if self.is_closed() {
      return;
    }
    let spawned = TokioAsyncTask {
      join: runtime.spawn(future),
      receiver,
      finish_on_message,
    };
    self.replace_work(AsyncWork {
      future: None,
      tokio_task: Some(spawned),
    });
  }

  /// Makes `work` the running work, unless the task is closed. Whichever work
  /// ends up unused (the replaced one, or `work` itself on a closed task) is
  /// dropped once the lock is released: dropping it runs user destructors,
  /// which may use a handle of this task.
  fn replace_work(&self, work: AsyncWork) {
    let unused = {
      let mut inner = self.inner.lock();
      if inner.closed.is_some() {
        work
      } else {
        std::mem::replace(&mut inner.work, work)
      }
    };
    drop(unused);
  }

  fn cancel(&self) {
    // Dropped once the lock is released: dropping a future runs its destructors.
    let cancelled = std::mem::take(&mut self.inner.lock().work);
    drop(cancelled);
  }

  /// Cancels the running work and keeps the task from starting again. The
  /// first reason given is kept.
  fn close(&self, reason: TaskClosed) {
    let cancelled = {
      let mut inner = self.inner.lock();
      inner.closed.get_or_insert(reason);
      std::mem::take(&mut inner.work)
    };
    drop(cancelled);
  }

  fn closed(&self) -> Option<TaskClosed> {
    self.inner.lock().closed
  }

  fn is_closed(&self) -> bool {
    self.closed().is_some()
  }

  pub(super) fn is_active(&self) -> bool {
    let inner = self.inner.lock();
    inner.work.future.is_some() || {
      #[cfg(feature = "tokio")]
      {
        inner.work.tokio_task.is_some()
      }
      #[cfg(not(feature = "tokio"))]
      {
        false
      }
    }
  }

  pub(super) fn poll(&self, cx: &mut TaskContext<'_>) -> bool {
    // Release the task lock before polling: a completing future sets its state
    // signal, and an observer of that signal may start this task again.
    let future = self.inner.lock().work.future.take();
    if let Some(mut future) = future {
      match future.as_mut().poll(cx) {
        Poll::Ready(()) => return true,
        Poll::Pending => {
          let mut inner = self.inner.lock();
          if inner.closed.is_none() && inner.work.future.is_none() {
            inner.work.future = Some(future);
          }
          return false;
        }
      }
    }

    #[cfg(feature = "tokio")]
    {
      let mut completion = None;
      let mut disconnected = false;
      let mut finished = None;
      {
        let mut inner = self.inner.lock();
        if let Some(task) = inner.work.tokio_task.as_mut() {
          match task.receiver.try_recv() {
            Ok(received) => {
              if task.finish_on_message {
                disconnected = true;
              }
              completion = Some(received);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => disconnected = true,
          }
        }
        if disconnected {
          finished = inner.work.tokio_task.take();
        }
      }
      drop(finished);

      if let Some(completion) = completion {
        completion();
        // One completion per turn, and the wakes of several handed over before a turn make one turn: wake the
        // loop again so that a completion still queued gets a turn of its own.
        self.waker.wake_by_ref();
        return true;
      }
    }

    false
  }
}

impl Ctx {
  /// Wakes this component's window's event loop; does nothing outside a window.
  pub(super) fn loop_waker(&self) -> Waker {
    match &self.window {
      Some(window) => Waker::from(Arc::new(LoopWake(window.loop_waker()))),
      None => Waker::from(Arc::new(NoopWake)),
    }
  }
}

fn start_future_task<T, E>(
  state: Signal<FutureState<T, E>>,
  task: AsyncTask,
  runtime_handle: RuntimeFutureHandle,
  future: BoxFutureResult<T, E>,
) where
  T: SignalValue + Clone + PartialEq + Send + Sync + 'static,
  E: SignalValue + Clone + PartialEq + Send + Sync + 'static,
{
  let previous_data = state.get_untracked().data;
  state.set(FutureState::pending(previous_data));

  #[cfg(feature = "tokio")]
  if let Some(handle) = runtime_handle {
    let completion_state = state.clone();
    let (sender, receiver) = task.completion_channel();
    let completion_task = async move {
      let result = future.await;
      let completion: FutureCompletion = Box::new(move || match result {
        Ok(data) => completion_state.set(FutureState::fulfilled(data)),
        Err(error) => {
          let previous_data = completion_state.get_untracked().data;
          completion_state.set(FutureState::rejected(error, previous_data));
        }
      });
      // `false` when the task was cancelled or replaced: its result is no longer wanted.
      sender.send(completion);
    };
    task.spawn(&handle, completion_task, receiver, true);
    return;
  }

  #[cfg(not(feature = "tokio"))]
  let _ = runtime_handle;

  let completion_state = state.clone();
  task.set(Box::pin(async move {
    match future.await {
      Ok(data) => completion_state.set(FutureState::fulfilled(data)),
      Err(error) => {
        let previous_data = completion_state.get_untracked().data;
        completion_state.set(FutureState::rejected(error, previous_data));
      }
    }
  }));
}

fn start_stream_task<T, E, Fut>(
  state: Signal<FutureState<T, E>>,
  task: AsyncTask,
  runtime_handle: RuntimeFutureHandle,
  factory: impl FnOnce(StreamEmitter<T, E>) -> Fut,
) where
  T: SignalValue + Clone + PartialEq + Send + Sync + 'static,
  E: SignalValue + Clone + PartialEq + Send + Sync + 'static,
  Fut: Future<Output = ()> + Send + 'static,
{
  let previous_data = state.get_untracked().data;
  state.set(FutureState::pending(previous_data));

  #[cfg(feature = "tokio")]
  if let Some(handle) = runtime_handle {
    let (sender, receiver) = task.completion_channel();
    let emitter = StreamEmitter {
      state,
      sender: Some(sender),
    };
    task.spawn(&handle, factory(emitter), receiver, false);
    return;
  }

  #[cfg(not(feature = "tokio"))]
  let _ = runtime_handle;

  let emitter = StreamEmitter {
    state,
    #[cfg(feature = "tokio")]
    sender: None,
  };
  task.set(Box::pin(factory(emitter)));
}

impl Ctx {
  /// Runs a finite async operation and restarts it when `deps` changes between renders.
  ///
  /// Use this for requests, loads, and other one-shot work that has a single result.
  /// Do not use `future` to model a continuous subscription by manually changing a
  /// dependency after each completion; that creates a render-dependent re-arm gap.
  /// Use [`Ctx::stream`] for receiver/watch/event sources that can produce multiple
  /// values over time.
  ///
  /// Called in `render`, the future belongs to its call position (see
  /// [`Ctx::future_action`]). Called in `create`, it gets a slot of its own: it
  /// starts once and runs until it completes, is cancelled, or the component
  /// unmounts; its `deps` never change.
  pub fn future<D, T, E, F, Fut>(&mut self, deps: D, factory: F) -> FutureHandle<T, E>
  where
    D: Clone + PartialEq + Send + Sync + 'static,
    T: SignalValue + Clone + PartialEq + Send + Sync + 'static,
    E: SignalValue + Clone + PartialEq + Send + Sync + 'static,
    F: Fn(D) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<T, E>> + Send + 'static,
  {
    let cursor = self.next_future_cursor();
    let runtime_handle = self.runtime_future_handle();

    if let Some(slot) = self.positional_future_slot(cursor)
      && let Some(handle) = slot.handle.downcast_ref::<FutureHandle<T, E>>()
    {
      let deps_changed = slot.deps.as_ref().and_then(|old| old.downcast_ref::<D>()) != Some(&deps);
      let handle = handle.clone();
      if deps_changed {
        slot.deps = Some(Box::new(deps.clone()));
        start_future_task(
          handle.state.clone(),
          handle.task.clone(),
          runtime_handle,
          Box::pin(factory(deps)),
        );
      }
      return handle;
    }

    let state = self.signal(FutureState::idle());
    let task = AsyncTask::new(self.loop_waker());
    let handle = FutureHandle {
      state: state.clone(),
      task: task.clone(),
    };
    self.store_future_slot(
      cursor,
      FutureSlot {
        deps: Some(Box::new(deps.clone())),
        handle: Box::new(handle.clone()),
        task: task.clone(),
      },
    );
    start_future_task(state, task, runtime_handle, Box::pin(factory(deps)));
    handle
  }

  /// Runs a continuous async producer and updates the handle state for every emitted item.
  ///
  /// The stream task starts on first render, restarts when `deps` changes between
  /// renders, and is cancelled when the component unmounts or stops calling
  /// `stream` at this cursor position. Call [`StreamEmitter::emit`] from the task
  /// for each item and [`StreamEmitter::reject`] to publish an error while keeping
  /// the stream alive. Called in `create`, the stream gets a slot of its own and
  /// runs until it ends, is cancelled, or the component unmounts.
  ///
  /// Use this for `watch::Receiver`, websocket/event subscriptions, file watchers,
  /// and other sources that can yield more than one value.
  pub fn stream<D, T, E, F, Fut>(&mut self, deps: D, factory: F) -> StreamHandle<T, E>
  where
    D: Clone + PartialEq + Send + Sync + 'static,
    T: SignalValue + Clone + PartialEq + Send + Sync + 'static,
    E: SignalValue + Clone + PartialEq + Send + Sync + 'static,
    F: Fn(D, StreamEmitter<T, E>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
  {
    let cursor = self.next_future_cursor();
    let runtime_handle = self.runtime_future_handle();

    if let Some(slot) = self.positional_future_slot(cursor)
      && let Some(handle) = slot.handle.downcast_ref::<StreamHandle<T, E>>()
    {
      let deps_changed = slot.deps.as_ref().and_then(|old| old.downcast_ref::<D>()) != Some(&deps);
      let handle = handle.clone();
      if deps_changed {
        slot.deps = Some(Box::new(deps.clone()));
        start_stream_task(
          handle.state.clone(),
          handle.task.clone(),
          runtime_handle,
          move |emitter| factory(deps, emitter),
        );
      }
      return handle;
    }

    let state = self.signal(FutureState::idle());
    let task = AsyncTask::new(self.loop_waker());
    let handle = StreamHandle {
      state: state.clone(),
      task: task.clone(),
    };
    self.store_future_slot(
      cursor,
      FutureSlot {
        deps: Some(Box::new(deps.clone())),
        handle: Box::new(handle.clone()),
        task: task.clone(),
      },
    );
    start_stream_task(state, task, runtime_handle, move |emitter| factory(deps, emitter));
    handle
  }

  /// Creates an async action that runs only when [`FutureAction::run`] or
  /// [`FutureAction::run_if_idle`] is called. `run` replaces a run in flight;
  /// `run_if_idle` leaves it and starts nothing.
  ///
  /// Where it is created decides how long it lives:
  /// - In `create`, the action gets a slot of its own and lives until the component unmounts. Keep it in the component
  ///   struct and run it from any render or handler. This is the simplest place for an action.
  /// - In `render`, the action belongs to its call position, like a hook: each render must make the same `ctx.future`,
  ///   `ctx.stream` and `ctx.future_action` calls in the same order. A render that skips the call, or makes a different
  ///   one at its position, drops the action: its run is cancelled and running a clone of it afterwards does nothing
  ///   but log an error that names the component.
  pub fn future_action<A, T, E, F, Fut>(&mut self, factory: F) -> FutureAction<A, T, E>
  where
    A: Send + Sync + 'static,
    T: SignalValue + Clone + PartialEq + Send + Sync + 'static,
    E: SignalValue + Clone + PartialEq + Send + Sync + 'static,
    F: Fn(A) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<T, E>> + Send + 'static,
  {
    let cursor = self.next_future_cursor();
    let runtime_handle = self.runtime_future_handle();
    let runner: ActionRunner<A, T, E> = Arc::new(move |args| Box::pin(factory(args)));

    if let Some(slot) = self.positional_future_slot(cursor)
      && let Some(action) = slot.handle.downcast_mut::<FutureAction<A, T, E>>()
    {
      *action.runner.lock() = runner;
      action.runtime_handle = runtime_handle;
      return action.clone();
    }

    let state = self.signal(FutureState::idle());
    let task = AsyncTask::new(self.loop_waker());
    let action = FutureAction {
      state,
      task: task.clone(),
      runner: Arc::new(Mutex::new(runner)),
      runtime_handle,
    };
    self.store_future_slot(
      cursor,
      FutureSlot {
        deps: None,
        handle: Box::new(action.clone()),
        task,
      },
    );
    action
  }

  /// The position of the next future call of this render, or `None` outside
  /// render (in `create`), where each call gets a stable slot of its own.
  fn next_future_cursor(&mut self) -> Option<usize> {
    if !self.rendering {
      return None;
    }
    let cursor = self.future_cursor;
    self.future_cursor += 1;
    Some(cursor)
  }

  fn positional_future_slot(&mut self, cursor: Option<usize>) -> Option<&mut FutureSlot> {
    self.future_slots.get_mut(cursor?)
  }

  /// Stores a new slot at `cursor`, or among the stable slots for `None`. A
  /// positional slot it replaces is dropped: its task is cancelled for good.
  fn store_future_slot(&mut self, cursor: Option<usize>, slot: FutureSlot) {
    let Some(cursor) = cursor else {
      self.stable_future_slots.push(slot);
      return;
    };
    let component = self.component_name;
    match self.future_slots.get_mut(cursor) {
      Some(current) => std::mem::replace(current, slot).drop_while_mounted(component),
      None => self.future_slots.push(slot),
    }
  }

  /// Drops the positional slots this render did not reach.
  pub(super) fn drop_unreached_future_slots(&mut self) {
    let component = self.component_name;
    for slot in self.future_slots.drain(self.future_cursor..) {
      slot.drop_while_mounted(component);
    }
  }

  /// Every future slot of this component: positional and stable.
  pub(super) fn future_slots(&self) -> impl Iterator<Item = &FutureSlot> {
    self.future_slots.iter().chain(&self.stable_future_slots)
  }
}
