//! `SwipeRow`: a row a finger swipes aside for its actions.

use std::{
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::Duration,
};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    events::{MouseButton, TouchPhase},
  },
  components::{Column, Row, SwipeAction, SwipeRow, SwipeRowProps, Text},
  core::Signal,
  node::{Element, color::Color, dimension::Dimension},
};

use crate::support::{pointer_click, run_pass};

#[derive(Clone)]
struct Setup {
  open: Signal<i8>,
  deleted: Arc<AtomicUsize>,
  flagged: Arc<AtomicUsize>,
}

impl PartialEq for Setup {
  fn eq(&self, other: &Self) -> bool {
    self.open.id() == other.open.id()
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
    let (deleted, flagged) = (setup.deleted.clone(), setup.flagged.clone());
    let row = SwipeRow::mount(
      ctx,
      SwipeRowProps::new().open(setup.open).trailing([
        SwipeAction::destructive("Delete", move || {
          deleted.fetch_add(1, Ordering::SeqCst);
        }),
        SwipeAction::new("Flag", move || {
          flagged.fetch_add(1, Ordering::SeqCst);
        }),
      ]),
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(60.0)
        .background(Color::from_hex("#ffffff"))
        .child(Text::new("a row")),
    );
    Column::new().width(Dimension::Pct(100.0)).child(row)
  }
}

struct Stage {
  tree: Tree,
  open: Signal<i8>,
  deleted: Arc<AtomicUsize>,
  flagged: Arc<AtomicUsize>,
}

fn stage() -> Stage {
  let (open, deleted, flagged) = (Signal::new(0), Arc::default(), Arc::default());
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 300);
  tree.mount_root::<Page>(
    &mut app,
    Setup {
      open: open.clone(),
      deleted: Arc::clone(&deleted),
      flagged: Arc::clone(&flagged),
    },
  );
  run_pass(&mut tree);
  Stage {
    tree,
    open,
    deleted,
    flagged,
  }
}

/// A finger that lands on the row, goes `by` sideways, rests and lifts: it throws the row nowhere.
fn swipe(tree: &mut Tree, by: f32) {
  tree.touch_event(1, TouchPhase::Start, 300.0, 30.0, None);
  for step in 1..=10 {
    tree.touch_event(1, TouchPhase::Move, 300.0 + by * step as f32 / 10.0, 30.0, None);
    run_pass(tree);
  }
  std::thread::sleep(Duration::from_millis(250));
  tree.touch_event(1, TouchPhase::Move, 300.0 + by, 30.0, None);
  tree.touch_event(1, TouchPhase::End, 300.0 + by, 30.0, None);
  run_pass(tree);
  run_pass(tree);
}

#[test]
fn a_row_swiped_far_enough_stays_open_on_its_actions() {
  let mut stage = stage();
  swipe(&mut stage.tree, -120.0);
  assert_eq!(stage.open.get_untracked(), -1);
  assert_eq!(stage.deleted.load(Ordering::SeqCst), 0, "nothing is done yet");
}

#[test]
fn a_row_swiped_a_little_goes_back() {
  let mut stage = stage();
  swipe(&mut stage.tree, -40.0);
  assert_eq!(stage.open.get_untracked(), 0);
}

#[test]
fn a_row_with_nothing_on_a_side_is_not_swiped_to_it() {
  let mut stage = stage();
  swipe(&mut stage.tree, 90.0);
  assert_eq!(stage.open.get_untracked(), 0);
}

#[test]
fn a_press_on_an_action_does_it_and_closes_the_row() {
  let mut stage = stage();
  stage.open.set(-1);
  for ahead in 1..=3 {
    run_pass(&mut stage.tree);
    stage
      .tree
      .set_animation_clock(Some(std::time::Instant::now() + Duration::from_secs(5 * ahead)));
  }
  run_pass(&mut stage.tree);
  // Two actions of 74 at the trailing edge: the second from the edge is the second given.
  pointer_click(&mut stage.tree, 400.0 - 74.0 - 37.0, 30.0, MouseButton::Left);
  assert_eq!(stage.flagged.load(Ordering::SeqCst), 1);
  assert_eq!(stage.deleted.load(Ordering::SeqCst), 0);
  assert_eq!(stage.open.get_untracked(), 0);
}

#[test]
fn a_swipe_all_the_way_does_the_first_action() {
  let mut stage = stage();
  swipe(&mut stage.tree, -290.0);
  assert_eq!(stage.deleted.load(Ordering::SeqCst), 1);
  assert_eq!(stage.flagged.load(Ordering::SeqCst), 0);
  assert_eq!(stage.open.get_untracked(), 0);
}
