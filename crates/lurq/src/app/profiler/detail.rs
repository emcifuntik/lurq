//! What a detailed recording (`RecordingOptions::detailed`) adds: a span per
//! component render and `after_layout` hook, and the causes of each render as
//! the reactive notifications sent them.
//!
//! Without `perf_profile` none of the hooks calling this is compiled. With it
//! and no detailed recording, a pass and an update each check one flag
//! (`Inner::detailed`), a component render reads one thread-local flag set by
//! that check, and a notification to a component reads one process-wide
//! counter. Nothing is timed, locked or allocated for it.
//!
//! Names are short static type names (`std::any::type_name` without its path
//! and generics), never values: a component is named by its type, a signal or
//! memo by the type of its value.
#![cfg_attr(not(feature = "perf_profile"), allow(dead_code))]
use std::{
  cell::{Cell, RefCell},
  collections::HashMap,
  sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
  },
};

use super::{
  model::{Phase, RenderCause, RenderSource, RenderTrigger},
  phase_slot::{Entered, PhaseSlot},
};

/// Component names a span can carry; later ones record their spans unnamed.
const MAX_COMPONENT_NAMES: usize = 4096;
/// Notifications kept until their component renders; the oldest go first.
const MAX_PENDING_CAUSES: usize = 1024;

/// Detailed recordings of every collector in the process: notifications are
/// kept for their components only while one runs.
static DETAILED_RECORDINGS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn recording_started() {
  DETAILED_RECORDINGS.fetch_add(1, Ordering::AcqRel);
}

pub(crate) fn recording_ended() {
  if DETAILED_RECORDINGS.fetch_sub(1, Ordering::AcqRel) == 1 {
    pending().lock().unwrap_or_else(|error| error.into_inner()).clear();
  }
}

fn recording() -> bool {
  DETAILED_RECORDINGS.load(Ordering::Relaxed) > 0
}

/// `std::any::type_name` without its module path and generics: `Counter`
/// for `app::widgets::Counter<T>`, `Vec` for `alloc::vec::Vec<u8>`.
pub(crate) fn short_type_name(type_name: &'static str) -> &'static str {
  let base = type_name.split('<').next().unwrap_or(type_name);
  base.rsplit("::").next().unwrap_or(base)
}

/// A `trigger` source whose value is a `T`, with its full type name: a
/// tracked read makes one, and only `note` shortens it, while a detailed
/// recording runs.
#[inline]
pub(crate) fn source_of<T: ?Sized>(trigger: RenderTrigger) -> RenderSource {
  RenderSource {
    trigger,
    value_type: Some(std::any::type_name::<T>()),
  }
}

fn shortened(source: RenderSource) -> RenderSource {
  RenderSource {
    value_type: source.value_type.map(short_type_name),
    ..source
  }
}

struct Names {
  /// Label `n` names `list[n - 1]`; 0 is no name.
  list: Vec<&'static str>,
  labels: HashMap<&'static str, u16>,
}

fn names() -> &'static Mutex<Names> {
  static NAMES: OnceLock<Mutex<Names>> = OnceLock::new();
  NAMES.get_or_init(|| {
    Mutex::new(Names {
      list: Vec::new(),
      labels: HashMap::new(),
    })
  })
}

/// The label a span of component `name` carries; 0 once
/// `MAX_COMPONENT_NAMES` names have one.
pub(crate) fn label_of(name: &'static str) -> u16 {
  let mut names = names().lock().unwrap_or_else(|error| error.into_inner());
  if let Some(label) = names.labels.get(name) {
    return *label;
  }
  if names.list.len() >= MAX_COMPONENT_NAMES {
    return 0;
  }
  names.list.push(name);
  let label = u16::try_from(names.list.len()).unwrap_or(0);
  names.labels.insert(name, label);
  label
}

/// The component name of `label`; None for 0.
pub(crate) fn name_of(label: u16) -> Option<&'static str> {
  if label == 0 {
    return None;
  }
  let names = names().lock().unwrap_or_else(|error| error.into_inner());
  names.list.get(usize::from(label) - 1).copied()
}

/// The registered name equal to `name`, registering a copy when it has
/// none. The layout trace names components by their nodes' tags, which hold
/// the same static type names in shared strings.
pub(crate) fn intern(name: &str) -> Option<&'static str> {
  let mut names = names().lock().unwrap_or_else(|error| error.into_inner());
  if let Some((name, _)) = names.labels.get_key_value(name) {
    return Some(*name);
  }
  if names.list.len() >= MAX_COMPONENT_NAMES {
    return None;
  }
  // Bounded by `MAX_COMPONENT_NAMES`, each a component type's name.
  let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
  names.list.push(name);
  let label = u16::try_from(names.list.len()).unwrap_or(0);
  names.labels.insert(name, label);
  Some(name)
}

/// A notification waiting for its component's render.
struct Pending {
  /// The address of the component's dirty flag, which identifies its context.
  key: usize,
  cause: RenderCause,
}

fn pending() -> &'static Mutex<Vec<Pending>> {
  static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());
  &PENDING
}

thread_local! {
  /// What made the memo recomputing on this thread recompute.
  static MEMO_TRIGGER: Cell<Option<RenderSource>> = const { Cell::new(None) };
}

/// Notes that `source` notified the component whose dirty flag is `dirty`,
/// while a detailed recording runs. Its render takes the cause.
#[inline]
pub(crate) fn note(dirty: &Arc<AtomicBool>, component: &'static str, source: RenderSource) {
  if recording() {
    note_cause(dirty, component, source);
  }
}

#[inline(never)]
fn note_cause(dirty: &Arc<AtomicBool>, component: &'static str, source: RenderSource) {
  let via = if source.trigger == RenderTrigger::Memo {
    MEMO_TRIGGER.with(Cell::get).map(shortened)
  } else {
    None
  };
  let cause = RenderCause {
    component: short_type_name(component),
    source: shortened(source),
    via,
  };
  let key = Arc::as_ptr(dirty) as usize;
  let mut pending = pending().lock().unwrap_or_else(|error| error.into_inner());
  if pending.iter().any(|entry| entry.key == key && entry.cause == cause) {
    return;
  }
  if pending.len() >= MAX_PENDING_CAUSES {
    pending.remove(0);
  }
  pending.push(Pending { key, cause });
}

/// Marks a memo recompute caused by `trigger` until dropped, so the
/// notifications its new value sends name it.
pub(crate) struct MemoRecompute {
  previous: Option<RenderSource>,
}

pub(crate) fn memo_recompute(trigger: RenderSource) -> Option<MemoRecompute> {
  recording().then(|| MemoRecompute {
    previous: MEMO_TRIGGER.with(|current| current.replace(Some(trigger))),
  })
}

impl Drop for MemoRecompute {
  fn drop(&mut self) {
    MEMO_TRIGGER.with(|current| current.set(self.previous));
  }
}

/// The update a detailed recording records on this thread.
struct Recorder {
  slot: Option<Arc<PhaseSlot>>,
  frame_id: Option<u64>,
  causes: Vec<RenderCause>,
}

thread_local! {
  static RECORDER: RefCell<Option<Recorder>> = const { RefCell::new(None) };
  /// True while `RECORDER` holds one: a component render checks only this.
  static RECORDING_UPDATE: Cell<bool> = const { Cell::new(false) };
}

/// A detailed recording of one update on this thread, until `finish` or
/// drop; an update nested in it records its own and gives the outer one back.
pub(crate) struct UpdateScope {
  previous: Option<Recorder>,
  finished: bool,
}

impl UpdateScope {
  pub(crate) fn begin(slot: Option<Arc<PhaseSlot>>, frame_id: Option<u64>) -> Self {
    let recorder = Recorder {
      slot,
      frame_id,
      causes: Vec::new(),
    };
    let previous = RECORDER.with(|current| current.borrow_mut().replace(recorder));
    RECORDING_UPDATE.with(|recording| recording.set(true));
    Self {
      previous,
      finished: false,
    }
  }

  /// The causes of the update's renders, in render order.
  pub(crate) fn finish(mut self) -> Vec<RenderCause> {
    self.restore().map(|recorder| recorder.causes).unwrap_or_default()
  }

  /// Gives the thread the update this one was nested in, if any, and
  /// returns this one's recorder.
  fn restore(&mut self) -> Option<Recorder> {
    self.finished = true;
    let previous = self.previous.take();
    RECORDING_UPDATE.with(|recording| recording.set(previous.is_some()));
    RECORDER.with(|current| std::mem::replace(&mut *current.borrow_mut(), previous))
  }
}

impl Drop for UpdateScope {
  fn drop(&mut self) {
    if !self.finished {
      self.restore();
    }
  }
}

/// The span of one component render or hook: ends when dropped.
pub(crate) struct ComponentSpan {
  slot: Arc<PhaseSlot>,
  entered: Entered,
}

impl ComponentSpan {
  fn enter(slot: &Arc<PhaseSlot>, frame_id: Option<u64>, phase: Phase, component: &'static str) -> Self {
    Self {
      slot: slot.clone(),
      entered: slot.enter_component(frame_id, phase, label_of(component)),
    }
  }
}

impl Drop for ComponentSpan {
  fn drop(&mut self) {
    self.slot.exit(self.entered);
  }
}

/// The spans of the `after_layout` hooks of one detailed pass.
pub(crate) struct HookSpans {
  slot: Arc<PhaseSlot>,
  frame_id: Option<u64>,
}

impl HookSpans {
  pub(crate) fn new(slot: Arc<PhaseSlot>, frame_id: Option<u64>) -> Self {
    Self { slot, frame_id }
  }

  /// The span of the hook of component `component` (a `std::any::type_name`),
  /// until dropped.
  pub(crate) fn enter(&self, component: &'static str) -> ComponentSpan {
    ComponentSpan::enter(
      &self.slot,
      self.frame_id,
      Phase::ComponentAfterLayout,
      short_type_name(component),
    )
  }
}

/// Called before component `component` (a `std::any::type_name`), whose
/// dirty flag is `dirty`, renders. In an update a detailed recording records,
/// takes the notifications that made it render as its causes, with
/// `trigger()` when its parent rendered it for another reason, and opens its
/// span; None otherwise, after one thread-local read.
#[inline]
pub(crate) fn component_render(
  dirty: &Arc<AtomicBool>,
  component: &'static str,
  trigger: impl FnOnce() -> Option<RenderTrigger>,
) -> Option<ComponentSpan> {
  if !RECORDING_UPDATE.with(Cell::get) {
    return None;
  }
  record_render(Arc::as_ptr(dirty) as usize, short_type_name(component), trigger())
}

#[inline(never)]
fn record_render(key: usize, component: &'static str, trigger: Option<RenderTrigger>) -> Option<ComponentSpan> {
  let mut causes: Vec<RenderCause> = Vec::new();
  if let Some(trigger) = trigger {
    causes.push(RenderCause {
      component,
      source: RenderSource {
        trigger,
        value_type: None,
      },
      via: None,
    });
  }
  {
    let mut pending = pending().lock().unwrap_or_else(|error| error.into_inner());
    pending.retain(|entry| {
      let taken = entry.key == key && entry.cause.component == component;
      if taken {
        causes.push(entry.cause);
      }
      !taken
    });
  }
  if causes.is_empty() {
    causes.push(RenderCause {
      component,
      source: RenderSource {
        trigger: RenderTrigger::Unknown,
        value_type: None,
      },
      via: None,
    });
  }
  RECORDER.with(|current| {
    let mut current = current.borrow_mut();
    let recorder = current.as_mut()?;
    for cause in causes {
      if !recorder.causes.contains(&cause) {
        recorder.causes.push(cause);
      }
    }
    let slot = recorder.slot.as_ref()?;
    Some(ComponentSpan::enter(slot, recorder.frame_id, Phase::Rebuild, component))
  })
}
