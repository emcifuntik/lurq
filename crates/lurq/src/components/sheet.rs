//! A sheet: content that comes up from the bottom of the window over everything in it, as a phone's apps
//! show a choice or a short task.
//!
//! - It comes up when its signal is set, and goes down when the signal is cleared, by Escape, by a tap beside
//!   it, or by a finger that drags it down far enough or fast enough.
//! - It stands at one of its [`SheetDetent`]s, and a finger moves it between them.
//! - Content that scrolls, named with [`SheetProps::scroll`], scrolls under a finger once the sheet is at its
//!   highest and gives the finger to the sheet when it is at its top.

use std::{sync::Arc, time::Duration};

use parking_lot::Mutex;

use crate::{
  animation::{Easing, Transition},
  app::{
    component::Component,
    ctx::{Ctx, Modal, Root, Timeout},
    events::{GesturePhase, PanEvent},
  },
  components::{Column, Row, Stack},
  core::{ElementRef, Signal},
  layout::{StackAlignment, layout_kind::ScrollState},
  node::{Element, color::Color, dimension::Dimension, transform::Transform2D},
};

const FILL: Dimension = Dimension::Pct(100.0);
/// How long after a sheet is first laid out, out of view, it sets off.
const BEGIN: Duration = Duration::from_millis(1);
/// How long after a move's duration the sheet is taken to be at rest.
const SETTLE_AFTER: Duration = Duration::from_millis(50);
/// How much of the sheet reaches under the window's bottom edge: what is seen under its content when a finger
/// has it higher than it stands.
const UNDER: f32 = 400.0;
/// How much of a pull past the sheet's highest place is given to it.
const RESISTANCE: f32 = 0.55;

/// A height a sheet stands at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SheetDetent {
  /// Half the window.
  Medium,
  /// The whole window under [`SheetStyle::top_margin`].
  Large,
  /// A share of [`SheetDetent::Large`], from 0 to 1.
  Fraction(f32),
  /// So many logical pixels, over the room the home indicator takes.
  Height(f32),
  /// As high as the content is.
  Content,
}

/// How a [`Sheet`] looks and moves. The values it starts with were set by eye beside the platform's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetStyle {
  /// The sheet's own color. A sheet that draws all of itself has a clear one and no radius.
  pub background: Color,
  /// How round the sheet's top corners are.
  pub corner_radius: f32,
  /// What is drawn over the window behind the sheet, and how much of it shows when the sheet is up: 0 leaves
  /// the window as it is.
  pub backdrop: Color,
  pub dim: f32,
  /// The bar at the sheet's top that says it can be dragged.
  pub grabber: bool,
  pub grabber_color: Color,
  pub grabber_width: f32,
  pub grabber_height: f32,
  /// How far under the sheet's top edge the grabber is.
  pub grabber_top: f32,
  /// How far under the status bar's room the highest sheet stops.
  pub top_margin: f32,
  /// The widest the sheet is; in a wider window it stands in the middle.
  pub max_width: f32,
  /// How long the sheet takes to come up, go down or move between heights. It follows a spring, which starts
  /// at the speed a finger let the sheet go with. Taken when the sheet is made.
  pub duration: Duration,
  /// How far ahead of where a finger let the sheet go it is taken to be going, in seconds of the finger's
  /// speed: the height nearest to there is where it goes.
  pub throw: f32,
}

impl Default for SheetStyle {
  fn default() -> Self {
    Self {
      background: Color::from_hex("#FFFFFF"),
      corner_radius: 20.0,
      backdrop: Color::from_hex("#000000"),
      dim: 0.3,
      grabber: false,
      grabber_color: Color::new(60, 60, 67, 77),
      grabber_width: 36.0,
      grabber_height: 5.0,
      grabber_top: 5.0,
      top_margin: 10.0,
      max_width: f32::INFINITY,
      duration: Duration::from_millis(500),
      throw: 0.15,
    }
  }
}

impl SheetStyle {
  pub fn new() -> Self {
    Self::default()
  }

  /// The platform's dark colors.
  pub fn dark() -> Self {
    Self {
      background: Color::from_hex("#1C1C1E"),
      grabber_color: Color::new(235, 235, 245, 77),
      dim: 0.5,
      ..Self::default()
    }
  }

  /// The platform's sheet for an appearance: `ctx.window().system.appearance`, or the theme's.
  pub fn for_appearance(appearance: crate::app::window::Appearance) -> Self {
    match appearance {
      crate::app::window::Appearance::Light => Self::new(),
      crate::app::window::Appearance::Dark => Self::dark(),
    }
  }

  pub fn background(mut self, color: Color) -> Self {
    self.background = color;
    self
  }

  pub fn corner_radius(mut self, radius: f32) -> Self {
    self.corner_radius = radius;
    self
  }

  pub fn dim(mut self, dim: f32) -> Self {
    self.dim = dim;
    self
  }

  pub fn grabber(mut self, grabber: bool) -> Self {
    self.grabber = grabber;
    self
  }
}

#[derive(Clone)]
pub struct SheetProps {
  open: Signal<bool>,
  detents: Vec<SheetDetent>,
  detent: Option<Signal<usize>>,
  style: SheetStyle,
  dismissable: bool,
  scroll: Option<ScrollState>,
  /// What makes the content, for a sheet whose content is made in it ([`Sheet::mount_with`]).
  content: Option<Arc<dyn Fn(&mut Ctx) -> Element + Send + Sync>>,
}

impl PartialEq for SheetProps {
  fn eq(&self, other: &Self) -> bool {
    let same_scroll = match (&self.scroll, &other.scroll) {
      (None, None) => true,
      (Some(a), Some(b)) => a.same(b),
      _ => false,
    };
    self.open.id() == other.open.id()
      && self.detents == other.detents
      && self.detent.as_ref().map(Signal::id) == other.detent.as_ref().map(Signal::id)
      && self.style == other.style
      && self.dismissable == other.dismissable
      && same_scroll
      && match (&self.content, &other.content) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
      }
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for SheetProps {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

impl SheetProps {
  /// A sheet that is up while `open` is set, as high as its content.
  pub fn new(open: Signal<bool>) -> Self {
    Self {
      open,
      detents: vec![SheetDetent::Content],
      detent: None,
      style: SheetStyle::default(),
      dismissable: true,
      scroll: None,
      content: None,
    }
  }

  /// The heights the sheet stands at. It comes up at the first; a finger moves it between them.
  pub fn detents(mut self, detents: impl IntoIterator<Item = SheetDetent>) -> Self {
    let detents: Vec<SheetDetent> = detents.into_iter().collect();
    if !detents.is_empty() {
      self.detents = detents;
    }
    self
  }

  /// Which of [`SheetProps::detents`] the sheet is at, by its place in them: the sheet goes where the signal
  /// says, and says here where a finger put it.
  pub fn detent(mut self, detent: Signal<usize>) -> Self {
    self.detent = Some(detent);
    self
  }

  pub fn style(mut self, style: SheetStyle) -> Self {
    self.style = style;
    self
  }

  /// Whether a tap beside the sheet, a drag down and Escape put it away. A sheet that is not dismissable goes
  /// down only when its signal is cleared.
  pub fn dismissable(mut self, dismissable: bool) -> Self {
    self.dismissable = dismissable;
    self
  }

  /// The state of the list the sheet's content scrolls in. With it a finger that goes down on the list drags
  /// the sheet only when the list is at its top, and one that goes up scrolls the list only when the sheet is
  /// at its highest.
  pub fn scroll(mut self, scroll: ScrollState) -> Self {
    self.scroll = Some(scroll);
    self
  }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
  /// Down, and not in the tree.
  Closed,
  /// Coming up; `started` once a frame was laid out with it under the window.
  Entering { started: bool },
  /// At rest at a detent, or under a finger.
  Open,
  /// On its way to a detent.
  Moving,
  /// Going down.
  Leaving,
}

struct State {
  phase: Phase,
  /// The sheet was first rendered and waits for its frame to set off.
  begin_due: bool,
  /// The speed a finger let the sheet go with, towards where it is going, as a share of the way left per the
  /// move's duration.
  velocity: f64,
  /// The detent last rendered.
  at: usize,
  /// How far the fingers had gone when they took the sheet, and where the sheet was then; `None` when no
  /// finger has it.
  held: Option<f32>,
  /// How high the content was last laid out, when the sheet is as high as its content, less what it was
  /// padded by at the bottom then: the padding changes with the keyboard, frame by frame, and the sheet with
  /// it in the same frame.
  content_height: f32,
  padded_by: f32,
  /// Where the sheet's top is at each detent, and where it is when down, as last rendered.
  tops: Vec<f32>,
  room: f32,
  /// How far the window behind draws back for this sheet, as rendered and as last told to the host.
  receded: Receded,
  told: Receded,
  host: Option<Behind>,
}

/// How far the content of a [`SheetHost`] is drawn back, from 0 to 1, and how it gets there.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Receded {
  share: f32,
  pace: Duration,
  curve: Easing,
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for Receded {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

impl Receded {
  const NONE: Self = Self {
    share: 0.0,
    pace: Duration::ZERO,
    curve: Easing::Linear,
  };
}

/// What a [`SheetHost`] gives the sheets under it: where they say how far its content draws back.
#[derive(Clone)]
struct Behind {
  receded: Signal<Receded>,
}

/// No finger has the sheet.
const NO_DRAG: f32 = f32::MIN;

pub struct Sheet {
  state: Arc<Mutex<State>>,
  changed: Signal<u64>,
  /// How far a finger has the sheet from where it stands, down being more, or [`NO_DRAG`].
  drag: Signal<f32>,
  /// The detent of a sheet whose props name no signal for it.
  detent: Signal<usize>,
  begin: Timeout,
  settle: Timeout,
  content: ElementRef,
}

impl Sheet {
  /// A sheet with `content`. It takes no room where it is mounted: it is laid over the window.
  pub fn mount(ctx: &mut Ctx, props: SheetProps, content: impl Into<Element>) -> Element {
    ctx.mount_with::<Self>(props, vec![content.into()])
  }

  /// A sheet whose content is made in it, as a route's is in its outlet. It is the way to put a stack of
  /// screens in a sheet: what `content` mounts is the sheet's own, so a `StackOutlet` there has its bar at the
  /// sheet's top, with nothing of the window's edges to keep clear of (`ctx.safe_area()` is nothing inside a
  /// sheet).
  pub fn mount_with(
    ctx: &mut Ctx,
    mut props: SheetProps,
    content: impl Fn(&mut Ctx) -> Element + Send + Sync + 'static,
  ) -> Element {
    props.content = Some(Arc::new(content));
    ctx.mount::<Self>(props)
  }

  /// [`Sheet::mount_with`], told apart from its siblings by `key`.
  pub fn mount_keyed_with(
    ctx: &mut Ctx,
    key: &str,
    mut props: SheetProps,
    content: impl Fn(&mut Ctx) -> Element + Send + Sync + 'static,
  ) -> Element {
    props.content = Some(Arc::new(content));
    ctx.mount_keyed::<Self>(key, props)
  }

  /// [`Sheet::mount`], told apart from its siblings by `key`.
  pub fn mount_keyed(ctx: &mut Ctx, key: &str, props: SheetProps, content: impl Into<Element>) -> Element {
    ctx.mount_keyed_with::<Self>(key, props, vec![content.into()])
  }
}

/// How high the sheet stands at `detent`, in a window `room` high whose highest sheet is `large` high.
/// The on-screen keyboard is under the sheet: a sheet rises by the `keyboard`'s height, as far as it can.
fn height_of(detent: SheetDetent, room: f32, large: f32, bottom: f32, keyboard: f32, content: f32) -> f32 {
  let height = match detent {
    SheetDetent::Medium => room * 0.5 + keyboard,
    SheetDetent::Large => large,
    SheetDetent::Fraction(share) => large * share.clamp(0.0, 1.0) + keyboard,
    SheetDetent::Height(height) => height + bottom,
    SheetDetent::Content => content + bottom,
  };
  height.clamp(0.0, large.max(0.0))
}

impl Component for Sheet {
  type Props = SheetProps;

  fn create(ctx: &mut Ctx) -> Self {
    // The sheet keeps its content clear of the window's edges itself.
    ctx.provide(crate::app::ctx::SafeAreaTaken);
    let state = Arc::new(Mutex::new(State {
      phase: Phase::Closed,
      begin_due: false,
      velocity: 0.0,
      at: 0,
      held: None,
      content_height: 0.0,
      padded_by: 0.0,
      tops: Vec::new(),
      room: 0.0,
      receded: Receded::NONE,
      told: Receded::NONE,
      host: None,
    }));
    let changed = Signal::new(0u64);
    let begin = ctx.create_timeout(BEGIN, {
      let (state, changed) = (state.clone(), changed.clone());
      move || {
        if let Phase::Entering { started } = &mut state.lock().phase {
          *started = true;
        }
        changed.update(|version| *version += 1);
      }
    });
    let duration = ctx.props::<SheetProps>().style.duration;
    let settle = ctx.create_timeout(duration + SETTLE_AFTER, {
      let (state, changed) = (state.clone(), changed.clone());
      move || {
        {
          let mut state = state.lock();
          state.phase = match state.phase {
            Phase::Leaving | Phase::Closed => Phase::Closed,
            _ => Phase::Open,
          };
          state.velocity = 0.0;
        }
        changed.update(|version| *version += 1);
      }
    });
    Self {
      state,
      changed,
      drag: Signal::new(NO_DRAG),
      detent: Signal::new(0),
      begin,
      settle,
      content: ElementRef::default(),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<SheetProps>().clone();
    let style = props.style;
    let open = props.open.get();
    let _ = self.changed.get();
    let drag = self.drag.get();
    let detent = props.detent.clone().unwrap_or_else(|| self.detent.clone());
    let at = detent.get().min(props.detents.len() - 1);

    let window = ctx.window();
    let (room, safe_area) = (window.logical_height(), window.safe_area);
    let large = room - safe_area.top - style.top_margin;

    let mut state = self.state.lock();
    match (open, state.phase) {
      (true, Phase::Closed) => {
        state.phase = Phase::Entering { started: false };
        state.velocity = 0.0;
        state.held = None;
        // It sets off once this frame, with the sheet under the window, is laid out: `after_layout`.
        state.begin_due = true;
        self.settle.restart();
      }
      (true, Phase::Leaving) => {
        state.phase = Phase::Moving;
        self.settle.restart();
      }
      (false, Phase::Entering { .. } | Phase::Open | Phase::Moving) => {
        state.phase = Phase::Leaving;
        state.held = None;
        // What was typed into in the sheet is no longer: the keyboard goes down with the sheet.
        ctx.blur();
        self.begin.cancel();
        self.settle.restart();
      }
      _ => {}
    }
    if state.phase == Phase::Open && state.at != at {
      // Sent to another height by the signal.
      state.phase = Phase::Moving;
      self.settle.restart();
    }
    state.at = at;
    state.host = ctx.use_context::<Behind>();
    if state.phase == Phase::Closed {
      state.receded = Receded::NONE;
      return Element::from(Modal::new(Element::new()).open_when(false));
    }

    let heights: Vec<f32> = props
      .detents
      .iter()
      .map(|detent| {
        height_of(
          *detent,
          room,
          large,
          safe_area.bottom,
          safe_area.keyboard,
          state.content_height,
        )
      })
      .collect();
    state.tops = heights.iter().map(|height| room - height).collect();
    state.room = room;
    let height = heights[at];
    let highest = heights.iter().copied().fold(0.0, f32::max);
    let lowest = heights.iter().copied().fold(f32::INFINITY, f32::min);

    // Where the sheet's top is: under the window before it sets off and when it goes, at its height
    // otherwise, and as far from there as a finger has it. Past its highest place it gives way less and less.
    let dragged = drag != NO_DRAG;
    // With motion reduced the sheet does not cross the window: it fades in where it stands, and out.
    let reduce_motion = window.system.reduce_motion;
    let away = matches!(state.phase, Phase::Entering { started: false } | Phase::Leaving);
    let top = match state.phase {
      Phase::Entering { started: false } | Phase::Leaving if !reduce_motion => room,
      _ if dragged => {
        let top = room - height + drag;
        let limit = room - highest;
        if top < limit {
          let over = limit - top;
          limit - over * RESISTANCE * room / (room + RESISTANCE * over)
        } else if !props.dismissable && top > room - lowest {
          let (limit, over) = (room - lowest, top - (room - lowest));
          limit + over * RESISTANCE * room / (room + RESISTANCE * over)
        } else {
          top
        }
      }
      _ => room - height,
    };
    let moving = !dragged
      && matches!(
        state.phase,
        Phase::Entering { started: true } | Phase::Moving | Phase::Leaving
      );
    // Every node that moves has its transition from its first frame; put where a finger has it, or where it
    // sets off from, the transition takes no time.
    let pace = if moving { style.duration } else { Duration::ZERO };
    let curve = Easing::Spring {
      velocity: state.velocity,
    };
    // How much of the lowest height is in view: the window behind dims by as much.
    let up = if reduce_motion && away {
      0.0
    } else if lowest > 0.0 {
      ((room - top) / lowest).clamp(0.0, 1.0)
    } else {
      0.0
    };
    let fits_content = props.detents[at] == SheetDetent::Content;
    state.padded_by = safe_area.bottom;
    // A sheet that takes all the room sends the window behind back, by as much of the way from its next
    // height down, or from the bottom, as it has come.
    let reaches = highest >= large - 1.0 && large > 0.0;
    let under = heights
      .iter()
      .copied()
      .filter(|height| *height < highest - 1.0)
      .fold(0.0, f32::max);
    state.receded = Receded {
      share: if reaches {
        ((room - top - under) / (highest - under)).clamp(0.0, 1.0)
      } else {
        0.0
      },
      pace,
      curve,
    };
    drop(state);

    let backdrop = Row::new()
      .size(FILL, FILL)
      .background(style.backdrop)
      .opacity(style.dim * up)
      .transition(Transition::opacity().duration(pace).easing(curve))
      .on_click({
        let (open, dismissable) = (props.open.clone(), props.dismissable);
        move |_| {
          if dismissable {
            open.set(false);
          }
        }
      });

    // The content, in the room the sheet has at its height, clear of the home indicator and the keyboard. A
    // sheet as high as its content lets the content say how high that is.
    let mut content = Column::new()
      .ref_element(self.content.clone())
      .width(FILL)
      .padding_bottom(safe_area.bottom)
      .padding_left(safe_area.left)
      .padding_right(safe_area.right);
    content = if fits_content {
      content.max_height(large.max(0.0))
    } else {
      content.height(height)
    };
    if let Some(build) = &props.content {
      content = content.child(build(ctx));
    }
    for child in ctx.children() {
      content = content.child(child.clone());
    }
    let mut body = Stack::new().width(FILL).child(content.clip());
    if style.grabber {
      body = body.child(
        Row::new()
          .width(FILL)
          .padding_top(style.grabber_top)
          .justify(crate::layout::layout_kind::Justify::Center)
          .pointer_events_none()
          .child(
            Row::new()
              .width(style.grabber_width)
              .height(style.grabber_height)
              .rounded(style.grabber_height / 2.0)
              .background(style.grabber_color),
          ),
      );
    }

    let panel = Column::new()
      .id("sheet")
      .width(FILL)
      .max_width(style.max_width)
      .height(room + UNDER)
      .background(style.background)
      .rounded(style.corner_radius)
      .clip()
      .child(body)
      .opacity(if reduce_motion && away { 0.0 } else { 1.0 })
      .transform(Transform2D::translate(0.0, top))
      .transition(Transition::transform().duration(pace).easing(curve))
      .transition(Transition::opacity().duration(pace).easing(curve))
      // A press on the sheet is the sheet's: it is not a tap beside it.
      .on_click(|_| {})
      .on_pan({
        let (state, drag, changed) = (self.state.clone(), self.drag.clone(), self.changed.clone());
        let (open, detent, settle) = (props.open.clone(), detent.clone(), self.settle.clone());
        let (scroll, dismissable) = (props.scroll.clone(), props.dismissable);
        move |event: PanEvent| {
          let mut state = state.lock();
          match event.phase {
            GesturePhase::Start => {
              if state.phase != Phase::Open || event.fingers != 1 {
                return;
              }
              if event.translation_y.abs() <= event.translation_x.abs() {
                return;
              }
              let top = state.tops.get(state.at).copied().unwrap_or(state.room);
              let highest = state.tops.iter().copied().fold(f32::INFINITY, f32::min);
              let at_highest = top <= highest + 0.5;
              let takes = if event.translation_y > 0.0 {
                // Down: the content's while it has something above to show.
                scroll.as_ref().is_none_or(|scroll| scroll.scroll_y() <= 0.0)
              } else {
                !at_highest
              };
              if !takes {
                return;
              }
              // The sheet's: the fingers neither scroll nor click.
              event.prevent_default();
              event.stop_propagation();
              state.held = Some(event.translation_y);
              drop(state);
              drag.set(0.0);
            }
            GesturePhase::Change => {
              if let Some(from) = state.held {
                event.stop_propagation();
                drop(state);
                drag.set(event.translation_y - from);
              }
            }
            GesturePhase::End => {
              let Some(from) = state.held.take() else {
                return;
              };
              event.stop_propagation();
              let stands = state.tops.get(state.at).copied().unwrap_or(state.room);
              let highest = state.tops.iter().copied().fold(f32::INFINITY, f32::min);
              let now = (stands + event.translation_y - from).max(highest);
              // Where it is going: the height, or the way down, nearest to where the finger threw it.
              let thrown = now + event.velocity_y * style.throw;
              let mut goes = (None, state.room);
              let mut nearest = if dismissable {
                (thrown - state.room).abs()
              } else {
                f32::INFINITY
              };
              for (index, top) in state.tops.iter().enumerate() {
                if (thrown - top).abs() < nearest {
                  nearest = (thrown - top).abs();
                  goes = (Some(index), *top);
                }
              }
              let left = goes.1 - now;
              state.velocity = if left.abs() > 1.0 {
                f64::from((event.velocity_y * style.duration.as_secs_f32() / left).max(0.0))
              } else {
                0.0
              };
              state.phase = Phase::Moving;
              let at = state.at;
              drop(state);
              drag.set(NO_DRAG);
              settle.restart();
              match goes.0 {
                Some(index) if index != at => detent.set(index),
                Some(_) => changed.update(|version| *version += 1),
                None => open.set(false),
              }
            }
          }
        }
      });

    let layer = Stack::new()
      .size(FILL, FILL)
      .stack_align(StackAlignment::TopCenter)
      .child(backdrop)
      .child(panel);
    let mut modal = Modal::new(layer).open_when(true).target(Root);
    modal = if props.dismissable {
      modal.dismisses(props.open.clone())
    } else {
      modal.dismiss_on_escape(false)
    };
    modal.into()
  }

  fn after_layout(&self) {
    let mut state = self.state.lock();
    if state.receded != state.told {
      state.told = state.receded;
      if let Some(host) = &state.host {
        host.receded.set(state.receded);
      }
    }
    if state.phase == Phase::Closed {
      return;
    }
    let mut changed = false;
    if self.content.is_attached() {
      let height = (self.content.rect().3 - state.padded_by).max(0.0);
      if (state.content_height - height).abs() > 0.25 {
        state.content_height = height;
        changed = true;
      }
    }
    if std::mem::take(&mut state.begin_due) {
      self.begin.restart();
    }
    drop(state);
    if changed {
      self.changed.update(|version| *version += 1);
    }
  }
}

/// How the content of a [`SheetHost`] draws back behind a sheet that takes all the room.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetHostStyle {
  /// How small the content gets: a share of its size. 1 leaves it as it is.
  pub scale: f32,
  /// How round its corners get.
  pub corner_radius: f32,
  /// What is seen around it.
  pub background: Color,
}

impl Default for SheetHostStyle {
  fn default() -> Self {
    Self {
      scale: 0.92,
      corner_radius: 12.0,
      background: Color::from_hex("#000000"),
    }
  }
}

/// What a [`SheetHost`] is made with: how it looks, and what makes its content.
#[derive(Clone)]
pub struct SheetHostProps {
  style: SheetHostStyle,
  content: Arc<dyn Fn(&mut Ctx) -> Element + Send + Sync>,
}

impl PartialEq for SheetHostProps {
  fn eq(&self, other: &Self) -> bool {
    self.style == other.style && Arc::ptr_eq(&self.content, &other.content)
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for SheetHostProps {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

/// The content of a window that draws back behind a sheet at its full height, as a phone's apps do: it gets
/// smaller, its top goes under the status bar's room and its corners round. Put it around the whole of the
/// app's content; the sheets mounted in that content tell it how far they are up. Without it a sheet only dims
/// what is behind.
pub struct SheetHost {
  receded: Signal<Receded>,
}

impl SheetHost {
  /// The host around what `content` makes. The content is made in the host, as a route's is in its outlet:
  /// that is what lets the sheets in it find the host.
  pub fn mount(ctx: &mut Ctx, content: impl Fn(&mut Ctx) -> Element + Send + Sync + 'static) -> Element {
    Self::mount_with(ctx, SheetHostStyle::default(), content)
  }

  pub fn mount_with(
    ctx: &mut Ctx,
    style: SheetHostStyle,
    content: impl Fn(&mut Ctx) -> Element + Send + Sync + 'static,
  ) -> Element {
    ctx.mount::<Self>(SheetHostProps {
      style,
      content: Arc::new(content),
    })
  }
}

impl Component for SheetHost {
  type Props = SheetHostProps;

  fn create(ctx: &mut Ctx) -> Self {
    let receded = Signal::new(Receded::NONE);
    ctx.provide(Behind {
      receded: receded.clone(),
    });
    Self { receded }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<SheetHostProps>().clone();
    let style = props.style;
    let receded = self.receded.get();
    let window = ctx.window();
    let (room, top) = (window.logical_height(), window.safe_area.top);
    // Smaller about its middle, and up or down so that its top is where the status bar's room ends.
    let scale = 1.0 - (1.0 - style.scale) * receded.share;
    let down = receded.share * (top - room * (1.0 - style.scale) / 2.0);
    let content = Stack::new()
      .size(FILL, FILL)
      .rounded(style.corner_radius * receded.share)
      .clip()
      .transform(Transform2D::translate(0.0, down).then(&Transform2D::scale_uniform(scale)))
      .transition(Transition::transform().duration(receded.pace).easing(receded.curve))
      .child((props.content)(ctx));
    Stack::new()
      .size(FILL, FILL)
      .background(style.background)
      .child(content)
  }
}
