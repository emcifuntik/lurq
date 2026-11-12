//! Pulling a list down to load it anew, and the spinner that says something is being waited for.
//!
//! [`ActivityIndicator`] is the platform's spinner: spokes around a centre that light one after another.
//! [`PullToRefresh`] goes around a list: pulled down past its top far enough, the list asks for a refresh and
//! stays down, with the spinner over it, until the app says the refresh is done.

use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};

use crate::{
  animation::{Easing, Transition},
  app::{
    component::Component,
    ctx::{Ctx, Interval},
    window::Haptic,
  },
  components::{Row, Stack},
  core::Signal,
  layout::layout_kind::{Justify, ScrollState},
  node::{Element, color::Color, dimension::Dimension, transform::Transform2D},
};

const FILL: Dimension = Dimension::Pct(100.0);
/// How many spokes the spinner has, and how long each is the brightest.
const SPOKES: usize = 8;
const STEP: Duration = Duration::from_millis(125);

/// What an [`ActivityIndicator`] is made with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActivityIndicatorProps {
  /// How wide and high it is.
  pub size: f32,
  pub color: Color,
  /// Whether its spokes go round. One that does not shows them at rest.
  pub animating: bool,
  /// How many of its spokes are there, from 0 to 1, for one that is not going round: a list being pulled
  /// shows as many as it is pulled.
  pub shown: f32,
}

impl Default for ActivityIndicatorProps {
  fn default() -> Self {
    Self {
      size: 20.0,
      color: Color::new(60, 60, 67, 153),
      animating: true,
      shown: 1.0,
    }
  }
}

impl ActivityIndicatorProps {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn size(mut self, size: f32) -> Self {
    self.size = size;
    self
  }

  pub fn color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }

  pub fn animating(mut self, animating: bool) -> Self {
    self.animating = animating;
    self
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for ActivityIndicatorProps {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

/// The platform's spinner.
pub struct ActivityIndicator {
  step: Signal<usize>,
  turns: Interval,
  turning: AtomicBool,
}

impl ActivityIndicator {
  pub fn mount(ctx: &mut Ctx, props: ActivityIndicatorProps) -> Element {
    ctx.mount::<Self>(props)
  }
}

impl Component for ActivityIndicator {
  type Props = ActivityIndicatorProps;

  fn create(ctx: &mut Ctx) -> Self {
    let step = Signal::new(0usize);
    let turns = ctx.create_interval(STEP, {
      let step = step.clone();
      move || step.update(|step| *step = (*step + 1) % SPOKES)
    });
    turns.stop();
    Self {
      step,
      turns,
      turning: AtomicBool::new(false),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = *ctx.props::<ActivityIndicatorProps>();
    // The spokes go round only while they are asked to: one at rest draws no frames.
    if self.turning.swap(props.animating, Ordering::AcqRel) != props.animating {
      if props.animating {
        self.turns.start();
      } else {
        self.turns.stop();
      }
    }
    let step = if props.animating { self.step.get() } else { 0 };
    let (width, length) = (props.size * 0.1, props.size * 0.28);
    let reach = props.size / 2.0 - length / 2.0;
    let there = (props.shown.clamp(0.0, 1.0) * SPOKES as f32).round() as usize;
    let mut spokes = Stack::new().width(props.size).height(props.size);
    for spoke in 0..SPOKES {
      let angle = spoke as f32 * std::f32::consts::TAU / SPOKES as f32;
      // The brightest is the one whose turn it is, and each one before it is dimmer.
      let behind = (step + SPOKES - spoke) % SPOKES;
      let opacity = if props.animating {
        1.0 - behind as f32 * 0.1
      } else if spoke < there {
        0.7
      } else {
        0.0
      };
      spokes = spokes.child(
        Row::new()
          .width(width)
          .height(length)
          .rounded(width / 2.0)
          .background(props.color)
          .opacity(opacity)
          // Laid out at the corner, put on the circle and turned about its own middle to point at the centre.
          .transform(
            Transform2D::translate(
              props.size / 2.0 - width / 2.0 + reach * angle.sin(),
              props.size / 2.0 - length / 2.0 - reach * angle.cos(),
            )
            .then(&Transform2D::rotate(angle)),
          ),
      );
    }
    spokes
  }
}

/// How a [`PullToRefresh`] looks and when it asks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PullToRefreshStyle {
  /// How far the list is pulled down past its top before it asks for a refresh.
  pub threshold: f32,
  /// How far down the list stays while it is refreshed, with the spinner in the room over it.
  pub hold: f32,
  pub spinner_size: f32,
  pub spinner_color: Color,
  /// How long the list takes to go down to where it is held, and back.
  pub duration: Duration,
  /// Whether the device ticks when the refresh is asked for.
  pub haptic: bool,
}

impl Default for PullToRefreshStyle {
  fn default() -> Self {
    Self {
      threshold: 100.0,
      hold: 60.0,
      spinner_size: 22.0,
      spinner_color: Color::new(60, 60, 67, 153),
      duration: Duration::from_millis(300),
      haptic: true,
    }
  }
}

impl PullToRefreshStyle {
  pub fn new() -> Self {
    Self::default()
  }

  /// The platform's dark spinner.
  pub fn dark() -> Self {
    Self {
      spinner_color: Color::new(235, 235, 245, 153),
      ..Self::default()
    }
  }
}

#[derive(Clone)]
pub struct PullToRefreshProps {
  refreshing: Signal<bool>,
  scroll: ScrollState,
  on_refresh: Option<Arc<dyn Fn() + Send + Sync>>,
  top: f32,
  style: PullToRefreshStyle,
}

impl PartialEq for PullToRefreshProps {
  fn eq(&self, other: &Self) -> bool {
    self.refreshing.id() == other.refreshing.id()
      && self.scroll.same(&other.scroll)
      && self.top == other.top
      && self.style == other.style
      && match (&self.on_refresh, &other.on_refresh) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
      }
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for PullToRefreshProps {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

impl PullToRefreshProps {
  /// A list that scrolls with `scroll` and is being refreshed while `refreshing` is set. The list sets the
  /// signal when it is pulled far enough; the app clears it when what it loads has come.
  pub fn new(refreshing: Signal<bool>, scroll: ScrollState) -> Self {
    Self {
      refreshing,
      scroll,
      on_refresh: None,
      top: 0.0,
      style: PullToRefreshStyle::default(),
    }
  }

  /// What is called when the list asks for a refresh, after `refreshing` is set.
  pub fn on_refresh(mut self, on_refresh: impl Fn() + Send + Sync + 'static) -> Self {
    self.on_refresh = Some(Arc::new(on_refresh));
    self
  }

  /// How far under the list's top its content starts: a bar the list goes on under. The spinner stands
  /// under it.
  pub fn top(mut self, top: f32) -> Self {
    self.top = top;
    self
  }

  pub fn style(mut self, style: PullToRefreshStyle) -> Self {
    self.style = style;
    self
  }
}

/// A list that is pulled down to load it anew.
pub struct PullToRefresh {
  /// How far the list is pulled down past its top.
  pull: Signal<f32>,
  /// The pull that asked for the last refresh has been let go: the next one may ask.
  armed: AtomicBool,
  was_refreshing: AtomicBool,
  /// The props of the last render, for the look at the list after each layout.
  seen: parking_lot::Mutex<Option<PullToRefreshProps>>,
}

impl PullToRefresh {
  /// Around `list`, the list that scrolls with the props' state.
  pub fn mount(ctx: &mut Ctx, props: PullToRefreshProps, list: impl Into<Element>) -> Element {
    ctx.mount_with::<Self>(props, vec![list.into()])
  }
}

impl Component for PullToRefresh {
  type Props = PullToRefreshProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self {
      pull: Signal::new(0.0),
      armed: AtomicBool::new(true),
      was_refreshing: AtomicBool::new(false),
      seen: parking_lot::Mutex::new(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<PullToRefreshProps>().clone();
    let style = props.style;
    let refreshing = props.refreshing.get();
    let pull = self.pull.get();
    *self.seen.lock() = Some(props.clone());
    if self.was_refreshing.swap(refreshing, Ordering::AcqRel) != refreshing && refreshing && style.haptic {
      ctx.window().haptic(Haptic::Light);
    }

    // While it is refreshed the list is held down, and the spinner goes round in the room over it; pulled,
    // the spinner shows as many spokes as the pull has of the way.
    let held = if refreshing { style.hold } else { 0.0 };
    let room = pull.max(held);
    let mut list = Stack::new()
      .size(FILL, FILL)
      .transform(Transform2D::translate(0.0, held))
      .transition(Transition::transform().duration(style.duration).easing(Easing::EASE));
    for child in ctx.children() {
      list = list.child(child.clone());
    }
    let spinner = ActivityIndicator::mount(
      ctx,
      ActivityIndicatorProps {
        size: style.spinner_size,
        color: style.spinner_color,
        animating: refreshing,
        shown: (pull / style.threshold).clamp(0.0, 1.0),
      },
    );
    Stack::new().size(FILL, FILL).clip().child(list).child(
      Row::new()
        .width(FILL)
        .justify(Justify::Center)
        .pointer_events_none()
        .padding_top(props.top + (room - style.spinner_size).max(0.0) / 2.0)
        .opacity(if refreshing || pull > 4.0 { 1.0 } else { 0.0 })
        .child(spinner),
    )
  }

  fn after_layout(&self) {
    let props = self.seen.lock().clone();
    if let Some(props) = props {
      self.watch(&props);
    }
  }
}

impl PullToRefresh {
  /// How far the list is pulled, read after every layout: a pull far enough asks for the refresh, once, until
  /// the list has been let go.
  fn watch(&self, props: &PullToRefreshProps) {
    let pull = (-props.scroll.overscroll().1).max(0.0);
    if (self.pull.get_untracked() - pull).abs() > 0.5 {
      self.pull.set(pull);
    }
    if pull < 4.0 {
      self.armed.store(true, Ordering::Release);
    }
    if pull >= props.style.threshold && !props.refreshing.get_untracked() && self.armed.swap(false, Ordering::AcqRel) {
      props.refreshing.set(true);
      if let Some(on_refresh) = &props.on_refresh {
        on_refresh();
      }
    }
  }
}
