//! What a `VirtualizedList` renders before it has measured a row: its
//! bootstrap rows come from where it opens. A list asked to open at its end
//! (`scroll_to_bottom_pending`) builds its last rows and none from its top,
//! the same rows however long it is; a list with a `reveal_key` builds from
//! that row; a list opened at its top builds its first rows, as before. The
//! same holds when the items are swapped for others under the list.

use std::{
  collections::BTreeSet,
  sync::{Arc, Mutex},
};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::ScrollPhase},
  components::{Column, VirtualizedList},
  layout::layout_kind::ScrollState,
  node::{Element, dimension::Dimension},
};

use crate::support::run_pass;

const ROWS: usize = 300;
const ROW_HEIGHT: f32 = 100.0;
const VIEW_HEIGHT: f32 = 600.0;
/// The list's default: how many rows it renders before it has measured one.
const BOOTSTRAP_ROWS: usize = 16;

/// Where the rows note that they rendered, by index.
type Rendered = Arc<Mutex<BTreeSet<usize>>>;

/// A row: its index, the generation of its items (the key tells them apart)
/// and its height.
#[derive(Clone)]
struct Line {
  index: usize,
  generation: u32,
  height: f32,
  rendered: Rendered,
}

impl Line {
  fn key(&self) -> String {
    format!("{}-{}", self.generation, self.index)
  }
}

impl PartialEq for Line {
  fn eq(&self, other: &Self) -> bool {
    self.index == other.index && self.generation == other.generation && self.height == other.height
  }
}

#[cfg(feature = "devtools")]
impl lurq::app::component::DevtoolsInspectable for Line {}

#[derive(Clone)]
struct SharedScroll(Arc<ScrollState>);

impl PartialEq for SharedScroll {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

#[cfg(feature = "devtools")]
impl lurq::app::component::DevtoolsInspectable for SharedScroll {}

struct LineView;

impl Component for LineView {
  type Props = Line;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let line = ctx.props::<Line>().clone();
    line.rendered.lock().unwrap().insert(line.index);
    Column::new()
      .id(format!("row-{}", line.index))
      .width(Dimension::Pct(100.0))
      .height(Dimension::Px(line.height))
  }
}

#[derive(Clone, PartialEq)]
struct ListProps {
  lines: Arc<Vec<Line>>,
  scroll: SharedScroll,
  reveal: Option<String>,
  bootstrap_rows: Option<usize>,
  /// Another key mounts another list, which starts without a measured row.
  list_key: &'static str,
}

#[cfg(feature = "devtools")]
impl lurq::app::component::DevtoolsInspectable for ListProps {}

struct Root;

impl Component for Root {
  type Props = ListProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let mut list = VirtualizedList::new_shared(ctx, props.lines)
      .with_scroll_state((*props.scroll.0).clone())
      .list_key(props.list_key)
      .size(800.0, VIEW_HEIGHT)
      .reveal_key(props.reveal);
    if let Some(rows) = props.bootstrap_rows {
      list = list.bootstrap_rows(rows);
    }
    list.mount_keyed::<LineView, _, _, _>(Line::key, Clone::clone)
  }
}

fn lines(count: usize, generation: u32, height: f32, rendered: &Rendered) -> Arc<Vec<Line>> {
  lines_of(count, generation, |_| height, rendered)
}

fn lines_of(count: usize, generation: u32, height: impl Fn(usize) -> f32, rendered: &Rendered) -> Arc<Vec<Line>> {
  Arc::new(
    (0..count)
      .map(|index| Line {
        index,
        generation,
        height: height(index),
        rendered: rendered.clone(),
      })
      .collect(),
  )
}

struct List {
  tree: Tree,
  scroll: Arc<ScrollState>,
  rendered: Rendered,
}

impl List {
  /// Mounts `count` rows of `height`, opened as `open` asks, and runs one
  /// pass: the first render and its layout.
  fn mount(count: usize, height: f32, open: Open) -> Self {
    let rendered: Rendered = Arc::default();
    let scroll = Arc::new(ScrollState::new());
    let (reveal, bootstrap_rows) = match open {
      Open::Top => (None, None),
      Open::End => {
        scroll.scroll_to_bottom_pending();
        (None, None)
      }
      Open::EndWithBootstrap(rows) => {
        scroll.scroll_to_bottom_pending();
        (None, Some(rows))
      }
      Open::Revealing(index) => (Some(format!("0-{index}")), None),
      Open::At(offset) => {
        scroll.set_scroll_pending(0.0, offset);
        (None, None)
      }
    };
    let mut tree = Tree::new();
    tree.mount_root::<Root>(
      &mut App::new(),
      ListProps {
        lines: lines(count, 0, height, &rendered),
        scroll: SharedScroll(scroll.clone()),
        reveal,
        bootstrap_rows,
        list_key: "vlist",
      },
    );
    run_pass(&mut tree);
    Self { tree, scroll, rendered }
  }

  fn settle(&mut self) {
    for _ in 0..12 {
      run_pass(&mut self.tree);
    }
  }

  /// The indices of the rows rendered so far.
  fn rendered(&self) -> BTreeSet<usize> {
    self.rendered.lock().unwrap().clone()
  }

  fn forget_rendered(&self) {
    self.rendered.lock().unwrap().clear();
  }

  /// Whether row `index` is laid out wholly inside the viewport.
  fn row_in_view(&mut self, index: usize) -> bool {
    let bounds = self
      .tree
      .get_element_by_id_mut(&format!("row-{index}"))
      .and_then(|element| element.bounds());
    bounds.is_some_and(|row| row.y >= -0.5 && row.y + row.height <= VIEW_HEIGHT + 0.5)
  }
}

#[derive(Clone, Copy)]
enum Open {
  Top,
  End,
  EndWithBootstrap(usize),
  Revealing(usize),
  /// A restored offset, queued with `set_scroll_pending`.
  At(f32),
}

fn assert_near(actual: f32, expected: f32, label: &str) {
  assert!(
    (actual - expected).abs() < 0.5,
    "{label}: scroll_y is {actual}, expected {expected}"
  );
}

#[test]
fn a_list_opened_at_its_end_renders_its_last_rows_and_none_from_its_top() {
  let mut list = List::mount(ROWS, ROW_HEIGHT, Open::End);
  let first_pass = list.rendered();
  assert!(first_pass.contains(&(ROWS - 1)), "the last row renders: {first_pass:?}");
  assert!(
    first_pass.iter().all(|index| *index >= ROWS - BOOTSTRAP_ROWS),
    "only the bootstrap rows at the end render: {first_pass:?}"
  );
  assert_eq!(first_pass.len(), BOOTSTRAP_ROWS, "{first_pass:?}");

  list.settle();
  let settled = list.rendered();
  assert!(
    settled.iter().all(|index| *index >= ROWS - BOOTSTRAP_ROWS),
    "the window at the end is within the bootstrap rows: {settled:?}"
  );
  assert_near(
    list.scroll_y(),
    ROWS as f32 * ROW_HEIGHT - VIEW_HEIGHT,
    "opened at the end",
  );
  assert!(list.row_in_view(ROWS - 1), "the last row is in view");
}

#[test]
fn a_list_opened_at_its_end_does_the_same_work_however_long_it_is() {
  let short = List::mount(ROWS, ROW_HEIGHT, Open::End);
  let long = List::mount(ROWS * 10, ROW_HEIGHT, Open::End);
  let (short_pass, long_pass) = (short.rendered(), long.rendered());
  assert_eq!(short_pass.len(), long_pass.len(), "{short_pass:?} vs {long_pass:?}");
  assert!(
    long_pass.iter().all(|index| *index >= ROWS * 10 - BOOTSTRAP_ROWS),
    "{long_pass:?}"
  );
}

#[test]
fn a_list_opened_at_its_top_renders_its_first_rows() {
  let mut list = List::mount(ROWS, ROW_HEIGHT, Open::Top);
  let first_pass = list.rendered();
  assert_eq!(first_pass, (0..BOOTSTRAP_ROWS).collect(), "{first_pass:?}");

  list.settle();
  let settled = list.rendered();
  assert!(
    settled.iter().all(|index| *index < BOOTSTRAP_ROWS),
    "the window at the top is within the bootstrap rows: {settled:?}"
  );
  assert_near(list.scroll_y(), 0.0, "opened at the top");
  assert!(list.row_in_view(0), "the first row is in view");
}

#[test]
fn a_list_of_short_rows_opened_at_its_end_lands_at_its_end() {
  // The bootstrap rows are shorter than the viewport, so the first layout
  // has nothing to scroll to; the list still opens at its end.
  let height = 20.0;
  let mut list = List::mount(ROWS, height, Open::End);
  let first_pass = list.rendered();
  assert!(first_pass.contains(&(ROWS - 1)), "{first_pass:?}");
  assert!(!first_pass.contains(&0), "{first_pass:?}");

  list.settle();
  assert_near(list.scroll_y(), ROWS as f32 * height - VIEW_HEIGHT, "opened at the end");
  assert!(list.row_in_view(ROWS - 1), "the last row is in view");
  assert!(
    !list.rendered().contains(&0),
    "the first row was never built: {:?}",
    list.rendered()
  );
}

#[test]
fn a_list_revealing_a_row_renders_from_that_row() {
  let revealed = 150;
  let mut list = List::mount(ROWS, ROW_HEIGHT, Open::Revealing(revealed));
  let first_pass = list.rendered();
  assert_eq!(first_pass.iter().next(), Some(&revealed), "{first_pass:?}");
  assert!(
    first_pass.iter().all(|index| *index < revealed + BOOTSTRAP_ROWS),
    "{first_pass:?}"
  );

  list.settle();
  assert_near(list.scroll_y(), revealed as f32 * ROW_HEIGHT, "revealed at the top");
  assert!(list.row_in_view(revealed), "the revealed row is in view");
  assert!(!list.rendered().contains(&0), "{:?}", list.rendered());
}

#[test]
fn bootstrap_rows_sets_how_many_rows_the_first_pass_renders() {
  let list = List::mount(ROWS, ROW_HEIGHT, Open::EndWithBootstrap(4));
  let first_pass = list.rendered();
  assert_eq!(first_pass, (ROWS - 4..ROWS).collect(), "{first_pass:?}");
}

#[test]
fn items_swapped_under_a_list_opened_at_its_end_are_windowed_at_the_end() {
  let mut list = List::mount(ROWS, ROW_HEIGHT, Open::End);
  list.settle();
  list.forget_rendered();

  // Another conversation: no key survives, and the app asks for its end.
  list.scroll.scroll_to_bottom_pending();
  let lines = lines(ROWS, 1, ROW_HEIGHT, &list.rendered);
  list.tree.update_root_props::<Root>(ListProps {
    lines,
    scroll: SharedScroll(list.scroll.clone()),
    reveal: None,
    bootstrap_rows: None,
    list_key: "vlist",
  });
  run_pass(&mut list.tree);
  let first_pass = list.rendered();
  assert!(first_pass.contains(&(ROWS - 1)), "{first_pass:?}");
  assert!(
    first_pass.iter().all(|index| *index >= ROWS - BOOTSTRAP_ROWS),
    "the new rows are windowed at the end from the kept estimate: {first_pass:?}"
  );

  list.settle();
  assert_near(
    list.scroll_y(),
    ROWS as f32 * ROW_HEIGHT - VIEW_HEIGHT,
    "opened at the end",
  );
  assert!(list.row_in_view(ROWS - 1), "the last row is in view");
  assert!(!list.rendered().contains(&0), "{:?}", list.rendered());
}

impl List {
  fn scroll_y(&self) -> f32 {
    self.scroll.scroll_y()
  }

  /// Turns the wheel over the list: a positive `delta` scrolls toward the end.
  fn wheel(&mut self, delta: f32) {
    self.tree.scroll(10.0, 10.0, 0.0, -delta, ScrollPhase::Scroll);
    run_pass(&mut self.tree);
  }

  fn update(&mut self, lines: Arc<Vec<Line>>, list_key: &'static str) {
    self.tree.update_root_props::<Root>(ListProps {
      lines,
      scroll: SharedScroll(self.scroll.clone()),
      reveal: None,
      bootstrap_rows: None,
      list_key,
    });
  }
}

/// A restored offset (a scroll position persisted by the app) is kept past
/// the bootstrap: the first layout holds the bootstrap rows alone, which
/// clamps the offset to them and leaves the list at their end, from where it
/// would follow to the real end as the estimate sizes the rest.
#[test]
fn a_list_opened_at_an_offset_lands_there() {
  let height = 30.0;
  for offset in [1200.0, 8000.0] {
    let mut list = List::mount(ROWS, height, Open::At(offset));
    list.settle();
    assert_near(list.scroll_y(), offset, "opened at the offset");
    let first_in_view = (offset / height) as usize;
    assert!(
      list.row_in_view(first_in_view + 1),
      "row {} is in view",
      first_in_view + 1
    );
  }
}

/// A deep offset in a list whose first rows are short: the content sized from
/// their estimate stops short of the offset, so the restore is queued again
/// while the rows measured at the end push the content out, until it lands.
/// The rows measured after it landed then move the offset by less than a
/// row, to keep what it shows in place.
#[test]
fn a_deep_offset_is_restored_while_the_estimate_grows_to_reach_it() {
  let rendered: Rendered = Arc::default();
  let scroll = Arc::new(ScrollState::new());
  scroll.set_scroll_pending(0.0, 3000.0);
  let mut tree = Tree::new();
  tree.mount_root::<Root>(
    &mut App::new(),
    ListProps {
      lines: lines_of(ROWS, 0, |index| if index < 16 { 10.0 } else { 200.0 }, &rendered),
      scroll: SharedScroll(scroll.clone()),
      reveal: None,
      bootstrap_rows: None,
      list_key: "vlist",
    },
  );
  run_pass(&mut tree);
  let mut list = List { tree, scroll, rendered };
  for _ in 0..12 {
    if (list.scroll_y() - 3000.0).abs() < 0.5 {
      break;
    }
    run_pass(&mut list.tree);
  }
  assert_near(list.scroll_y(), 3000.0, "opened at the offset");
  list.settle();
  assert!(
    (list.scroll_y() - 3000.0).abs() < 200.0,
    "the offset stays where the restore put it, within a row: {}",
    list.scroll_y()
  );
  assert!(
    list.scroll.content_height() - VIEW_HEIGHT > list.scroll_y() + 1.0,
    "and not at the end of a content of {}",
    list.scroll.content_height()
  );
}

/// Items swapped in with all-new keys while the list was still restoring a
/// deep offset for the rows before open at their top, not at that offset nor
/// at the end the offset, clamped, would follow.
#[test]
fn items_swapped_in_while_an_offset_is_restored_open_at_their_top() {
  let rendered: Rendered = Arc::default();
  let scroll = Arc::new(ScrollState::new());
  scroll.set_scroll_pending(0.0, 30000.0);
  let mut tree = Tree::new();
  tree.mount_root::<Root>(
    &mut App::new(),
    ListProps {
      lines: lines_of(ROWS, 0, |index| if index < 16 { 10.0 } else { 200.0 }, &rendered),
      scroll: SharedScroll(scroll.clone()),
      reveal: None,
      bootstrap_rows: None,
      list_key: "vlist",
    },
  );
  run_pass(&mut tree);
  run_pass(&mut tree);
  let mut list = List { tree, scroll, rendered };

  let lines = lines(ROWS, 1, 30.0, &list.rendered);
  list.update(lines, "vlist");
  list.settle();
  assert_near(list.scroll_y(), 0.0, "the new rows open at their top");
  assert!(list.row_in_view(0), "the first row is in view");
}

/// A scroll of the person's right after the restore was queued stands: the
/// restore is not queued again over it.
#[test]
fn a_scroll_after_the_restore_stands() {
  // The bootstrap rows hold 1600 px, so the offset is clamped to 1000 and the
  // restore queued. The wheel lays out what the next frame shows first, where
  // the restore lands, and moves the list from there.
  let mut list = List::mount(ROWS, ROW_HEIGHT, Open::At(8000.0));
  assert_near(list.scroll_y(), 1000.0, "clamped to the bootstrap rows");
  list.wheel(-300.0);
  assert_near(
    list.scroll_y(),
    7700.0,
    "the wheel moved the list from the restored offset",
  );
  list.settle();
  assert_near(list.scroll_y(), 7700.0, "the person's scroll stands");
}

/// A list mounted under another key, with the scroll state of the list before
/// (an app that keeps one per screen), opens at the offset that state holds.
#[test]
fn a_list_remounted_with_a_shared_scroll_state_keeps_its_offset() {
  let height = 30.0;
  let mut list = List::mount(ROWS, height, Open::Top);
  list.settle();
  list.scroll.set_scroll_pending(0.0, 1200.0);
  list.settle();
  assert_near(list.scroll_y(), 1200.0, "scrolled");

  let lines = lines(ROWS, 1, height, &list.rendered);
  list.update(lines, "another");
  list.settle();
  assert_near(list.scroll_y(), 1200.0, "the remounted list opens at the shared offset");
  assert!(list.row_in_view(41), "row 41 is in view");
}

/// Rows measured at no height leave no estimate behind: the next rows, taller,
/// are bootstrapped instead of windowed to nothing from a zero estimate.
#[test]
fn rows_of_no_height_leave_no_estimate_for_the_next_rows() {
  let mut list = List::mount(50, 0.0, Open::Top);
  list.settle();
  list.forget_rendered();

  list.scroll.scroll_to_bottom_pending();
  let lines = lines(ROWS, 1, ROW_HEIGHT, &list.rendered);
  list.update(lines, "vlist");
  list.settle();
  assert!(list.rendered().contains(&(ROWS - 1)), "{:?}", list.rendered());
  assert_near(
    list.scroll_y(),
    ROWS as f32 * ROW_HEIGHT - VIEW_HEIGHT,
    "opened at the end",
  );
  assert!(list.row_in_view(ROWS - 1), "the last row is in view");
}
