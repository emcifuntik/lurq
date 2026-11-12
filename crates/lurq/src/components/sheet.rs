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
  /// How high the content was last laid out, when the sheet is as high as its content.
  content_height: f32,
  /// Where the sheet's top is at each detent, and where it is when down, as last rendered.
  tops: Vec<f32>,
  room: f32,
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

  /// The same, told apart from its siblings by `key`.
  pub fn mount_keyed(ctx: &mut Ctx, key: &str, props: SheetProps, content: impl Into<Element>) -> Element {
    ctx.mount_keyed_with::<Self>(key, props, vec![content.into()])
  }
}

/// How high the sheet stands at `detent`, in a window `room` high whose highest sheet is `large` high.
fn height_of(detent: SheetDetent, room: f32, large: f32, bottom: f32, content: f32) -> f32 {
  let height = match detent {
    SheetDetent::Medium => room * 0.5,
    SheetDetent::Large => large,
    SheetDetent::Fraction(share) => large * share.clamp(0.0, 1.0),
    SheetDetent::Height(height) => height + bottom,
    SheetDetent::Content => content,
  };
  height.clamp(0.0, large.max(0.0))
}

impl Component for Sheet {
  type Props = SheetProps;

  fn create(ctx: &mut Ctx) -> Self {
    let state = Arc::new(Mutex::new(State {
      phase: Phase::Closed,
      begin_due: false,
      velocity: 0.0,
      at: 0,
      held: None,
      content_height: 0.0,
      tops: Vec::new(),
      room: 0.0,
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
    if state.phase == Phase::Closed {
      return Element::from(Modal::new(Element::new()).open_when(false));
    }

    let heights: Vec<f32> = props
      .detents
      .iter()
      .map(|detent| height_of(*detent, room, large, safe_area.bottom, state.content_height))
      .collect();
    state.tops = heights.iter().map(|height| room - height).collect();
    state.room = room;
    let height = heights[at];
    let highest = heights.iter().copied().fold(0.0, f32::max);
    let lowest = heights.iter().copied().fold(f32::INFINITY, f32::min);

    // Where the sheet's top is: under the window before it sets off and when it goes, at its height
    // otherwise, and as far from there as a finger has it. Past its highest place it gives way less and less.
    let dragged = drag != NO_DRAG;
    let top = match state.phase {
      Phase::Entering { started: false } | Phase::Leaving => room,
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
    let up = if lowest > 0.0 {
      ((room - top) / lowest).clamp(0.0, 1.0)
    } else {
      0.0
    };
    let fits_content = props.detents[at] == SheetDetent::Content;
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
      .transform(Transform2D::translate(0.0, top))
      .transition(Transition::transform().duration(pace).easing(curve))
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
    if state.phase == Phase::Closed {
      return;
    }
    let mut changed = false;
    if self.content.is_attached() {
      let height = self.content.rect().3;
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
