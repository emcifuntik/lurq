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

use std::{
  any::Any,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};

use parking_lot::Mutex;

use super::router_component::render_route_match;
use crate::{
  animation::{Easing, Transition},
  app::{
    component::Component,
    ctx::{Ctx, Timeout},
    events::{EdgePanEvent, GesturePhase, LongPressEvent, ScreenEdge},
    window::SafeAreaInsets,
  },
  components::{Column, Row, ScrollVertical, Stack, Text, TextOverflow},
  core::{ElementRef, Signal},
  layout::{
    Alignment,
    layout_kind::{Justify, ScrollState},
    text_style::{FontWeight, TextStyle},
  },
  node::{Element, color::Color, dimension::Dimension, transform::Transform2D},
  router::{
    Navigator, StackEntry,
    route_match::{OutletDepth, RouteMatch, RouterMatches, StackState},
  },
};

/// How a [`StackOutlet`]'s screens and bar move. The values it starts with were set by eye beside the
/// platform's, not measured against it; an app that knows better gives its own with
/// [`StackOutletProps::motion`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackMotion {
  /// How long a screen takes to come in or go out. It follows a spring ([`Easing::Spring`]), which starts at
  /// the speed a finger let the screen go with. Taken when the outlet is made.
  pub duration: Duration,
  /// How far the screen under gives way while one is over it, as a share of the width.
  pub parallax: f32,
  /// How dark the screen under gets, from 0 to 1.
  pub dim: f32,
  /// How far across a finger from the edge takes the top screen for it to go back when lifted, as a share of
  /// the width.
  pub back_share: f32,
  /// The speed of the finger that takes the screen back from anywhere, in logical pixels a second.
  pub back_speed: f32,
  /// How long the bar's title and color take to come and go as a large title scrolls out of view and back.
  pub title_fade: Duration,
  /// How much larger a large title gets as its content is pulled down: 1.1 is a tenth larger.
  pub large_title_growth: f32,
  /// The pull, in logical pixels, that takes it there.
  pub large_title_pull: f32,
}

impl Default for StackMotion {
  fn default() -> Self {
    Self {
      duration: Duration::from_millis(500),
      parallax: 0.3,
      dim: 0.1,
      back_share: 0.5,
      back_speed: 500.0,
      title_fade: Duration::from_millis(200),
      large_title_growth: 1.1,
      large_title_pull: 150.0,
    }
  }
}

/// A frame is painted with the screens where the move starts before they are sent on their way: a transition
/// goes from what was last painted.
const BEGIN: Duration = Duration::from_millis(1);
/// How long after a move's duration the screens are taken to have arrived.
const SETTLE_AFTER: Duration = Duration::from_millis(50);
/// No finger is dragging the top screen.
const NO_DRAG: f32 = -1.0;
/// The bar's height, and its height in a window too low for that: a phone held on its side.
const BAR_HEIGHT: f32 = 44.0;
/// A large title: its size, the space around it, and how long the bar's own title takes to come and go.
const LARGE_TITLE_SIZE: f32 = 34.0;
const LARGE_TITLE_HEIGHT: f32 = 52.0;
const LARGE_TITLE_INSET: f32 = 16.0;
/// How large the bar's text is beside a large title.
const SMALL_TITLE_SHARE: f32 = 17.0 / LARGE_TITLE_SIZE;
const LOW_BAR_HEIGHT: f32 = 32.0;
const LOW_WINDOW: f32 = 480.0;
/// What the way back says when the title of the screen under has no room, or there is none.
const BACK: &str = "Back";
/// The way back's arrow, its stroke, and the space between it and the label.
const CHEVRON_WIDTH: f32 = 11.0;
const CHEVRON_HEIGHT: f32 = 20.0;
const CHEVRON_STROKE: f32 = 2.6;
const BACK_SPACING: f32 = 5.0;
/// The point size the platform's arrow is asked for at: the label's.
#[cfg(all(target_os = "ios", feature = "raster"))]
const CHEVRON_POINT_SIZE: f32 = 17.0;
/// From the bar's edge to where the way back's label starts, and the least space between the label and the
/// title.
const BACK_LEADING: f32 = 9.0 + CHEVRON_WIDTH + BACK_SPACING;
const BACK_GAP: f32 = 8.0;
/// How wide the list of the screens under is.
const BACK_MENU_WIDTH: f32 = 250.0;
/// About how wide [`BACK`] is in the bar's text.
const BACK_WIDTH: f32 = 40.0;
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
  pub title: String,
  pub back_title: Option<String>,
  pub back_gesture: bool,
  pub leading: Option<Element>,
  pub trailing: Option<Element>,
  pub title_view: Option<Element>,
  pub bar_hidden: bool,
  pub large_title: bool,
}

impl NavigationItem {
  pub fn new(title: impl Into<String>) -> Self {
    Self {
      title: title.into(),
      back_title: None,
      back_gesture: true,
      leading: None,
      trailing: None,
      title_view: None,
      bar_hidden: false,
      large_title: false,
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

  /// What is at the bar's leading end in place of the way back: a control of the screen's own. It is shown on
  /// the bottom screen too, which has no way back. Shown as [`trailing`](Self::trailing) is.
  pub fn leading(mut self, element: impl Into<Element>) -> Self {
    self.leading = Some(element.into());
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

  /// Shows the title large, at the top of the screen's content, where it scrolls away with the content; the
  /// bar's own title comes in once it has. The screen puts its content in
  /// [`StackOutlet::large_title_scroll`], which draws the large title over it and scrolls the two.
  pub fn large_title(mut self, large: bool) -> Self {
    self.large_title = large;
    self
  }

  fn same_as(&self, other: &Self) -> bool {
    self.title == other.title
      && self.back_title == other.back_title
      && self.back_gesture == other.back_gesture
      && self.leading.is_some() == other.leading.is_some()
      && self.trailing.is_some() == other.trailing.is_some()
      && self.title_view.is_some() == other.title_view.is_some()
      && self.bar_hidden == other.bar_hidden
      && self.large_title == other.large_title
  }
}

/// Where a screen's [`NavigationItem`] goes: provided to each screen of a [`StackOutlet`].
#[derive(Clone)]
pub(crate) struct NavigationSlot {
  item: Arc<Mutex<NavigationItem>>,
  /// The outlet reads this in its render, to be rendered again when an item changes.
  changed: Signal<u64>,
  /// What scrolls the screen's content under a large title, and how far a finger has pulled that content
  /// down past its top, which the large title grows with.
  scroll: ScrollState,
  pull: Signal<f32>,
  /// See [`LayerBar`].
  large: ElementRef,
  flying: Signal<bool>,
  /// How much a large title grows, and over what pull ([`StackMotion`]).
  growth: (f32, f32),
  /// The color the bar gives a title.
  title_color: Option<Color>,
}

impl NavigationSlot {
  pub(crate) fn set(&self, item: NavigationItem) {
    let same = {
      let mut current = self.item.lock();
      // An element the screen gave cannot be told from the one it gave before: the bar is drawn anew for
      // every render of a screen that gives one, to show it as it now is.
      let same =
        current.same_as(&item) && item.leading.is_none() && item.trailing.is_none() && item.title_view.is_none();
      *current = item;
      same
    };
    if !same {
      self.changed.update(|version| *version += 1);
    }
  }
}

/// What a bar of an app's own is drawn from: [`StackOutletProps::bar_with`].
#[derive(Clone)]
pub struct NavigationBarState {
  /// The screen in front: the top one, or the one on its way out.
  pub top: NavigationBarScreen,
  /// The screen behind it, while the two move or a finger drags the front one.
  pub under: Option<NavigationBarScreen>,
  /// How far out of view the front screen is, from 0, in place, to 1, gone. It is where the screens are going
  /// when `duration` is not zero, and where they are put when it is: under a finger.
  pub out: f32,
  /// How long the screens take to get there, and the curve they follow. Giving a part of the bar a
  /// transition of this duration and easing moves it with the screens.
  pub duration: Duration,
  pub easing: Easing,
  /// The outlet's width.
  pub width: f32,
  pub safe_area: SafeAreaInsets,
  pub navigator: Navigator,
}

/// A screen's part in a [`NavigationBarState`].
#[derive(Clone)]
pub struct NavigationBarScreen {
  pub item: NavigationItem,
  /// What the way back from this screen says: the title of the screen under it, or what that screen gave
  /// for it. `None` on the bottom screen.
  pub back_label: Option<String>,
}

/// Draws a bar from the state of the stack. It is called at each render of the outlet.
pub type NavigationBarBuilder = Arc<dyn Fn(&NavigationBarState) -> Element + Send + Sync>;

#[derive(Clone)]
enum Bar {
  Standard(NavigationBar),
  Custom(NavigationBarBuilder),
}

#[derive(Clone, Default)]
pub struct StackOutletProps {
  bar: Option<Bar>,
  background: Option<Color>,
  motion: StackMotion,
}

impl PartialEq for StackOutletProps {
  fn eq(&self, other: &Self) -> bool {
    let same_bar = match (&self.bar, &other.bar) {
      (None, None) => true,
      (Some(Bar::Standard(a)), Some(Bar::Standard(b))) => a == b,
      (Some(Bar::Custom(a)), Some(Bar::Custom(b))) => Arc::ptr_eq(a, b),
      _ => false,
    };
    same_bar && self.background == other.background && self.motion == other.motion
  }
}

impl std::fmt::Debug for StackOutletProps {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("StackOutletProps")
      .field("bar", &self.bar.is_some())
      .field("background", &self.background)
      .finish()
  }
}

impl StackOutletProps {
  pub fn new() -> Self {
    Self::default()
  }

  /// Draws a bar over the screens.
  pub fn bar(mut self, bar: NavigationBar) -> Self {
    self.bar = Some(Bar::Standard(bar));
    self
  }

  /// Draws a bar of the app's own over the screens: what `builder` returns for the state of the stack, across
  /// the top of the outlet at the height it takes. The builder keeps its content clear of the status bar by
  /// the state's safe area, and moves it with the screens by the state's `out`, `duration` and `easing`.
  /// Make the builder once and keep it: a new one at each render renders the outlet again.
  pub fn bar_with(mut self, builder: NavigationBarBuilder) -> Self {
    self.bar = Some(Bar::Custom(builder));
    self
  }

  /// How the screens and the bar move, in place of [`StackMotion::default`].
  pub fn motion(mut self, motion: StackMotion) -> Self {
    self.motion = motion;
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
#[derive(Clone)]
struct LayerBar {
  item: Arc<Mutex<NavigationItem>>,
  /// The title as it is laid out in the bar, and the way back's whole label laid out unseen: how wide each is
  /// says whether the label has room beside the title (`after_layout`).
  title: ElementRef,
  back_probe: ElementRef,
  /// How much of the way back's label there is room for.
  label_fit: LabelFit,
  /// What scrolls the screen's content under a large title, and whether that title has scrolled out of view
  /// (`after_layout`).
  scroll: ScrollState,
  title_gone: Arc<AtomicBool>,
  pull: Signal<f32>,
  /// The large title as it is laid out in the content, and the way back's label as it is laid out in the bar:
  /// the two places a title goes between as a screen is pushed over this one or taken off it.
  large: ElementRef,
  label: ElementRef,
  /// The large title is on its way between the two, drawn by the outlet: the screen's own is not shown.
  flying: Signal<bool>,
}

impl Default for LayerBar {
  fn default() -> Self {
    Self {
      item: Arc::default(),
      title: ElementRef::default(),
      back_probe: ElementRef::default(),
      label_fit: LabelFit::Whole,
      scroll: ScrollState::new(),
      title_gone: Arc::default(),
      pull: Signal::new(0.0),
      large: ElementRef::default(),
      label: ElementRef::default(),
      flying: Signal::new(false),
    }
  }
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
  /// The speed a finger let the front screen go with, towards where it is going, as a share of the way left
  /// per the move's duration; 0 for a move no finger started.
  velocity: f64,
}

pub struct StackOutlet {
  shown: Arc<Mutex<Shown>>,
  /// Bumped for a render that no signal of the router's asks for: a move's next step, a changed item.
  changed: Signal<u64>,
  /// How far a finger has dragged the top screen, or [`NO_DRAG`].
  drag: Signal<f32>,
  begin: Timeout,
  settle: Timeout,
  /// The outlet as it is laid out: where its corner is in the window.
  origin: ElementRef,
  /// The list of the screens under the top one, which a long press on the way back opens.
  menu: BackMenu,
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
      velocity: 0.0,
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
    let duration = ctx.props::<StackOutletProps>().motion.duration;
    let settle = ctx.create_timeout(duration + SETTLE_AFTER, {
      let (shown, changed) = (shown.clone(), changed.clone());
      move || {
        {
          let mut shown = shown.lock();
          shown.motion = Motion::None;
          shown.leaving = None;
          shown.velocity = 0.0;
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
      origin: ElementRef::default(),
      menu: BackMenu {
        open: Signal::new(false),
        held: Arc::default(),
      },
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<StackOutletProps>().clone();
    let motion = props.motion;
    let depth = ctx.use_context::<OutletDepth>().unwrap_or(OutletDepth(0)).0 + 1;
    let Some(navigator) = ctx.use_context::<Navigator>() else {
      return ctx.children().first().cloned().unwrap_or_else(Element::new);
    };
    // Rendered again when the history moves, a move takes its next step or a finger drags.
    let _ = navigator.path().get();
    let _ = self.changed.get();
    let drag = self.drag.get();
    let menu_open = self.menu.open.get();

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
    // A window too low for it has no large titles, as a phone on its side has none.
    let low = window.logical_height() < LOW_WINDOW;
    let bar_height = if low { LOW_BAR_HEIGHT } else { BAR_HEIGHT };

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
        if drag < 0.0 {
          // No finger let it go: it sets off from rest.
          shown.velocity = 0.0;
        }
      } else {
        old.extend(top);
        if pushed {
          shown.motion = Motion::Entering { started: false };
          shown.velocity = 0.0;
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
    let pace = if moving { motion.duration } else { Duration::ZERO };
    let curve = Easing::Spring {
      velocity: shown.velocity,
    };
    let slide = Transition::transform().duration(pace).easing(curve);
    let fade = Transition::opacity().duration(pace).easing(curve);
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
            scroll: layer.bar.scroll.clone(),
            pull: layer.bar.pull.clone(),
            large: layer.bar.large.clone(),
            flying: layer.bar.flying.clone(),
            growth: (motion.large_title_growth, motion.large_title_pull),
            title_color: match &props.bar {
              Some(Bar::Standard(bar)) => Some(bar.title),
              _ => None,
            },
          },
        },
        id == front || id == back,
      );
      // Every screen has its holder, shown or not, in the order of the stack: a node keeps its identity, and
      // with it where it was last painted, by its place among its siblings.
      let offset = if id == front {
        out * width
      } else if id == back {
        -motion.parallax * width * (1.0 - out)
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
          .opacity(motion.dim * (1.0 - out))
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
            let far = event.translation > shown.width * motion.back_share || event.velocity > motion.back_speed;
            // The speed the screen goes on with: the finger's, towards where the screen is going, over the
            // way it has left.
            let (towards, left) = if far {
              (event.velocity, shown.width - event.translation)
            } else {
              (-event.velocity, event.translation)
            };
            shown.velocity = if left > 1.0 {
              f64::from(towards.max(0.0) * motion.duration.as_secs_f32() / left)
            } else {
              0.0
            };
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
    let bar = match bar {
      Bar::Standard(bar) => bar,
      Bar::Custom(builder) => {
        let layers: Vec<&Layer> = shown.layers.iter().chain(shown.leaving.iter()).collect();
        let screen_of = |id: Option<u64>| {
          let index = layers.iter().position(|layer| Some(layer.entry.id) == id)?;
          Some(NavigationBarScreen {
            item: layers[index].bar.item.lock().clone(),
            back_label: index
              .checked_sub(1)
              .map(|under| back_label_of(&layers[under].bar.item.lock())),
          })
        };
        let Some(top) = screen_of(front) else {
          return screens.into();
        };
        let state = NavigationBarState {
          top,
          under: screen_of(back),
          out,
          duration: pace,
          easing: curve,
          width,
          safe_area,
          navigator: navigator.clone(),
        };
        drop(shown);
        return Column::new()
          .size(FILL, FILL)
          .child(builder(&state))
          .child(Column::new().width(FILL).flex(1.0).child(screens))
          .into();
      }
    };

    // The bar: the front screen's item, and the back screen's while they move.
    let layers: Vec<&Layer> = shown.layers.iter().chain(shown.leaving.iter()).collect();
    let part_of = |id: Option<u64>| {
      let index = layers.iter().position(|layer| Some(layer.entry.id) == id)?;
      let layer = layers[index];
      let back_label = index
        .checked_sub(1)
        .map(|under| back_label_of(&layers[under].bar.item.lock()));
      // Taken and let go here: a lock taken inside the struct below would be held to its end.
      let item = layer.bar.item.lock().clone();
      let title_shown = !item.large_title || low || layer.bar.title_gone.load(Ordering::Acquire);
      Some(BarPart {
        item,
        back_label,
        label_fit: layer.bar.label_fit,
        title: layer.bar.title.clone(),
        back_probe: layer.bar.back_probe.clone(),
        title_shown,
        title_fade: motion.title_fade,
        // The bar less, at each side, the least a way back takes: its arrow and the word for it.
        title_room: (width - safe_area.left - safe_area.right - 2.0 * (BACK_LEADING + BACK_WIDTH + BACK_GAP)).max(0.0),
        label: layer.bar.label.clone(),
        label_hidden: false,
      })
    };
    let (back_part, mut front_part) = (part_of(back), part_of(front));
    // The screens under the top one, the nearest first: what a long press on the way back lists.
    let menu_titles: Vec<String> = shown
      .layers
      .iter()
      .rev()
      .skip(1)
      .map(|layer| back_label_of(&layer.bar.item.lock()))
      .collect();

    // A large title in view on the screen behind goes to the front screen's way back as that screen comes
    // in, and back as it goes out: one title, drawn here over both, between the two places.
    let layer_of = |id: Option<u64>| layers.iter().copied().find(|layer| Some(layer.entry.id) == id);
    let flight = match (layer_of(back), layer_of(front), &back_part, &front_part) {
      (Some(under), Some(over), Some(under_part), Some(over_part))
        if under_part.item.large_title
          && !low
          && !under.bar.title_gone.load(Ordering::Acquire)
          && !under_part.item.bar_hidden
          && !over_part.item.bar_hidden
          && over_part.item.leading.is_none()
          && over_part.back_label.is_some()
          && under.bar.large.is_attached() =>
      {
        let origin = self.origin.rect();
        let large = under.bar.large.rect();
        let label = over.bar.label.rect();
        // Until the label is laid out, at the push's first frame, it is taken to be where the title is: the
        // title is all there is to see then.
        let label = if over.bar.label.is_attached() && label.2 > 0.0 {
          label
        } else {
          (
            large.0,
            large.1,
            large.2 * SMALL_TITLE_SHARE,
            large.3 * SMALL_TITLE_SHARE,
          )
        };
        (large.2 > 0.0 && large.3 > 0.0).then(|| Flight {
          title: under_part.item.title.clone(),
          says: match over_part.label_fit {
            LabelFit::Whole => over_part.back_label.clone().unwrap_or_default(),
            LabelFit::Short => BACK.to_owned(),
            LabelFit::Arrow => String::new(),
          },
          large: (large.0 - origin.0, large.1 - origin.1, large.2, large.3),
          label: (label.0 - origin.0, label.1 - origin.1, label.2, label.3),
        })
      }
      _ => None,
    };
    for layer in &layers {
      let flying = flight.is_some() && Some(layer.entry.id) == back;
      if layer.bar.flying.get_untracked() != flying {
        layer.bar.flying.set(flying);
      }
    }
    if let (Some(_), Some(part)) = (&flight, &mut front_part) {
      part.label_hidden = true;
    }
    let overlay = Stack::new().size(FILL, FILL).pointer_events_none();
    let overlay = match &flight {
      Some(flight) => flight.draw(overlay, &bar, out, pace, curve),
      None => overlay,
    };
    // How much of the bar there is: all of it, or none over a screen that hides it, and between the two as
    // one such screen gives way to another.
    let shows = |part: &Option<BarPart>| part.as_ref().map(|part| if part.item.bar_hidden { 0.0 } else { 1.0 });
    let front_shows = shows(&front_part).unwrap_or(1.0);
    // The line under the bar comes with the bar's title: over a large title still in view the bar is one
    // with the content.
    let lines = |part: &Option<BarPart>| part.as_ref().map(|part| if part.title_shown { 1.0 } else { 0.0 });
    let front_lined = lines(&front_part).unwrap_or(1.0);
    // Between two screens it is between theirs, as far as the front one is out: the bar's color comes and
    // goes with the screens, and with a finger, and not after them.
    let lined = front_lined * (1.0 - out) + lines(&back_part).unwrap_or(front_lined) * out;
    let (line_pace, line_curve) = if moving {
      (pace, curve)
    } else if back.is_some() {
      (Duration::ZERO, curve)
    } else {
      (motion.title_fade, Easing::EASE)
    };
    let shown_share = front_shows * (1.0 - out) + shows(&back_part).unwrap_or(front_shows) * out;
    let mut items = Stack::new().width(FILL).height(bar_height);
    if let Some(part) = back_part {
      items = items.child(bar_item(
        &bar,
        &navigator,
        part,
        out,
        -(1.0 - out) * width * motion.parallax,
        pace,
        curve,
        false,
        &self.menu,
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
        curve,
        true,
        &self.menu,
      ));
    }
    drop(shown);

    let content = Column::new()
      .size(FILL, FILL)
      .child(
        Column::new()
          .width(FILL)
          .height((safe_area.top + bar_height) * shown_share)
          .clip()
          .transition(Transition::height().duration(pace).easing(curve))
          .child(
            Column::new()
              .width(FILL)
              .padding_top(safe_area.top)
              .padding_left(safe_area.left)
              .padding_right(safe_area.right)
              // Over a large title still in view the bar has no color of its own: it is one with what is
              // behind the screens.
              .background(Color::new(
                bar.background.r(),
                bar.background.g(),
                bar.background.b(),
                (f32::from(bar.background.a()) * lined).round() as u8,
              ))
              .transition(Transition::background_color().duration(line_pace).easing(line_curve))
              .child(items),
          ),
      )
      .child(
        Row::new()
          .width(FILL)
          .height(0.5 * shown_share)
          .background(bar.border)
          .opacity(lined)
          .transition(Transition::opacity().duration(line_pace).easing(line_curve)),
      )
      .child(Column::new().width(FILL).flex(1.0).child(screens));
    let mut whole = Stack::new()
      .size(FILL, FILL)
      .ref_element(self.origin.clone())
      .child(content)
      .child(overlay);
    if menu_open && !menu_titles.is_empty() {
      // Over everything: a tap on a screen's name goes back to it, and a tap anywhere else puts the list away.
      let close = self.menu.clone();
      let mut list = Column::new()
        .width(BACK_MENU_WIDTH)
        .background(bar.background)
        .border_inside(0.5, bar.border)
        .rounded(13.0)
        .clip()
        .absolute_position(safe_area.left + 8.0, safe_area.top + bar_height + 4.0);
      for (steps, title) in menu_titles.iter().enumerate() {
        let (navigator, open) = (navigator.clone(), self.menu.open.clone());
        let steps = steps + 1;
        if steps > 1 {
          list = list.child(Row::new().width(FILL).height(0.5).background(bar.border));
        }
        list = list.child(
          Row::new()
            .width(FILL)
            .height(BAR_HEIGHT)
            .align_items(Alignment::Center)
            .padding_horizontal(16.0)
            .child(
              Text::styled(
                title,
                TextStyle {
                  font_size: 17.0,
                  color: bar.title,
                  ..TextStyle::default()
                },
              )
              .nowrap(),
            )
            .on_click(move |_| {
              open.set(false);
              navigator.back_by(steps);
            }),
        );
      }
      whole = whole.child(
        Stack::new()
          .size(FILL, FILL)
          .child(Row::new().size(FILL, FILL).on_click(move |_| {
            // The list came up under the finger that opened it: that finger's lift leaves it open.
            if !close.held.swap(false, Ordering::AcqRel) {
              close.open.set(false);
            }
          }))
          .child(list),
      );
    }
    whole.into()
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
      let fit = if layer.bar.back_probe.width() <= room {
        LabelFit::Whole
      } else if BACK_WIDTH <= room {
        LabelFit::Short
      } else {
        LabelFit::Arrow
      };
      changed |= std::mem::replace(&mut layer.bar.label_fit, fit) != fit;
    }
    // Whether each large title has scrolled out of view, all but its last few points: the bar's own title
    // comes in then. Looked at after every layout, which every scroll ends in, however it was scrolled.
    for layer in shown.layers.iter().chain(shown.leaving.iter()) {
      let gone = layer.bar.scroll.scroll_y() > LARGE_TITLE_HEIGHT - 8.0;
      changed |= layer.bar.title_gone.swap(gone, Ordering::AcqRel) != gone;
      // And how far the content is pulled down past its top: the large title is drawn anew for it.
      let pull = (-layer.bar.scroll.overscroll().1).max(0.0);
      if (layer.bar.pull.get_untracked() - pull).abs() > 0.25 {
        layer.bar.pull.set(pull);
      }
    }
    if changed {
      self.changed.update(|version| *version += 1);
    }
  }
}

/// How much of a way back's label the bar has room for, beside the title.
#[derive(Clone, Copy, PartialEq)]
enum LabelFit {
  /// The title of the screen under.
  Whole,
  /// The word for going back.
  Short,
  /// Neither: the arrow alone.
  Arrow,
}

/// What the way back to a screen says.
fn back_label_of(under: &NavigationItem) -> String {
  match &under.back_title {
    Some(title) => title.clone(),
    None if under.title.is_empty() => BACK.to_owned(),
    None => under.title.clone(),
  }
}

/// The list a long press on the way back opens: whether it is open, and that the press which opened it is
/// still down, whose lift is not a tap on the way back.
#[derive(Clone)]
struct BackMenu {
  open: Signal<bool>,
  held: Arc<AtomicBool>,
}

/// A large title on its way between the content of the screen behind and the way back of the screen in
/// front: where it is at each end, in the outlet, and what it says there.
struct Flight {
  title: String,
  says: String,
  large: (f32, f32, f32, f32),
  label: (f32, f32, f32, f32),
}

impl Flight {
  /// Adds the title to `overlay`, `out` of the way from the label to the content: the large one, which
  /// shrinks and fades as it nears the label, and the label, which grows and fades as it nears the content.
  fn draw(&self, overlay: Stack, bar: &NavigationBar, out: f32, pace: Duration, curve: Easing) -> Stack {
    let between = |at_label: f32, at_large: f32| at_label + (at_large - at_label) * out;
    let centre = (
      between(self.label.0 + self.label.2 / 2.0, self.large.0 + self.large.2 / 2.0),
      between(self.label.1 + self.label.3 / 2.0, self.large.1 + self.large.3 / 2.0),
    );
    let part = |says: &str, size: f32, weight: FontWeight, color: Color, laid: (f32, f32), scale: f32, opacity: f32| {
      Text::styled(
        says,
        TextStyle {
          font_size: size,
          weight,
          color,
          ..TextStyle::default()
        },
      )
      .nowrap()
      // Laid out at the outlet's corner, scaled about its middle and then put where it goes.
      .transform(
        Transform2D::translate(centre.0 - laid.0 / 2.0, centre.1 - laid.1 / 2.0).then(&Transform2D::scale_uniform(scale)),
      )
      .opacity(opacity)
      .transition(Transition::transform().duration(pace).easing(curve))
      .transition(Transition::opacity().duration(pace).easing(curve))
    };
    let ratio = if self.large.3 > 0.0 && self.label.3 > 0.0 {
      self.label.3 / self.large.3
    } else {
      SMALL_TITLE_SHARE
    };
    overlay
      .child(part(
        &self.title,
        LARGE_TITLE_SIZE,
        FontWeight::Bold,
        bar.title,
        (self.large.2, self.large.3),
        between(ratio, 1.0),
        out,
      ))
      .child(part(
        &self.says,
        17.0,
        FontWeight::Normal,
        bar.tint,
        (self.label.2, self.label.3),
        between(1.0, 1.0 / ratio),
        1.0 - out,
      ))
  }
}

/// One screen's part of the bar.
struct BarPart {
  item: NavigationItem,
  /// What the way back says when there is room; `None` on the screen at the bottom, which has none.
  back_label: Option<String>,
  label_fit: LabelFit,
  title: ElementRef,
  back_probe: ElementRef,
  /// The title is in the bar: always, but for a large title that is still in view in the content.
  title_shown: bool,
  title_fade: Duration,
  /// The most room the title has: what it does not fit in, it gives up with an ellipsis.
  title_room: f32,
  /// The way back's label as it is laid out, and whether it is left out of view: while the outlet carries
  /// the title of the screen under to it or away.
  label: ElementRef,
  label_hidden: bool,
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
  curve: Easing,
  front: bool,
  menu: &BackMenu,
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
    None => text(&part.item.title, FontWeight::SemiBold, bar.title)
      .max_width(part.title_room)
      .text_overflow(TextOverflow::Elipsis)
      .into(),
  };
  let title = Row::new()
    .size(FILL, FILL)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .pointer_events_none()
    .child(
      Row::new()
        .ref_element(part.title.clone())
        .child(middle)
        .opacity(if part.title_shown { 1.0 } else { 0.0 })
        .transition(Transition::opacity().duration(part.title_fade)),
    )
    .transform(Transform2D::translate(shift, 0.0))
    .transition(Transition::transform().duration(pace).easing(curve));

  let mut whole = Stack::new().size(FILL, FILL).child(title);
  let mut sides = Row::new().size(FILL, FILL).align_items(Alignment::Center);
  if let Some(leading) = &part.item.leading {
    sides = sides.child(
      Row::new()
        .align_items(Alignment::Center)
        .padding_left(8.0)
        .child(leading.clone()),
    );
  } else if let Some(label) = &part.back_label {
    let navigator = navigator.clone();
    let says = match part.label_fit {
      LabelFit::Whole => label.as_str(),
      LabelFit::Short => BACK,
      LabelFit::Arrow => "",
    };
    sides = sides.child(
      Row::new()
        .id("navigation-back")
        .align_items(Alignment::Center)
        .height(FILL)
        .padding_left(BACK_LEADING - CHEVRON_WIDTH - BACK_SPACING)
        .padding_right(12.0)
        .spacing(BACK_SPACING)
        .child(chevron(bar.tint))
        .child(
          text(says, FontWeight::Normal, bar.tint)
            .ref_element(part.label.clone())
            .opacity(if part.label_hidden { 0.0 } else { 1.0 }),
        )
        .on_click({
          let menu = menu.clone();
          move |_| {
            // The lift of the press that opened the list is not a tap.
            if !menu.held.swap(false, Ordering::AcqRel) {
              navigator.back();
            }
          }
        })
        .on_long_press({
          let menu = menu.clone();
          move |_: LongPressEvent| {
            menu.held.store(true, Ordering::Release);
            menu.open.set(true);
          }
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
    .transition(Transition::opacity().duration(pace).easing(curve))
    .into()
}

/// The way back's arrow: the platform's symbol on iOS with the `raster` feature, and otherwise two strokes
/// that meet at a point on the left, drawn after it.
fn chevron(color: Color) -> Element {
  // The platform's own, where there is a platform to ask and images to show it with.
  #[cfg(all(target_os = "ios", feature = "raster"))]
  if let Some(symbol) = crate::app::ios_symbol::symbol("chevron.backward", CHEVRON_POINT_SIZE, color) {
    return crate::components::Image::new(symbol.image)
      .width(symbol.width)
      .height(symbol.height)
      .into();
  }
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
  /// A screen's content under its large title ([`NavigationItem::large_title`]), the two scrolling as one: the
  /// title goes up out of view with the content, and the bar's own title comes in when it has. Call it in the
  /// screen's `render`, after `ctx.navigation_item`. Outside a stack, or for a screen whose item asks for no
  /// large title, it is the content in a list that scrolls.
  pub fn large_title_scroll(ctx: &mut Ctx, content: impl Into<Element>) -> ScrollVertical {
    let Some(slot) = ctx.use_context::<NavigationSlot>() else {
      return ScrollVertical::new(content.into());
    };
    let item = slot.item.lock().clone();
    // A window too low for it, a phone on its side, has the title in the bar alone.
    if !item.large_title || ctx.window().logical_height() < LOW_WINDOW {
      return ScrollVertical::new(content.into()).with_scroll_state(slot.scroll.clone());
    }
    let title = ctx.mount::<LargeTitle>(LargeTitleProps {
      title: item.title.clone(),
      color: slot.title_color.unwrap_or(Color::from_hex("#000000")),
      pull: slot.pull.clone(),
      large: slot.large.clone(),
      flying: slot.flying.clone(),
      growth: slot.growth,
    });
    ScrollVertical::new(Column::new().width(FILL).child(title).child(content.into()))
      .with_scroll_state(slot.scroll.clone())
  }

  /// The state of the list [`large_title_scroll`](Self::large_title_scroll) makes for this screen, to set it
  /// up or to scroll it; `None` outside a stack.
  pub fn screen_scroll(ctx: &mut Ctx) -> Option<ScrollState> {
    ctx.use_context::<NavigationSlot>().map(|slot| slot.scroll)
  }

  /// A stack of screens with no bar.
  pub fn mount(ctx: &mut Ctx) -> Element {
    ctx.mount::<Self>(StackOutletProps::default())
  }

  pub fn mount_with(ctx: &mut Ctx, props: StackOutletProps) -> Element {
    ctx.mount::<Self>(props)
  }
}

#[derive(Clone)]
struct LargeTitleProps {
  title: String,
  color: Color,
  pull: Signal<f32>,
  large: ElementRef,
  flying: Signal<bool>,
  growth: (f32, f32),
}

impl PartialEq for LargeTitleProps {
  fn eq(&self, other: &Self) -> bool {
    self.title == other.title
      && self.color == other.color
      && self.pull.id() == other.pull.id()
      && self.growth == other.growth
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for LargeTitleProps {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

/// A screen's large title. It grows a little as the content is pulled down past its top, from its leading
/// edge, as the platform's does: a component of its own, so that only it is rendered for each point pulled.
struct LargeTitle;

impl Component for LargeTitle {
  type Props = LargeTitleProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<LargeTitleProps>().clone();
    let (growth, pull) = props.growth;
    let grown = if pull > 0.0 {
      1.0 + props.pull.get().min(pull) / pull * (growth - 1.0)
    } else {
      1.0
    };
    Row::new()
      .width(FILL)
      .height(LARGE_TITLE_HEIGHT)
      .align_items(Alignment::Center)
      .padding_horizontal(LARGE_TITLE_INSET)
      .child(
        Text::styled(
          &props.title,
          TextStyle {
            font_size: LARGE_TITLE_SIZE * grown,
            weight: FontWeight::Bold,
            color: props.color,
            ..TextStyle::default()
          },
        )
        .nowrap()
        .id("navigation-large-title")
        .ref_element(props.large.clone())
        // While the outlet carries the title to or from a way back, this one is not seen.
        .opacity(if props.flying.get() { 0.0 } else { 1.0 }),
      )
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
