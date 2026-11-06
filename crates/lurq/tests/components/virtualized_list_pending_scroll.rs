//! A scroll the app queues on a `VirtualizedList`'s `ScrollState`
//! (`scroll_to_bottom_pending`, `set_scroll_pending`) or asks for with
//! `reveal_key` wins over the list's own scroll anchor. Changing the key order
//! up to the first visible row anchors that row; that anchor stands for the
//! position before the requested scroll and must not move the view back there,
//! neither in the pass that resolves the scroll nor frames later, when the
//! measure march across unmeasured rows completes. A content change with no
//! scroll request stays anchored. Rows added or removed below the first
//! visible row do not move it, so they anchor nothing and measure nothing.

use std::sync::{Arc, Mutex};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::ScrollPhase},
  components::{Rect, VirtualizedList},
  layout::layout_kind::ScrollState,
  node::{Element, color::Color},
};
use lurq_macros::DevtoolsInspectable;
use tracing::{
  Event, Metadata, Subscriber,
  field::{Field, Visit},
  span,
};

use crate::support::run_pass;

const VIEW_H: f32 = 100.0;
/// The rows the list renders before it has measured one, asked for with
/// `bootstrap_rows`: more than the short list, so every height of it is known
/// after the first pass and the positions these tests compute from the rows'
/// heights are the list's.
const BOOTSTRAP: usize = 64;
/// Fewer rows than the bootstrap window: every height is known after the
/// first pass, so an anchor restores in the pass that captured it.
const SHORT_LIST: usize = 40;
/// Far more rows than one measure-march chunk: an anchor waits frames for
/// every height before it restores.
const LONG_LIST: usize = 1000;
/// Key of the trailing room row an app shows only while the list is away from
/// its end.
const ROOM: usize = usize::MAX;
/// The short rows at the end of a chat of tall rows: the rows the list renders,
/// and so measures, when it opens at its end.
const CHAT_TAIL: usize = 16;
const SHORT_H: f32 = 20.0;
const TALL_H: f32 = 300.0;
/// The rows a window over the viewport holds at most, without overscan: the
/// viewport's worth of short rows, and one partly shown at each edge.
const WINDOW_ROWS: usize = (VIEW_H / SHORT_H) as usize + 2;
const WHEEL_TICK: f32 = 100.0;

#[derive(Clone)]
struct SharedScroll(Arc<ScrollState>);

impl PartialEq for SharedScroll {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

#[cfg(feature = "devtools")]
impl lurq::app::component::DevtoolsInspectable for SharedScroll {}

#[derive(Clone, PartialEq, DevtoolsInspectable)]
struct RowData {
  id: usize,
  height: f32,
}

struct Row;

impl Component for Row {
  type Props = RowData;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let row = ctx.props::<Self::Props>();
    Rect::new(100.0, row.height)
      .id(format!("row-{}", row.id))
      .background(Color::new((row.id % 256) as u8, 64, 128, 255))
  }
}

struct Root;

impl Component for Root {
  /// Rows, the app's scroll state, the key to reveal and the bootstrap rows.
  type Props = (Vec<RowData>, SharedScroll, Option<String>, usize);

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let (items, scroll, reveal, bootstrap) = ctx.props::<Self::Props>().clone();
    VirtualizedList::new(ctx, items)
      .size(100.0, VIEW_H)
      .overscan_px(0.0)
      .bootstrap_rows(bootstrap)
      .with_scroll_state((*scroll.0).clone())
      .reveal_key(reveal)
      .mount_keyed::<Row, _, _, _>(|row| row.id, |row| (*row).clone())
  }
}

/// Varied heights between 20 and 80.
fn row_height(id: usize) -> f32 {
  20.0 + ((id * 37) % 61) as f32
}

fn rows(range: std::ops::Range<usize>) -> Vec<RowData> {
  range
    .map(|id| RowData {
      id,
      height: row_height(id),
    })
    .collect()
}

/// A chat of tall rows ending in `CHAT_TAIL` short ones.
fn chat_rows() -> Vec<RowData> {
  (0..LONG_LIST)
    .map(|id| RowData {
      id,
      height: if id + CHAT_TAIL >= LONG_LIST { SHORT_H } else { TALL_H },
    })
    .collect()
}

fn room() -> RowData {
  RowData { id: ROOM, height: 60.0 }
}

fn content_height(items: &[RowData]) -> f32 {
  items.iter().map(|row| row.height).sum()
}

fn top_of(items: &[RowData], id: usize) -> f32 {
  items.iter().take_while(|row| row.id != id).map(|row| row.height).sum()
}

struct List {
  tree: Tree,
  scroll: Arc<ScrollState>,
  bootstrap: usize,
}

impl List {
  fn mount(items: Vec<RowData>) -> Self {
    Self::open(items, BOOTSTRAP, ScrollState::new())
  }

  /// Opens the list at its end, as a chat does, with the last `CHAT_TAIL`
  /// rows rendered first: the only rows measured until others are shown.
  fn open_at_end(items: Vec<RowData>) -> Self {
    let scroll = ScrollState::new();
    scroll.scroll_to_bottom_pending();
    Self::open(items, CHAT_TAIL, scroll)
  }

  fn open(items: Vec<RowData>, bootstrap: usize, scroll: ScrollState) -> Self {
    let mut tree = Tree::new();
    let scroll = Arc::new(scroll);
    tree.mount_root::<Root>(&mut App::new(), (items, SharedScroll(scroll.clone()), None, bootstrap));
    let mut list = Self {
      tree,
      scroll,
      bootstrap,
    };
    list.settle();
    list
  }

  fn update(&mut self, items: Vec<RowData>, reveal: Option<&str>) {
    self.tree.update_root_props::<Root>((
      items,
      SharedScroll(self.scroll.clone()),
      reveal.map(str::to_owned),
      self.bootstrap,
    ));
  }

  /// Scroll up by `delta` with the wheel.
  fn wheel_up(&mut self, delta: f32) {
    self.tree.scroll(10.0, 10.0, 0.0, delta, ScrollPhase::Scroll);
    run_pass(&mut self.tree);
  }

  /// Scroll down by `delta` with the wheel, as a person would.
  fn wheel_down(&mut self, delta: f32) {
    self.tree.scroll(10.0, 10.0, 0.0, -delta, ScrollPhase::Scroll);
    run_pass(&mut self.tree);
  }

  fn settle(&mut self) {
    for _ in 0..12 {
      run_pass(&mut self.tree);
    }
  }

  fn scroll_y(&self) -> f32 {
    self.scroll.scroll_y()
  }

  /// The list's end: unmeasured rows count with the estimated height, so the
  /// end of a long list comes from its scroll state.
  fn end(&self) -> f32 {
    self.scroll.content_height() - VIEW_H
  }

  /// Whether row `id` is laid out wholly inside the viewport.
  fn row_in_view(&mut self, id: usize) -> bool {
    let bounds = self
      .tree
      .get_element_by_id_mut(&format!("row-{id}"))
      .and_then(|element| element.bounds());
    bounds.is_some_and(|row| row.y >= -0.5 && row.y + row.height <= VIEW_H + 0.5)
  }

  /// The row laid out at the viewport's top, and its top on screen.
  fn first_shown(&mut self) -> (usize, f32) {
    (0..LONG_LIST + 1)
      .chain([ROOM])
      .find_map(|id| {
        let row = self.tree.get_element_by_id_mut(&format!("row-{id}"))?.bounds()?;
        (row.y <= 0.5 && row.y + row.height > 0.5).then_some((id, row.y))
      })
      .expect("a row at the viewport's top")
  }

  /// How many rows the last layout laid out: the content's children that are
  /// row wrappers, not the spacers standing for the rows between them.
  fn rows_laid_out(&self) -> usize {
    let layout = self.tree.last_layout().expect("the list was laid out");
    layout.children[0]
      .result
      .children
      .iter()
      .filter(|child| !child.result.children.is_empty())
      .count()
  }

  /// The scroll offset of the last layout, as drawn.
  fn laid_out_scroll_y(&self) -> f32 {
    -self.tree.last_layout().expect("the list was laid out").children[0]
      .offset
      .y
  }
}

fn assert_near(actual: f32, expected: f32, label: &str) {
  assert!(
    (actual - expected).abs() < 0.5,
    "{label}: scroll_y is {actual}, expected {expected}"
  );
}

#[test]
fn scroll_to_bottom_pending_wins_over_the_anchor_of_a_removed_trailing_row() {
  // The app's room row under the last row, shown while away from the end.
  let mut items = rows(0..SHORT_LIST);
  items.push(room());
  let mut list = List::mount(items);
  list.wheel_down(600.0);
  assert_near(list.scroll_y(), 600.0, "scrolled away from the end");

  // *Jump to latest*: queue the scroll to the end and drop the room row in
  // the same update, so the key order changes.
  let items = rows(0..SHORT_LIST);
  list.scroll.scroll_to_bottom_pending();
  list.update(items.clone(), None);
  list.settle();

  let end = content_height(&items) - VIEW_H;
  assert_near(list.scroll_y(), end, "after the jump to the end");
  assert_near(list.laid_out_scroll_y(), end, "the layout after the jump to the end");
}

#[test]
fn set_scroll_pending_wins_over_the_anchor_of_an_added_row() {
  let mut list = List::mount(rows(0..SHORT_LIST));
  list.wheel_down(600.0);

  // A row arrives at the end while the app queues an exact offset.
  let items = rows(0..SHORT_LIST + 1);
  let target = top_of(&items, 5);
  list.scroll.set_scroll_pending(0.0, target);
  list.update(items, None);
  list.settle();

  assert_near(list.scroll_y(), target, "after the queued offset");
}

#[test]
fn scroll_to_bottom_pending_is_not_undone_when_the_measure_march_completes() {
  let mut list = List::mount(rows(0..LONG_LIST));
  list.wheel_down(3000.0);
  let before = list.scroll_y();

  // A row arrives above the view: the key order changes there while most rows
  // were never measured, so the anchor waits for the measure march.
  let mut items = rows(LONG_LIST..LONG_LIST + 1);
  items.extend(rows(0..LONG_LIST));
  list.update(items, None);
  run_pass(&mut list.tree);
  assert_near(list.scroll_y(), before, "the anchor has not restored yet");

  // The person jumps to the end while the march is still running.
  list.scroll.scroll_to_bottom_pending();
  run_pass(&mut list.tree);
  list.settle();

  let end = list.end();
  assert!(end > 40_000.0, "the end of the long list, {end}");
  assert_near(list.scroll_y(), end, "frames after the jump to the end");
  assert_near(list.laid_out_scroll_y(), end, "the layout frames after the jump");
  assert!(list.row_in_view(LONG_LIST - 1), "the last row is in view");
}

#[test]
fn scroll_to_bottom_pending_with_an_arrival_wins_over_a_marching_anchor() {
  let mut list = List::mount(rows(0..LONG_LIST));
  list.wheel_down(3000.0);

  // Arrival and jump in one update, most rows unmeasured.
  let items = rows(0..LONG_LIST + 1);
  list.scroll.scroll_to_bottom_pending();
  list.update(items.clone(), None);
  list.settle();

  let end = list.end();
  assert!(end > 40_000.0, "the end of the long list, {end}");
  assert_near(list.scroll_y(), end, "frames after the jump to the end");
  assert!(list.row_in_view(LONG_LIST), "the arrived last row is in view");
}

#[test]
fn reveal_key_is_not_undone_when_the_measure_march_completes() {
  // Long enough that the march outlasts the reveal: the reveal lands and
  // settles while the anchor still waits for heights.
  let count = 5000;
  let mut list = List::mount(rows(0..count));
  list.wheel_down(3000.0);

  // A row arrives above the view, so an anchor marches.
  let mut items = rows(count..count + 1);
  items.extend(rows(0..count));
  list.update(items.clone(), None);
  run_pass(&mut list.tree);

  // Reveal a row far below while the anchor still marches.
  list.update(items, Some("4000"));
  for _ in 0..40 {
    run_pass(&mut list.tree);
  }

  assert!(
    list.row_in_view(4000),
    "row 4000 is in view frames after the reveal (scroll_y {})",
    list.scroll_y()
  );
}

#[test]
fn rows_inserted_above_without_a_scroll_request_stay_anchored_after_the_march() {
  let mut list = List::mount(rows(0..LONG_LIST));
  list.wheel_down(3000.0);
  // The first visible row and how far it is scrolled past.
  let before = list.scroll_y();
  let mut top = 0.0;
  let mut first = 0;
  for id in 0..LONG_LIST {
    if top + row_height(id) > before {
      first = id;
      break;
    }
    top += row_height(id);
  }
  let into_row = before - top;

  let mut items = rows(LONG_LIST..LONG_LIST + 3);
  items.extend(rows(0..LONG_LIST));
  list.update(items.clone(), None);
  list.settle();

  assert_near(
    list.scroll_y(),
    top_of(&items, first) + into_row,
    "row {first} kept its place after the insert above",
  );
}

/// Opens `opened`, a chat of tall rows, at its end, scrolls up one wheel tick
/// and changes its items to `items` below the view: after every pass, the row
/// at the viewport's top stays where it is on screen, and the list lays out
/// no more rows than the window over the viewport holds. The rows above it,
/// never measured, are not measured to keep it there.
fn assert_tail_change_keeps_the_first_row(opened: Vec<RowData>, items: Vec<RowData>) {
  let mut list = List::open_at_end(opened);
  list.wheel_up(WHEEL_TICK);
  assert_near(list.scroll_y(), list.end() - WHEEL_TICK, "one tick up from the end");
  let shown = list.first_shown();
  assert!(shown.0 + CHAT_TAIL >= LONG_LIST, "a short row at the top, {shown:?}");

  list.update(items, None);
  for pass in 0..12 {
    run_pass(&mut list.tree);
    let now = list.first_shown();
    assert!(
      now.0 == shown.0 && (now.1 - shown.1).abs() < 0.5,
      "pass {pass}: row {} at {} is at the top, was row {} at {}",
      now.0,
      now.1,
      shown.0,
      shown.1
    );
    let laid_out = list.rows_laid_out();
    assert!(
      laid_out <= WINDOW_ROWS,
      "pass {pass}: {laid_out} rows laid out, the window holds {WINDOW_ROWS}"
    );
  }
}

#[test]
fn a_row_appended_below_the_view_keeps_the_first_row_without_measuring_the_rows_above() {
  let mut items = chat_rows();
  items.push(room());
  assert_tail_change_keeps_the_first_row(chat_rows(), items);
}

#[test]
fn a_trailing_row_removed_below_the_view_keeps_the_first_row_without_measuring_the_rows_above() {
  let mut opened = chat_rows();
  opened.push(room());
  assert_tail_change_keeps_the_first_row(opened, chat_rows());
}

#[test]
fn a_row_appended_to_a_list_at_its_end_follows_the_end_without_measuring_the_rows_above() {
  let mut list = List::open_at_end(chat_rows());
  assert_near(list.scroll_y(), list.end(), "opened at the end");

  let mut items = chat_rows();
  items.push(room());
  list.update(items, None);
  for pass in 0..12 {
    run_pass(&mut list.tree);
    let laid_out = list.rows_laid_out();
    assert!(
      laid_out <= WINDOW_ROWS,
      "pass {pass}: {laid_out} rows laid out, the window holds {WINDOW_ROWS}"
    );
  }
  assert_near(list.scroll_y(), list.end(), "at the end after the arrival");
  assert!(list.row_in_view(ROOM), "the arrived row is in view");
}

/// Collects `lurq::vlist` warnings about a stale window on this thread.
#[derive(Clone, Default)]
struct WindowWarnings(Arc<Mutex<Vec<String>>>);

struct MessageVisitor<'a>(&'a mut String);

impl Visit for MessageVisitor<'_> {
  fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
    if field.name() == "message" {
      *self.0 = format!("{value:?}");
    }
  }
}

impl Subscriber for WindowWarnings {
  fn enabled(&self, metadata: &Metadata<'_>) -> bool {
    metadata.target() == "lurq::vlist"
  }

  fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
    span::Id::from_u64(1)
  }

  fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

  fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

  fn event(&self, event: &Event<'_>) {
    let mut message = String::new();
    event.record(&mut MessageVisitor(&mut message));
    if message.contains("viewport hole") || message.contains("healed after missed scroll events") {
      self.0.lock().unwrap().push(message);
    }
  }

  fn enter(&self, _span: &span::Id) {}

  fn exit(&self, _span: &span::Id) {}
}

/// Runs `f` and returns the stale-window warnings it logged.
fn window_warnings(f: impl FnOnce()) -> Vec<String> {
  let warnings = WindowWarnings::default();
  tracing::subscriber::with_default(warnings.clone(), || {
    // Callsites first hit by a test on another thread without this
    // subscriber may have cached that nothing listens.
    tracing::callsite::rebuild_interest_cache();
    f();
  });
  warnings.0.lock().unwrap().clone()
}

#[test]
fn the_jump_to_the_end_keeps_the_window_on_the_viewport() {
  let warnings = window_warnings(|| {
    let mut items = rows(0..SHORT_LIST);
    items.push(room());
    let mut list = List::mount(items);
    list.wheel_down(600.0);

    list.scroll.scroll_to_bottom_pending();
    list.update(rows(0..SHORT_LIST), None);
    list.settle();
  });
  assert!(warnings.is_empty(), "stale-window warnings: {warnings:#?}");
}

#[test]
fn the_jump_to_the_end_of_a_list_of_estimated_rows_keeps_the_window_on_the_viewport() {
  let warnings = window_warnings(|| {
    let mut items = rows(0..LONG_LIST);
    items.push(room());
    let mut list = List::mount(items);
    list.wheel_down(3000.0);

    list.scroll.scroll_to_bottom_pending();
    list.update(rows(0..LONG_LIST), None);
    list.settle();
  });
  // The end is predicted from estimated heights; the rows at the end, first
  // measured by the same layout, move it by a few pixels, which this list's
  // zero overscan reports as a healed window. The rows still cover the view.
  let holes: Vec<&String> = warnings
    .iter()
    .filter(|warning| warning.contains("viewport hole"))
    .collect();
  assert!(holes.is_empty(), "viewport holes: {holes:#?}");
}
