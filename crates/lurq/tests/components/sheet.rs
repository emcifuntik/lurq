//! A `Sheet`: content that comes up from the bottom of the window, stands at its heights and goes down.

use std::time::{Duration, Instant};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::MouseButton, events::TouchPhase},
  components::{Column, Sheet, SheetDetent, SheetHost, SheetProps, SheetStyle, Spacer, Text},
  core::Signal,
  node::{Element, color::Color, dimension::Dimension},
};

use crate::support::{pointer_click, render_pass, run_pass};

/// The sheet's color, which nothing else here has.
const SHEET: Color = Color::new(18, 52, 86, 255);
/// The page's, behind the sheet.
const PAGE: Color = Color::new(86, 52, 18, 255);

#[derive(Clone)]
struct Setup {
  open: Signal<bool>,
  detent: Signal<usize>,
  detents: Vec<SheetDetent>,
  dismissable: bool,
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
    let sheet = Sheet::mount(
      ctx,
      SheetProps::new(setup.open)
        .detents(setup.detents)
        .detent(setup.detent)
        .dismissable(setup.dismissable)
        .style(SheetStyle::new().background(SHEET)),
      Column::new()
        .child(Text::new("in the sheet"))
        .child(Spacer::new().width(100.0).height(180.0)),
    );
    Column::new()
      .size(Dimension::Pct(100.0), Dimension::Pct(100.0))
      .background(PAGE)
      .child(Text::new("the page"))
      .child(sheet)
  }
}

/// The app: its page in what draws it back behind a sheet.
struct Whole;

impl Component for Whole {
  type Props = Setup;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let setup = ctx.props::<Setup>().clone();
    SheetHost::mount(ctx, move |ctx| ctx.mount::<Page>(setup.clone()))
  }
}

struct Stage {
  tree: Tree,
  open: Signal<bool>,
  detent: Signal<usize>,
  ahead: u64,
}

impl Stage {
  fn new(detents: &[SheetDetent], dismissable: bool) -> Self {
    let (open, detent) = (Signal::new(false), Signal::new(0));
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.resize(400, 800);
    tree.mount_root::<Whole>(
      &mut app,
      Setup {
        open: open.clone(),
        detent: detent.clone(),
        detents: detents.to_vec(),
        dismissable,
      },
    );
    run_pass(&mut tree);
    Self {
      tree,
      open,
      detent,
      ahead: 0,
    }
  }

  /// Lets a move of the sheet end: its timers fire, and every transition is at its end.
  fn settle(&mut self) {
    run_pass(&mut self.tree);
    self.tree.tick_timers_at(Instant::now() + Duration::from_secs(1));
    run_pass(&mut self.tree);
    self.tree.tick_timers_at(Instant::now() + Duration::from_secs(2));
    run_pass(&mut self.tree);
    self.ahead += 10;
    self
      .tree
      .set_animation_clock(Some(Instant::now() + Duration::from_secs(self.ahead)));
    run_pass(&mut self.tree);
    run_pass(&mut self.tree);
  }

  fn shows(&mut self) -> bool {
    self
      .tree
      .find_element(|element| element.text_content() == Some("in the sheet"))
      .is_some()
  }

  /// Where the sheet's top is drawn, if any of it is in the window.
  fn drawn(&mut self) -> Option<f32> {
    render_pass(&mut self.tree)
      .rects
      .iter()
      .find(|rect| rect.color == SHEET)
      .map(|rect| rect.y)
  }

  /// How much of its width the page behind the sheet is drawn at.
  fn page_scale(&mut self) -> f32 {
    render_pass(&mut self.tree)
      .rects
      .iter()
      .find(|rect| rect.color == PAGE)
      .expect("the page")
      .transform[0]
  }

  fn top(&mut self) -> f32 {
    self.drawn().expect("the sheet")
  }

  /// A finger down on the sheet that goes `by` down, rests and lifts: it throws the sheet nowhere.
  fn drag(&mut self, by: f32) {
    let from = self.top() + 20.0;
    self.tree.touch_event(1, TouchPhase::Start, 200.0, from, None);
    for step in 1..=10 {
      let at = from + by * step as f32 / 10.0;
      self.tree.touch_event(1, TouchPhase::Move, 200.0, at, None);
      run_pass(&mut self.tree);
    }
    std::thread::sleep(Duration::from_millis(250));
    self.tree.touch_event(1, TouchPhase::Move, 200.0, from + by, None);
    self.tree.touch_event(1, TouchPhase::End, 200.0, from + by, None);
    self.settle();
  }
}

#[test]
fn a_sheet_is_not_there_until_it_is_opened() {
  let mut stage = Stage::new(&[SheetDetent::Medium], true);
  assert!(!stage.shows());

  stage.open.set(true);
  stage.settle();
  assert!(stage.shows());
  assert!((stage.top() - 400.0).abs() < 1.0, "half the window: {}", stage.top());
}

#[test]
fn a_sheet_comes_up_from_under_the_window() {
  let mut stage = Stage::new(&[SheetDetent::Medium], true);

  stage.open.set(true);
  let drawn = stage.drawn();
  assert!(stage.shows(), "laid out");
  assert!(
    drawn.is_none_or(|top| top >= 799.0),
    "under the window first: {drawn:?}"
  );
}

#[test]
fn a_closed_sheet_is_there_until_it_has_gone_down() {
  let mut stage = Stage::new(&[SheetDetent::Medium], true);
  stage.open.set(true);
  stage.settle();

  stage.open.set(false);
  run_pass(&mut stage.tree);
  assert!(stage.shows(), "on its way down");
  stage.settle();
  assert!(!stage.shows());
}

#[test]
fn a_sheet_is_as_high_as_its_content() {
  let mut stage = Stage::new(&[SheetDetent::Content], true);
  stage.open.set(true);
  stage.settle();

  let top = stage.top();
  assert!(
    top > 550.0 && top < 620.0,
    "the content's height from the bottom: {top}"
  );
}

#[test]
fn a_press_beside_a_sheet_puts_it_away() {
  let mut stage = Stage::new(&[SheetDetent::Medium], true);
  stage.open.set(true);
  stage.settle();

  pointer_click(&mut stage.tree, 200.0, 500.0, MouseButton::Left);
  assert!(stage.open.get_untracked(), "a press on the sheet is the sheet's");
  pointer_click(&mut stage.tree, 200.0, 100.0, MouseButton::Left);
  assert!(!stage.open.get_untracked());
  stage.settle();
  assert!(!stage.shows());
}

#[test]
fn escape_puts_a_sheet_away() {
  let mut stage = Stage::new(&[SheetDetent::Medium], true);
  stage.open.set(true);
  stage.settle();

  stage
    .tree
    .key_down("Escape".into(), "Escape".into(), false, false, false);
  assert!(!stage.open.get_untracked());
}

#[test]
fn a_sheet_that_is_not_dismissable_stays() {
  let mut stage = Stage::new(&[SheetDetent::Medium], false);
  stage.open.set(true);
  stage.settle();

  pointer_click(&mut stage.tree, 200.0, 100.0, MouseButton::Left);
  stage
    .tree
    .key_down("Escape".into(), "Escape".into(), false, false, false);
  stage.drag(350.0);
  assert!(stage.open.get_untracked());
  assert!(
    (stage.top() - 400.0).abs() < 1.0,
    "back where it stood: {}",
    stage.top()
  );
}

#[test]
fn a_finger_drags_a_sheet_down_and_away() {
  let mut stage = Stage::new(&[SheetDetent::Medium], true);
  stage.open.set(true);
  stage.settle();

  stage.drag(300.0);
  assert!(!stage.open.get_untracked());
  assert!(!stage.shows());
}

#[test]
fn a_sheet_let_go_near_where_it_stood_returns() {
  let mut stage = Stage::new(&[SheetDetent::Medium], true);
  stage.open.set(true);
  stage.settle();

  stage.drag(80.0);
  assert!(stage.open.get_untracked());
  assert!(
    (stage.top() - 400.0).abs() < 1.0,
    "back where it stood: {}",
    stage.top()
  );
}

#[test]
fn a_sheet_follows_the_finger_that_has_it() {
  let mut stage = Stage::new(&[SheetDetent::Medium], true);
  stage.open.set(true);
  stage.settle();

  stage.tree.touch_event(1, TouchPhase::Start, 200.0, 420.0, None);
  for y in [440.0, 480.0, 520.0] {
    stage.tree.touch_event(1, TouchPhase::Move, 200.0, y, None);
  }
  run_pass(&mut stage.tree);
  run_pass(&mut stage.tree);
  let top = stage.top();
  assert!(top > 460.0 && top < 500.0, "down with the finger: {top}");
}

#[test]
fn a_finger_moves_a_sheet_between_its_heights() {
  let mut stage = Stage::new(&[SheetDetent::Medium, SheetDetent::Large], true);
  stage.open.set(true);
  stage.settle();

  stage.drag(-300.0);
  assert_eq!(stage.detent.get_untracked(), 1);
  assert!((stage.top() - 10.0).abs() < 1.0, "all the room: {}", stage.top());

  stage.drag(300.0);
  assert_eq!(stage.detent.get_untracked(), 0);
  assert!(stage.open.get_untracked());
  assert!((stage.top() - 400.0).abs() < 1.0, "half again: {}", stage.top());
}

#[test]
fn a_sheet_goes_to_the_height_its_signal_names() {
  let mut stage = Stage::new(&[SheetDetent::Medium, SheetDetent::Large], true);
  stage.open.set(true);
  stage.settle();

  stage.detent.set(1);
  stage.settle();
  assert!((stage.top() - 10.0).abs() < 1.0, "all the room: {}", stage.top());
}

#[test]
fn the_window_draws_back_behind_a_sheet_at_its_full_height() {
  let mut stage = Stage::new(&[SheetDetent::Medium, SheetDetent::Large], true);
  stage.open.set(true);
  stage.settle();
  assert!((stage.page_scale() - 1.0).abs() < 0.001, "as it is behind half a sheet");

  stage.detent.set(1);
  stage.settle();
  assert!(
    (stage.page_scale() - 0.92).abs() < 0.001,
    "smaller: {}",
    stage.page_scale()
  );

  stage.open.set(false);
  stage.settle();
  assert!(
    (stage.page_scale() - 1.0).abs() < 0.001,
    "and back: {}",
    stage.page_scale()
  );
}

/// A sheet whose content is made in it, and says what it keeps clear of.
struct Built;

impl Component for Built {
  type Props = Setup;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let setup = ctx.props::<Setup>().clone();
    let outside = ctx.safe_area();
    let sheet = Sheet::mount_with(ctx, SheetProps::new(setup.open).detents(setup.detents), |ctx| {
      let inside = ctx.safe_area();
      Text::new(&format!("inside {} {}", inside.top, inside.bottom)).into()
    });
    Column::new()
      .child(Text::new(&format!("outside {} {}", outside.top, outside.bottom)))
      .child(sheet)
  }
}

#[test]
fn inside_a_sheet_that_makes_its_content_there_is_nothing_of_the_window_to_keep_clear_of() {
  let open = Signal::new(true);
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 800);
  tree.window().set_safe_area(lurq::app::window::SafeAreaInsets {
    top: 62.0,
    bottom: 34.0,
    ..Default::default()
  });
  tree.mount_root::<Built>(
    &mut app,
    Setup {
      open,
      detent: Signal::new(0),
      detents: vec![SheetDetent::Medium],
      dismissable: true,
    },
  );
  for _ in 0..3 {
    run_pass(&mut tree);
    tree.tick_timers_at(Instant::now() + Duration::from_secs(2));
  }
  run_pass(&mut tree);
  let shows = |tree: &mut Tree, text: &str| {
    tree
      .find_element(|element| element.text_content() == Some(text))
      .is_some()
  };
  assert!(shows(&mut tree, "outside 62 34"));
  assert!(shows(&mut tree, "inside 0 0"));
}
