//! Input that comes right after a pointer move rebuilt components, before any
//! frame laid them out: a wheel or a press lands on what the next frame
//! shows, not on the layout of the frame before. A wheel over a scroll box the
//! move just opened scrolls that box; a press where the move pushed a button
//! away does not press it.

use std::sync::{Arc, Mutex};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    events::{MouseButton, ScrollPhase},
  },
  components::{Column, Rect, ScrollVertical},
  core::Signal,
  layout::layout_kind::ScrollState,
  node::{Element, color::Color},
};

use crate::support::TestSurface;

const WIDTH: f32 = 200.0;
const VIEW: f32 = 400.0;

fn settle(tree: &mut Tree, app: &mut App) {
  for _ in 0..3 {
    tree.request_redraw();
    tree.pass(app, &TestSurface);
  }
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct BoxProps {
  open: Signal<bool>,
  #[devtools_ignore]
  outer: ScrollState,
  #[devtools_ignore]
  inner: ScrollState,
}

impl PartialEq for BoxProps {
  fn eq(&self, other: &Self) -> bool {
    self.open.id() == other.open.id()
  }
}

/// A scrolling page; in it, a block that opens into a scroll box of its own
/// when the pointer enters it.
struct OpensOnHover;

impl Component for OpensOnHover {
  type Props = BoxProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let BoxProps { open, outer, inner } = ctx.props::<Self::Props>().clone();
    let opener = open.clone();
    let block: Element = if open.get() {
      ScrollVertical::new(Rect::new(WIDTH, 1000.0).background(Color::new(30, 90, 160, 255)))
        .with_scroll_state(inner)
        .size(WIDTH, 200.0)
        .into()
    } else {
      Rect::new(WIDTH, 200.0)
        .background(Color::new(90, 90, 90, 255))
        .on_mouse_enter(move || opener.set(true))
        .into()
    };
    let content = Column::new()
      .spacing(0.0)
      .child(Rect::new(WIDTH, 100.0))
      .child(block)
      .child(Rect::new(WIDTH, 2000.0));
    ScrollVertical::new(content).with_scroll_state(outer).size(WIDTH, VIEW)
  }
}

#[test]
fn a_wheel_after_a_move_opened_a_scroll_box_scrolls_that_box() {
  let props = BoxProps {
    open: Signal::new(false),
    outer: ScrollState::new(),
    inner: ScrollState::new(),
  };
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<OpensOnHover>(&mut app, props.clone());
  tree.resize(WIDTH as u32, VIEW as u32);
  settle(&mut tree, &mut app);

  // The pointer enters the block, which opens; the wheel follows before a
  // frame lays the opened box out.
  tree.mouse_move(100.0, 200.0);
  assert!(props.open.get_untracked(), "the move opened the block");
  tree.scroll(100.0, 200.0, 0.0, -120.0, ScrollPhase::Scroll);
  settle(&mut tree, &mut app);

  assert!(
    props.inner.scroll_y() > 0.0,
    "the wheel scrolled the opened box: {}",
    props.inner.scroll_y()
  );
  assert_eq!(props.outer.scroll_y(), 0.0, "the page under it stays");
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct PushProps {
  pushed: Signal<bool>,
  #[devtools_ignore]
  pressed: Arc<Mutex<Vec<&'static str>>>,
}

impl PartialEq for PushProps {
  fn eq(&self, other: &Self) -> bool {
    self.pushed.id() == other.pushed.id()
  }
}

/// Two buttons stacked under a block that grows when the pointer enters the
/// column, pushing them down.
struct PushedOnHover;

impl Component for PushedOnHover {
  type Props = PushProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let PushProps { pushed, pressed } = ctx.props::<Self::Props>().clone();
    let pusher = pushed.clone();
    let button = |name: &'static str| {
      let pressed = pressed.clone();
      Rect::new(WIDTH, 100.0)
        .background(Color::new(40, 160, 90, 255))
        .on_click(move |_| pressed.lock().unwrap().push(name))
    };
    Column::new()
      .spacing(0.0)
      .size(WIDTH, VIEW)
      .on_mouse_enter(move || pusher.set(true))
      .child(Rect::new(WIDTH, if pushed.get() { 100.0 } else { 0.0 }))
      .child(button("first"))
      .child(button("second"))
  }
}

#[test]
fn a_press_after_a_move_pushed_a_button_away_presses_what_is_there_now() {
  let props = PushProps {
    pushed: Signal::new(false),
    pressed: Arc::default(),
  };
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<PushedOnHover>(&mut app, props.clone());
  tree.resize(WIDTH as u32, VIEW as u32);
  settle(&mut tree, &mut app);

  // The pointer enters over the second button; the move pushes both down,
  // so the first one is under the pointer at y 150 now.
  tree.mouse_move(100.0, 150.0);
  assert!(props.pushed.get_untracked(), "the move pushed the buttons down");
  tree.mouse_down(100.0, 150.0, MouseButton::Left);
  tree.mouse_up(100.0, 150.0, MouseButton::Left);
  settle(&mut tree, &mut app);

  assert_eq!(
    *props.pressed.lock().unwrap(),
    vec!["first"],
    "the press lands on the first button, which the move pushed under the pointer"
  );
}
