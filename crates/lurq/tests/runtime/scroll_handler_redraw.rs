//! A scroll made through a `ScrollState` is drawn in the next frame, whoever made it: an `on_scroll` handler that
//! prevents the built-in scroll and scrolls its container itself, a timer, or code outside any event. A write that
//! leaves the offset where it is asks for no frame.

use std::{
  sync::Arc,
  time::{Duration, Instant},
};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::{Ctx, Timeout},
    events::{ScrollEvent, ScrollPhase},
  },
  components::{Column, Rect, Row, ScrollHorizontal, ScrollVertical},
  layout::layout_kind::ScrollState,
  node::{Element, color::Color},
};

use crate::support::{RenderSnapshot, render_pass_if_needed, render_pass_with_app};

const MARK: Color = Color::new(255, 0, 255, 255);
/// Inside the 200 by 100 scroll container.
const OVER: (f32, f32) = (50.0, 50.0);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Axis {
  Horizontal,
  Vertical,
}

/// How a handler writes the offset.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Write {
  Now,
  Pending,
}

impl Write {
  fn scroll(self, state: &ScrollState, x: f32, y: f32) {
    match self {
      Self::Now => state.set_scroll(x, y),
      Self::Pending => state.set_scroll_pending(x, y),
    }
  }
}

/// 600 px of content along `axis`, starting with a marked 20 px square.
fn content(axis: Axis) -> Element {
  let mark = Rect::new(20.0, 20.0).background(MARK);
  match axis {
    Axis::Horizontal => Row::new().spacing(0.0).child(mark).child(Rect::new(580.0, 20.0)).into(),
    Axis::Vertical => Column::new()
      .spacing(0.0)
      .child(mark)
      .child(Rect::new(20.0, 580.0))
      .into(),
  }
}

/// A 200 by 100 container scrolling along `axis` whose `on_scroll` handler prevents the built-in scroll and moves
/// the content by the wheel's delta itself, as `write` says.
fn handled_scroller(axis: Axis, write: Write, state: &ScrollState) -> Element {
  let handler = {
    let state = state.clone();
    move |event: ScrollEvent| {
      event.prevent_default();
      let x = (state.scroll_x() - event.delta_x).clamp(0.0, 400.0);
      let y = (state.scroll_y() - event.delta_y).clamp(0.0, 500.0);
      write.scroll(&state, x, y);
    }
  };
  match axis {
    Axis::Horizontal => ScrollHorizontal::new(content(axis))
      .with_scroll_state(state.clone())
      .on_scroll(handler)
      .size(200.0, 100.0)
      .into(),
    Axis::Vertical => ScrollVertical::new(content(axis))
      .with_scroll_state(state.clone())
      .on_scroll(handler)
      .size(200.0, 100.0)
      .into(),
  }
}

/// Where the frame paints the marked square.
fn mark_at(snapshot: &RenderSnapshot) -> (f32, f32) {
  let mark = snapshot
    .rects
    .iter()
    .find(|rect| rect.color == MARK)
    .expect("the mark is painted");
  (mark.x, mark.y)
}

struct Scene {
  tree: Tree,
  app: App,
  state: ScrollState,
}

impl Scene {
  /// The scroller drawn, with the pointer at rest over it and nothing left to draw.
  fn new(axis: Axis, write: Write) -> Self {
    let state = ScrollState::new();
    let mut tree = Tree::new();
    tree.resize(400, 400);
    tree.set_root(Column::new().child(handled_scroller(axis, write, &state)));
    let mut scene = Self {
      tree,
      app: App::new(),
      state,
    };
    render_pass_with_app(&mut scene.tree, &mut scene.app);
    scene.tree.mouse_move(OVER.0, OVER.1);
    render_pass_with_app(&mut scene.tree, &mut scene.app);
    assert!(scene.frame().is_none(), "the scene is at rest");
    scene
  }

  /// The frame the tree asks for by itself, if it asks for one.
  fn frame(&mut self) -> Option<RenderSnapshot> {
    render_pass_if_needed(&mut self.tree, &mut self.app)
  }

  fn wheel(&mut self, axis: Axis, delta: f32) {
    let (delta_x, delta_y) = match axis {
      Axis::Horizontal => (delta, 0.0),
      Axis::Vertical => (0.0, delta),
    };
    self.tree.scroll(OVER.0, OVER.1, delta_x, delta_y, ScrollPhase::Scroll);
  }
}

#[test]
fn a_scroll_made_in_an_on_scroll_handler_is_drawn_in_the_next_frame() {
  for axis in [Axis::Horizontal, Axis::Vertical] {
    for write in [Write::Now, Write::Pending] {
      let mut scene = Scene::new(axis, write);

      scene.wheel(axis, -10.0);
      assert!(
        scene.tree.needs_redraw(),
        "{axis:?} {write:?}: the handler's scroll asks for a frame"
      );
      let frame = scene
        .frame()
        .unwrap_or_else(|| panic!("{axis:?} {write:?}: the handler's scroll draws a frame"));
      let expected = match axis {
        Axis::Horizontal => (-10.0, 0.0),
        Axis::Vertical => (0.0, -10.0),
      };
      assert_eq!(mark_at(&frame), expected, "{axis:?} {write:?}: at the new offset");
      assert_eq!(
        (scene.state.scroll_x(), scene.state.scroll_y()),
        (-expected.0, -expected.1)
      );
      assert!(scene.frame().is_none(), "{axis:?} {write:?}: and no frame after it");
    }
  }
}

#[test]
fn a_handler_that_leaves_the_offset_where_it_is_asks_for_no_frame() {
  for axis in [Axis::Horizontal, Axis::Vertical] {
    for write in [Write::Now, Write::Pending] {
      let mut scene = Scene::new(axis, write);

      // The handler clamps at the start: the wheel toward it writes the offset the content is already at.
      scene.wheel(axis, 40.0);
      assert!(
        !scene.tree.needs_redraw(),
        "{axis:?} {write:?}: nothing moved, so no frame is asked for"
      );
      assert!(scene.frame().is_none());
      assert_eq!((scene.state.scroll_x(), scene.state.scroll_y()), (0.0, 0.0));
    }
  }
}

#[test]
fn a_scroll_made_outside_any_event_is_drawn_in_the_next_frame() {
  for write in [Write::Now, Write::Pending] {
    let mut scene = Scene::new(Axis::Vertical, write);

    write.scroll(&scene.state, 0.0, 10.0);
    assert!(scene.tree.needs_redraw(), "{write:?}: the scroll asks for a frame");
    let frame = scene.frame().expect("the scroll draws a frame");
    assert_eq!(mark_at(&frame), (0.0, -10.0), "{write:?}");

    write.scroll(&scene.state, 0.0, 10.0);
    assert!(
      !scene.tree.needs_redraw(),
      "{write:?}: the same offset again asks for none"
    );
    assert!(scene.frame().is_none());
  }
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct SharedScroll(#[allow(dead_code)] Arc<ScrollState>);

impl PartialEq for SharedScroll {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

/// A scroller that a timer scrolls by 10 px after 100 ms.
struct TimedScroller {
  _scroll_later: Timeout,
}

impl Component for TimedScroller {
  type Props = SharedScroll;

  fn create(ctx: &mut Ctx) -> Self {
    let state = ctx.props::<SharedScroll>().0.clone();
    let scroll_later = ctx.create_timeout(Duration::from_millis(100), move || state.set_scroll(0.0, 10.0));
    scroll_later.start();
    Self {
      _scroll_later: scroll_later,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let state = ctx.props::<SharedScroll>().0.clone();
    ScrollVertical::new(content(Axis::Vertical))
      .with_scroll_state((*state).clone())
      .size(200.0, 100.0)
  }
}

#[test]
fn a_scroll_made_by_a_timer_is_drawn_in_the_next_frame() {
  let state = Arc::new(ScrollState::new());
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 400);
  tree.mount_root::<TimedScroller>(&mut app, SharedScroll(state.clone()));
  render_pass_with_app(&mut tree, &mut app);
  render_pass_with_app(&mut tree, &mut app);

  tree.tick_timers_at(Instant::now() + Duration::from_secs(1));
  assert_eq!(state.scroll_y(), 10.0, "the timer ran");
  let frame = render_pass_if_needed(&mut tree, &mut app).expect("the timer's scroll draws a frame");
  assert_eq!(mark_at(&frame), (0.0, -10.0));
}
