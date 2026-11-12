#[cfg(feature = "devtools")]
use std::sync::atomic::AtomicUsize;
#[cfg(feature = "devtools")]
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
  any::{Any, TypeId},
  collections::HashSet,
  sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
  },
  task::Context as TaskContext,
  time::{Duration, Instant},
};

use parking_lot::Mutex;

#[cfg(feature = "devtools")]
use super::component::{ComponentInfo, DevtoolsInspectable};
#[cfg(feature = "i18n")]
use super::i18n::I18n;
use super::{
  app_state::App,
  component::Component,
  events::MouseEvent,
  theme::Theme,
  window::{Window, WindowWaker},
};
use crate::{
  core::{
    ContextMap, ElementRef, ElementRefMut, ReactiveContext, Store,
    cell_ref::Ref,
    effect::Effect,
    memo::Memo,
    signal::{Signal, SignalValue},
    tracking,
  },
  node::{Element, HitTestBehavior, Node},
};

mod futures;

pub use futures::{FutureAction, FutureHandle, FutureState, FutureStatus, StreamEmitter, StreamHandle};
use futures::{FutureSlot, RuntimeFutureHandle};

static NEXT_COMPONENT_SLOT_ID: AtomicU64 = AtomicU64::new(1);

fn next_component_slot_id() -> u64 {
  NEXT_COMPONENT_SLOT_ID.fetch_add(1, Ordering::Relaxed)
}

static NEXT_RENDER_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Orders the outputs slots keep: a later render has a greater one.
fn next_render_generation() -> u64 {
  NEXT_RENDER_GENERATION.fetch_add(1, Ordering::Relaxed)
}

#[cfg(feature = "devtools")]
fn set_component_debug_metadata(node: &mut Node, ctx: &Ctx) {
  node.set_component_props_debug(ctx.props_debug());
  node.set_component_signals_debug(ctx.signals_debug());
  node.set_component_memos_debug(ctx.memos_debug());
  node.set_component_effects_debug(ctx.effects_debug());
  node.set_component_contexts_debug(ctx.contexts_debug());
}

fn attach_component_metadata(
  mut node: Box<Node>,
  tag_name: Arc<str>,
  slot_id: u64,
  key: Option<&str>,
  #[cfg(feature = "devtools")] ctx: &Ctx,
) -> Box<Node> {
  if node.component_slot_id().is_some() {
    node = Box::new(Node::logical().child(*node));
  }

  node.set_tag_name(tag_name);
  node.set_component_slot_id(slot_id);
  node.set_component_key(key);
  #[cfg(feature = "devtools")]
  set_component_debug_metadata(&mut node, ctx);
  node
}

pub(crate) fn component_tag_name<C: 'static>() -> Arc<str> {
  let type_name = std::any::type_name::<C>();
  let base = type_name.split('<').next().unwrap_or(type_name);
  Arc::from(base.rsplit("::").next().unwrap_or(base))
}

pub(crate) type ClickOutsideCallback = Arc<dyn Fn(&MouseEvent) + Send + Sync>;

struct ClickOutsideEntry {
  scope_id: u64,
  cursor: usize,
  element_ref: ElementRef,
  callback: ClickOutsideCallback,
}

#[derive(Clone)]
pub enum OpenState {
  Static(bool),
  Signal(Signal<bool>),
}

impl OpenState {
  pub fn is_open(&self) -> bool {
    match self {
      Self::Static(open) => *open,
      Self::Signal(open) => open.get(),
    }
  }

  pub(crate) fn signal(&self) -> Option<Signal<bool>> {
    match self {
      Self::Static(_) => None,
      Self::Signal(open) => Some(open.clone()),
    }
  }
}

impl From<bool> for OpenState {
  fn from(value: bool) -> Self {
    Self::Static(value)
  }
}

impl From<Signal<bool>> for OpenState {
  fn from(value: Signal<bool>) -> Self {
    Self::Signal(value)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
  TopStart,
  Top,
  TopEnd,
  BottomStart,
  Bottom,
  BottomEnd,
  LeftStart,
  Left,
  LeftEnd,
  RightStart,
  Right,
  RightEnd,
}

impl Default for Placement {
  fn default() -> Self {
    Self::BottomStart
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollisionStrategy {
  None,
  Clamp,
  Flip,
  FlipThenClamp,
}

impl Default for CollisionStrategy {
  fn default() -> Self {
    Self::FlipThenClamp
  }
}

/// What a left press outside an open popup does besides closing it, for
/// overlays with `dismiss_on_outside_click(true)` (`Popup`, `Popover`,
/// `Overlay`) and for `Select` menus.
///
/// A press is outside a popup when it lands neither on the popup, nor on its
/// anchor (a select's trigger), nor on a layer opened above it, such as a
/// select menu or a popup opened from inside it.
///
/// A press on another dropdown's trigger is always delivered, so one press
/// switches dropdowns, like a menu bar. The first interactive element under
/// the pointer (a button, a select, anything focusable) is a trigger when it is
/// a `Select`, or the anchor itself of a signal-driven overlay with
/// `dismiss_on_outside_click(true)`, open or closed, that has not opted out
/// with `switch_on_anchor_press(false)`. Other elements inside an anchor, such
/// as the other half of a split button, are not triggers. An overlay that is
/// only declared while open has no trigger while it is closed: declare it
/// always and drive it with its open signal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutsidePress {
  /// The press only closes the popup, like a native menu: the element under
  /// the pointer receives no press, release or click, and focus does not move.
  /// Pressing again reaches it. A press on another dropdown's trigger is
  /// delivered all the same.
  #[default]
  Consume,
  /// The press closes the popup and is also delivered to the element under
  /// the pointer, like a light-dismiss web popover.
  PassThrough,
}

pub(crate) struct OverlaySpec {
  pub(crate) anchor: ElementRef,
  pub(crate) node: Node,
  pub(crate) placement: Placement,
  pub(crate) offset_x: f32,
  pub(crate) offset_y: f32,
  pub(crate) match_anchor_width: bool,
  pub(crate) collision: CollisionStrategy,
  pub(crate) hit_test: HitTestBehavior,
  pub(crate) open_signal: Option<Signal<bool>>,
  pub(crate) dismiss_on_outside_click: bool,
  pub(crate) outside_press: OutsidePress,
  pub(crate) dismiss_on_escape: bool,
}

#[derive(Clone)]
pub enum ModalTarget {
  Parent,
  Root,
  Element(ElementRef),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Parent;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Root;

impl From<Parent> for ModalTarget {
  fn from(_: Parent) -> Self {
    Self::Parent
  }
}

impl From<Root> for ModalTarget {
  fn from(_: Root) -> Self {
    Self::Root
  }
}

impl From<ElementRef> for ModalTarget {
  fn from(value: ElementRef) -> Self {
    Self::Element(value)
  }
}

impl From<ElementRefMut> for ModalTarget {
  fn from(value: ElementRefMut) -> Self {
    Self::Element(value.into())
  }
}

/// What an open modal layer is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModalLayer {
  /// A dialog declared with `Modal`: the topmost one is the Tab scope and traps Tab.
  Dialog,
  /// `WindowChrome`'s title bar and resize zones: window decoration over the
  /// page, not a dialog, so it never becomes a Tab scope. Its stops, if any,
  /// are part of the window's order.
  WindowChrome,
}

pub(crate) struct ModalSpec {
  pub(crate) target: ModalTarget,
  pub(crate) node: Node,
  pub(crate) open_signal: Option<Signal<bool>>,
  pub(crate) dismiss_on_escape: bool,
  pub(crate) layer: ModalLayer,
}

impl ModalSpec {
  pub(crate) fn clone_for_reuse(&self) -> Self {
    self.clone_with_node(self.node.clone_for_reuse())
  }

  /// This declaration around `node`, a clone of its node.
  pub(crate) fn clone_with_node(&self, node: Node) -> Self {
    Self {
      target: self.target.clone(),
      node,
      open_signal: self.open_signal.clone(),
      dismiss_on_escape: self.dismiss_on_escape,
      layer: self.layer,
    }
  }
}

impl OverlaySpec {
  pub(crate) fn clone_for_reuse(&self) -> Self {
    self.clone_with_node(self.node.clone_for_reuse())
  }

  /// This declaration around `node`, a clone of its node.
  pub(crate) fn clone_with_node(&self, node: Node) -> Self {
    Self {
      anchor: self.anchor.clone(),
      node,
      placement: self.placement,
      offset_x: self.offset_x,
      offset_y: self.offset_y,
      match_anchor_width: self.match_anchor_width,
      collision: self.collision,
      hit_test: self.hit_test,
      open_signal: self.open_signal.clone(),
      dismiss_on_outside_click: self.dismiss_on_outside_click,
      outside_press: self.outside_press,
      dismiss_on_escape: self.dismiss_on_escape,
    }
  }
}

pub struct Modal {
  node: Box<Node>,
  open: OpenState,
  target: ModalTarget,
  dismiss_on_escape: bool,
  layer: ModalLayer,
  dismiss: Option<Signal<bool>>,
}

impl Modal {
  pub fn new(content: impl Into<Element>) -> Self {
    Self {
      node: content.into().node,
      open: OpenState::Static(true),
      target: ModalTarget::Parent,
      dismiss_on_escape: true,
      layer: ModalLayer::Dialog,
      dismiss: None,
    }
  }

  pub fn open(mut self, open: impl Into<OpenState>) -> Self {
    self.open = open.into();
    self
  }

  pub fn open_when(mut self, open: bool) -> Self {
    self.open = OpenState::Static(open);
    self
  }

  pub fn target(mut self, target: impl Into<ModalTarget>) -> Self {
    self.target = target.into();
    self
  }

  pub fn dismiss_on_escape(mut self, dismiss: bool) -> Self {
    self.dismiss_on_escape = dismiss;
    self
  }

  /// The signal Escape sets to `false`, for a modal that stays open after it: one that is shown on its way
  /// out, as a [`Sheet`](crate::components::Sheet) is.
  pub(crate) fn dismisses(mut self, open: Signal<bool>) -> Self {
    self.dismiss = Some(open);
    self
  }

  /// Marks this layer as `WindowChrome`'s decoration instead of a dialog.
  pub(crate) fn window_chrome_layer(mut self) -> Self {
    self.layer = ModalLayer::WindowChrome;
    self
  }

  fn into_spec(self) -> Option<ModalSpec> {
    if !self.open.is_open() {
      return None;
    }

    Some(ModalSpec {
      target: self.target,
      node: *self.node,
      open_signal: self.dismiss.or_else(|| self.open.signal()),
      dismiss_on_escape: self.dismiss_on_escape,
      layer: self.layer,
    })
  }
}

impl From<Modal> for Element {
  fn from(modal: Modal) -> Self {
    let mut node = Node::logical();
    node.set_layout_neutral(true);
    if let Some(spec) = modal.into_spec() {
      node.set_modal_declaration(spec);
    }
    Element::from_node(node)
  }
}

pub struct Overlay {
  anchor: Option<ElementRef>,
  node: Box<Node>,
  open: OpenState,
  placement: Placement,
  offset_x: f32,
  offset_y: f32,
  match_anchor_width: bool,
  collision: CollisionStrategy,
  hit_test: HitTestBehavior,
  dismiss_on_outside_click: bool,
  outside_press: OutsidePress,
  dismiss_on_escape: bool,
  switch_on_anchor_press: bool,
}

impl Overlay {
  pub fn new(content: impl Into<Element>) -> Self {
    Self {
      anchor: None,
      node: content.into().node,
      open: OpenState::Static(true),
      placement: Placement::default(),
      offset_x: 0.0,
      offset_y: 0.0,
      match_anchor_width: false,
      collision: CollisionStrategy::default(),
      hit_test: HitTestBehavior::Auto,
      dismiss_on_outside_click: false,
      outside_press: OutsidePress::default(),
      dismiss_on_escape: false,
      switch_on_anchor_press: true,
    }
  }

  pub fn anchor(mut self, anchor: impl Into<ElementRef>) -> Self {
    self.anchor = Some(anchor.into());
    self
  }

  pub fn open(mut self, open: impl Into<OpenState>) -> Self {
    self.open = open.into();
    self
  }

  pub fn open_when(mut self, open: bool) -> Self {
    self.open = OpenState::Static(open);
    self
  }

  pub fn placement(mut self, placement: Placement) -> Self {
    self.placement = placement;
    self
  }

  pub fn offset(mut self, x: f32, y: f32) -> Self {
    self.offset_x = x;
    self.offset_y = y;
    self
  }

  pub fn match_anchor_width(mut self, match_anchor_width: bool) -> Self {
    self.match_anchor_width = match_anchor_width;
    self
  }

  pub fn collision(mut self, collision: CollisionStrategy) -> Self {
    self.collision = collision;
    self
  }

  pub fn hit_test(mut self, behavior: HitTestBehavior) -> Self {
    self.hit_test = behavior;
    self
  }

  /// Closes the overlay on a left press outside it and its anchor. Needs a
  /// signal-backed open state.
  pub fn dismiss_on_outside_click(mut self, dismiss: bool) -> Self {
    self.dismiss_on_outside_click = dismiss;
    self
  }

  /// Whether the press that closes the overlay also reaches the element under
  /// the pointer. Defaults to [`OutsidePress::Consume`].
  pub fn outside_press(mut self, outside_press: OutsidePress) -> Self {
    self.outside_press = outside_press;
    self
  }

  pub fn dismiss_on_escape(mut self, dismiss: bool) -> Self {
    self.dismiss_on_escape = dismiss;
    self
  }

  /// Whether the anchor is this overlay's trigger, which a left press opens.
  /// While another dropdown is open, a press on a trigger closes it and goes
  /// on to the trigger, so one press switches dropdowns. Defaults to `true`;
  /// set `false` when a left press on the anchor does something else, as for
  /// a context menu or an autocomplete list. Applies to overlays with
  /// `dismiss_on_outside_click(true)` and a signal-backed open state.
  pub fn switch_on_anchor_press(mut self, switch: bool) -> Self {
    self.switch_on_anchor_press = switch;
    self
  }

  fn into_spec(self) -> Option<OverlaySpec> {
    if !self.open.is_open() {
      return None;
    }

    let open_signal = self.open.signal();

    Some(OverlaySpec {
      anchor: self.anchor?,
      node: *self.node,
      placement: self.placement,
      offset_x: self.offset_x,
      offset_y: self.offset_y,
      match_anchor_width: self.match_anchor_width,
      collision: self.collision,
      hit_test: self.hit_test,
      open_signal,
      dismiss_on_outside_click: self.dismiss_on_outside_click,
      outside_press: self.outside_press,
      dismiss_on_escape: self.dismiss_on_escape,
    })
  }
}

impl From<Overlay> for Element {
  fn from(overlay: Overlay) -> Self {
    let mut node = Node::logical();
    node.set_layout_neutral(true);
    if overlay.dismiss_on_outside_click
      && overlay.switch_on_anchor_press
      && matches!(overlay.open, OpenState::Signal(_))
      && let Some(anchor) = &overlay.anchor
    {
      node.set_dropdown_anchor(anchor.clone());
    }
    if let Some(spec) = overlay.into_spec() {
      node.set_overlay_declaration(spec);
    }
    Element::from_node(node)
  }
}

#[derive(Clone)]
pub struct Timeout {
  timer: Timer,
}

#[derive(Clone)]
pub struct Interval {
  timer: Timer,
}

#[derive(Clone)]
struct Timer {
  inner: Arc<Mutex<TimerInner>>,
}

struct TimerInner {
  duration: Duration,
  repeat: bool,
  next_fire: Option<Instant>,
  callback: Arc<dyn Fn() + Send + Sync>,
  /// Wakes the event loop when the timer gets a new fire time, so a shell that waits for the earliest timer learns
  /// about it; `None` outside a window.
  wake: Option<WindowWaker>,
}

impl Timeout {
  /// A timeout no window runs: starting it never fires. For a component's
  /// render run outside a window, such as a Markdown document rendered to
  /// find its text.
  #[cfg(feature = "markdown")]
  pub(crate) fn detached(f: impl Fn() + Send + Sync + 'static) -> Self {
    Self {
      timer: Timer::new(Duration::ZERO, false, f, None),
    }
  }

  pub fn start(&self) {
    self.timer.start();
  }

  pub fn restart(&self) {
    self.timer.restart();
  }

  pub fn cancel(&self) {
    self.timer.stop();
  }

  /// The delay of the next start or restart; a running timeout keeps its
  /// fire time.
  #[cfg(feature = "markdown")]
  pub(crate) fn set_duration(&self, duration: Duration) {
    self.timer.inner.lock().duration = duration;
  }

  pub fn is_active(&self) -> bool {
    self.timer.is_active()
  }
}

impl Interval {
  pub fn start(&self) {
    self.timer.start();
  }

  pub fn restart(&self) {
    self.timer.restart();
  }

  pub fn stop(&self) {
    self.timer.stop();
  }

  pub fn is_active(&self) -> bool {
    self.timer.is_active()
  }
}

impl Timer {
  fn new(
    duration: Duration,
    repeat: bool,
    callback: impl Fn() + Send + Sync + 'static,
    wake: Option<WindowWaker>,
  ) -> Self {
    Self {
      inner: Arc::new(Mutex::new(TimerInner {
        duration,
        repeat,
        next_fire: None,
        callback: Arc::new(callback),
        wake,
      })),
    }
  }

  fn start(&self) {
    let now = Instant::now();
    let wake = {
      let mut inner = self.inner.lock();
      if inner.next_fire.is_some() {
        return;
      }
      inner.next_fire = Some(now + inner.duration);
      inner.wake.clone()
    };
    Self::wake(wake);
  }

  fn restart(&self) {
    self.restart_at(Instant::now());
  }

  fn restart_at(&self, now: Instant) {
    let wake = {
      let mut inner = self.inner.lock();
      inner.next_fire = Some(now + inner.duration);
      inner.wake.clone()
    };
    Self::wake(wake);
  }

  /// Called outside the timer's lock: the waker may re-enter window state.
  fn wake(wake: Option<WindowWaker>) {
    if let Some(wake) = wake {
      wake();
    }
  }

  fn next_fire(&self) -> Option<Instant> {
    self.inner.lock().next_fire
  }

  fn stop(&self) {
    self.inner.lock().next_fire = None;
  }

  fn is_active(&self) -> bool {
    self.inner.lock().next_fire.is_some()
  }

  fn tick(&self, now: Instant) -> bool {
    let callback = {
      let mut inner = self.inner.lock();
      let Some(next_fire) = inner.next_fire else {
        return false;
      };
      if now < next_fire {
        return false;
      }
      if inner.repeat {
        inner.next_fire = Some(now + inner.duration);
      } else {
        inner.next_fire = None;
      }
      inner.callback.clone()
    };
    callback();
    true
  }
}

/// What components asked the runtime to do with an element of the tree,
/// shared by every `Ctx` of a window and applied once the render that
/// attaches the element is reconciled (focus) or laid out (scroll).
#[derive(Default)]
struct ElementRequests {
  focus: Option<ElementRef>,
  scroll_into_view: Option<ElementRef>,
  blur: bool,
  scroll_to_top: bool,
}

/// What a component's descendants keep clear of at the bottom, for a bar the component draws there:
/// [`Ctx::safe_area`] follows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BottomBarInset {
  /// A bar over the descendants, so high over the window's own bottom inset: they keep clear of both.
  Over(f32),
  /// A bar under the descendants, which end where it starts: nothing is left to keep clear of.
  Under,
}

pub struct Ctx {
  dirty: Arc<AtomicBool>,
  subtree_dirty: Arc<AtomicBool>,
  dirty_child_slots: Arc<Mutex<Vec<u64>>>,
  ancestor_dirty_slots: Vec<DirtyAncestor>,
  batch: Arc<BatchState>,
  props: Option<Box<dyn Any + Send>>,
  #[cfg(feature = "devtools")]
  props_debug: Option<DevtoolsInspectableDebug>,
  #[cfg(feature = "devtools")]
  signals_debug: Vec<ComponentSignalDebug>,
  #[cfg(feature = "devtools")]
  memos_debug: Vec<ComponentMemoDebug>,
  #[cfg(feature = "devtools")]
  effects_debug: Vec<ComponentEffectDebug>,
  #[cfg(feature = "devtools")]
  contexts_debug: Vec<ComponentContextDebug>,
  theme: Option<Theme>,
  window: Option<crate::app::window::Window>,
  breakpoint: Option<crate::core::Memo<Option<crate::app::theme::Breakpoint>>>,
  #[cfg(feature = "i18n")]
  i18n: Option<I18n>,
  app: Option<App>,
  element_requests: Arc<Mutex<ElementRequests>>,
  #[cfg(feature = "tokio")]
  runtime_future_handle: RuntimeFutureHandle,
  context_map: ContextMap,
  /// Values this component provided outside render (in `create`). They are
  /// re-applied on top of the inherited contexts whenever those change.
  provided_contexts: ContextMap,
  /// Inherited contexts with `provided_contexts` on top: what every render of
  /// this component starts from.
  base_contexts: ContextMap,
  /// Revision of the parent's context map the inherited values were taken from.
  inherited_context_revision: u64,
  /// The types this component read from its contexts (`use_context`,
  /// `consume_context`), in `create`, a render or a handler: a change of
  /// an inherited value of one of them renders the component again, a
  /// change of any other does not.
  context_reads: Vec<TypeId>,
  /// Whether the last render provided a value: such a component renders
  /// again whenever its inherited contexts change, so it provides again.
  provided_in_render: bool,
  /// The map the previous render left, while this one renders: a value
  /// `provide_if_changed` finds unchanged in it keeps its entry.
  previous_render_contexts: Option<ContextMap>,
  slot_children: Option<Vec<Element>>,
  children: Vec<ChildSlot>,
  child_cursor: usize,
  scope_id: u64,
  element_ref_cursor: usize,
  future_cursor: usize,
  watch_handles: Vec<Box<dyn Any + Send + Sync>>,
  render_watch_handles: Vec<Box<dyn Any + Send + Sync>>,
  effects: Vec<Effect>,
  timers: Vec<Timer>,
  /// Slots of the `ctx.future`, `ctx.stream` and `ctx.future_action` calls of
  /// a render, by call position.
  future_slots: Vec<FutureSlot>,
  /// Slots of those calls made outside render (in `create`): one each, kept
  /// until the component unmounts.
  stable_future_slots: Vec<FutureSlot>,
  /// The component this context belongs to, for diagnostics.
  component_name: &'static str,
  #[cfg(feature = "query")]
  query_registry: crate::query::QueryRegistry,
  #[cfg(feature = "query")]
  query_slots: Vec<crate::query::Observer>,
  #[cfg(feature = "query")]
  query_cursor: usize,
  element_refs: Vec<ElementRefMut>,
  click_outside_registry: Arc<Mutex<Vec<ClickOutsideEntry>>>,
  click_outside_cursor: usize,
  click_outside_active_cursors: Vec<usize>,
  rendering: bool,
}

#[derive(Clone)]
struct DirtyAncestor {
  subtree_dirty: Arc<AtomicBool>,
  dirty_child_slots: Arc<Mutex<Vec<u64>>>,
  child_slot_id: u64,
}

#[cfg(feature = "devtools")]
#[derive(Clone, Debug, PartialEq)]
pub struct DevtoolsInspectableDebug {
  pub type_name: Arc<str>,
  pub fields: Vec<ComponentInfo>,
}

#[cfg(feature = "devtools")]
#[derive(Clone, Debug)]
pub struct ComponentSignalDebug {
  pub id: usize,
  pub type_name: Arc<str>,
  value: Arc<Mutex<Option<Arc<str>>>>,
  history: Arc<Mutex<Vec<ComponentValueChangeDebug>>>,
  subscriber_count: Arc<AtomicUsize>,
}

#[cfg(feature = "devtools")]
#[derive(Clone, Debug)]
pub struct ComponentMemoDebug {
  pub id: usize,
  pub type_name: Arc<str>,
  value: Arc<Mutex<Option<Arc<str>>>>,
  history: Arc<Mutex<Vec<ComponentValueChangeDebug>>>,
  subscriber_count: Arc<AtomicUsize>,
}

#[cfg(feature = "devtools")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentValueChangeDebug {
  pub timestamp: String,
  pub from_value: String,
  pub to_value: String,
}

#[cfg(feature = "devtools")]
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentEffectDebug {
  pub id: usize,
}

#[cfg(feature = "devtools")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentContextKind {
  Provided,
  Consumed,
}

#[cfg(feature = "devtools")]
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentContextDebug {
  pub kind: ComponentContextKind,
  pub type_name: Arc<str>,
}

#[cfg(feature = "devtools")]
impl DevtoolsInspectableDebug {
  fn from_props<T: DevtoolsInspectable + 'static>(props: &T) -> Self {
    let mut fields = Vec::new();
    props.write_info(&mut fields);
    Self {
      type_name: Arc::from(std::any::type_name::<T>()),
      fields,
    }
  }
}

#[cfg(feature = "devtools")]
impl ComponentSignalDebug {
  pub fn formatted_value(&self) -> Option<String> {
    self.value.lock().as_ref().map(|value| value.to_string())
  }

  pub fn history(&self) -> Vec<ComponentValueChangeDebug> {
    self.history.lock().clone()
  }

  pub fn subscriber_count(&self) -> usize {
    self.subscriber_count.load(Ordering::Relaxed)
  }

  #[cfg_attr(not(feature = "perf_profile"), allow(dead_code))]
  pub(crate) fn estimated_memory_bytes(&self) -> usize {
    let history = self.history.lock();
    self.value.lock().as_ref().map(|value| value.len()).unwrap_or(0)
      + history.capacity() * std::mem::size_of::<ComponentValueChangeDebug>()
      + history
        .iter()
        .map(ComponentValueChangeDebug::estimated_memory_bytes)
        .sum::<usize>()
  }
}

#[cfg(feature = "devtools")]
impl ComponentMemoDebug {
  pub fn formatted_value(&self) -> Option<String> {
    self.value.lock().as_ref().map(|value| value.to_string())
  }

  pub fn history(&self) -> Vec<ComponentValueChangeDebug> {
    self.history.lock().clone()
  }

  pub fn subscriber_count(&self) -> usize {
    self.subscriber_count.load(Ordering::Relaxed)
  }

  pub(crate) fn estimated_memory_bytes(&self) -> usize {
    let history = self.history.lock();
    self.value.lock().as_ref().map(|value| value.len()).unwrap_or(0)
      + history.capacity() * std::mem::size_of::<ComponentValueChangeDebug>()
      + history
        .iter()
        .map(ComponentValueChangeDebug::estimated_memory_bytes)
        .sum::<usize>()
  }
}

#[cfg(feature = "devtools")]
impl ComponentValueChangeDebug {
  #[cfg_attr(not(feature = "perf_profile"), allow(dead_code))]
  fn estimated_memory_bytes(&self) -> usize {
    self.timestamp.capacity() + self.from_value.capacity() + self.to_value.capacity()
  }
}

#[cfg(feature = "devtools")]
impl PartialEq for ComponentSignalDebug {
  fn eq(&self, other: &Self) -> bool {
    self.id == other.id
      && self.type_name == other.type_name
      && self.formatted_value() == other.formatted_value()
      && self.history() == other.history()
      && self.subscriber_count() == other.subscriber_count()
  }
}

#[cfg(feature = "devtools")]
impl PartialEq for ComponentMemoDebug {
  fn eq(&self, other: &Self) -> bool {
    self.id == other.id
      && self.type_name == other.type_name
      && self.formatted_value() == other.formatted_value()
      && self.history() == other.history()
      && self.subscriber_count() == other.subscriber_count()
  }
}

#[cfg(feature = "devtools")]
fn update_debug_value_history(
  current_value: &Arc<Mutex<Option<Arc<str>>>>,
  history: &Arc<Mutex<Vec<ComponentValueChangeDebug>>>,
  next_value: Option<Arc<str>>,
) {
  let mut current_value = current_value.lock();
  if *current_value == next_value {
    return;
  }

  if let Some(previous) = current_value.as_ref() {
    let mut history = history.lock();
    history.push(ComponentValueChangeDebug {
      timestamp: current_compact_timestamp(),
      from_value: previous.to_string(),
      to_value: next_value
        .as_ref()
        .map(|value| value.to_string())
        .unwrap_or_else(|| "<unknown>".to_owned()),
    });
    if history.len() > 64 {
      let overflow = history.len() - 64;
      history.drain(0..overflow);
    }
  }

  *current_value = next_value;
}

#[cfg(feature = "devtools")]
fn current_compact_timestamp() -> String {
  let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
  let millis = duration.subsec_millis();
  let seconds_of_day = duration.as_secs() % 86_400;
  let hour = seconds_of_day / 3_600;
  let minute = seconds_of_day % 3_600 / 60;
  let second = seconds_of_day % 60;
  format!("{hour:02}:{minute:02}:{second:02}.{millis:03}")
}

#[cfg(feature = "devtools")]
fn format_debug_value<T: DevtoolsInspectable + 'static>(value: &T) -> Option<Arc<str>> {
  let mut fields = Vec::new();
  value.write_info(&mut fields);
  format_debug_fields(&fields).map(Arc::from)
}

#[cfg(feature = "devtools")]
fn format_debug_fields(fields: &[ComponentInfo]) -> Option<String> {
  if fields.is_empty() {
    return None;
  }

  if fields.len() == 1 {
    let info = &fields[0];
    if matches!(info.name(), "value" | "variant") {
      if let Some(value) = info.formatted_value() {
        return Some(value.to_owned());
      }
    }
  }

  Some(fields.iter().map(format_debug_info).collect::<Vec<_>>().join(", "))
}

#[cfg(feature = "devtools")]
fn format_debug_info(info: &ComponentInfo) -> String {
  if let Some(value) = info.formatted_value() {
    if info.name() == "value" {
      value.to_owned()
    } else {
      format!("{}: {}", info.name(), value)
    }
  } else if let Some(children) = format_debug_fields(info.children()) {
    if info.name() == "Some" {
      format!("Some({children})")
    } else {
      format!("{}: {{{children}}}", info.name())
    }
  } else {
    format!("{}: {}", info.name(), info.type_name())
  }
}

#[derive(Default)]
struct BatchState {
  inner: Mutex<BatchStateInner>,
  /// Wakes the window's event loop when a write off its thread marks components dirty
  /// ([`Window::off_loop_waker`]); `None` without a window.
  off_loop_wake: Option<WindowWaker>,
}

#[derive(Default)]
struct BatchStateInner {
  depth: usize,
  pending_dirty: Vec<Arc<AtomicBool>>,
}

struct BatchGuard<'a> {
  state: &'a BatchState,
}

impl BatchState {
  fn waking(off_loop_wake: WindowWaker) -> Self {
    Self {
      inner: Mutex::default(),
      off_loop_wake: Some(off_loop_wake),
    }
  }

  fn batch<R>(&self, f: impl FnOnce() -> R) -> R {
    self.inner.lock().depth += 1;
    let guard = BatchGuard { state: self };
    let result = f();
    drop(guard);
    result
  }

  /// Marks `dirty`, or holds the mark back until the batch ends. Returns whether the flag went from clean to
  /// dirty now: a flag already dirty has its pass coming, so marking it again needs no wake.
  fn mark_dirty(&self, dirty: &Arc<AtomicBool>) -> bool {
    let mut inner = self.inner.lock();
    if inner.depth == 0 {
      drop(inner);
      return !dirty.swap(true, Ordering::Relaxed);
    }

    inner.pending_dirty.push(dirty.clone());
    false
  }

  fn end_batch(&self) {
    let pending = {
      let mut inner = self.inner.lock();
      debug_assert!(inner.depth > 0);
      inner.depth = inner.depth.saturating_sub(1);
      if inner.depth > 0 {
        return;
      }
      std::mem::take(&mut inner.pending_dirty)
    };
    let mut seen = HashSet::new();
    let mut newly_dirty = false;
    for dirty in pending {
      let ptr = Arc::as_ptr(&dirty) as usize;
      if seen.insert(ptr) {
        newly_dirty |= !dirty.swap(true, Ordering::Relaxed);
      }
    }
    if newly_dirty {
      self.wake_loop();
    }
  }

  /// Wakes the event loop for a write off its thread that made a component dirty. Only a flag that goes from
  /// clean to dirty wakes it, so a thread writing thousands of times before the loop renders wakes it once
  /// instead of posting a message per write.
  fn wake_loop(&self) {
    if let Some(wake) = &self.off_loop_wake {
      wake();
    }
  }
}

impl Drop for BatchGuard<'_> {
  fn drop(&mut self) {
    self.state.end_batch();
  }
}

struct ChildSlot {
  id: u64,
  key: Option<String>,
  component: Box<dyn AnyComponent>,
  ctx: Ctx,
  /// The component's own output from its last render, with placeholders for
  /// the components it mounted (`node/component_output.rs`). It shares its
  /// runtime state (scroll states, text-input states, canvases) with the
  /// live tree. Read to hand an unchanged component back to a parent that
  /// renders again ([`Self::assembled_output`]) and to carry runtime state
  /// over to the component's next render, the only source of that state for
  /// a component resuming from offstage.
  rendered: Option<Box<Node>>,
  /// When `rendered` was rendered ([`next_render_generation`]).
  rendered_at: u64,
  mounted: bool,
  offstage: bool,
  offstage_dirty: bool,
}

trait AnyComponent: Send + Sync + 'static {
  fn render(&self, ctx: &mut Ctx) -> Element;
  fn after_layout(&self);
  fn on_mounted(&self);
  fn on_unmounted(&self);
  fn type_name(&self) -> &'static str;
  fn tag_name(&self) -> Arc<str>;
}

struct ComponentWrapper<C: Component> {
  component: C,
}

impl<C: Component> AnyComponent for ComponentWrapper<C> {
  fn render(&self, ctx: &mut Ctx) -> Element {
    self.component.render(ctx).into()
  }

  fn after_layout(&self) {
    self.component.after_layout();
  }

  fn on_mounted(&self) {
    self.component.on_mounted();
  }

  fn on_unmounted(&self) {
    self.component.on_unmounted();
  }

  fn type_name(&self) -> &'static str {
    std::any::type_name::<C>()
  }

  fn tag_name(&self) -> Arc<str> {
    component_tag_name::<C>()
  }
}

impl Ctx {
  pub fn new_root() -> Self {
    Self::new()
  }

  pub(crate) fn new() -> Self {
    Self {
      dirty: Arc::new(AtomicBool::new(true)),
      subtree_dirty: Arc::new(AtomicBool::new(true)),
      dirty_child_slots: Arc::new(Mutex::new(Vec::new())),
      ancestor_dirty_slots: Vec::new(),
      batch: Arc::new(BatchState::default()),
      props: None,
      #[cfg(feature = "devtools")]
      props_debug: None,
      #[cfg(feature = "devtools")]
      signals_debug: Vec::new(),
      #[cfg(feature = "devtools")]
      memos_debug: Vec::new(),
      #[cfg(feature = "devtools")]
      effects_debug: Vec::new(),
      #[cfg(feature = "devtools")]
      contexts_debug: Vec::new(),
      theme: None,
      window: None,
      breakpoint: None,
      #[cfg(feature = "i18n")]
      i18n: None,
      app: None,
      element_requests: Arc::default(),
      #[cfg(feature = "tokio")]
      runtime_future_handle: None,
      context_map: ContextMap::default(),
      provided_contexts: ContextMap::default(),
      base_contexts: ContextMap::default(),
      inherited_context_revision: 0,
      context_reads: Vec::new(),
      provided_in_render: false,
      previous_render_contexts: None,
      slot_children: None,
      children: Vec::new(),
      child_cursor: 0,
      scope_id: 0,
      element_ref_cursor: 0,
      future_cursor: 0,
      watch_handles: Vec::new(),
      render_watch_handles: Vec::new(),
      effects: Vec::new(),
      timers: Vec::new(),
      future_slots: Vec::new(),
      stable_future_slots: Vec::new(),
      component_name: "root",
      #[cfg(feature = "query")]
      query_registry: crate::query::QueryRegistry::default(),
      #[cfg(feature = "query")]
      query_slots: Vec::new(),
      #[cfg(feature = "query")]
      query_cursor: 0,
      element_refs: Vec::new(),
      click_outside_registry: Arc::new(Mutex::new(Vec::new())),
      click_outside_cursor: 0,
      click_outside_active_cursors: Vec::new(),
      rendering: false,
    }
  }

  pub(crate) fn with_theme(mut self, theme: Theme) -> Self {
    self.theme = Some(theme);
    self
  }

  pub(crate) fn with_window(mut self, window: crate::app::window::Window) -> Self {
    self.batch = Arc::new(BatchState::waking(window.off_loop_waker()));
    self.window = Some(window);
    self
  }

  /// Builds the reactive breakpoint memo from the current window and theme.
  /// Must be called after `with_theme` and `with_window`. The memo recomputes on
  /// window resize and theme changes but only notifies subscribers when the
  /// resolved breakpoint actually changes, so `breakpoint()`/`responsive()`
  /// re-render on threshold crossings, not on every resize tick.
  pub(crate) fn with_breakpoint(mut self) -> Self {
    if let (Some(window), Some(theme)) = (self.window.clone(), self.theme.clone()) {
      self.breakpoint = Some(crate::core::Memo::new(move || {
        window.track_access();
        theme.track_access();
        theme.breakpoints().resolve(window.info().logical_width())
      }));
    }
    self
  }

  #[cfg(feature = "i18n")]
  pub(crate) fn with_i18n(mut self, i18n: I18n) -> Self {
    self.i18n = Some(i18n);
    self
  }

  pub(crate) fn set_app_ref(&mut self, app: &mut App) {
    self.app = Some(app.clone());
    #[cfg(feature = "tokio")]
    {
      self.runtime_future_handle = app.tokio_handle();
    }
    for slot in &mut self.children {
      slot.ctx.set_app_ref(app);
    }
  }

  fn runtime_future_handle(&self) -> RuntimeFutureHandle {
    #[cfg(feature = "tokio")]
    {
      self.runtime_future_handle.clone()
    }
    #[cfg(not(feature = "tokio"))]
    {}
  }

  pub fn is_dirty(&self) -> bool {
    self.dirty.load(Ordering::Relaxed)
  }

  fn mark_dirty_targets(
    batch: &BatchState,
    dirty: &Arc<AtomicBool>,
    subtree_dirty: &Arc<AtomicBool>,
    ancestor_dirty_slots: &[DirtyAncestor],
  ) {
    // Every flag is marked; any that was clean wakes the loop once the marks are stored. Inside a batch, its
    // end stores and wakes.
    let mut newly_dirty = batch.mark_dirty(dirty);
    newly_dirty |= batch.mark_dirty(subtree_dirty);
    for ancestor in ancestor_dirty_slots {
      newly_dirty |= batch.mark_dirty(&ancestor.subtree_dirty);
      Self::mark_dirty_child_slot(&ancestor.dirty_child_slots, ancestor.child_slot_id);
    }
    if newly_dirty {
      batch.wake_loop();
    }
  }

  fn mark_self_dirty(&self) {
    self.dirty.store(true, Ordering::Relaxed);
    self.subtree_dirty.store(true, Ordering::Relaxed);
    for ancestor in &self.ancestor_dirty_slots {
      ancestor.subtree_dirty.store(true, Ordering::Relaxed);
      Self::mark_dirty_child_slot(&ancestor.dirty_child_slots, ancestor.child_slot_id);
    }
  }

  fn mark_dirty_child_slot(dirty_child_slots: &Mutex<Vec<u64>>, slot_id: u64) {
    let mut dirty_child_slots = dirty_child_slots.lock();
    if !dirty_child_slots.contains(&slot_id) {
      dirty_child_slots.push(slot_id);
    }
  }

  fn inherit_dirty_ancestors_from(&mut self, parent: &Ctx, slot_id: u64) {
    self.ancestor_dirty_slots = parent.ancestor_dirty_slots.clone();
    self.ancestor_dirty_slots.push(DirtyAncestor {
      subtree_dirty: parent.subtree_dirty.clone(),
      dirty_child_slots: parent.dirty_child_slots.clone(),
      child_slot_id: slot_id,
    });
  }

  fn update_subtree_dirty_from_children(&self) {
    let dirty = self.is_dirty() || !self.dirty_child_slots.lock().is_empty();
    self.subtree_dirty.store(dirty, Ordering::Relaxed);
  }

  fn take_dirty_child_slot_ids(&self) -> Vec<u64> {
    let mut dirty_child_slots = self.dirty_child_slots.lock();
    std::mem::take(&mut *dirty_child_slots)
  }

  pub(crate) fn clear_dirty(&self) {
    self.dirty.store(false, Ordering::Relaxed);
    self.subtree_dirty.store(false, Ordering::Relaxed);
    self.dirty_child_slots.lock().clear();
  }

  pub fn batch<R>(&self, f: impl FnOnce() -> R) -> R {
    self.batch.batch(f)
  }

  pub fn props<T: Send + PartialEq + 'static>(&self) -> &T {
    self
      .props
      .as_ref()
      .and_then(|props| props.downcast_ref::<T>())
      .expect("component props are not available for this type")
  }

  #[cfg(feature = "devtools")]
  fn set_props<T: Send + PartialEq + DevtoolsInspectable + 'static>(&mut self, props: T) {
    self.props_debug = Some(DevtoolsInspectableDebug::from_props(&props));
    self.props = Some(Box::new(props));
  }

  #[cfg(not(feature = "devtools"))]
  fn set_props<T: Send + PartialEq + 'static>(&mut self, props: T) {
    self.props = Some(Box::new(props));
  }

  pub(crate) fn set_component_name(&mut self, name: &'static str) {
    self.component_name = name;
  }

  /// The `std::any::type_name` of this context's component.
  #[cfg(feature = "perf_profile")]
  pub(crate) fn component_name(&self) -> &'static str {
    self.component_name
  }

  /// What a notification from `source` runs for this component: marks it
  /// dirty and, while a detailed recording runs, notes `source` as a cause of
  /// its next render.
  fn dirty_notifier(
    &self,
    #[cfg(feature = "perf_profile")] source: crate::app::profiler::RenderSource,
  ) -> impl Fn() + Send + Sync + 'static {
    let dirty = self.dirty.clone();
    let subtree_dirty = self.subtree_dirty.clone();
    let ancestor_dirty_slots = self.ancestor_dirty_slots.clone();
    let batch = self.batch.clone();
    #[cfg(feature = "perf_profile")]
    let component = self.component_name;
    move || {
      #[cfg(feature = "perf_profile")]
      crate::app::profiler::detail::note(&dirty, component, source);
      Self::mark_dirty_targets(&batch, &dirty, &subtree_dirty, &ancestor_dirty_slots);
    }
  }

  /// In an update a detailed recording records, the span of this
  /// component's render, with its causes noted: the notifications it got and
  /// `trigger()`, the reason its parent rendered it, if any. None otherwise.
  #[cfg(feature = "perf_profile")]
  pub(crate) fn render_span(
    &self,
    trigger: impl FnOnce() -> Option<crate::app::profiler::RenderTrigger>,
  ) -> Option<crate::app::profiler::detail::ComponentSpan> {
    crate::app::profiler::detail::component_render(&self.dirty, self.component_name, trigger)
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn set_root_props<T: Send + PartialEq + DevtoolsInspectable + 'static>(&mut self, props: T) {
    self.set_props(props);
  }

  #[cfg(not(feature = "devtools"))]
  pub(crate) fn set_root_props<T: Send + PartialEq + 'static>(&mut self, props: T) {
    self.set_props(props);
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn update_root_props<T: Send + PartialEq + DevtoolsInspectable + 'static>(&mut self, props: T) -> bool {
    if !self.props_changed(&props) {
      return false;
    }
    self.set_props(props);
    self.mark_self_dirty();
    true
  }

  #[cfg(not(feature = "devtools"))]
  pub(crate) fn update_root_props<T: Send + PartialEq + 'static>(&mut self, props: T) -> bool {
    if !self.props_changed(&props) {
      return false;
    }
    self.set_props(props);
    self.mark_self_dirty();
    true
  }

  fn props_changed<T: Send + PartialEq + 'static>(&self, props: &T) -> bool {
    self.props.as_ref().and_then(|existing| existing.downcast_ref::<T>()) != Some(props)
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn props_debug(&self) -> Option<DevtoolsInspectableDebug> {
    self.props_debug.clone()
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn signals_debug(&self) -> Vec<ComponentSignalDebug> {
    self.signals_debug.clone()
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn memos_debug(&self) -> Vec<ComponentMemoDebug> {
    self.memos_debug.clone()
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn effects_debug(&self) -> Vec<ComponentEffectDebug> {
    self.effects_debug.clone()
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn contexts_debug(&self) -> Vec<ComponentContextDebug> {
    self.contexts_debug.clone()
  }

  // --- Reactive primitives ---

  #[cfg(feature = "devtools")]
  pub fn signal<T: Send + Sync + DevtoolsInspectable + 'static>(&mut self, initial: T) -> Signal<T> {
    let debug_value = Arc::new(Mutex::new(format_debug_value(&initial)));
    let debug_history = Arc::new(Mutex::new(Vec::new()));
    let sig = Signal::new(initial);
    self.signals_debug.push(ComponentSignalDebug {
      id: sig.id(),
      type_name: Arc::from(std::any::type_name::<T>()),
      value: debug_value.clone(),
      history: debug_history.clone(),
      subscriber_count: sig.devtools_subscriber_count(),
    });
    let debug_handle = sig.subscribe_debug(move |value| {
      update_debug_value_history(&debug_value, &debug_history, format_debug_value(value));
    });
    let handle = sig.watch(self.dirty_notifier(crate::app::profiler::detail::source_of::<T>(
      crate::app::profiler::RenderTrigger::Signal,
    )));
    self.watch_handles.push(Box::new(debug_handle));
    self.watch_handles.push(Box::new(handle));
    sig
  }

  #[cfg(not(feature = "devtools"))]
  pub fn signal<T: SignalValue + Send + Sync + 'static>(&mut self, initial: T) -> Signal<T> {
    let sig = Signal::new(initial);
    let handle = sig.watch(self.dirty_notifier(
      #[cfg(feature = "perf_profile")]
      crate::app::profiler::detail::source_of::<T>(crate::app::profiler::RenderTrigger::Signal),
    ));
    self.watch_handles.push(Box::new(handle));
    sig
  }

  #[cfg(feature = "devtools")]
  pub fn memo<T: Clone + PartialEq + Send + Sync + DevtoolsInspectable + 'static>(
    &mut self,
    f: impl Fn() -> T + Send + Sync + 'static,
  ) -> Memo<T> {
    let memo = Memo::new(f);
    let debug_value = Arc::new(Mutex::new(memo.with(|value| format_debug_value(value))));
    let debug_history = Arc::new(Mutex::new(Vec::new()));
    self.memos_debug.push(ComponentMemoDebug {
      id: memo.id(),
      type_name: Arc::from(std::any::type_name::<T>()),
      value: debug_value.clone(),
      history: debug_history.clone(),
      subscriber_count: memo.devtools_subscriber_count(),
    });
    let debug_handle = memo.subscribe(move |value| {
      update_debug_value_history(&debug_value, &debug_history, format_debug_value(value));
    });
    self.watch_handles.push(Box::new(debug_handle));
    memo
  }

  #[cfg(not(feature = "devtools"))]
  pub fn memo<T: SignalValue + Clone + PartialEq + Send + Sync + 'static>(
    &mut self,
    f: impl Fn() -> T + Send + Sync + 'static,
  ) -> Memo<T> {
    Memo::new(f)
  }

  pub fn create_ref<T: Send + Sync + 'static>(&self, initial: T) -> Ref<T> {
    Ref::new(initial)
  }

  pub fn on_effect(&mut self, f: impl Fn() + Send + Sync + 'static) {
    let effect = Effect::new(f);
    #[cfg(feature = "devtools")]
    self.effects_debug.push(ComponentEffectDebug { id: effect.id() });
    self.effects.push(effect);
  }

  pub fn create_timeout(&mut self, duration: Duration, f: impl Fn() + Send + Sync + 'static) -> Timeout {
    let timer = Timer::new(duration, false, f, self.window.as_ref().map(Window::loop_waker));
    self.timers.push(timer.clone());
    Timeout { timer }
  }

  pub fn create_interval(&mut self, duration: Duration, f: impl Fn() + Send + Sync + 'static) -> Interval {
    let timer = Timer::new(duration, true, f, self.window.as_ref().map(Window::loop_waker));
    self.timers.push(timer.clone());
    Interval { timer }
  }

  #[cfg(feature = "form")]
  pub fn form(&mut self, options: crate::components::FormOptions) -> crate::components::FormHandle {
    crate::components::FormHandle::with_dirty(
      options,
      Arc::new(self.dirty_notifier(
        #[cfg(feature = "perf_profile")]
        crate::app::profiler::RenderSource {
          trigger: crate::app::profiler::RenderTrigger::Form,
          value_type: None,
        },
      )),
    )
  }

  #[cfg(feature = "form")]
  pub fn form_view<R>(&mut self, form: crate::components::FormHandle, render: impl FnOnce(&mut Ctx) -> R) -> Element
  where
    R: Into<Element>,
  {
    self.form_view_with(crate::components::FormProps::new(form), render)
  }

  #[cfg(feature = "form")]
  pub fn form_view_with<R>(
    &mut self,
    props: crate::components::FormProps,
    render: impl FnOnce(&mut Ctx) -> R,
  ) -> Element
  where
    R: Into<Element>,
  {
    let previous_context = self.context_map.clone();
    if let Some(form) = props.form.clone() {
      self.provide(crate::components::FormContext::new(form));
    }
    let child = render(self).into();
    self.context_map = previous_context;
    crate::components::Form::element(props, child)
  }

  #[cfg(feature = "form")]
  pub fn form_control<T>(&mut self, control: &crate::components::Control<T>) -> crate::components::ResolvedControl<T>
  where
    T: SignalValue + Clone + PartialEq + Send + Sync + 'static,
  {
    control.resolve()
  }

  #[cfg(feature = "form")]
  pub fn string_control(&mut self, name: impl Into<Arc<str>>) -> crate::components::ResolvedControl<String> {
    self.current_form().string_control(name).resolve()
  }

  #[cfg(feature = "form")]
  pub fn number_control(&mut self, name: impl Into<Arc<str>>) -> crate::components::ResolvedControl<f64> {
    self.current_form().number_control(name).resolve()
  }

  #[cfg(feature = "form")]
  pub fn bool_control(&mut self, name: impl Into<Arc<str>>) -> crate::components::ResolvedControl<bool> {
    self.current_form().bool_control(name).resolve()
  }

  #[cfg(feature = "form")]
  fn current_form(&mut self) -> crate::components::FormHandle {
    self
      .use_context::<crate::components::FormContext>()
      .map(|ctx| ctx.form())
      .expect("form controls must be resolved inside a Form render context")
  }

  #[cfg(feature = "router")]
  pub fn router(&mut self, routes: crate::router::Routes) -> crate::router::RouterHandle {
    let current_path = Signal::new(String::new());
    crate::router::RouterHandle::new_with_signal(routes, current_path)
  }

  #[cfg(feature = "router")]
  pub fn navigator(&mut self) -> Option<crate::router::Navigator> {
    self.use_context::<crate::router::Navigator>()
  }

  #[cfg(feature = "router")]
  pub fn route_params(&mut self) -> crate::router::Params {
    self
      .use_context::<crate::router::route_match::RouterMatches>()
      .and_then(|matches| matches.0.last().map(|m| m.params().clone()))
      .or_else(|| {
        self
          .use_context::<crate::router::route_match::OutletDepth>()
          .and_then(|depth| {
            self
              .use_context::<crate::router::route_match::RouterMatches>()
              .and_then(|matches| matches.0.get(depth.0).map(|m| m.params().clone()))
          })
      })
      .unwrap_or_default()
  }

  #[cfg(feature = "router")]
  pub fn route_path(&mut self) -> String {
    self
      .use_context::<crate::router::Navigator>()
      .map(|nav| nav.path().get())
      .unwrap_or_default()
  }

  #[cfg(feature = "router")]
  pub fn route_query(&mut self) -> crate::router::Query {
    self
      .use_context::<crate::router::Navigator>()
      .map(|nav| crate::router::Query::from_path(&nav.path().get()))
      .unwrap_or_default()
  }

  #[cfg(feature = "router")]
  pub fn route_state<T: std::any::Any + Send + Sync>(&mut self) -> Option<std::sync::Arc<T>> {
    // A screen of a stack keeps its own entry's state under the screens pushed over it.
    if let Some(state) = self.use_context::<crate::router::route_match::StackState>() {
      return state.0?.downcast::<T>().ok();
    }
    let nav = self.use_context::<crate::router::Navigator>()?;
    // Subscribe to navigations so this re-runs when the location (and its state) changes.
    let _ = nav.path().get();
    nav.state::<T>()
  }

  /// Says what the bar of the [`StackOutlet`](crate::components::StackOutlet) this screen is in shows for it:
  /// its title, what the way back to it says, what is at the bar's end. Call it in the screen's `render`.
  /// It does nothing outside a stack.
  #[cfg(feature = "router")]
  pub fn navigation_item(&mut self, item: crate::components::NavigationItem) {
    if let Some(slot) = self.use_context::<crate::components::stack_outlet::NavigationSlot>() {
      slot.set(item);
    }
  }

  /// Returns the client provided by a stable ancestor or this root context.
  #[cfg(feature = "query")]
  pub fn query_client(&mut self) -> crate::query::QueryClient {
    let client = self
      .use_context::<crate::query::QueryClient>()
      .expect("QueryClient is missing: call ctx.provide(QueryClient::new()) in the root component's create method");
    self.query_registry.attach(
      &client,
      self.window.as_ref().map(|window| window.loop_waker()),
      #[cfg(feature = "tokio")]
      self.runtime_future_handle.clone(),
    );
    client
  }

  /// Observe a typed query during render. Repeated renders reuse the observer;
  /// equal descriptors in different components share their cache and request.
  #[cfg(feature = "query")]
  pub fn query<D: crate::query::QueryDefinition>(
    &mut self,
    query: crate::query::Query<D>,
  ) -> crate::query::QueryHandle<D::Data, D::Error> {
    assert!(self.rendering, "ctx.query must be called during render");
    let client = self.query_client();
    let cursor = self.query_cursor;
    self.query_cursor += 1;
    if let Some(observer) = self.query_slots.get(cursor) {
      if observer.matches(&client, &query) {
        return observer.handle::<D>();
      }
    }
    let (handle, observer) = client.observe(query, Instant::now());
    if cursor < self.query_slots.len() {
      self.query_slots[cursor] = observer;
    } else {
      self.query_slots.push(observer);
    }
    handle
  }

  pub fn watch<T: SignalValue + Send + Sync + 'static>(
    &mut self,
    signal: &Signal<T>,
    f: impl Fn(&T) + Send + Sync + 'static,
  ) {
    let sub = signal.subscribe(f);
    self.watch_handles.push(Box::new(sub));
  }

  // --- Store + Lenses ---

  pub fn store<T: SignalValue + Clone + Send + Sync + 'static>(&mut self, initial: T) -> Store<T> {
    let store = Store::new(initial);
    let handle = store.signal().watch(self.dirty_notifier(
      #[cfg(feature = "perf_profile")]
      crate::app::profiler::detail::source_of::<T>(crate::app::profiler::RenderTrigger::Store),
    ));
    self.watch_handles.push(Box::new(handle));
    store
  }

  // --- Context (Dependency Injection) ---

  /// Provides `value` to this component and its descendants, shadowing a
  /// value of the same type from an ancestor.
  ///
  /// A value provided in `create` stays provided for the component's lifetime,
  /// across re-renders of the component and of its ancestors, until it
  /// provides another value of the same type. A value provided during `render`
  /// belongs to that render: later code in the render and the children it
  /// mounts see it, and the next render starts without it unless it is
  /// provided again.
  ///
  /// A provided value counts as new, since nothing compares it: the
  /// descendants that read its type render again, the others do not. A
  /// render that provides a value it computes every time passes it with
  /// [`provide_if_changed`](Self::provide_if_changed) so an equal value
  /// renders nothing again.
  pub fn provide<T: Clone + Send + Sync + 'static>(&mut self, value: T) {
    #[cfg(feature = "devtools")]
    self.push_context_debug(ComponentContextKind::Provided, std::any::type_name::<T>());
    self.store_context(value);
  }

  /// [`provide`](Self::provide), unless the value equals the one this
  /// component provided before: then the descendants that read it see the
  /// same providing and do not render again for it. In `render` the value
  /// is compared with the previous render's, also when this render was
  /// caused by a change of the inherited contexts; in `create` or a
  /// handler, with the one provided there.
  pub fn provide_if_changed<T: Clone + PartialEq + Send + Sync + 'static>(&mut self, value: T) {
    #[cfg(feature = "devtools")]
    self.push_context_debug(ComponentContextKind::Provided, std::any::type_name::<T>());
    let id = TypeId::of::<T>();
    if self.rendering {
      let unchanged = self
        .previous_render_contexts
        .as_ref()
        .and_then(|previous| previous.entry(id))
        .filter(|entry| entry.value::<T>() == Some(&value))
        .cloned();
      match unchanged {
        Some(entry) => {
          self.context_map.insert(id, entry);
          // With every entry the previous render left in place, the map is
          // that render's: the children mounted after this find nothing
          // to inherit.
          if let Some(previous) = &self.previous_render_contexts
            && self.context_map.has_same_entries(previous)
          {
            self.context_map.set_revision(previous.revision());
          }
        }
        None => self.context_map.provide(value),
      }
      self.provided_in_render = true;
      return;
    }
    let unchanged = self
      .provided_contexts
      .entry(id)
      .is_some_and(|entry| entry.value::<T>() == Some(&value));
    if !unchanged {
      self.store_context(value);
    }
  }

  fn store_context<T: Clone + Send + Sync + 'static>(&mut self, value: T) {
    if self.rendering {
      self.context_map.provide(value);
      self.provided_in_render = true;
      return;
    }
    self.provided_contexts.provide(value.clone());
    self.base_contexts.provide(value);
    self.context_map = self.base_contexts.clone();
  }

  fn note_context_read(&mut self, id: TypeId) {
    if !self.context_reads.contains(&id) {
      self.context_reads.push(id);
    }
  }

  pub fn use_context<T: Clone + Send + Sync + 'static>(&mut self) -> Option<T> {
    #[cfg(feature = "devtools")]
    self.push_context_debug(ComponentContextKind::Consumed, std::any::type_name::<T>());
    self.note_context_read(TypeId::of::<T>());
    self.context_map.get::<T>()
  }

  pub fn create_context<T: Clone + std::hash::Hash + Send + Sync + 'static>(&mut self, value: T) -> ReactiveContext<T> {
    #[cfg(feature = "devtools")]
    self.push_context_debug(
      ComponentContextKind::Provided,
      std::any::type_name::<ReactiveContext<T>>(),
    );
    let ctx = ReactiveContext::new(value);
    self.store_context(ctx.clone());
    ctx.subscribe(self.dirty_notifier(
      #[cfg(feature = "perf_profile")]
      crate::app::profiler::detail::source_of::<T>(crate::app::profiler::RenderTrigger::Context),
    ));
    ctx
  }

  pub fn consume_context<T: Clone + std::hash::Hash + Send + Sync + 'static>(&mut self) -> Option<ReactiveContext<T>> {
    #[cfg(feature = "devtools")]
    self.push_context_debug(
      ComponentContextKind::Consumed,
      std::any::type_name::<ReactiveContext<T>>(),
    );
    self.note_context_read(TypeId::of::<ReactiveContext<T>>());
    let ctx = self.context_map.get::<ReactiveContext<T>>()?;
    ctx.subscribe(self.dirty_notifier(
      #[cfg(feature = "perf_profile")]
      crate::app::profiler::detail::source_of::<T>(crate::app::profiler::RenderTrigger::Context),
    ));
    Some(ctx)
  }

  /// Takes the contexts `parent` passes down, keeping the values this
  /// component provided on top, and tells whether the component renders
  /// again for them: when a value it read changed, or when its last render
  /// provided a value, which only a render provides again.
  fn inherit_contexts(&mut self, parent: &ContextMap) -> Inherited {
    if self.inherited_context_revision == parent.revision() {
      return Inherited::Same;
    }
    self.inherited_context_revision = parent.revision();
    let left_by_render = self.context_map.revision() != self.base_contexts.revision();
    let previous = std::mem::replace(
      &mut self.base_contexts,
      ContextMap::layered(parent, &self.provided_contexts),
    );
    // The map the last render left is kept for the next render to compare
    // its values with (`provide_if_changed`), as `begin_render` keeps it.
    if left_by_render {
      self.previous_render_contexts = Some(std::mem::take(&mut self.context_map));
    }
    self.context_map = self.base_contexts.clone();
    let read_changed = self
      .context_reads
      .iter()
      .any(|id| previous.entry_revision(*id) != self.base_contexts.entry_revision(*id));
    if read_changed || self.provided_in_render {
      Inherited::Changed
    } else {
      Inherited::Refreshed
    }
  }

  /// Passes refreshed contexts down a component that does not render again:
  /// a descendant component that read a changed value is marked dirty and
  /// renders in place with the pass (`refresh_dirty_subtrees`), the others
  /// pass the contexts on to theirs. A `for_each` item is rendered by its
  /// owner's render alone (its closure lives there), so an item that read a
  /// changed value, or provides, needs its owner rendered again: the nearest
  /// component above it is marked, or, when that is this component, `true`
  /// is returned so the caller renders it.
  fn propagate_contexts(&mut self) -> bool {
    let contexts = std::mem::take(&mut self.context_map);
    let mut render_again = false;
    for slot in self.children.iter_mut().filter(|slot| !slot.offstage) {
      let for_each_item = slot.component.type_name() == ForEachSlot::TYPE_NAME;
      let changed = match slot.ctx.inherit_contexts(&contexts) {
        Inherited::Same => false,
        Inherited::Refreshed => slot.ctx.propagate_contexts(),
        Inherited::Changed => true,
      };
      if !changed {
        continue;
      }
      if for_each_item {
        render_again = true;
      } else {
        #[cfg(feature = "perf_profile")]
        crate::app::profiler::detail::note(
          &slot.ctx.dirty,
          slot.ctx.component_name,
          crate::app::profiler::RenderSource {
            trigger: crate::app::profiler::RenderTrigger::Context,
            value_type: None,
          },
        );
        slot.ctx.mark_dirty();
      }
    }
    self.context_map = contexts;
    render_again
  }

  /// Marks this component dirty, with its ancestors' subtree flags, and
  /// wakes the loop when it was clean.
  fn mark_dirty(&self) {
    Self::mark_dirty_targets(
      &self.batch,
      &self.dirty,
      &self.subtree_dirty,
      &self.ancestor_dirty_slots,
    );
  }

  #[cfg(feature = "devtools")]
  fn push_context_debug(&mut self, kind: ComponentContextKind, type_name: &'static str) {
    if self
      .contexts_debug
      .iter()
      .any(|ctx| ctx.kind == kind && ctx.type_name.as_ref() == type_name)
    {
      return;
    }
    self.contexts_debug.push(ComponentContextDebug {
      kind,
      type_name: Arc::from(type_name),
    });
  }

  // --- Theme ---

  pub fn theme(&self) -> &Theme {
    let theme = self.theme.as_ref().expect("theme not set");
    theme.track_access();
    theme
  }

  /// Current window geometry/state plus platform window commands.
  /// Reading this subscribes the component to window resize/move/scale changes.
  pub fn window(&self) -> crate::app::window::WindowHandle {
    let window = self.window.as_ref().expect("window not set");
    window.track_access();
    window.handle()
  }

  /// Current viewport breakpoint, resolved from the window's logical width
  /// against the theme's breakpoint thresholds. `None` is the base tier (below
  /// the smallest breakpoint). Reading this subscribes the component to
  /// breakpoint changes only — it re-renders when the resolved breakpoint
  /// crosses a threshold, not on every resize.
  pub fn breakpoint(&self) -> Option<crate::app::theme::Breakpoint> {
    self.breakpoint.as_ref().and_then(|memo| memo.get())
  }

  /// Resolve a [`Responsive`](crate::responsive::Responsive) value for the
  /// current breakpoint. Re-renders when the resolved breakpoint changes.
  pub fn responsive<T: Clone>(&self, value: &crate::responsive::Responsive<T>) -> T {
    value.resolve(self.breakpoint()).clone()
  }

  /// Shared application services retained by this component. The handle stays
  /// valid if the App passed to the tree is moved or dropped.
  pub fn app_ref(&self) -> &App {
    self.app.as_ref().expect("app ref not set")
  }

  /// The retained app services, if a pass or mount set them.
  pub(crate) fn app_handle(&self) -> Option<App> {
    self.app.clone()
  }

  /// Cloneable handle for opening secondary OS windows; safe to capture in
  /// event handlers (see [`crate::app::WindowOpener`]).
  pub fn window_opener(&self) -> crate::app::WindowOpener {
    self.app_ref().window_opener()
  }

  /// Mutates the same shared services as the App passed to the tree.
  pub fn app_ref_mut(&mut self) -> &mut App {
    self.app.as_mut().expect("app ref not set")
  }

  #[cfg(feature = "persistent_storage")]
  pub fn persistent_value<T: crate::persistent_storage::PersistentValue>(&self, key: &str) -> Option<T> {
    self.app_ref().persistent_value(key)
  }

  #[cfg(feature = "persistent_storage")]
  pub fn set_persistent_value<T: crate::persistent_storage::IntoPersistentValue>(
    &self,
    key: &str,
    value: T,
  ) -> Result<(), crate::persistent_storage::PersistentStorageError> {
    self.app_ref().set_persistent_value(key, value)
  }

  #[cfg(feature = "persistent_storage")]
  pub fn read_bulk<I, K>(
    &self,
    keys: I,
  ) -> Result<crate::persistent_storage::PersistentReadBatch, crate::persistent_storage::PersistentStorageError>
  where
    I: IntoIterator<Item = K>,
    K: AsRef<str>,
  {
    self.app_ref().read_bulk(keys)
  }

  #[cfg(feature = "persistent_storage")]
  pub fn read_bulk_values<T, I, K>(
    &self,
    keys: I,
  ) -> Result<Vec<Option<T>>, crate::persistent_storage::PersistentStorageError>
  where
    T: crate::persistent_storage::PersistentValue,
    I: IntoIterator<Item = K>,
    K: AsRef<str>,
  {
    self.app_ref().read_bulk_values(keys)
  }

  #[cfg(feature = "persistent_storage")]
  pub fn write_bulk<I, E>(&self, entries: I) -> Result<(), crate::persistent_storage::PersistentStorageError>
  where
    I: IntoIterator<Item = E>,
    E: crate::persistent_storage::IntoPersistentWrite,
  {
    self.app_ref().write_bulk(entries)
  }

  #[cfg(feature = "i18n")]
  pub fn i18n(&self) -> &I18n {
    let i18n = self.i18n.as_ref().expect("i18n not set");
    i18n.track_access();
    i18n
  }

  #[cfg(feature = "i18n")]
  pub fn t(&self, key: &str) -> Arc<str> {
    self.i18n().t(key)
  }

  #[cfg(feature = "i18n")]
  pub fn t_ns(&self, namespace: &str, key: &str) -> Arc<str> {
    self.i18n().t_ns(namespace, key)
  }

  #[cfg(feature = "i18n")]
  pub fn t_args<I, K, V>(&self, key: &str, args: I) -> Arc<str>
  where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: ToString,
  {
    self.i18n().t_args(key, args)
  }

  #[cfg(feature = "i18n")]
  pub fn t_ns_args<I, K, V>(&self, namespace: &str, key: &str, args: I) -> Arc<str>
  where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: ToString,
  {
    self.i18n().t_ns_args(namespace, key, args)
  }

  // --- Slot Children ---

  pub fn children(&self) -> &[Element] {
    self.slot_children.as_deref().unwrap_or(&[])
  }

  pub fn has_children(&self) -> bool {
    self.slot_children.as_ref().is_some_and(|c| !c.is_empty())
  }

  // --- Helpers ---

  pub fn element_ref(&mut self) -> ElementRef {
    self.element_ref_mut().as_ref()
  }

  /// Requests focus after the current render is reconciled. The ref may be
  /// attached by the render that follows this call (for example after navigation).
  /// Requests for refs absent from that tree are ignored. The last request wins.
  ///
  /// The focused element is then scrolled into view like
  /// [`Ctx::scroll_into_view`], as a browser's `element.focus()` does.
  pub fn focus(&self, element: &ElementRef) {
    self.element_requests.lock().focus = Some(element.clone());
    self.wake_window();
  }

  /// Scrolls the element into view after the current render is laid out:
  /// every scroll container around it scrolls by the smallest amount that
  /// shows it, innermost first, on each axis the container scrolls — the
  /// web's `scrollIntoView({ block: "nearest", inline: "nearest" })`. An
  /// element already in view does not move anything; one larger than a
  /// viewport is aligned to the viewport start. The frame that follows is
  /// drawn at the new position.
  ///
  /// The ref may be attached by the render that follows this call. Requests
  /// for refs absent from that tree are ignored. The last request wins.
  ///
  /// Inside a [`VirtualizedList`](crate::components::VirtualizedList) this
  /// reaches elements of mounted rows only; reveal a row that may be outside
  /// the rendered window with
  /// [`VirtualizedList::reveal_key`](crate::components::VirtualizedList::reveal_key).
  pub fn scroll_into_view(&self, element: &ElementRef) {
    self.element_requests.lock().scroll_into_view = Some(element.clone());
    self.wake_window();
  }

  fn wake_window(&self) {
    if let Some(window) = &self.window {
      window.wake();
    }
  }

  /// Whether a focus or scroll-into-view request is waiting for the runtime.
  pub(crate) fn has_element_request(&self) -> bool {
    let requests = self.element_requests.lock();
    requests.focus.is_some() || requests.scroll_into_view.is_some() || requests.blur || requests.scroll_to_top
  }

  /// Takes the focus from whatever has it, after the current render: a text input being typed into is left,
  /// and a phone's keyboard goes away. A request to focus made in the same render wins.
  pub fn blur(&self) {
    self.element_requests.lock().blur = true;
    self.wake_window();
  }

  pub(crate) fn take_blur_request(&self) -> bool {
    std::mem::take(&mut self.element_requests.lock().blur)
  }

  /// Scrolls the page to its top, after the current render, as a tap on a phone's status bar does: the
  /// outermost list at the middle of the window that scrolls down and is away from its top, unless it is left
  /// out with `ScrollState::set_scrolls_to_top(false)`.
  pub fn scroll_to_top(&self) {
    self.element_requests.lock().scroll_to_top = true;
    self.wake_window();
  }

  pub(crate) fn take_scroll_to_top_request(&self) -> bool {
    std::mem::take(&mut self.element_requests.lock().scroll_to_top)
  }

  /// How far in from each edge of the window this component's content stays clear of what is over it there:
  /// the window's [`safe_area`](crate::app::window::WindowInfo), and at the bottom also the bar a component
  /// around this one draws over its content, as a [`TabOutlet`](crate::components::TabOutlet) does. Content
  /// that scrolls under such a bar is padded by this inside its list, and shows through the bar.
  pub fn safe_area(&mut self) -> crate::app::window::SafeAreaInsets {
    let mut safe_area = self.window().safe_area;
    match self.use_context::<BottomBarInset>() {
      Some(BottomBarInset::Over(bar)) => safe_area.bottom += bar,
      Some(BottomBarInset::Under) => safe_area.bottom = 0.0,
      None => {}
    }
    safe_area
  }

  pub(crate) fn focus_request(&self) -> Option<ElementRef> {
    self.element_requests.lock().focus.clone()
  }

  pub(crate) fn take_focus_request(&self) -> Option<ElementRef> {
    self.element_requests.lock().focus.take()
  }

  pub(crate) fn take_scroll_into_view_request(&self) -> Option<ElementRef> {
    self.element_requests.lock().scroll_into_view.take()
  }

  pub fn element_ref_mut(&mut self) -> ElementRefMut {
    if !self.rendering {
      return ElementRefMut::new();
    }

    let cursor = self.element_ref_cursor;
    self.element_ref_cursor += 1;

    if cursor == self.element_refs.len() {
      self.element_refs.push(ElementRefMut::new());
    }

    self.element_refs[cursor].clone()
  }

  pub fn interaction(&self) -> crate::node::interaction_state::InteractionState {
    crate::node::interaction_state::InteractionState::new()
  }

  pub fn on_click_outside(
    &mut self,
    element_ref: impl Into<ElementRef>,
    f: impl Fn(&MouseEvent) + Send + Sync + 'static,
  ) {
    if !self.rendering {
      return;
    }

    let cursor = self.click_outside_cursor;
    self.click_outside_cursor += 1;
    self.click_outside_active_cursors.push(cursor);

    let element_ref = element_ref.into();
    let callback: ClickOutsideCallback = Arc::new(f);
    let mut registry = self.click_outside_registry.lock();
    if let Some(entry) = registry
      .iter_mut()
      .find(|entry| entry.scope_id == self.scope_id && entry.cursor == cursor)
    {
      entry.element_ref = element_ref;
      entry.callback = callback;
      return;
    }

    registry.push(ClickOutsideEntry {
      scope_id: self.scope_id,
      cursor,
      element_ref,
      callback,
    });
  }

  pub(crate) fn click_outside_callbacks_at(&self, x: f32, y: f32) -> Vec<ClickOutsideCallback> {
    self
      .click_outside_registry
      .lock()
      .iter()
      .filter(|entry| element_ref_is_click_outside(&entry.element_ref, x, y))
      .map(|entry| entry.callback.clone())
      .collect()
  }

  // --- Component mounting ---

  pub fn mount<C: Component>(&mut self, props: C::Props) -> Element {
    self.mount_inner::<C>(None, props, None, true)
  }

  pub fn mount_keyed<C: Component>(&mut self, key: &str, props: C::Props) -> Element {
    self.mount_inner::<C>(Some(key), props, None, true)
  }

  /// Retains a component and its complete context while excluding its output
  /// from the active tree. Offstage components keep their slots, signals,
  /// futures, and last rendered node, but do not participate in layout,
  /// painting, hit testing, dirty refreshes, timers, or future polling until
  /// they become active again. Future polling means applying results: a future
  /// or stream polled cooperatively (no Tokio handle) waits, while a task
  /// already running on Tokio keeps running and the results it produced while
  /// offstage are applied, in order, once the component is active again. A
  /// component removed while offstage unmounts and its tasks are cancelled.
  /// When it becomes active again, its output takes back the runtime state it
  /// had: scroll offsets (including those of scroll areas whose `ScrollState`
  /// the component does not hold), text-input carets and selections, open
  /// selects and canvases.
  pub fn mount_offstage<C: Component>(&mut self, props: C::Props, active: bool) -> Element {
    self.mount_inner::<C>(None, props, None, active)
  }

  /// Keyed variant of [`Self::mount_offstage`].
  pub fn mount_keyed_offstage<C: Component>(&mut self, key: &str, props: C::Props, active: bool) -> Element {
    self.mount_inner::<C>(Some(key), props, None, active)
  }

  pub fn mount_with<C: Component>(&mut self, props: C::Props, slot_children: Vec<Element>) -> Element {
    self.mount_inner::<C>(None, props, Some(slot_children), true)
  }

  pub fn mount_keyed_with<C: Component>(&mut self, key: &str, props: C::Props, slot_children: Vec<Element>) -> Element {
    self.mount_inner::<C>(Some(key), props, Some(slot_children), true)
  }

  fn mount_inner<C: Component>(
    &mut self,
    key: Option<&str>,
    props: C::Props,
    slot_children: Option<Vec<Element>>,
    active: bool,
  ) -> Element {
    let cursor = self.child_cursor;
    self.child_cursor += 1;

    let type_name = std::any::type_name::<C>();

    if let Some(key) = key
      && let Some(found) = self.children[cursor..]
        .iter()
        .position(|slot| slot.key.as_deref() == Some(key) && slot.component.type_name() == type_name)
        .map(|offset| cursor + offset)
      && found != cursor
    {
      let slot = self.children.remove(found);
      self.children.insert(cursor, slot);
    }

    let can_reuse = self.children.get(cursor).is_some_and(|slot| {
      let key_match = match key {
        Some(k) => slot.key.as_deref() == Some(k),
        None => slot.key.is_none(),
      };
      key_match && slot.component.type_name() == type_name
    });

    if can_reuse {
      let slot = &mut self.children[cursor];
      let has_slot_children = slot.ctx.slot_children.is_some() || slot_children.is_some();
      let props_changed = slot.ctx.props_changed(&props);
      let inherited = slot.ctx.inherit_contexts(&self.context_map);
      slot.ctx.slot_children = slot_children;
      if props_changed {
        slot.ctx.set_props(props);
      }
      if !active {
        slot.offstage = true;
        slot.offstage_dirty = true;
        slot.ctx.clear_modal_entries_recursive();
        slot.ctx.clear_dirty();
        return Element::new();
      }

      slot.offstage = false;
      #[cfg(feature = "perf_profile")]
      let resumed = slot.rendered.is_none() || slot.offstage_dirty;
      #[cfg(feature = "perf_profile")]
      let mut context_changed = inherited == Inherited::Changed;
      let mut needs_render = has_slot_children
        || props_changed
        || inherited == Inherited::Changed
        || slot.ctx.any_dirty()
        || slot.rendered.is_none()
        || slot.offstage_dirty;
      slot.offstage_dirty = false;
      if !needs_render && inherited == Inherited::Refreshed && slot.ctx.propagate_contexts() {
        needs_render = true;
        #[cfg(feature = "perf_profile")]
        {
          context_changed = true;
        }
      }
      if needs_render {
        #[cfg(feature = "perf_profile")]
        let _span = slot.ctx.render_span(|| {
          use crate::app::profiler::RenderTrigger;
          if resumed {
            Some(RenderTrigger::Mount)
          } else if props_changed {
            Some(RenderTrigger::Props)
          } else if context_changed {
            Some(RenderTrigger::Context)
          } else if has_slot_children {
            Some(RenderTrigger::SlotChildren)
          } else {
            None
          }
        });
        // Carry runtime state over from the previous render, as a dirty
        // refresh does, so `rendered` keeps sharing the live scroll states
        // with the active tree. A slot resuming from offstage has no node in
        // the active tree, so this is the only place its scroll offsets,
        // text-input carets and selections come back from.
        let previous = slot.rendered.take();
        slot.ctx.begin_render();
        let mut element = slot.component.render(&mut slot.ctx);
        slot.ctx.end_render();
        element.node = attach_component_metadata(
          element.node,
          slot.component.tag_name(),
          slot.id,
          slot.key.as_deref(),
          #[cfg(feature = "devtools")]
          &slot.ctx,
        );
        if let Some(previous) = previous.as_ref() {
          element.node.preserve_runtime_state_from(previous);
        }
        slot.keep_rendered(&element.node);
        return element;
      }
      let output = slot
        .assembled_output()
        .expect("a component that is not rendered again has kept its output");
      return Element::from_boxed_node(output);
    }

    let slot_id = next_component_slot_id();
    let mut child_ctx = Ctx::new();
    child_ctx.inherit_dirty_ancestors_from(self, slot_id);
    child_ctx.batch = self.batch.clone();
    child_ctx.theme = self.theme.clone();
    child_ctx.window = self.window.clone();
    child_ctx.breakpoint = self.breakpoint.clone();
    child_ctx.app = self.app.clone();
    child_ctx.element_requests = self.element_requests.clone();
    #[cfg(feature = "tokio")]
    {
      child_ctx.runtime_future_handle = self.runtime_future_handle.clone();
    }
    #[cfg(feature = "query")]
    {
      child_ctx.query_registry = self.query_registry.clone();
    }
    child_ctx.click_outside_registry = self.click_outside_registry.clone();
    #[cfg(feature = "i18n")]
    {
      child_ctx.i18n = self.i18n.clone();
    }
    child_ctx.inherit_contexts(&self.context_map);
    child_ctx.slot_children = slot_children;
    child_ctx.set_props(props);
    child_ctx.scope_id = slot_id;
    child_ctx.component_name = type_name;
    // A first mount's span takes in `create` too.
    #[cfg(feature = "perf_profile")]
    let _span = active
      .then(|| child_ctx.render_span(|| Some(crate::app::profiler::RenderTrigger::Mount)))
      .flatten();
    let component = C::create(&mut child_ctx);
    let wrapper = ComponentWrapper { component };
    let mut element = Element::new();
    if active {
      child_ctx.begin_render();
      element = wrapper.render(&mut child_ctx);
      child_ctx.end_render();
      element.node = attach_component_metadata(
        element.node,
        wrapper.tag_name(),
        slot_id,
        key,
        #[cfg(feature = "devtools")]
        &child_ctx,
      );
    } else {
      child_ctx.clear_dirty();
    }

    let mut slot = ChildSlot {
      id: slot_id,
      key: key.map(str::to_owned),
      component: Box::new(wrapper),
      ctx: child_ctx,
      rendered: None,
      rendered_at: 0,
      mounted: false,
      offstage: !active,
      offstage_dirty: !active,
    };
    if active {
      slot.keep_rendered(&element.node);
    }

    if key.is_some() {
      self.insert_child_slot(cursor, slot);
    } else {
      self.set_child_slot(cursor, slot);
    }

    element
  }

  // --- Keyed list rendering ---

  pub fn for_each<T, K, KF, CF>(
    &mut self,
    items: impl IntoIterator<Item = T>,
    key_fn: KF,
    component_fn: CF,
  ) -> Vec<Element>
  where
    K: std::fmt::Display,
    KF: Fn(&T) -> K,
    CF: Fn(&mut Ctx, T) -> Element,
  {
    let cursor = self.child_cursor;
    self.child_cursor += 1;

    let can_reuse_group = self
      .children
      .get(cursor)
      .is_some_and(|slot| slot.key.is_none() && slot.component.type_name() == ForEachSlot::TYPE_NAME);

    if !can_reuse_group {
      let slot_id = next_component_slot_id();
      let mut group_ctx = Ctx::new();
      group_ctx.inherit_dirty_ancestors_from(self, slot_id);
      group_ctx.batch = self.batch.clone();
      group_ctx.theme = self.theme.clone();
      group_ctx.window = self.window.clone();
      group_ctx.breakpoint = self.breakpoint.clone();
      group_ctx.app = self.app.clone();
      group_ctx.element_requests = self.element_requests.clone();
      #[cfg(feature = "tokio")]
      {
        group_ctx.runtime_future_handle = self.runtime_future_handle.clone();
      }
      #[cfg(feature = "query")]
      {
        group_ctx.query_registry = self.query_registry.clone();
      }
      group_ctx.click_outside_registry = self.click_outside_registry.clone();
      #[cfg(feature = "i18n")]
      {
        group_ctx.i18n = self.i18n.clone();
      }
      group_ctx.inherit_contexts(&self.context_map);
      group_ctx.scope_id = slot_id;
      group_ctx.component_name = ForEachSlot::TYPE_NAME;
      let slot = ChildSlot {
        id: slot_id,
        key: None,
        component: Box::new(ForEachSlot),
        ctx: group_ctx,
        rendered: None,
        rendered_at: 0,
        mounted: false,
        offstage: false,
        offstage_dirty: false,
      };
      self.set_child_slot(cursor, slot);
    }

    let slot = &mut self.children[cursor];
    slot.ctx.inherit_contexts(&self.context_map);
    slot.ctx.begin_render();
    let elements = items
      .into_iter()
      .map(|item| {
        let key = format!("{}", key_fn(&item));
        slot.ctx.render_for_each_item(key, item, &component_fn)
      })
      .collect();
    slot.ctx.end_render();
    elements
  }

  fn render_for_each_item<T, CF>(&mut self, key: String, item: T, component_fn: &CF) -> Element
  where
    CF: Fn(&mut Ctx, T) -> Element,
  {
    let cursor = self.child_cursor;
    self.child_cursor += 1;

    if let Some(found) = self.children[cursor..]
      .iter()
      .position(|slot| {
        slot.key.as_deref() == Some(key.as_str()) && slot.component.type_name() == ForEachSlot::TYPE_NAME
      })
      .map(|offset| cursor + offset)
    {
      if found != cursor {
        let slot = self.children.remove(found);
        self.children.insert(cursor, slot);
      }
    }

    let can_reuse = self.children.get(cursor).is_some_and(|slot| {
      slot.key.as_deref() == Some(key.as_str()) && slot.component.type_name() == ForEachSlot::TYPE_NAME
    });

    if can_reuse {
      let slot = &mut self.children[cursor];
      slot.ctx.inherit_contexts(&self.context_map);
      #[cfg(feature = "perf_profile")]
      let _span = slot
        .ctx
        .render_span(|| Some(crate::app::profiler::RenderTrigger::ListItem));
      slot.ctx.begin_render();
      let mut element = component_fn(&mut slot.ctx, item);
      slot.ctx.end_render();
      element.node = attach_component_metadata(
        element.node,
        slot.component.tag_name(),
        slot.id,
        slot.key.as_deref(),
        #[cfg(feature = "devtools")]
        &slot.ctx,
      );
      slot.keep_rendered(&element.node);
      return element;
    }

    let slot_id = next_component_slot_id();
    let mut child_ctx = Ctx::new();
    child_ctx.inherit_dirty_ancestors_from(self, slot_id);
    child_ctx.batch = self.batch.clone();
    child_ctx.theme = self.theme.clone();
    child_ctx.window = self.window.clone();
    child_ctx.breakpoint = self.breakpoint.clone();
    child_ctx.app = self.app.clone();
    child_ctx.element_requests = self.element_requests.clone();
    #[cfg(feature = "tokio")]
    {
      child_ctx.runtime_future_handle = self.runtime_future_handle.clone();
    }
    #[cfg(feature = "query")]
    {
      child_ctx.query_registry = self.query_registry.clone();
    }
    child_ctx.click_outside_registry = self.click_outside_registry.clone();
    #[cfg(feature = "i18n")]
    {
      child_ctx.i18n = self.i18n.clone();
    }
    child_ctx.inherit_contexts(&self.context_map);
    child_ctx.scope_id = slot_id;
    child_ctx.component_name = ForEachSlot::TYPE_NAME;
    #[cfg(feature = "perf_profile")]
    let _span = child_ctx.render_span(|| Some(crate::app::profiler::RenderTrigger::Mount));
    child_ctx.begin_render();
    let mut element = component_fn(&mut child_ctx, item);
    child_ctx.end_render();
    element.node = attach_component_metadata(
      element.node,
      component_tag_name::<ForEachSlot>(),
      slot_id,
      Some(key.as_str()),
      #[cfg(feature = "devtools")]
      &child_ctx,
    );

    let mut slot = ChildSlot {
      id: slot_id,
      key: Some(key),
      component: Box::new(ForEachSlot),
      ctx: child_ctx,
      rendered: None,
      rendered_at: 0,
      mounted: false,
      offstage: false,
      offstage_dirty: false,
    };
    slot.keep_rendered(&element.node);

    self.insert_child_slot(cursor, slot);

    element
  }

  // --- Error boundary ---

  pub fn error_boundary(
    &mut self,
    component_fn: impl FnOnce(&mut Ctx) -> Element,
    fallback_fn: impl FnOnce() -> Element,
  ) -> Element {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| component_fn(self)));
    match result {
      Ok(node) => node,
      Err(_) => fallback_fn(),
    }
  }

  // --- Render lifecycle ---

  pub fn begin_render(&mut self) {
    // Values provided by the previous render are dropped; `render` provides
    // them again if it still wants to. Their map is kept while it renders:
    // a value provided again unchanged keeps its entry, and a render that
    // leaves the same entries gives the map its revision back (`end_render`).
    if self.context_map.revision() != self.base_contexts.revision() {
      self.previous_render_contexts = Some(std::mem::replace(&mut self.context_map, self.base_contexts.clone()));
    }
    self.provided_in_render = false;
    self.clear_dirty();
    self.child_cursor = 0;
    self.element_ref_cursor = 0;
    self.click_outside_cursor = 0;
    self.click_outside_active_cursors.clear();
    self.future_cursor = 0;
    #[cfg(feature = "query")]
    {
      self.query_cursor = 0;
    }
    tracking::start_tracking();
    self.rendering = true;
  }

  fn set_child_slot(&mut self, cursor: usize, slot: ChildSlot) {
    if cursor < self.children.len() {
      self.children[cursor].ctx.clear_modal_entries_recursive();
      if self.children[cursor].mounted {
        self.children[cursor].component.on_unmounted();
      }
      self.children[cursor] = slot;
    } else {
      self.children.push(slot);
    }
  }

  fn insert_child_slot(&mut self, cursor: usize, slot: ChildSlot) {
    if cursor < self.children.len() {
      self.children.insert(cursor, slot);
    } else {
      self.children.push(slot);
    }
  }

  fn clear_modal_entries_recursive(&mut self) {
    self
      .click_outside_registry
      .lock()
      .retain(|entry| entry.scope_id != self.scope_id);
    for slot in &mut self.children {
      slot.ctx.clear_modal_entries_recursive();
    }
  }

  pub(crate) fn end_render(&mut self) {
    self
      .click_outside_registry
      .lock()
      .retain(|entry| entry.scope_id != self.scope_id || self.click_outside_active_cursors.contains(&entry.cursor));

    for slot in &self.children[self.child_cursor..] {
      if slot.mounted {
        slot.component.on_unmounted();
      }
    }
    for slot in &mut self.children[self.child_cursor..] {
      slot.ctx.clear_modal_entries_recursive();
    }
    self.children.truncate(self.child_cursor);

    for slot in &mut self.children {
      if !slot.mounted && !slot.offstage {
        slot.component.on_mounted();
        slot.mounted = true;
      }
    }

    self.element_refs.truncate(self.element_ref_cursor);
    // Dropping a slot cancels its task.
    self.drop_unreached_future_slots();
    #[cfg(feature = "query")]
    self.query_slots.truncate(self.query_cursor);
    self.rendering = false;
    if let Some(previous) = self.previous_render_contexts.take()
      && self.context_map.has_same_entries(&previous)
    {
      self.context_map.set_revision(previous.revision());
    }
    let deps = tracking::stop_tracking();
    let handles = deps
      .into_iter()
      .map(|dep| {
        let notifier = self.dirty_notifier(
          #[cfg(feature = "perf_profile")]
          dep.source,
        );
        let handle = (dep.subscribe_fn)(Arc::new(notifier));
        Box::new(handle) as Box<dyn Any + Send + Sync>
      })
      .collect();
    self.render_watch_handles = handles;
  }

  pub(crate) fn any_dirty(&self) -> bool {
    self.subtree_dirty.load(Ordering::Relaxed)
  }

  pub(crate) fn after_layout_recursive(&self) {
    for slot in self.children.iter().filter(|slot| !slot.offstage) {
      slot.component.after_layout();
      slot.ctx.after_layout_recursive();
    }
  }

  /// [`Self::after_layout_recursive`] in a pass a detailed recording takes:
  /// each component's span holds its hook and its descendants' spans.
  #[cfg(feature = "perf_profile")]
  pub(crate) fn after_layout_recursive_spans(&self, spans: &crate::app::profiler::detail::HookSpans) {
    for slot in self.children.iter().filter(|slot| !slot.offstage) {
      let _span = spans.enter(slot.component.type_name());
      slot.component.after_layout();
      slot.ctx.after_layout_recursive_spans(spans);
    }
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn has_active_timers(&self) -> bool {
    self.timers.iter().any(Timer::is_active)
      || self
        .children
        .iter()
        .any(|slot| !slot.offstage && slot.ctx.has_active_timers())
  }

  /// When the earliest running timer of this component or a mounted descendant fires.
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn next_timer_fire(&self) -> Option<Instant> {
    let own = self.timers.iter().filter_map(Timer::next_fire).min();
    let children = self
      .children
      .iter()
      .filter(|slot| !slot.offstage)
      .filter_map(|slot| slot.ctx.next_timer_fire())
      .min();
    own.into_iter().chain(children).min()
  }

  /// Whether a query of this component or a mounted descendant has work the next `tick_futures` must start or
  /// finish. Other futures wake the event loop themselves.
  #[cfg(feature = "query")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn has_ready_queries(&self) -> bool {
    self.query_registry.ready()
      || self
        .children
        .iter()
        .any(|slot| !slot.offstage && slot.ctx.has_ready_queries())
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn has_active_futures(&self) -> bool {
    #[cfg(feature = "query")]
    if self.query_registry.ready() {
      return true;
    }
    self.future_slots().any(|slot| slot.task.is_active())
      || self
        .children
        .iter()
        .any(|slot| !slot.offstage && slot.ctx.has_active_futures())
  }

  pub(crate) fn tick_timers(&mut self, now: Instant) -> bool {
    let mut fired = false;
    for timer in &self.timers {
      fired |= timer.tick(now);
    }
    for slot in self.children.iter_mut().filter(|slot| !slot.offstage) {
      fired |= slot.ctx.tick_timers(now);
    }
    fired
  }

  pub(crate) fn tick_futures(&mut self) -> bool {
    let waker = self.loop_waker();
    let mut cx = TaskContext::from_waker(&waker);
    #[cfg(feature = "query")]
    let query_changed = self.query_registry.tick(Instant::now(), &mut cx) | self.publish_query_updates();
    #[cfg(not(feature = "query"))]
    let query_changed = false;
    self.poll_futures(&mut cx) | query_changed
  }

  #[cfg(feature = "query")]
  pub(crate) fn next_query_deadline(&self) -> Option<Instant> {
    self.query_registry.deadline()
  }

  #[cfg(feature = "query")]
  fn publish_query_updates(&self) -> bool {
    let mut changed = false;
    for observer in &self.query_slots {
      changed |= observer.publish();
    }
    for slot in &self.children {
      changed |= slot.ctx.publish_query_updates();
    }
    changed
  }

  fn poll_futures(&mut self, cx: &mut TaskContext<'_>) -> bool {
    let mut completed = false;
    for slot in self.future_slots() {
      completed |= slot.task.poll(cx);
    }
    for slot in self.children.iter_mut().filter(|slot| !slot.offstage) {
      completed |= slot.ctx.poll_futures(cx);
    }
    completed
  }

  pub(crate) fn refresh_dirty_subtrees(&mut self) -> Vec<(u64, Box<Node>)> {
    let mut replacements = Vec::new();
    let dirty_child_slot_ids = self.take_dirty_child_slot_ids();

    for dirty_slot_id in dirty_child_slot_ids {
      let Some(index) = self.children.iter().position(|slot| slot.id == dirty_slot_id) else {
        continue;
      };
      let slot = &mut self.children[index];
      if !slot.ctx.any_dirty() {
        continue;
      }
      if slot.offstage {
        slot.offstage_dirty = true;
        slot.ctx.clear_dirty();
        continue;
      }

      if slot.ctx.is_dirty() {
        #[cfg(feature = "perf_profile")]
        let _span = slot.ctx.render_span(|| None);
        let old_rendered = slot.rendered.take();
        slot.ctx.begin_render();
        let mut element = slot.component.render(&mut slot.ctx);
        slot.ctx.end_render();
        let needs_followup_refresh = slot.ctx.any_dirty();
        element.node = attach_component_metadata(
          element.node,
          slot.component.tag_name(),
          slot.id,
          slot.key.as_deref(),
          #[cfg(feature = "devtools")]
          &slot.ctx,
        );
        if let Some(old) = old_rendered.as_ref() {
          element.node.preserve_runtime_state_from(old);
        }
        slot.keep_rendered(&element.node);
        replacements.push((slot.id, element.node));
        if needs_followup_refresh {
          Self::mark_dirty_child_slot(&self.dirty_child_slots, dirty_slot_id);
        }
        continue;
      } else {
        // The caches above a component hold a placeholder for it, so its
        // fresh output only goes to the live tree.
        replacements.extend(slot.ctx.refresh_dirty_subtrees());
      }
    }

    self.update_subtree_dirty_from_children();
    replacements
  }

  /// The slot of a component this one mounted, `for_each` items included.
  fn child_slot(&self, id: u64) -> Option<&ChildSlot> {
    for slot in &self.children {
      if slot.id == id {
        return Some(slot);
      }
      if slot.is_for_each_group()
        && let Some(item) = slot.ctx.children.iter().find(|item| item.id == id)
      {
        return Some(item);
      }
    }
    None
  }

  /// The slot of a component anywhere below this one.
  fn descendant_slot(&self, id: u64) -> Option<&ChildSlot> {
    for slot in &self.children {
      if slot.id == id {
        return Some(slot);
      }
      if let Some(found) = slot.ctx.descendant_slot(id) {
        return Some(found);
      }
    }
    None
  }

  #[cfg_attr(not(feature = "perf_profile"), allow(dead_code))]
  pub(crate) fn estimated_memory_bytes(&self) -> usize {
    std::mem::size_of::<Self>()
      + self
        .props
        .as_ref()
        .map(|_| std::mem::size_of::<Box<dyn Any + Send>>())
        .unwrap_or(0)
      + self.children.capacity() * std::mem::size_of::<ChildSlot>()
      + self.watch_handles.capacity() * std::mem::size_of::<Box<dyn Any + Send + Sync>>()
      + self.render_watch_handles.capacity() * std::mem::size_of::<Box<dyn Any + Send + Sync>>()
      + {
        #[cfg(feature = "devtools")]
        {
          self.memos_debug.capacity() * std::mem::size_of::<ComponentMemoDebug>()
            + self.effects_debug.capacity() * std::mem::size_of::<ComponentEffectDebug>()
        }
        #[cfg(not(feature = "devtools"))]
        {
          0
        }
      }
      + self.effects.capacity() * std::mem::size_of::<Effect>()
      + self.timers.capacity() * std::mem::size_of::<Timer>()
      + (self.future_slots.capacity() + self.stable_future_slots.capacity()) * std::mem::size_of::<FutureSlot>()
      + self.element_refs.capacity() * std::mem::size_of::<ElementRefMut>()
      + self.click_outside_active_cursors.capacity() * std::mem::size_of::<usize>()
      + self
        .children
        .iter()
        .map(ChildSlot::estimated_memory_bytes)
        .sum::<usize>()
  }
}

/// What a component found in the contexts its parent passes down.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Inherited {
  /// The parent's map is the one inherited last: nothing to take.
  Same,
  /// New values were taken, none the component renders again for; its
  /// descendants still take them (`propagate_contexts`).
  Refreshed,
  /// The component renders again: a value it read changed, or its render
  /// provides values of its own.
  Changed,
}

fn element_ref_is_click_outside(element_ref: &ElementRef, x: f32, y: f32) -> bool {
  if !element_ref.is_attached() {
    return false;
  }

  let rect = element_ref.bounds();
  x < rect.x || x > rect.x + rect.width || y < rect.y || y > rect.y + rect.height
}

struct ForEachSlot;

impl ForEachSlot {
  const TYPE_NAME: &'static str = "ForEachSlot";
}

impl AnyComponent for ForEachSlot {
  fn render(&self, _ctx: &mut Ctx) -> Element {
    Element::new()
  }
  fn after_layout(&self) {}
  fn on_mounted(&self) {}
  fn on_unmounted(&self) {}
  fn type_name(&self) -> &'static str {
    Self::TYPE_NAME
  }
  fn tag_name(&self) -> Arc<str> {
    Arc::from("ForEachSlot")
  }
}

impl ChildSlot {
  #[cfg_attr(not(feature = "perf_profile"), allow(dead_code))]
  fn estimated_memory_bytes(&self) -> usize {
    self.key.as_ref().map(|key| key.capacity()).unwrap_or(0)
      + std::mem::size_of::<Box<dyn AnyComponent>>()
      + self.ctx.estimated_memory_bytes()
  }

  fn is_for_each_group(&self) -> bool {
    self.key.is_none() && self.component.type_name() == ForEachSlot::TYPE_NAME
  }

  /// Keeps `node`, the output this slot's component just rendered, with
  /// placeholders for the components it mounted (`node/component_output.rs`).
  fn keep_rendered(&mut self, node: &Node) {
    let mut own = Vec::new();
    for child in &self.ctx.children {
      if child.is_for_each_group() {
        own.extend(child.ctx.children.iter().map(|item| item.id));
      } else {
        own.push(child.id);
      }
    }
    own.sort_unstable();
    self.rendered = Some(node.clone_own_output(&|id| own.binary_search(&id).is_ok()));
    self.rendered_at = next_render_generation();
  }

  /// The output of this slot's last render, put together again from its
  /// cache and those of the components in it, for a parent that mounts the
  /// component unchanged.
  fn assembled_output(&self) -> Option<Box<Node>> {
    self.assemble(&self.ctx, None).map(Box::new)
  }

  /// `scope` holds every slot whose component's root the output can hold:
  /// the unchanged component's own slot, which has no slot children.
  fn assemble(&self, scope: &Ctx, enclosing: Option<&SlotOwners<'_>>) -> Option<Node> {
    let rendered = self.rendered.as_deref()?;
    let owners = SlotOwners {
      ctx: &self.ctx,
      enclosing,
    };
    Some(rendered.clone_with_component_roots(&mut |root: &Node| {
      let id = root.component_slot_id()?;
      if root.is_component_slot_placeholder() {
        let mut output = self.ctx.child_slot(id)?.assemble(scope, Some(&owners))?;
        output.adopt_outer_decoration(root);
        return Some(output);
      }
      // A root this render was handed, kept whole: take its component's
      // fresh output when that component rendered again by itself since.
      let slot = enclosing
        .and_then(|owners| owners.slot(id))
        .or_else(|| scope.descendant_slot(id))?;
      if slot.rendered_at <= self.rendered_at {
        return None;
      }
      let mut output = slot.assemble(scope, Some(&owners))?;
      output.adopt_outer_decoration(root);
      Some(output)
    }))
  }
}

/// The components enclosing the one whose output is put together, nearest
/// first: the slots handed to a component are those of a component around it.
struct SlotOwners<'a> {
  ctx: &'a Ctx,
  enclosing: Option<&'a SlotOwners<'a>>,
}

impl<'a> SlotOwners<'a> {
  fn slot(&self, id: u64) -> Option<&'a ChildSlot> {
    self
      .ctx
      .child_slot(id)
      .or_else(|| self.enclosing.and_then(|owners| owners.slot(id)))
  }
}

impl Drop for Ctx {
  fn drop(&mut self) {
    for slot in &self.children {
      if slot.mounted {
        slot.component.on_unmounted();
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::Ctx;
  use crate::{
    app::component::Component,
    components::{Column, Rect},
    node::Element,
  };

  struct CachedChild;

  impl Component for CachedChild {
    type Props = ();

    fn create(_ctx: &mut Ctx) -> Self {
      Self
    }

    fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
      Column::new()
        .child(Rect::new(1.0, 1.0))
        .child(Rect::new(2.0, 2.0))
        .child(Rect::new(3.0, 3.0))
    }
  }

  struct CachedParent;

  impl Component for CachedParent {
    type Props = ();

    fn create(_ctx: &mut Ctx) -> Self {
      Self
    }

    fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
      Column::new().child(ctx.mount::<CachedChild>(()).id("child"))
    }
  }

  /// A slot keeps the nodes of its own component only, so a child that
  /// renders again by itself touches no cache above it.
  #[test]
  fn a_slot_keeps_a_placeholder_for_each_component_it_mounted() {
    let mut root = Ctx::new_root();
    let output = root.mount::<CachedParent>(());
    assert_eq!(output.node.children()[0].children().len(), 3);

    let parent = &root.children[0];
    let kept = &parent
      .rendered
      .as_deref()
      .expect("the parent kept its output")
      .children()[0];
    assert!(kept.is_component_slot_placeholder());
    assert!(kept.children().is_empty());
    assert_eq!(kept.component_slot_id(), Some(parent.ctx.children[0].id));

    let assembled = parent.assembled_output().expect("the parent's output");
    let child = &assembled.children()[0];
    assert!(!child.is_component_slot_placeholder());
    assert_eq!(child.children().len(), 3);
    assert_eq!(child.element_id(), Some("child"), "the mounting code's id is put back");
  }

  #[test]
  fn batch_defers_dirty_marking_until_end() {
    let mut ctx = Ctx::new_root();
    let signal = ctx.signal(0);
    ctx.clear_dirty();

    ctx.batch(|| {
      signal.set(1);
      assert!(!ctx.is_dirty());
      assert!(!ctx.any_dirty());
    });

    assert!(ctx.is_dirty());
    assert!(ctx.any_dirty());
  }

  #[test]
  fn dirty_mark_during_render_survives_end_render() {
    let mut ctx = Ctx::new_root();
    let signal = ctx.signal(0);
    ctx.clear_dirty();

    ctx.begin_render();
    signal.set(1);
    ctx.end_render();

    assert!(ctx.is_dirty());
    assert!(ctx.any_dirty());
  }

  #[test]
  fn dirty_mark_for_previous_render_dependency_survives_rerender() {
    let mut ctx = Ctx::new_root();
    let signal = ctx.signal(0);

    ctx.begin_render();
    let _ = signal.get();
    ctx.end_render();
    ctx.clear_dirty();

    ctx.begin_render();
    let _ = signal.get();
    signal.set(1);
    ctx.end_render();

    assert!(ctx.is_dirty());
    assert!(ctx.any_dirty());
  }

  #[test]
  fn descendant_dirty_marks_ancestor_subtrees_without_marking_ancestors_dirty() {
    let root = Ctx::new_root();
    let mut child = Ctx::new();
    child.inherit_dirty_ancestors_from(&root, 10);
    let mut grandchild = Ctx::new();
    grandchild.inherit_dirty_ancestors_from(&child, 20);

    root.clear_dirty();
    child.clear_dirty();
    grandchild.clear_dirty();

    grandchild.mark_self_dirty();

    assert!(!root.is_dirty());
    assert!(!child.is_dirty());
    assert!(grandchild.is_dirty());
    assert!(root.any_dirty());
    assert!(child.any_dirty());
    assert!(grandchild.any_dirty());
    assert_eq!(*root.dirty_child_slots.lock(), vec![10]);
    assert_eq!(*child.dirty_child_slots.lock(), vec![20]);

    grandchild.clear_dirty();
    let _ = child.take_dirty_child_slot_ids();
    let _ = root.take_dirty_child_slot_ids();
    child.update_subtree_dirty_from_children();
    root.update_subtree_dirty_from_children();

    assert!(!root.any_dirty());
    assert!(!child.any_dirty());
    assert!(!grandchild.any_dirty());
  }

  #[cfg(feature = "devtools")]
  #[test]
  fn signal_debug_value_updates_when_signal_changes() {
    let mut ctx = Ctx::new_root();
    let signal = ctx.signal(0_i32);
    let debug = ctx.signals_debug();

    assert_eq!(debug[0].formatted_value().as_deref(), Some("0"));

    signal.set(42);

    assert_eq!(debug[0].formatted_value().as_deref(), Some("42"));
  }

  #[cfg(feature = "devtools")]
  #[test]
  fn signal_debug_history_records_value_changes() {
    let mut ctx = Ctx::new_root();
    let signal = ctx.signal(0_i32);
    let debug = ctx.signals_debug();

    signal.set(1);
    signal.set(2);

    let history = debug[0].history();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].from_value, "0");
    assert_eq!(history[0].to_value, "1");
    assert_eq!(history[1].from_value, "1");
    assert_eq!(history[1].to_value, "2");
  }

  #[cfg(feature = "devtools")]
  #[test]
  fn signal_debug_subscriber_count_excludes_runtime_and_debug_hooks() {
    let mut ctx = Ctx::new_root();
    let signal = ctx.signal(0_i32);
    let debug = ctx.signals_debug();

    assert_eq!(debug[0].subscriber_count(), 0);

    ctx.watch(&signal, |_| {});
    assert_eq!(debug[0].subscriber_count(), 1);

    let memo_signal = signal.clone();
    let _memo = ctx.memo(move || memo_signal.get() + 1);
    assert_eq!(debug[0].subscriber_count(), 2);
  }

  #[cfg(feature = "devtools")]
  #[derive(crate::DevtoolsInspectable)]
  struct DebugSignalValue {
    count: i32,
    active: bool,
  }

  #[cfg(feature = "devtools")]
  #[test]
  fn signal_debug_value_uses_devtools_inspectable_fields() {
    let mut ctx = Ctx::new_root();
    let signal = ctx.signal(DebugSignalValue { count: 2, active: true });
    let debug = ctx.signals_debug();

    assert_eq!(debug[0].formatted_value().as_deref(), Some("count: 2, active: true"));

    signal.set(DebugSignalValue {
      count: 3,
      active: false,
    });

    assert_eq!(debug[0].formatted_value().as_deref(), Some("count: 3, active: false"));
  }

  #[cfg(feature = "devtools")]
  #[test]
  fn memo_debug_value_updates_when_dependencies_change() {
    let mut ctx = Ctx::new_root();
    let signal = ctx.signal(2_i32);
    let memo_signal = signal.clone();
    let _memo = ctx.memo(move || memo_signal.get() * 2);
    let debug = ctx.memos_debug();

    assert_eq!(debug[0].formatted_value().as_deref(), Some("4"));

    signal.set(3);

    assert_eq!(debug[0].formatted_value().as_deref(), Some("6"));
  }
}
