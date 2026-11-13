//! `ZoomScroll`: two fingers pinch its content larger about their middle, and a double tap goes in and out.

use std::time::Duration;

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::TouchPhase},
  components::{Spacer, ZoomScroll, ZoomScrollProps},
  core::Signal,
  layout::layout_kind::ScrollState,
  node::Element,
};

use crate::support::run_pass;

#[derive(Clone)]
struct Setup {
  zoom: Signal<f32>,
  scroll: ScrollState,
  range: (f32, f32),
}

impl PartialEq for Setup {
  fn eq(&self, other: &Self) -> bool {
    self.zoom.id() == other.zoom.id()
  }
}

impl lurq::app::component::DevtoolsInspectable for Setup {
  fn write_info(&self, _buffer: &mut Vec<lurq::app::component::ComponentInfo>) {}
}

struct Page;

impl Component for Page {
  type Props = Setup;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let setup = ctx.props::<Setup>().clone();
    ZoomScroll::mount(
      ctx,
      ZoomScrollProps::new(400.0, 400.0)
        .range(setup.range.0, setup.range.1)
        .zoom(setup.zoom)
        .scroll(setup.scroll),
      Spacer::new().width(400.0).height(400.0),
    )
  }
}

/// Content 400 by 400 in a window 200 by 200, pinched between the scales of `range`.
fn stage(range: (f32, f32)) -> (Tree, Signal<f32>, ScrollState) {
  let (zoom, scroll) = (Signal::new(1.0), ScrollState::new());
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(200, 200);
  tree.mount_root::<Page>(
    &mut app,
    Setup {
      zoom: zoom.clone(),
      scroll: scroll.clone(),
      range,
    },
  );
  run_pass(&mut tree);
  run_pass(&mut tree);
  (tree, zoom, scroll)
}

/// Two fingers land `from` apart about a point and move to `to` apart about it, in steps.
fn pinch(tree: &mut Tree, about: (f32, f32), from: f32, to: f32) {
  let at = |apart: f32| ((about.0 - apart / 2.0, about.1), (about.0 + apart / 2.0, about.1));
  let (first, second) = at(from);
  tree.touch_event(1, TouchPhase::Start, first.0, first.1, None);
  tree.touch_event(2, TouchPhase::Start, second.0, second.1, None);
  for step in 1..=20 {
    let (first, second) = at(from + (to - from) * step as f32 / 20.0);
    tree.touch_event(1, TouchPhase::Move, first.0, first.1, None);
    tree.touch_event(2, TouchPhase::Move, second.0, second.1, None);
    run_pass(tree);
  }
  let (first, second) = at(to);
  tree.touch_event(1, TouchPhase::End, first.0, first.1, None);
  tree.touch_event(2, TouchPhase::End, second.0, second.1, None);
  run_pass(tree);
}

#[test]
fn two_fingers_moving_apart_make_the_content_larger_about_their_middle() {
  let (mut tree, zoom, scroll) = stage((1.0, 4.0));
  assert_eq!((scroll.content_width(), scroll.content_height()), (400.0, 400.0));

  pinch(&mut tree, (100.0, 100.0), 40.0, 100.0);

  let now = zoom.get_untracked();
  assert!(
    now > 1.5 && now < 2.6,
    "larger, by about how far the fingers went apart: {now}"
  );
  assert!(
    (scroll.content_width() - 400.0 * now).abs() < 0.5,
    "the list scrolls all of it"
  );
  // The point of the content the fingers took, 100 in from its corner, is still under their middle.
  let under = ((scroll.scroll_x() + 100.0) / now, (scroll.scroll_y() + 100.0) / now);
  assert!(
    (under.0 - 100.0).abs() < 1.0 && (under.1 - 100.0).abs() < 1.0,
    "{under:?}"
  );
}

#[test]
fn a_pinch_stops_at_the_largest_and_the_smallest_scale() {
  let (mut tree, zoom, _scroll) = stage((1.0, 1.5));

  pinch(&mut tree, (100.0, 100.0), 30.0, 180.0);
  assert_eq!(zoom.get_untracked(), 1.5);

  pinch(&mut tree, (100.0, 100.0), 180.0, 30.0);
  assert_eq!(zoom.get_untracked(), 1.0);
}

#[test]
fn a_double_tap_goes_in_at_the_point_tapped_and_back_out() {
  let (mut tree, zoom, scroll) = stage((1.0, 4.0));

  tree.touch_tap(50.0, 50.0, Duration::ZERO);
  tree.touch_tap(50.0, 50.0, Duration::ZERO);
  run_pass(&mut tree);
  assert_eq!(zoom.get_untracked(), 2.0);
  // What was tapped, 50 in from the corner, is drawn 100 in, and is still under the finger.
  assert_eq!((scroll.scroll_x(), scroll.scroll_y()), (50.0, 50.0));

  std::thread::sleep(Duration::from_millis(5));
  tree.touch_tap(50.0, 50.0, Duration::ZERO);
  tree.touch_tap(50.0, 50.0, Duration::ZERO);
  run_pass(&mut tree);
  assert_eq!(zoom.get_untracked(), 1.0);
  assert_eq!((scroll.scroll_x(), scroll.scroll_y()), (0.0, 0.0));
}

#[test]
fn content_shown_smaller_than_the_list_scrolls_nowhere() {
  let (mut tree, zoom, scroll) = stage((0.25, 4.0));

  zoom.set(0.25);
  run_pass(&mut tree);
  run_pass(&mut tree);

  // 100 by 100 in a list of 200 by 200: what scrolls is as large as the list, with the content in its middle.
  assert_eq!((scroll.content_width(), scroll.content_height()), (200.0, 200.0));
  assert_eq!((scroll.scroll_x(), scroll.scroll_y()), (0.0, 0.0));
}
