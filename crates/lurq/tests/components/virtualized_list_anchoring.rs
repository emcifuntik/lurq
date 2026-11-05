//! A row of a `VirtualizedList` that grows (a person opens it) does not move
//! the rows on screen: the list keeps the first row in view where it was,
//! whatever the grown row does to the estimate of the rows it has not
//! measured yet. A list scrolled to its end after such a row grew lands at
//! that row's end. An app that knows its rows' heights can estimate them
//! itself (`VirtualizedList::estimate_height`).

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::ScrollPhase},
  components::{Rect, VirtualizedList},
  core::Signal,
  layout::layout_kind::ScrollState,
  node::{Element, color::Color},
};

use crate::support::TestSurface;

const ROW_HEIGHT: f32 = 40.0;
const VIEWPORT_HEIGHT: f32 = 400.0;
const ROWS: usize = 300;
/// What a grown row measures: several viewports.
const GROWN_HEIGHT: f32 = 3000.0;

#[derive(Clone)]
struct RowProps {
  index: usize,
  /// The row that grew, and its height.
  grown: Signal<Option<(usize, f32)>>,
}

impl PartialEq for RowProps {
  fn eq(&self, other: &Self) -> bool {
    self.index == other.index && self.grown.id() == other.grown.id()
  }
}

#[cfg(feature = "devtools")]
impl lurq::app::component::DevtoolsInspectable for RowProps {}

struct Row;

impl Component for Row {
  type Props = RowProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let RowProps { index, grown } = ctx.props::<Self::Props>().clone();
    let height = match grown.get() {
      Some((row, height)) if row == index => height,
      _ => ROW_HEIGHT,
    };
    Rect::new(100.0, height)
      .id(format!("row-{index}"))
      .background(Color::new((index % 256) as u8, 64, 128, 255))
  }
}

#[derive(Clone)]
struct ListProps {
  grown: Signal<Option<(usize, f32)>>,
  scroll: ScrollState,
  /// Rows estimated at [`ROW_HEIGHT`] before they are measured.
  estimate: bool,
}

impl PartialEq for ListProps {
  fn eq(&self, other: &Self) -> bool {
    self.grown.id() == other.grown.id() && self.estimate == other.estimate
  }
}

#[cfg(feature = "devtools")]
impl lurq::app::component::DevtoolsInspectable for ListProps {}

struct List;

impl Component for List {
  type Props = ListProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let ListProps {
      grown,
      scroll,
      estimate,
    } = ctx.props::<Self::Props>().clone();
    let mut list = VirtualizedList::new(ctx, 0..ROWS)
      .size(100.0, VIEWPORT_HEIGHT)
      .bootstrap_rows(4)
      .with_scroll_state(scroll);
    if estimate {
      list = list.estimate_height(|_| ROW_HEIGHT);
    }
    list.mount_keyed::<Row, _, _, _>(
      |index| format!("row-{index}"),
      move |index| RowProps {
        index: *index,
        grown: grown.clone(),
      },
    )
  }
}

struct Chat {
  tree: Tree,
  app: App,
  grown: Signal<Option<(usize, f32)>>,
  scroll: ScrollState,
}

impl Chat {
  /// A list of [`ROWS`] rows opened at its end, as a chat opens.
  fn open(estimate: bool) -> Self {
    let grown = Signal::new(None);
    let scroll = ScrollState::new();
    scroll.scroll_to_bottom_pending();
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.mount_root::<List>(
      &mut app,
      ListProps {
        grown: grown.clone(),
        scroll: scroll.clone(),
        estimate,
      },
    );
    tree.resize(100, VIEWPORT_HEIGHT as u32);
    let mut chat = Self {
      tree,
      app,
      grown,
      scroll,
    };
    chat.settle();
    chat
  }

  fn settle(&mut self) {
    for _ in 0..12 {
      self.tree.request_redraw();
      self.tree.pass(&mut self.app, &TestSurface);
    }
  }

  /// A positive `delta` scrolls toward the start.
  fn wheel(&mut self, delta: f32) {
    self.tree.mouse_move(50.0, 200.0);
    self.tree.scroll(50.0, 200.0, 0.0, delta, ScrollPhase::Scroll);
    self.settle();
  }

  /// The row's top and bottom in the window, while it is mounted.
  fn row(&mut self, index: usize) -> Option<(f32, f32)> {
    self
      .tree
      .get_element_by_id_mut(&format!("row-{index}"))
      .and_then(|row| row.bounds())
      .map(|bounds| (bounds.y, bounds.y + bounds.height))
  }

  /// The first row whose bottom is below the viewport's top, and its top.
  fn first_shown(&mut self) -> (usize, f32) {
    (0..ROWS)
      .find_map(|index| {
        self
          .row(index)
          .filter(|(_, bottom)| *bottom > 0.5)
          .map(|(top, _)| (index, top))
      })
      .expect("a row is shown")
  }
}

#[test]
fn a_row_that_grows_keeps_the_rows_on_screen_in_place() {
  let mut chat = Chat::open(false);
  chat.wheel(200.0);
  let (first, top) = chat.first_shown();
  // A row in the middle of the view opens; most of the list above was never
  // measured, so the grown row skews their estimate.
  chat.grown.set(Some((first + 4, GROWN_HEIGHT)));
  chat.settle();
  let (row_top, _) = chat.row(first).expect("the first row stays mounted");
  assert!(
    (row_top - top).abs() <= 0.5,
    "row {first} stays where it was shown: {top} before, {row_top} after"
  );
  let (grown_top, _) = chat.row(first + 4).expect("the grown row stays mounted");
  assert!(
    (grown_top - (top + 4.0 * ROW_HEIGHT)).abs() <= 0.5,
    "the grown row's top stays too: {grown_top}"
  );
}

#[test]
fn a_row_that_grows_above_the_view_keeps_the_rows_on_screen_in_place() {
  let mut chat = Chat::open(false);
  chat.wheel(200.0);
  let (first, top) = chat.first_shown();
  // A row in the overscan above the view grows: what is on screen moves down
  // in the content, the view follows it.
  chat.grown.set(Some((first - 2, GROWN_HEIGHT)));
  chat.settle();
  let (row_top, _) = chat.row(first).expect("the first row stays mounted");
  assert!(
    (row_top - top).abs() <= 0.5,
    "row {first} stays where it was shown: {top} before, {row_top} after"
  );
}

#[test]
fn scrolling_to_the_end_after_the_last_row_grew_shows_its_end() {
  let mut chat = Chat::open(false);
  let last = ROWS - 1;
  chat.grown.set(Some((last, GROWN_HEIGHT)));
  chat.settle();
  chat.wheel(2000.0);
  let (_, bottom) = chat.row(last).expect("the grown row is still mounted");
  assert!(
    bottom > VIEWPORT_HEIGHT + 0.5,
    "scrolled up, the row's end is below the view: {bottom}"
  );
  chat.scroll.scroll_to_bottom_pending();
  chat.settle();
  let (_, bottom) = chat.row(last).expect("the grown row is shown");
  assert!(
    (bottom - VIEWPORT_HEIGHT).abs() <= 0.5,
    "the grown row's end is at the view's end: {bottom}"
  );
}

#[test]
fn estimated_heights_leave_the_rows_not_measured_where_they_were() {
  let mut chat = Chat::open(true);
  let content = chat.scroll.content_height();
  assert!(
    (content - ROWS as f32 * ROW_HEIGHT).abs() <= 0.5,
    "every row counts as its estimate: {content}"
  );
  let last = ROWS - 1;
  chat.grown.set(Some((last, GROWN_HEIGHT)));
  chat.settle();
  let content = chat.scroll.content_height();
  assert!(
    (content - ((ROWS - 1) as f32 * ROW_HEIGHT + GROWN_HEIGHT)).abs() <= 0.5,
    "only the grown row's height changed the content: {content}"
  );
}
