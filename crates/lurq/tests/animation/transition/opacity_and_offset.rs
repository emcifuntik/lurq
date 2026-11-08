//! Opacity and offset transitions run to the value the last render set, even when nothing renders again while
//! they run, and a render that changes the value mid-run retargets from the value drawn at that moment.
//!
//! The tests drive the animation clock explicitly (`Tree::set_animation_clock`), so nothing depends on how long a
//! pass takes.

use std::time::{Duration, Instant};

use lurq::{
  animation::Transition,
  app::{App, Tree, component::Component, ctx::Ctx, events::MouseButton},
  components::{Rect, Row},
  core::Signal,
  layout::quad::{Quad, QuadContent},
  node::{Element, color::Color},
};

use crate::support::pointer_click;

const TOGGLE_COLOR: &str = "#22c55e";
const MOVED_COLOR: &str = "#2563eb";

/// What the transitioned rect changes when the toggle flips.
#[derive(Clone, Copy, Debug, PartialEq, lurq::DevtoolsInspectable)]
enum Moves {
  /// Opacity from 1 to 0.2, over a second.
  Opacity,
  /// The horizontal offset from 0 to 100 px, over a second.
  OffsetX,
  /// The vertical offset from 0 to 100 px, over a second.
  OffsetY,
}

/// A toggle, then a rect whose opacity or offset follows it with a linear transition.
struct Toggled {
  on: Signal<bool>,
}

impl Component for Toggled {
  type Props = Moves;

  fn create(ctx: &mut Ctx) -> Self {
    Self { on: ctx.signal(false) }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let moves = *ctx.props::<Moves>();
    let on = self.on.clone();
    let toggle = Rect::new(40.0, 40.0)
      .background(TOGGLE_COLOR)
      .on_click(move |_| on.update(|on| *on = !*on));
    let moved = Rect::new(40.0, 40.0).background(MOVED_COLOR);
    let shift = if self.on.get() { 100.0 } else { 0.0 };
    let moved = match moves {
      Moves::Opacity => moved
        .opacity(if self.on.get() { 0.2 } else { 1.0 })
        .transition(Transition::opacity().duration_ms(1000).linear()),
      Moves::OffsetX => moved
        .offset(shift, 0.0)
        .transition(Transition::offset_x().duration_ms(1000).linear()),
      Moves::OffsetY => moved
        .offset(0.0, shift)
        .transition(Transition::offset_y().duration_ms(1000).linear()),
    };
    Row::new().spacing(8.0).child(toggle).child(moved)
  }
}

struct Host {
  tree: Tree,
  app: App,
  start: Instant,
}

impl Host {
  fn new(moves: Moves) -> Self {
    let (mut tree, mut app) = (Tree::new(), App::new());
    tree.mount_root::<Toggled>(&mut app, moves);
    tree.resize(400, 400);
    Self {
      tree,
      app,
      start: Instant::now(),
    }
  }

  /// Draws a frame `ms` after the host's start.
  fn pass_at(&mut self, ms: u64) -> &mut Self {
    self
      .tree
      .set_animation_clock(Some(self.start + Duration::from_millis(ms)));
    self.tree.pass_headless(&mut self.app);
    self
  }

  fn flip(&mut self) -> &mut Self {
    pointer_click(&mut self.tree, 20.0, 20.0, MouseButton::Left);
    self
  }

  /// The quad the transitioned rect drew in the last pass.
  fn moved(&self) -> Quad {
    let color = Color::from_hex(MOVED_COLOR);
    self
      .tree
      .resolve_quads(self.tree.last_layout().expect("a pass laid the tree out"))
      .into_iter()
      .find(|quad| matches!(quad.content, QuadContent::Rect { color: drawn, .. } if drawn == color))
      .expect("the transitioned rect draws")
  }
}

#[track_caller]
fn assert_near(drawn: f32, expected: f32, what: &str) {
  assert!(
    (drawn - expected).abs() < 1e-3,
    "{what}: drawn {drawn}, expected {expected}"
  );
}

#[test]
fn an_opacity_transition_reaches_its_target_without_a_render_between_frames() {
  let mut host = Host::new(Moves::Opacity);
  host.pass_at(0);
  assert_near(host.moved().opacity, 1.0, "before the flip");

  host.flip().pass_at(100);
  assert_near(host.moved().opacity, 1.0, "the first frame of the run");
  host.pass_at(350);
  assert_near(host.moved().opacity, 0.8, "a quarter into the run");
  host.pass_at(600);
  assert_near(host.moved().opacity, 0.6, "half way through the run");
  host.pass_at(1100);
  assert_near(host.moved().opacity, 0.2, "at the end of the run");
  host.pass_at(1500);
  assert_near(host.moved().opacity, 0.2, "after the run");
  assert!(
    !host.tree.has_active_timeline(),
    "the finished run must release the timeline"
  );
}

#[test]
fn an_opacity_transition_retargets_from_the_value_it_draws() {
  let mut host = Host::new(Moves::Opacity);
  host.pass_at(0);
  host.flip().pass_at(100);
  host.pass_at(600);
  assert_near(host.moved().opacity, 0.6, "half way to faded");

  // Back to opaque: the new run starts from the half it reached, not from either end.
  host.flip().pass_at(600);
  assert_near(host.moved().opacity, 0.6, "the first frame of the second run");
  host.pass_at(1100);
  assert_near(host.moved().opacity, 0.8, "half way back");
  host.pass_at(1600);
  assert_near(host.moved().opacity, 1.0, "at the end of the second run");
  assert!(
    !host.tree.has_active_timeline(),
    "the finished run must release the timeline"
  );
}

#[test]
fn an_offset_x_transition_reaches_its_target_without_a_render_between_frames() {
  let mut host = Host::new(Moves::OffsetX);
  host.pass_at(0);
  let rest = host.moved().x;

  host.flip().pass_at(100);
  assert_near(host.moved().x - rest, 0.0, "the first frame of the run");
  host.pass_at(600);
  assert_near(host.moved().x - rest, 50.0, "half way through the run");
  host.pass_at(1100);
  assert_near(host.moved().x - rest, 100.0, "at the end of the run");
  host.pass_at(1500);
  assert_near(host.moved().x - rest, 100.0, "after the run");
  assert!(
    !host.tree.has_active_timeline(),
    "the finished run must release the timeline"
  );
}

#[test]
fn an_offset_y_transition_reaches_its_target_without_a_render_between_frames() {
  let mut host = Host::new(Moves::OffsetY);
  host.pass_at(0);
  let rest = host.moved().y;

  host.flip().pass_at(100);
  assert_near(host.moved().y - rest, 0.0, "the first frame of the run");
  host.pass_at(600);
  assert_near(host.moved().y - rest, 50.0, "half way through the run");
  host.pass_at(1100);
  assert_near(host.moved().y - rest, 100.0, "at the end of the run");
  host.pass_at(1500);
  assert_near(host.moved().y - rest, 100.0, "after the run");
  assert!(
    !host.tree.has_active_timeline(),
    "the finished run must release the timeline"
  );
}

#[test]
fn an_offset_transition_retargets_from_the_value_it_draws() {
  let mut host = Host::new(Moves::OffsetX);
  host.pass_at(0);
  let rest = host.moved().x;
  host.flip().pass_at(100);
  host.pass_at(600);
  assert_near(host.moved().x - rest, 50.0, "half way out");

  host.flip().pass_at(600);
  assert_near(host.moved().x - rest, 50.0, "the first frame of the second run");
  host.pass_at(1100);
  assert_near(host.moved().x - rest, 25.0, "half way back");
  host.pass_at(1600);
  assert_near(host.moved().x - rest, 0.0, "at the end of the second run");
  assert!(
    !host.tree.has_active_timeline(),
    "the finished run must release the timeline"
  );
}
