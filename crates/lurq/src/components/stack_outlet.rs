//! A stack of screens, as a phone's apps move between theirs: [`StackOutlet`] in the place of an
//! [`Outlet`](super::Outlet).
//!
//! The router's history is the stack. Every entry from the first to the current one that has a route at this
//! outlet is a screen, and stays mounted under the ones pushed over it, with its state and its scroll: only the
//! top one is laid out and painted, and the one under it while the top one is on its way in or out.
//!
//! - A push brings the new screen in from the right, over the one before, which gives way a little and dims.
//! - Going back takes the top screen out to the right, and it is unmounted when it is gone.
//! - A finger in from the left edge drags the top screen with it; lifted far enough or fast enough it goes
//!   back, and otherwise the screen returns.
//! - A replace, and any other change of the history, swaps the screens without a move.
//!
//! With a [`NavigationBar`] the outlet draws a bar over its screens: the top screen's title, and the way back
//! with the title of the screen under it. A screen says what the bar shows with
//! [`Ctx::navigation_item`](crate::app::ctx::Ctx::navigation_item). The titles move with the screens.

use std::{any::Any, sync::Arc, time::Duration};

use parking_lot::Mutex;

use super::router_component::render_route_match;
use crate::{
  animation::{Easing, Transition},
  app::{
    component::Component,
    ctx::{Ctx, Timeout},
    events::{EdgePanEvent, GesturePhase, ScreenEdge},
  },
  components::{Column, Row, Stack, Text},
  core::{ElementRef, Signal},
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{Element, color::Color, dimension::Dimension, transform::Transform2D},
  router::{
    Navigator, StackEntry,
    route_match::{OutletDepth, RouteMatch, RouterMatches, StackState},
  },
};

/// How long a screen takes to come in or go out, and the curve it follows: quick at first, settling slowly.
const MOVE: Duration = Duration::from_millis(350);
const CURVE: Easing = Easing::CubicBezier {
  x1: 0.32,
  y1: 0.72,
  x2: 0.0,
  y2: 1.0,
};
/// A frame is painted with the screens where the move starts before they are sent on their way: a transition
/// goes from what was last painted.
const BEGIN: Duration = Duration::from_millis(1);
/// When the screens are taken to have arrived.
const SETTLE: Duration = Duration::from_millis(400);
/// How far the screen under gives way, as a share of the width, and how dark it gets.
const PARALLAX: f32 = 0.3;
const DIM: f32 = 0.1;
/// How far across a finger from the edge takes the screen for it to go back when lifted, as a share of the
/// width, and the speed that takes it back from anywhere, in logical pixels per second.
const BACK_SHARE: f32 = 0.5;
const BACK_SPEED: f32 = 500.0;
/// No finger is dragging the top screen.
const NO_DRAG: f32 = -1.0;
/// The bar's height, and its height in a window too low for that: a phone held on its side.
const BAR_HEIGHT: f32 = 44.0;
const LOW_BAR_HEIGHT: f32 = 32.0;
const LOW_WINDOW: f32 = 480.0;
/// What the way back says when the title of the screen under has no room, or there is none.
const BACK: &str = "Back";
/// The way back's arrow, its stroke, and the space between it and the label.
const CHEVRON_WIDTH: f32 = 11.0;
const CHEVRON_HEIGHT: f32 = 20.0;
const CHEVRON_STROKE: f32 = 2.6;
const BACK_SPACING: f32 = 5.0;
/// From the bar's edge to where the way back's label starts, and the least space between the label and the
/// title.
const BACK_LEADING: f32 = 9.0 + CHEVRON_WIDTH + BACK_SPACING;
const BACK_GAP: f32 = 8.0;
const FILL: Dimension = Dimension::Pct(100.0);

/// The bar a [`StackOutlet`] draws over its screens, and its colors. The bar is as high as a phone's, 44
/// logical pixels under the safe area's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavigationBar {
  pub background: Color,
  /// The way back, and what a screen puts at the bar's trailing end by default.
  pub tint: Color,
  pub title: Color,
  /// The line under the bar.
  pub border: Color,
}

impl NavigationBar {
  /// A light bar.
  pub fn new() -> Self {
    Self {
      background: Color::from_hex("#f9f9f9"),
      tint: Color::from_hex("#007aff"),
      title: Color::from_hex("#000000"),
      border: Color::from_hex("#c6c6c8"),
    }
  }

  /// A dark bar.
  pub fn dark() -> Self {
    Self {
      background: Color::from_hex("#1c1c1e"),
      tint: Color::from_hex("#0a84ff"),
      title: Color::from_hex("#ffffff"),
      border: Color::from_hex("#38383a"),
    }
  }

  pub fn background(mut self, color: Color) -> Self {
    self.background = color;
    self
  }

  pub fn tint(mut self, color: Color) -> Self {
    self.tint = color;
    self
  }

  pub fn title(mut self, color: Color) -> Self {
    self.title = color;
    self
  }

  pub fn border(mut self, color: Color) -> Self {
    self.border = color;
    self
  }
}

impl Default for NavigationBar {
  fn default() -> Self {
    Self::new()
  }
}

/// What a screen of a [`StackOutlet`] shows in the outlet's bar, and whether a finger from the edge takes it
/// back. A screen gives it in its `render` with
/// [`Ctx::navigation_item`](crate::app::ctx::Ctx::navigation_item).
#[derive(Clone, Default)]
pub struct NavigationItem {
  pub(crate) title: String,
  pub(crate) back_title: Option<String>,
  pub(crate) back_gesture: bool,
  pub(crate) trailing: Option<Element>,
  pub(crate) title_view: Option<Element>,
  pub(crate) bar_hidden: bool,
}

impl NavigationItem {
  pub fn new(title: impl Into<String>) -> Self {
    Self {
      title: title.into(),
      back_title: None,
      back_gesture: true,
      trailing: None,
      title_view: None,
      bar_hidden: false,
    }
  }

  /// What the way back says on the screen pushed over this one, in place of this one's title.
  pub fn back_title(mut self, title: impl Into<String>) -> Self {
    self.back_title = Some(title.into());
    self
  }

  /// Whether a finger in from the left edge takes this screen back; it does unless set otherwise. A screen
  /// with something unsaved turns it off. The bar's way back is not affected.
  pub fn back_gesture(mut self, enabled: bool) -> Self {
    self.back_gesture = enabled;
    self
  }

  /// What is at the bar's trailing end while this screen is the top one. The bar shows it as it was when the
  /// item's title, back title or back gesture last changed.
  pub fn trailing(mut self, element: impl Into<Element>) -> Self {
    self.trailing = Some(element.into());
    self
  }

  /// What stands in the middle of the bar in place of the title: a control, a picture. The title still names
  /// the screen on the way back to it. Shown as [`trailing`](Self::trailing) is.
  pub fn title_view(mut self, element: impl Into<Element>) -> Self {
    self.title_view = Some(element.into());
    self
  }

  /// Takes the bar away while this screen is the top one: the screen then reaches the top of the outlet, under
  /// the status bar, and keeps clear of it by the safe area itself.
  pub fn bar_hidden(mut self, hidden: bool) -> Self {
    self.bar_hidden = hidden;
    self
  }

  fn same_as(&self, other: &Self) -> bool {
    self.title == other.title
      && self.back_title == other.back_title
      && self.back_gesture == other.back_gesture
      && self.trailing.is_some() == other.trailing.is_some()
      && self.title_view.is_some() == other.title_view.is_some()
      && self.bar_hidden == other.bar_hidden
  }
}

/// Where a screen's [`NavigationItem`] goes: provided to each screen of a [`StackOutlet`].
#[derive(Clone)]
pub(crate) struct NavigationSlot {
  item: Arc<Mutex<NavigationItem>>,
  /// The outlet reads this in its render, to be rendered again when an item changes.
  changed: Signal<u64>,
}

impl NavigationSlot {
  pub(crate) fn set(&self, item: NavigationItem) {
    let same = {
      let mut current = self.item.lock();
      let same = current.same_as(&item);
      *current = item;
      same
    };
    if !same {
      self.changed.update(|version| *version += 1);
    }
  }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StackOutletProps {
  bar: Option<NavigationBar>,
  background: Option<Color>,
}

impl StackOutletProps {
  pub fn new() -> Self {
    Self::default()
  }

  /// Draws a bar over the screens.
  pub fn bar(mut self, bar: NavigationBar) -> Self {
    self.bar = Some(bar);
    self
  }

  /// What is behind each screen. A screen that paints no background of its own shows the one under it
  /// through while it moves.
  pub fn background(mut self, color: Color) -> Self {
    self.background = Some(color);
    self
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for StackOutletProps {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

struct Layer {
  entry: StackEntry,
  matches: Arc<Vec<RouteMatch>>,
  bar: LayerBar,
}

/// What a screen keeps of its part of the bar from one render to the next.
#[derive(Clone, Default)]
struct LayerBar {
  item: Arc<Mutex<NavigationItem>>,
  /// The title as it is laid out in the bar, and the way back's whole label laid out unseen: how wide each is
  /// says whether the label has room beside the title (`after_layout`).
  title: ElementRef,
  back_probe: ElementRef,
  /// The label does not fit, and the way back says only that it goes back.
  short_back: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Motion {
  None,
  /// The top screen is coming in; `started` once a frame was painted with it out of view.
  Entering {
    started: bool,
  },
  /// The screen that was on top (`Shown::leaving`) is going out; `started` once a frame was painted with it
  /// where it was.
  Leaving {
    started: bool,
  },
  /// A finger let the top screen go short of going back, and it returns.
  Returning,
}

struct Shown {
  layers: Vec<Layer>,
  leaving: Option<Layer>,
  motion: Motion,
  /// The outlet's width when it was last rendered.
  width: f32,
  /// A move was set up by the last render and waits for its frame to send the screens on their way.
  begin_due: bool,
}

pub struct StackOutlet {
  shown: Arc<Mutex<Shown>>,
  /// Bumped for a render that no signal of the router's asks for: a move's next step, a changed item.
  changed: Signal<u64>,
  /// How far a finger has dragged the top screen, or [`NO_DRAG`].
  drag: Signal<f32>,
  begin: Timeout,
  settle: Timeout,
}

impl Component for StackOutlet {
  type Props = StackOutletProps;

  fn create(ctx: &mut Ctx) -> Self {
    let shown = Arc::new(Mutex::new(Shown {
      layers: Vec::new(),
      leaving: None,
      motion: Motion::None,
      width: 0.0,
      begin_due: false,
    }));
    let changed = Signal::new(0u64);
    let drag = Signal::new(NO_DRAG);
    let begin = ctx.create_timeout(BEGIN, {
      let (shown, changed, drag) = (shown.clone(), changed.clone(), drag.clone());
      move || {
        if let Motion::Entering { started } | Motion::Leaving { started } = &mut shown.lock().motion {
          *started = true;
        }
        drag.set(NO_DRAG);
        changed.update(|version| *version += 1);
      }
    });
    let settle = ctx.create_timeout(SETTLE, {
      let (shown, changed) = (shown.clone(), changed.clone());
      move || {
        {
          let mut shown = shown.lock();
          shown.motion = Motion::None;
          shown.leaving = None;
        }
        changed.update(|version| *version += 1);
      }
    });
    Self {
      shown,
      changed,
      drag,
      begin,
      settle,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<StackOutletProps>().clone();
    let depth = ctx.use_context::<OutletDepth>().unwrap_or(OutletDepth(0)).0 + 1;
    let Some(navigator) = ctx.use_context::<Navigator>() else {
      return ctx.children().first().cloned().unwrap_or_else(Element::new);
    };
    // Rendered again when the history moves, a move takes its next step or a finger drags.
    let _ = navigator.path().get();
    let _ = self.changed.get();
    let drag = self.drag.get();

    // The screens: the entries at the history's end that have a route at this outlet.
    let mut entries = Vec::new();
    for entry in navigator.handle.stack().into_iter().rev() {
      let matches = navigator.handle.inner.routes.resolve(&entry.path);
      if matches.len() <= depth {
        break;
      }
      entries.push((entry, Arc::new(matches)));
    }
    entries.reverse();
    if entries.is_empty() {
      return ctx.children().first().cloned().unwrap_or_else(Element::new);
    }

    let window = ctx.window();
    let (width, safe_area) = (window.logical_width(), window.safe_area);
    let bar_height = if window.logical_height() < LOW_WINDOW {
      LOW_BAR_HEIGHT
    } else {
      BAR_HEIGHT
    };

    let mut shown = self.shown.lock();
    shown.width = width;
    let before: Vec<u64> = shown.layers.iter().map(|layer| layer.entry.id).collect();
    let after: Vec<u64> = entries.iter().map(|(entry, _)| entry.id).collect();
    if before != after {
      let pushed = !before.is_empty() && after.len() > before.len() && after[..before.len()] == before[..];
      let popped = before.len() > after.len() && before[..after.len()] == after[..];
      let mut old = std::mem::take(&mut shown.layers);
      let top = old.pop();
      shown.leaving = None;
      shown.motion = Motion::None;
      if popped {
        shown.leaving = top;
        shown.motion = Motion::Leaving { started: false };
      } else {
        old.extend(top);
        if pushed {
          shown.motion = Motion::Entering { started: false };
        }
      }
      shown.layers = entries
        .into_iter()
        .map(|(entry, matches)| {
          let bar = old
            .iter()
            .find(|layer| layer.entry.id == entry.id)
            .map(|layer| layer.bar.clone())
            .unwrap_or_default();
          Layer { entry, matches, bar }
        })
        .collect();
      self.begin.cancel();
      if shown.motion == Motion::None {
        self.settle.cancel();
      } else {
        // The move starts once this frame, with the screens where it starts from, is laid out: `after_layout`.
        shown.begin_due = true;
        self.settle.restart();
      }
    }

    // The screen in front and the one behind it, how far out of view the front one is, as a share of the
    // width, and whether they are on their way there or put there.
    let count = shown.layers.len();
    let dragged = if drag >= 0.0 && width > 0.0 {
      (drag / width).clamp(0.0, 1.0)
    } else {
      0.0
    };
    let below = count.checked_sub(2).map(|index| shown.layers[index].entry.id);
    let top = shown.layers.last().map(|layer| layer.entry.id);
    let (front, back, out, moving) = match shown.motion {
      Motion::Entering { started } => (top, below, if started { 0.0 } else { 1.0 }, started),
      Motion::Leaving { started } => (
        shown.leaving.as_ref().map(|layer| layer.entry.id),
        top,
        if started { 1.0 } else { dragged },
        started,
      ),
      Motion::Returning => (top, below, 0.0, true),
      Motion::None if drag >= 0.0 && below.is_some() => (top, below, dragged, false),
      Motion::None => (top, None, 0.0, false),
    };

    // Every node that moves has its transition from its first frame, which is what lets one start from where
    // the node was painted; put where a finger has it, the transition takes no time.
    let pace = if moving { MOVE } else { Duration::ZERO };
    let slide = Transition::transform().duration(pace).easing(CURVE);
    let fade = Transition::opacity().duration(pace).easing(CURVE);
    let mut screens = Stack::new().size(FILL, FILL).clip();
    for layer in shown.layers.iter().chain(shown.leaving.iter()) {
      let id = Some(layer.entry.id);
      let content = ctx.mount_keyed_offstage::<StackLayer>(
        &layer.entry.id.to_string(),
        StackLayerProps {
          id: layer.entry.id,
          path: layer.entry.path.clone(),
          matches: layer.matches.clone(),
          depth,
          state: layer.entry.state.clone(),
          slot: NavigationSlot {
            item: layer.bar.item.clone(),
            changed: self.changed.clone(),
          },
        },
        id == front || id == back,
      );
      // Every screen has its holder, shown or not, in the order of the stack: a node keeps its identity, and
      // with it where it was last painted, by its place among its siblings.
      let offset = if id == front {
        out * width
      } else if id == back {
        -PARALLAX * width * (1.0 - out)
      } else {
        0.0
      };
      let mut holder = Stack::new()
        .key(format!("stack-screen-{}", layer.entry.id))
        .size(FILL, FILL)
        .child(content)
        .transform(Transform2D::translate(offset, 0.0));
      if let Some(background) = props.background {
        holder = holder.background(background);
      }
      if id == back {
        let shade = Row::new()
          .size(FILL, FILL)
          .background(Color::from_hex("#000000"))
          .opacity(DIM * (1.0 - out))
          .pointer_events_none();
        holder = holder.child(shade.transition(fade.clone()));
      }
      screens = screens.child(holder.transition(slide.clone()));
    }

    let screens = screens.on_edge_pan({
      let (shown, drag, changed) = (self.shown.clone(), self.drag.clone(), self.changed.clone());
      let (navigator, settle) = (navigator.clone(), self.settle.clone());
      move |event: EdgePanEvent| {
        if event.edge != ScreenEdge::Left {
          return;
        }
        let mut shown = shown.lock();
        if shown.layers.len() < 2 {
          return;
        }
        // The stack's: the history is not also moved by the swipe this would end as.
        event.prevent_default();
        event.stop_propagation();
        let allowed = shown
          .layers
          .last()
          .is_some_and(|layer| layer.bar.item.lock().back_gesture);
        if !allowed || shown.motion != Motion::None {
          return;
        }
        match event.phase {
          GesturePhase::Start | GesturePhase::Change => {
            drop(shown);
            drag.set(event.translation.max(0.0));
          }
          GesturePhase::End => {
            let far = event.translation > shown.width * BACK_SHARE || event.velocity > BACK_SPEED;
            if far {
              // The screen goes on out from where the finger left it: the drag is let go once that has started.
              drop(shown);
              navigator.back();
            } else {
              shown.motion = Motion::Returning;
              drop(shown);
              drag.set(NO_DRAG);
              settle.restart();
              changed.update(|version| *version += 1);
            }
          }
        }
      }
    });

    let Some(bar) = props.bar else {
      return screens.into();
    };

    // The bar: the front screen's item, and the back screen's while they move.
    let layers: Vec<&Layer> = shown.layers.iter().chain(shown.leaving.iter()).collect();
    let part_of = |id: Option<u64>| {
      let index = layers.iter().position(|layer| Some(layer.entry.id) == id)?;
      let layer = layers[index];
      let back_label = index.checked_sub(1).map(|under| {
        let under = layers[under].bar.item.lock();
        match under.back_title.clone() {
          Some(title) => title,
          None if under.title.is_empty() => BACK.to_owned(),
          None => under.title.clone(),
        }
      });
      Some(BarPart {
        item: layer.bar.item.lock().clone(),
        back_label,
        short_back: layer.bar.short_back,
        title: layer.bar.title.clone(),
        back_probe: layer.bar.back_probe.clone(),
      })
    };
    let (back_part, front_part) = (part_of(back), part_of(front));
    // How much of the bar there is: all of it, or none over a screen that hides it, and between the two as
    // one such screen gives way to another.
    let shows = |part: &Option<BarPart>| part.as_ref().map(|part| if part.item.bar_hidden { 0.0 } else { 1.0 });
    let front_shows = shows(&front_part).unwrap_or(1.0);
    let shown_share = front_shows * (1.0 - out) + shows(&back_part).unwrap_or(front_shows) * out;
    let mut items = Stack::new().width(FILL).height(bar_height);
    if let Some(part) = back_part {
      items = items.child(bar_item(
        &bar,
        &navigator,
        part,
        out,
        -(1.0 - out) * width * PARALLAX,
        pace,
        false,
      ));
    }
    if let Some(part) = front_part {
      items = items.child(bar_item(
        &bar,
        &navigator,
        part,
        1.0 - out,
        out * width * 0.5,
        pace,
        true,
      ));
    }
    drop(shown);

    Column::new()
      .size(FILL, FILL)
      .child(
        Column::new()
          .width(FILL)
          .height((safe_area.top + bar_height) * shown_share)
          .clip()
          .transition(Transition::height().duration(pace).easing(CURVE))
          .child(
            Column::new()
              .width(FILL)
              .padding_top(safe_area.top)
              .padding_left(safe_area.left)
              .padding_right(safe_area.right)
              .background(bar.background)
              .child(items),
          ),
      )
      .child(Row::new().width(FILL).height(0.5 * shown_share).background(bar.border))
      .child(Column::new().width(FILL).flex(1.0).child(screens))
      .into()
  }

  fn after_layout(&self) {
    let mut shown = self.shown.lock();
    if std::mem::take(&mut shown.begin_due) {
      self.begin.restart();
    }
    // Whether each way back has room for its label beside the title, now that both are laid out. The title
    // is in the middle of the bar, so the label has half of what the title leaves.
    let width = shown.width;
    let shown = &mut *shown;
    let mut changed = false;
    for layer in shown.layers.iter_mut().chain(shown.leaving.iter_mut()) {
      if !layer.bar.title.is_attached() || !layer.bar.back_probe.is_attached() {
        continue;
      }
      let room = (width - layer.bar.title.width()) / 2.0 - BACK_LEADING - BACK_GAP;
      let short = layer.bar.back_probe.width() > room;
      changed |= std::mem::replace(&mut layer.bar.short_back, short) != short;
    }
    if changed {
      self.changed.update(|version| *version += 1);
    }
  }
}

/// One screen's part of the bar.
struct BarPart {
  item: NavigationItem,
  /// What the way back says when there is room; `None` on the screen at the bottom, which has none.
  back_label: Option<String>,
  short_back: bool,
  title: ElementRef,
  back_probe: ElementRef,
}

/// One screen's part of the bar: its title in the middle, the way back before it and what the screen put at
/// the end. `shift` is how far the title is from the middle.
fn bar_item(
  bar: &NavigationBar,
  navigator: &Navigator,
  part: BarPart,
  opacity: f32,
  shift: f32,
  pace: Duration,
  front: bool,
) -> Element {
  let text = |content: &str, weight: FontWeight, color: Color| {
    Text::styled(
      content,
      TextStyle {
        font_size: 17.0,
        weight,
        color,
        ..TextStyle::default()
      },
    )
    .nowrap()
  };
  let middle: Element = match &part.item.title_view {
    Some(view) => view.clone(),
    None => text(&part.item.title, FontWeight::SemiBold, bar.title).into(),
  };
  let title = Row::new()
    .size(FILL, FILL)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .pointer_events_none()
    .child(Row::new().ref_element(part.title.clone()).child(middle))
    .transform(Transform2D::translate(shift, 0.0))
    .transition(Transition::transform().duration(pace).easing(CURVE));

  let mut whole = Stack::new().size(FILL, FILL).child(title);
  let mut sides = Row::new().size(FILL, FILL).align_items(Alignment::Center);
  if let Some(label) = &part.back_label {
    let navigator = navigator.clone();
    let says = if part.short_back { BACK } else { label.as_str() };
    sides = sides.child(
      Row::new()
        .id("navigation-back")
        .align_items(Alignment::Center)
        .height(FILL)
        .padding_left(BACK_LEADING - CHEVRON_WIDTH - BACK_SPACING)
        .padding_right(12.0)
        .spacing(BACK_SPACING)
        .child(chevron(bar.tint))
        .child(text(says, FontWeight::Normal, bar.tint))
        .on_click(move |_| {
          navigator.back();
        }),
    );
    // The whole label, laid out and not seen, for its width.
    whole =
      whole.child(
        Row::new().opacity(0.0).pointer_events_none().child(
          Row::new()
            .ref_element(part.back_probe.clone())
            .child(text(label, FontWeight::Normal, bar.tint)),
        ),
      );
  }
  sides = sides.child(Row::new().flex(1.0));
  if let Some(trailing) = &part.item.trailing {
    sides = sides.child(
      Row::new()
        .align_items(Alignment::Center)
        .padding_right(8.0)
        .child(trailing.clone()),
    );
  }

  let mut whole = whole.child(sides).opacity(opacity);
  if !front {
    whole = whole.pointer_events_none();
  }
  whole
    .transition(Transition::opacity().duration(pace).easing(CURVE))
    .into()
}

/// The way back's arrow: two strokes that meet at a point on the left, as the platform draws it.
fn chevron(color: Color) -> Element {
  // Each stroke goes from the point, at the middle of the left side, to a corner on the right.
  let (rise, run) = (
    CHEVRON_HEIGHT / 2.0 - CHEVRON_STROKE / 2.0,
    CHEVRON_WIDTH - CHEVRON_STROKE,
  );
  let length = rise.hypot(run) + CHEVRON_STROKE;
  let angle = rise.atan2(run);
  let stroke = |up: bool| {
    let (centre_x, centre_y) = (
      CHEVRON_STROKE / 2.0 + run / 2.0,
      CHEVRON_HEIGHT / 2.0 + if up { -rise / 2.0 } else { rise / 2.0 },
    );
    Row::new()
      .width(length)
      .height(CHEVRON_STROKE)
      .rounded(CHEVRON_STROKE / 2.0)
      .background(color)
      .transform(
        Transform2D::translate(centre_x - length / 2.0, centre_y - CHEVRON_STROKE / 2.0)
          .then(&Transform2D::rotate(if up { -angle } else { angle })),
      )
  };
  Stack::new()
    .width(CHEVRON_WIDTH)
    .height(CHEVRON_HEIGHT)
    .child(stroke(true))
    .child(stroke(false))
    .into()
}

impl StackOutlet {
  /// A stack of screens with no bar.
  pub fn mount(ctx: &mut Ctx) -> Element {
    ctx.mount::<Self>(StackOutletProps::default())
  }

  pub fn mount_with(ctx: &mut Ctx, props: StackOutletProps) -> Element {
    ctx.mount::<Self>(props)
  }
}

#[derive(Clone)]
struct StackLayerProps {
  id: u64,
  path: String,
  matches: Arc<Vec<RouteMatch>>,
  depth: usize,
  state: Option<Arc<dyn Any + Send + Sync>>,
  slot: NavigationSlot,
}

impl PartialEq for StackLayerProps {
  fn eq(&self, other: &Self) -> bool {
    self.id == other.id && self.path == other.path && self.depth == other.depth
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for StackLayerProps {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

/// One screen: the route of its history entry, which reads its own params and state whatever the router's
/// current path is.
struct StackLayer;

impl Component for StackLayer {
  type Props = StackLayerProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<StackLayerProps>().clone();
    ctx.provide(RouterMatches(props.matches.clone()));
    ctx.provide(OutletDepth(props.depth));
    ctx.provide(StackState(props.state.clone()));
    ctx.provide(props.slot.clone());
    match props.matches.get(props.depth) {
      Some(route_match) => render_route_match(ctx, route_match),
      None => Element::new(),
    }
  }
}
