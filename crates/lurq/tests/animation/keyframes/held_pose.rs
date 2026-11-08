//! A run that fills forwards keeps drawing the pose it stopped on: on later frames, after its component renders
//! again (which builds the animated node afresh, with no overrides), for layout properties as well as opacity,
//! and for the pose a reversed, fractional or zero-duration run stops on.
//!
//! The tests drive the animation clock explicitly (`Tree::set_animation_clock`), so nothing depends on how long a
//! pass takes.
use std::time::{Duration, Instant};

use lurq::{
  animation::{
    AnimatableProperty, AnimatableValue, Animation, AnimationDirection, AnimationFillMode, AnimationIterationCount,
    Keyframes, KeyframesId,
  },
  app::{App, Tree, component::Component, ctx::Ctx, events::MouseButton},
  components::{Rect, Row},
  core::Signal,
  layout::quad::{Quad, QuadContent},
  node::{Element, color::Color},
};

use crate::support::pointer_click;

const TOGGLE_COLOR: &str = "#22c55e";
const ANIMATED_COLOR: &str = "#2563eb";
const KEYFRAMES: KeyframesId = KeyframesId::new(40);
/// Where the animated rect sits with no offset: after the 40 px toggle and the row's 8 px spacing.
const REST_X: f32 = 48.0;

/// How the animated rect plays `KEYFRAMES`; every run fills forwards and plays linearly.
#[derive(Clone, Copy, Debug, PartialEq, lurq::DevtoolsInspectable)]
enum Spec {
  /// Once, over 100 ms.
  Once,
  /// Once backwards, over 100 ms.
  Reversed,
  /// One and a half iterations of 100 ms.
  OneAndAHalf,
  /// One and a quarter iterations of 100 ms, alternating direction.
  AlternatingOneAndAQuarter,
  /// Once, in no time.
  Instant,
}

impl Spec {
  fn animation(self) -> Animation {
    let forwards = |duration_ms| {
      Animation::new(KEYFRAMES)
        .duration_ms(duration_ms)
        .linear()
        .fill_mode(AnimationFillMode::Forwards)
    };
    match self {
      Spec::Once => forwards(100),
      Spec::Reversed => forwards(100).direction(AnimationDirection::Reverse),
      Spec::OneAndAHalf => forwards(100).iteration_count(AnimationIterationCount::Count(1.5)),
      Spec::AlternatingOneAndAQuarter => forwards(100)
        .direction(AnimationDirection::Alternate)
        .iteration_count(AnimationIterationCount::Count(1.25)),
      Spec::Instant => forwards(0),
    }
  }
}

/// A toggle, then a rect playing `KEYFRAMES` as its spec says. Clicking the toggle renders the component again.
struct Held {
  on: Signal<bool>,
}

impl Component for Held {
  type Props = Spec;

  fn create(ctx: &mut Ctx) -> Self {
    Self { on: ctx.signal(false) }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let spec = ctx.props::<Spec>().animation();
    let on = self.on.clone();
    let toggle = Rect::new(40.0, 40.0)
      .background(TOGGLE_COLOR)
      .opacity(if self.on.get() { 0.9 } else { 1.0 })
      .on_click(move |_| on.update(|on| *on = !*on));
    let animated = Rect::new(40.0, 40.0).background(ANIMATED_COLOR).animation(spec);
    Row::new().spacing(8.0).child(toggle).child(animated)
  }
}

struct Host {
  tree: Tree,
  app: App,
  start: Instant,
}

impl Host {
  fn new(keyframes: Keyframes, spec: Spec) -> Self {
    let (mut tree, mut app) = (Tree::new(), App::new());
    tree.register_keyframes(keyframes);
    let start = Instant::now();
    tree.set_animation_clock(Some(start));
    tree.mount_root::<Held>(&mut app, spec);
    tree.resize(400, 400);
    Self { tree, app, start }
  }

  /// Draws a frame `ms` after the host's start.
  fn pass_at(&mut self, ms: u64) -> &mut Self {
    self
      .tree
      .set_animation_clock(Some(self.start + Duration::from_millis(ms)));
    self.tree.pass_headless(&mut self.app);
    self
  }

  /// Renders the component again.
  fn flip(&mut self) -> &mut Self {
    pointer_click(&mut self.tree, 20.0, 20.0, MouseButton::Left);
    self
  }

  /// The quad the animated rect drew in the last pass.
  fn animated(&self) -> Quad {
    let color = Color::from_hex(ANIMATED_COLOR);
    self
      .tree
      .resolve_quads(self.tree.last_layout().expect("a pass laid the tree out"))
      .into_iter()
      .find(|quad| matches!(quad.content, QuadContent::Rect { color: drawn, .. } if drawn == color))
      .expect("the animated rect draws")
  }
}

fn float_keyframes(prop: AnimatableProperty, from: f32, to: f32) -> Keyframes {
  Keyframes::new(KEYFRAMES)
    .frame(0.0, |f| {
      f.set(prop, AnimatableValue::Float(from));
    })
    .frame(1.0, |f| {
      f.set(prop, AnimatableValue::Float(to));
    })
}

#[track_caller]
fn assert_near(drawn: f32, expected: f32, what: &str) {
  assert!(
    (drawn - expected).abs() < 1e-3,
    "{what}: drawn {drawn}, expected {expected}"
  );
}

/// Ends the run, then checks the held value on a later frame, and after the component renders again.
#[track_caller]
fn assert_holds(host: &mut Host, read: impl Fn(&Quad) -> f32, expected: f32) {
  host.pass_at(0).pass_at(200);
  assert_near(read(&host.animated()), expected, "the frame the run ends on");
  host.pass_at(400);
  assert_near(read(&host.animated()), expected, "a later frame");
  host.flip().pass_at(600);
  assert_near(read(&host.animated()), expected, "after the component renders again");
  host.pass_at(800);
  assert_near(read(&host.animated()), expected, "a frame after that render");
}

#[test]
fn a_forwards_offset_x_holds_after_a_render() {
  let mut host = Host::new(float_keyframes(AnimatableProperty::OffsetX, 0.0, 100.0), Spec::Once);
  assert_holds(&mut host, |quad| quad.x - REST_X, 100.0);
}

#[test]
fn a_forwards_width_holds_after_a_render() {
  let mut host = Host::new(float_keyframes(AnimatableProperty::Width, 40.0, 120.0), Spec::Once);
  assert_holds(&mut host, |quad| quad.width, 120.0);
}

#[test]
fn a_forwards_opacity_holds_after_a_render() {
  let mut host = Host::new(float_keyframes(AnimatableProperty::Opacity, 1.0, 0.3), Spec::Once);
  assert_holds(&mut host, |quad| quad.opacity, 0.3);
}

#[test]
fn a_reversed_run_holds_its_first_keyframe() {
  let mut host = Host::new(float_keyframes(AnimatableProperty::Opacity, 0.2, 1.0), Spec::Reversed);
  assert_holds(&mut host, |quad| quad.opacity, 0.2);
}

#[test]
fn a_fractional_iteration_count_holds_where_the_run_stopped() {
  let mut host = Host::new(
    float_keyframes(AnimatableProperty::Opacity, 1.0, 0.0),
    Spec::OneAndAHalf,
  );
  assert_holds(&mut host, |quad| quad.opacity, 0.5);
}

#[test]
fn a_fractional_alternating_run_holds_where_the_run_stopped() {
  // The second iteration plays backwards, so a quarter into it the keyframes stand at 0.75: opacity 0.25.
  let mut host = Host::new(
    float_keyframes(AnimatableProperty::Opacity, 1.0, 0.0),
    Spec::AlternatingOneAndAQuarter,
  );
  assert_holds(&mut host, |quad| quad.opacity, 0.25);
}

#[test]
fn a_zero_duration_run_holds_its_last_keyframe() {
  let mut host = Host::new(float_keyframes(AnimatableProperty::Opacity, 1.0, 0.3), Spec::Instant);
  assert_holds(&mut host, |quad| quad.opacity, 0.3);
}
