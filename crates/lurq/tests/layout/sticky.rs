//! Sticky positioning: an element held at an edge of the scroll container it
//! is in while that scrolls, for as long as the box of its parent has room
//! for it.

use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    events::{MouseButton, ScrollPhase},
  },
  components::{Column, Rect, Row, ScrollHorizontal, ScrollVertical},
  core::Signal,
  layout::{layout_kind::Sticky, quad::QuadContent},
  node::{Element, color::Color},
};

use crate::support::{TestSurface, pointer_click};

const HEADER: Color = Color::new(200, 30, 30, 255);
const ROW: Color = Color::new(30, 30, 200, 255);

type Scene = Arc<dyn Fn() -> Element + Send + Sync>;

#[derive(Clone, lurq::DevtoolsInspectable)]
struct Props {
  #[devtools_ignore]
  scene: Scene,
}

impl PartialEq for Props {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.scene, &other.scene)
  }
}

struct Page;

impl Component for Page {
  type Props = Props;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    (ctx.props::<Props>().scene)()
  }
}

struct Stage {
  app: App,
  tree: Tree,
}

impl Stage {
  fn new(scene: impl Fn() -> Element + Send + Sync + 'static) -> Self {
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.mount_root::<Page>(&mut app, Props { scene: Arc::new(scene) });
    let mut stage = Self { app, tree };
    stage.pass();
    stage
  }

  fn pass(&mut self) {
    for _ in 0..2 {
      self.tree.request_redraw();
      self.tree.pass(&mut self.app, &TestSurface);
    }
  }

  /// Scrolls the container under (100, 50) down by `dy` and right by `dx`.
  fn scroll(&mut self, dx: f32, dy: f32) {
    self.tree.scroll(100.0, 50.0, -dx, -dy, ScrollPhase::Scroll);
    self.pass();
  }

  fn top_left(&mut self, id: &str) -> (f32, f32) {
    let bounds = self
      .tree
      .get_element_by_id_mut(id)
      .and_then(|element| element.bounds())
      .unwrap_or_else(|| panic!("#{id} should be laid out"));
    (bounds.x, bounds.y)
  }

  /// Where the first rectangle of `color` is in the paint order.
  fn painted_at(&self, color: Color) -> usize {
    self
      .tree
      .resolve_quads(self.tree.last_layout().unwrap())
      .iter()
      .position(|quad| matches!(&quad.content, QuadContent::Rect { color: fill, .. } if *fill == color))
      .expect("the rectangle is painted")
  }
}

/// A list 200 by 100 that scrolls: 30 of lead, a section of a header of 20 and three rows of 40, and a tail.
/// The section is at 30..170 of the content.
fn list(header: impl Fn(Rect) -> Rect + Send + Sync + 'static) -> Stage {
  Stage::new(move || {
    let mut section = Column::new()
      .id("section")
      .child(header(Rect::new(200.0, 20.0).id("header").background(HEADER)));
    for index in 0..3 {
      section = section.child(Rect::new(200.0, 40.0).id(format!("row-{index}")).background(ROW));
    }
    let content = Column::new()
      .child(Rect::new(200.0, 30.0))
      .child(section)
      .child(Rect::new(200.0, 400.0));
    ScrollVertical::new(content).size(200.0, 100.0).into()
  })
}

#[test]
fn a_sticky_header_is_in_flow_until_scrolling_would_take_it_past_the_top() {
  let mut stage = list(|header| header.sticky(Sticky::top(0.0)));
  assert_eq!(stage.top_left("header"), (0.0, 30.0));
  assert_eq!(stage.top_left("row-0"), (0.0, 50.0), "it keeps its room in flow");

  stage.scroll(0.0, 20.0);
  assert_eq!(stage.top_left("header").1, 10.0, "still on its way to the top");

  stage.scroll(0.0, 30.0);
  assert_eq!(stage.top_left("header").1, 0.0, "held at the top");
  assert_eq!(stage.top_left("row-0").1, 0.0, "while its rows scroll under it");

  stage.scroll(0.0, 60.0);
  assert_eq!(stage.top_left("header").1, 0.0);
  assert_eq!(stage.top_left("row-1").1, -20.0);
}

#[test]
fn the_end_of_its_parent_takes_a_sticky_header_with_it() {
  let mut stage = list(|header| header.sticky(Sticky::top(0.0)));

  // The section ends at 170 of the content: scrolled by 160, ten pixels of it are left.
  stage.scroll(0.0, 160.0);
  assert_eq!(stage.top_left("section").1, -130.0);
  assert_eq!(stage.top_left("header").1, -10.0, "it does not leave the section");

  stage.scroll(0.0, 100.0);
  assert_eq!(stage.top_left("header").1, -110.0, "and is gone with it");
}

#[test]
fn scrolling_back_puts_a_sticky_header_back_in_its_place() {
  let mut stage = list(|header| header.sticky(Sticky::top(0.0)));
  stage.scroll(0.0, 120.0);
  assert_eq!(stage.top_left("header").1, 0.0);

  stage.scroll(0.0, -100.0);
  assert_eq!(stage.top_left("header").1, 10.0);

  stage.scroll(0.0, -20.0);
  assert_eq!(stage.top_left("header").1, 30.0);
  assert_eq!(stage.top_left("row-0").1, 50.0);
}

#[test]
fn an_inset_holds_it_that_far_inside_the_edge() {
  let mut stage = list(|header| header.sticky(Sticky::top(8.0)));

  stage.scroll(0.0, 20.0);
  assert_eq!(stage.top_left("header").1, 10.0);

  stage.scroll(0.0, 40.0);
  assert_eq!(stage.top_left("header").1, 8.0);
}

#[test]
fn what_sticks_is_painted_over_its_siblings_and_hit_before_them() {
  let clicks = Arc::new(AtomicUsize::new(0));
  let row_clicks = Arc::new(AtomicUsize::new(0));
  let mut stage = Stage::new({
    let (clicks, row_clicks) = (clicks.clone(), row_clicks.clone());
    move || {
      let (clicks, row_clicks) = (clicks.clone(), row_clicks.clone());
      let header = Rect::new(200.0, 20.0)
        .background(HEADER)
        .sticky(Sticky::top(0.0))
        .on_click(move |_| {
          clicks.fetch_add(1, Ordering::SeqCst);
        });
      let rows = Rect::new(200.0, 300.0).background(ROW).on_click(move |_| {
        row_clicks.fetch_add(1, Ordering::SeqCst);
      });
      ScrollVertical::new(Column::new().child(header).child(rows).child(Rect::new(200.0, 300.0)))
        .size(200.0, 100.0)
        .into()
    }
  });
  stage.scroll(0.0, 60.0);
  assert!(
    stage.painted_at(HEADER) > stage.painted_at(ROW),
    "over the rows under it"
  );

  pointer_click(&mut stage.tree, 100.0, 10.0, MouseButton::Left);
  assert_eq!(
    (clicks.load(Ordering::SeqCst), row_clicks.load(Ordering::SeqCst)),
    (1, 0)
  );
  pointer_click(&mut stage.tree, 100.0, 60.0, MouseButton::Left);
  assert_eq!(
    (clicks.load(Ordering::SeqCst), row_clicks.load(Ordering::SeqCst)),
    (1, 1)
  );
}

#[test]
fn a_sticky_footer_is_held_at_the_bottom_while_its_place_is_below_the_viewport() {
  let mut stage = Stage::new(|| {
    let section = Column::new().child(Rect::new(200.0, 300.0).background(ROW)).child(
      Rect::new(200.0, 20.0)
        .id("footer")
        .background(HEADER)
        .sticky(Sticky::bottom(0.0)),
    );
    ScrollVertical::new(Column::new().child(section).child(Rect::new(200.0, 300.0)))
      .size(200.0, 100.0)
      .into()
  });
  assert_eq!(stage.top_left("footer").1, 80.0, "its place is at 300");

  stage.scroll(0.0, 150.0);
  assert_eq!(stage.top_left("footer").1, 80.0);

  stage.scroll(0.0, 100.0);
  assert_eq!(stage.top_left("footer").1, 50.0, "in view, it is in its place again");
}

#[test]
fn a_sticky_column_is_held_at_the_left_of_what_scrolls_sideways() {
  let mut stage = Stage::new(|| {
    let row = Row::new()
      .child(
        Rect::new(40.0, 30.0)
          .id("first")
          .background(HEADER)
          .sticky(Sticky::left(0.0)),
      )
      .child(Rect::new(500.0, 30.0).id("cells").background(ROW));
    ScrollHorizontal::new(row).size(200.0, 100.0).into()
  });
  assert_eq!(stage.top_left("first").0, 0.0);

  stage.scroll(120.0, 0.0);
  assert_eq!(stage.top_left("first").0, 0.0);
  assert_eq!(stage.top_left("cells").0, -80.0);
}

#[test]
fn outside_a_scroll_container_a_sticky_element_stays_where_it_is_laid_out() {
  let mut stage = Stage::new(|| {
    Column::new()
      .child(Rect::new(200.0, 30.0))
      .child(Rect::new(200.0, 20.0).id("header").sticky(Sticky::top(0.0)))
      .into()
  });

  assert_eq!(stage.top_left("header"), (0.0, 30.0));
}

#[test]
fn a_sticky_element_follows_the_scroll_container_it_is_in_not_one_further_out() {
  let mut stage = Stage::new(|| {
    let inner = ScrollVertical::new(
      Column::new()
        .child(
          Rect::new(180.0, 20.0)
            .id("header")
            .background(HEADER)
            .sticky(Sticky::top(0.0)),
        )
        .child(Rect::new(180.0, 300.0)),
    )
    .size(180.0, 60.0);
    let content = Column::new()
      .child(Rect::new(200.0, 30.0))
      .child(inner)
      .child(Rect::new(200.0, 400.0));
    ScrollVertical::new(content).size(200.0, 100.0).into()
  });
  assert_eq!(stage.top_left("header").1, 30.0);

  // The inner container is at 30..90. Scrolled, it holds the header at its own top.
  stage.scroll(0.0, 40.0);
  assert_eq!(stage.top_left("header").1, 30.0);

  // The outer one, scrolled from a point below the inner one, takes the inner one and its header along.
  stage.tree.scroll(100.0, 95.0, 0.0, -50.0, ScrollPhase::Scroll);
  stage.pass();
  assert_eq!(stage.top_left("header").1, -20.0);
}

#[test]
fn a_layout_while_scrolled_keeps_what_sticks_stuck() {
  let tall = Signal::new(false);
  let mut stage = Stage::new({
    let tall = tall.clone();
    move || {
      let section = Column::new()
        .child(
          Rect::new(200.0, 20.0)
            .id("header")
            .background(HEADER)
            .sticky(Sticky::top(0.0)),
        )
        .child(
          Rect::new(200.0, if tall.get() { 260.0 } else { 200.0 })
            .id("body")
            .background(ROW),
        );
      let content = Column::new()
        .child(Rect::new(200.0, 30.0))
        .child(section)
        .child(Rect::new(200.0, 400.0));
      ScrollVertical::new(content).size(200.0, 100.0).into()
    }
  });
  stage.scroll(0.0, 80.0);
  assert_eq!(stage.top_left("header").1, 0.0);

  tall.set(true);
  stage.pass();
  assert_eq!(stage.top_left("header").1, 0.0);
  assert_eq!(stage.top_left("body").1, -30.0);

  // The taller section holds its header for longer: it ends at 310 of the content.
  stage.scroll(0.0, 190.0);
  assert_eq!(stage.top_left("header").1, 0.0);
  stage.scroll(0.0, 30.0);
  assert_eq!(stage.top_left("header").1, -10.0);
}

#[test]
fn an_element_that_no_longer_sticks_goes_back_to_its_place() {
  let sticks = Signal::new(true);
  let mut stage = Stage::new({
    let sticks = sticks.clone();
    move || {
      let mut header = Rect::new(200.0, 20.0).id("header").background(HEADER);
      if sticks.get() {
        header = header.sticky(Sticky::top(0.0));
      }
      let section = Column::new()
        .child(header)
        .child(Rect::new(200.0, 200.0).background(ROW));
      let content = Column::new()
        .child(Rect::new(200.0, 30.0))
        .child(section)
        .child(Rect::new(200.0, 400.0));
      ScrollVertical::new(content).size(200.0, 100.0).into()
    }
  });
  stage.scroll(0.0, 80.0);
  assert_eq!(stage.top_left("header").1, 0.0);

  sticks.set(false);
  stage.pass();
  assert_eq!(stage.top_left("header").1, -50.0);

  sticks.set(true);
  stage.pass();
  assert_eq!(stage.top_left("header").1, 0.0);
}

#[test]
fn a_corner_cell_sticks_on_both_axes() {
  let mut stage = Stage::new(|| {
    let grid = Column::new()
      .child(
        Rect::new(40.0, 20.0)
          .id("corner")
          .background(HEADER)
          .sticky(Sticky::top(0.0).with_left(0.0)),
      )
      .child(Rect::new(600.0, 400.0).background(ROW));
    lurq::components::ScrollBoth::new(grid).size(200.0, 100.0).into()
  });

  stage.scroll(70.0, 0.0);
  assert_eq!(stage.top_left("corner"), (0.0, 0.0));
  stage.scroll(0.0, 90.0);
  assert_eq!(stage.top_left("corner"), (0.0, 0.0));
}
