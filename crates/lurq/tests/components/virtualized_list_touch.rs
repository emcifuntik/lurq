//! A finger on a `VirtualizedList`: the list mounts and unmounts rows as it scrolls, and the gesture, its
//! momentum and the pull past an end go on through that as on a list that holds all its rows.

use std::time::{Duration, Instant};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::ScrollPhase},
  components::{Rect, VirtualizedList},
  node::{Element, color::Color},
};

use crate::support::run_pass;

const ROW_HEIGHT: f32 = 30.0;
const VIEWPORT_HEIGHT: f32 = 300.0;
const ROWS: usize = 2000;
/// Where the list ends: every row above the last viewport of them.
const END: f32 = ROWS as f32 * ROW_HEIGHT - VIEWPORT_HEIGHT;

struct Row;

impl Component for Row {
  type Props = usize;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let index = *ctx.props::<Self::Props>();
    Rect::new(100.0, ROW_HEIGHT)
      .id(format!("row-{index}"))
      .background(Color::new((index % 256) as u8, 64, 128, 255))
  }
}

struct Root;

impl Component for Root {
  type Props = usize;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let count = *ctx.props::<Self::Props>();
    VirtualizedList::new(ctx, 0..count)
      .size(100.0, VIEWPORT_HEIGHT)
      .estimate_height(|_| ROW_HEIGHT)
      .mount_keyed::<Row, _, _, _>(|index| format!("row-{index}"), |index| *index)
  }
}

fn mount() -> Tree {
  let mut tree = Tree::new();
  tree.mount_root::<Root>(&mut App::new(), ROWS);
  for _ in 0..4 {
    run_pass(&mut tree);
  }
  tree
}

/// Where the content is drawn: the scroll offset and what the list is pulled past its end by.
fn offset(tree: &Tree) -> f32 {
  -tree.last_layout().expect("the list was laid out").children[0].offset.y
}

/// A finger that lands in the middle of the list and travels `steps` times by `step`, a frame after each.
fn drag(tree: &mut Tree, step: f32, steps: usize) {
  tree.touch_scroll_begin(50.0, 150.0, 0.0, step);
  tree.touch_scroll(50.0, 150.0, 0.0, 0.0, ScrollPhase::Start, false);
  for _ in 0..steps {
    tree.touch_scroll(50.0, 150.0, 0.0, step, ScrollPhase::Scroll, false);
    run_pass(tree);
  }
}

#[test]
fn a_drag_scrolls_through_rows_that_are_mounted_on_the_way() {
  let mut tree = mount();

  // 100 frames of 45: 4500, a hundred and fifty rows, none of which was mounted when the finger landed.
  drag(&mut tree, -45.0, 100);
  tree.touch_scroll(50.0, 150.0, 0.0, 0.0, ScrollPhase::End, false);
  run_pass(&mut tree);

  assert_eq!(offset(&tree), 4500.0);
  assert!(
    tree.get_element_by_id("row-150").is_some(),
    "the rows there are mounted"
  );
  assert!(tree.get_element_by_id("row-0").is_none(), "and the first ones are not");
}

#[test]
fn momentum_goes_on_through_rows_that_are_mounted_on_the_way_and_stops_at_the_end() {
  let mut tree = mount();
  drag(&mut tree, -45.0, 4);
  tree.touch_scroll(50.0, 150.0, 0.0, 0.0, ScrollPhase::End, false);

  // The shell's momentum: steps that get shorter, until one moves nothing.
  tree.touch_scroll(50.0, 150.0, 0.0, 0.0, ScrollPhase::Start, true);
  let mut frames = 0;
  while tree.touch_scroll(50.0, 150.0, 0.0, -900.0, ScrollPhase::Scroll, true) {
    run_pass(&mut tree);
    frames += 1;
    assert!(frames < 1000, "the list has an end");
  }
  run_pass(&mut tree);

  assert_eq!(offset(&tree), END);
  assert!(tree.get_element_by_id(&format!("row-{}", ROWS - 1)).is_some());
}

#[test]
fn a_list_pulled_past_its_start_springs_back_with_its_first_rows_in_place() {
  let mut tree = mount();

  drag(&mut tree, 40.0, 3);
  let pulled = offset(&tree);
  assert!(
    (-120.0..-20.0).contains(&pulled),
    "drawn before its start, by less than the 120 travelled: {pulled}"
  );
  assert!(tree.get_element_by_id("row-0").is_some());

  tree.touch_scroll(50.0, 150.0, 0.0, 0.0, ScrollPhase::End, false);
  assert!(tree.touch_animating());
  let lifted = Instant::now();
  tree.touch_tick(lifted + Duration::from_millis(60));
  run_pass(&mut tree);
  let on_its_way = offset(&tree);
  assert!(pulled < on_its_way && on_its_way < 0.0, "{pulled} then {on_its_way}");
  assert!(!tree.touch_tick(lifted + Duration::from_secs(3)));
  run_pass(&mut tree);

  assert_eq!(offset(&tree), 0.0);
  assert!(tree.get_element_by_id("row-0").is_some());
}

#[test]
fn a_list_pulled_past_its_end_keeps_its_last_rows_and_comes_back_to_its_end() {
  let mut tree = mount();
  tree.touch_scroll_begin(50.0, 150.0, 0.0, -10.0);
  tree.touch_scroll(50.0, 150.0, 0.0, -1_000_000.0, ScrollPhase::Scroll, true);
  for _ in 0..6 {
    run_pass(&mut tree);
  }
  assert_eq!(offset(&tree), END);

  drag(&mut tree, -40.0, 3);
  let pulled = offset(&tree) - END;
  assert!((20.0..120.0).contains(&pulled), "drawn beyond its end: {pulled}");
  assert!(tree.get_element_by_id(&format!("row-{}", ROWS - 1)).is_some());

  tree.touch_scroll(50.0, 150.0, 0.0, 0.0, ScrollPhase::End, false);
  assert!(!tree.touch_tick(Instant::now() + Duration::from_secs(3)));
  run_pass(&mut tree);

  assert_eq!(offset(&tree), END);
}

#[test]
fn a_tap_on_the_status_bar_brings_a_long_list_to_its_top() {
  let mut tree = mount();
  // The page under the middle of the window is the list.
  tree.resize(100, VIEWPORT_HEIGHT as u32);
  run_pass(&mut tree);
  drag(&mut tree, -45.0, 100);
  tree.touch_scroll(50.0, 150.0, 0.0, 0.0, ScrollPhase::End, false);
  run_pass(&mut tree);
  assert_eq!(offset(&tree), 4500.0);

  let tapped = Instant::now();
  assert!(tree.touch_scroll_to_top(tapped));
  for frame in 1..=50 {
    tree.touch_tick(tapped + Duration::from_millis(frame * 8));
    run_pass(&mut tree);
  }

  assert_eq!(offset(&tree), 0.0);
  assert!(tree.get_element_by_id("row-0").is_some());
  assert!(!tree.touch_animating());
}

/// Rows of seven heights, and a list that is told none of them: it places the rows it has not measured from
/// the average of those it has, and moves the offset as it measures more.
struct UnevenRow;

fn uneven_height(index: usize) -> f32 {
  20.0 + (index % 7) as f32 * 12.0
}

impl Component for UnevenRow {
  type Props = usize;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let index = *ctx.props::<Self::Props>();
    Rect::new(100.0, uneven_height(index)).id(format!("row-{index}"))
  }
}

struct UnevenRoot;

impl Component for UnevenRoot {
  type Props = usize;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let count = *ctx.props::<Self::Props>();
    VirtualizedList::new(ctx, 0..count)
      .size(100.0, VIEWPORT_HEIGHT)
      .mount_keyed::<UnevenRow, _, _, _>(|index| format!("row-{index}"), |index| *index)
  }
}

/// The mounted row whose top is nearest the middle of the viewport, and that top.
fn row_near_the_middle(tree: &mut Tree) -> (usize, f32) {
  (0..ROWS)
    .filter_map(|index| {
      let bounds = tree.get_element_by_id_mut(&format!("row-{index}"))?.bounds()?;
      Some((index, bounds.y))
    })
    .min_by(|a, b| (a.1 - 150.0).abs().total_cmp(&(b.1 - 150.0).abs()))
    .expect("a mounted row")
}

#[test]
fn the_row_under_the_finger_stays_under_it_while_the_list_measures_rows_it_had_only_guessed() {
  let mut tree = Tree::new();
  tree.mount_root::<UnevenRoot>(&mut App::new(), ROWS);
  for _ in 0..4 {
    run_pass(&mut tree);
  }

  tree.touch_scroll_begin(50.0, 150.0, 0.0, -30.0);
  tree.touch_scroll(50.0, 150.0, 0.0, 0.0, ScrollPhase::Start, false);
  let mut worst: f32 = 0.0;
  for _ in 0..120 {
    let (row, before) = row_near_the_middle(&mut tree);
    tree.touch_scroll(50.0, 150.0, 0.0, -30.0, ScrollPhase::Scroll, false);
    run_pass(&mut tree);
    let after = tree
      .get_element_by_id_mut(&format!("row-{row}"))
      .and_then(|row| row.bounds())
      .expect("a row near the middle is still mounted after 30 pixels")
      .y;
    worst = worst.max((before - 30.0 - after).abs());
  }

  // 3600 of travel, through rows the list had placed by guess: each frame the content moved by the finger's 30.
  assert!(worst < 0.5, "a row jumped by {worst} against the finger");
}

/// A row's number and where taps are written. Equal when the number is: the signal is the same one.
#[derive(Clone)]
struct Tappable(usize, lurq::core::Signal<Vec<usize>>);

impl lurq::app::component::DevtoolsInspectable for Tappable {}

impl PartialEq for Tappable {
  fn eq(&self, other: &Self) -> bool {
    self.0 == other.0
  }
}

struct TappableRow;

impl Component for TappableRow {
  type Props = Tappable;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let Tappable(index, taps) = ctx.props::<Self::Props>().clone();
    // A row as a screen writes one: a label in a row that takes the click.
    lurq::components::Row::new()
      .child(lurq::components::Text::new(&format!("Row {index}")))
      .size(100.0, ROW_HEIGHT)
      .on_click(move |_| taps.update(|taps| taps.push(index)))
  }
}

struct TappableRoot;

impl Component for TappableRoot {
  type Props = Tappable;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let Tappable(_, taps) = ctx.props::<Self::Props>().clone();
    // The root reads what the rows write, as a screen that shows a count of taps does.
    let _ = taps.get();
    VirtualizedList::new(ctx, 0..ROWS)
      .size(100.0, VIEWPORT_HEIGHT)
      .mount_keyed::<TappableRow, _, _, _>(
        |index| format!("row-{index}"),
        move |index| Tappable(*index, taps.clone()),
      )
  }
}

#[test]
fn a_tap_on_a_row_clicks_it_before_and_after_the_list_scrolled() {
  let taps = lurq::core::Signal::new(Vec::new());
  let mut tree = Tree::new();
  tree.mount_root::<TappableRoot>(&mut App::new(), Tappable(0, taps.clone()));
  for _ in 0..4 {
    run_pass(&mut tree);
  }

  // The fourth row: 90 to 120.
  tree.touch_tap(50.0, 100.0, Duration::ZERO);
  run_pass(&mut tree);
  assert_eq!(taps.get(), [3]);

  drag(&mut tree, -30.0, 40);
  tree.touch_scroll(50.0, 150.0, 0.0, 0.0, ScrollPhase::End, false);
  run_pass(&mut tree);
  tree.touch_tap(50.0, 100.0, Duration::ZERO);
  run_pass(&mut tree);
  assert_eq!(taps.get(), [3, 43]);
}
