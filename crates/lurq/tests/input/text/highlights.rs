//! `Text::highlights`: fills behind byte ranges of a text, one per laid-out
//! line, drawn from the carets of the painted layout (lurq#75). The probe
//! face is monospaced, 5px cells at 10px, and has "a", "-" and space.

use std::cell::RefCell;

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::MouseButton},
  components::{Text, TextHighlight},
  core::Signal,
  layout::text_style::TextStyle,
  node::{Element, color::Color},
};

use crate::support::{RectSnapshot, RenderSnapshot, render_pass_with_app};

const FAMILY: &str = "Lurq Ligature Probe";
const PROBE: &[u8] = include_bytes!("../../assets/ligature_probe/LurqLigatureProbe-Regular.ttf");
const CELL: f32 = 5.0;
/// How far a painted edge may sit from its unsnapped position.
const SNAP: f32 = 1.0;
/// Three words of four cells; at 30px wide each word is a line.
const WORDS: &str = "aaaa aaaa aaaa";

fn probe_app() -> App {
  let mut app = App::new();
  app.install_fonts([PROBE.to_vec()], std::iter::empty::<(&str, &str)>());
  app
}

fn probe_style() -> TextStyle {
  TextStyle {
    font_family: FAMILY.into(),
    font_size: 10.0,
    ..TextStyle::default()
  }
}

fn current() -> Color {
  Color::from_hex("#ffcc00")
}

fn other() -> Color {
  Color::from_hex("#00ccff")
}

/// The fills drawn in `color`, top to bottom.
fn fills(snapshot: &RenderSnapshot, color: Color) -> Vec<RectSnapshot> {
  let mut rects: Vec<_> = snapshot
    .rects
    .iter()
    .filter(|rect| rect.color == color && rect.width > 0.0 && rect.height > 0.0)
    .cloned()
    .collect();
  rects.sort_by(|a, b| a.y.total_cmp(&b.y));
  rects
}

fn assert_near(actual: f32, expected: f32, what: &str) {
  assert!(
    (actual - expected).abs() <= SNAP,
    "{what} at {actual}, expected {expected}"
  );
}

/// Asserts that `rect` spans cells `start..end` of a line of the text at `x`.
fn assert_cells(rect: &RectSnapshot, x: f32, cells: std::ops::Range<usize>, what: &str) {
  assert_near(rect.x - x, cells.start as f32 * CELL, &format!("{what} starts"));
  assert_near(
    rect.x + rect.width - x,
    cells.end as f32 * CELL,
    &format!("{what} ends"),
  );
}

fn text_x(tree: &mut Tree) -> f32 {
  tree.find_element(|_| true).expect("the text is mounted").bounds().x
}

#[test]
fn a_highlight_fills_its_range_of_a_text_that_is_not_selectable() {
  let mut app = probe_app();
  let mut tree = Tree::new();
  tree.set_root(Text::styled("aaaa", probe_style()).highlights(vec![TextHighlight::new(1..3, current()).radius(2.0)]));
  let snapshot = render_pass_with_app(&mut tree, &mut app);
  let x = text_x(&mut tree);
  let rects = fills(&snapshot, current());
  assert_eq!(rects.len(), 1, "one fill: {rects:?}");
  assert_cells(&rects[0], x, 1..3, "the fill");
  assert_eq!(rects[0].radii, [2.0; 4], "the fill has the highlight's radius");

  let first_glyph = snapshot.glyphs.iter().map(|glyph| glyph.order).min().expect("glyphs");
  assert!(rects[0].order < first_glyph, "the fill is under the glyphs");
}

#[test]
fn highlights_each_draw_in_their_own_color() {
  let mut app = probe_app();
  let mut tree = Tree::new();
  tree.set_root(Text::styled(WORDS, probe_style()).highlights(vec![
    TextHighlight::new(0..4, current()),
    TextHighlight::new(10..14, other()),
  ]));
  let snapshot = render_pass_with_app(&mut tree, &mut app);
  let x = text_x(&mut tree);
  let current = fills(&snapshot, current());
  let other = fills(&snapshot, other());
  assert_eq!((current.len(), other.len()), (1, 1), "{current:?} {other:?}");
  assert_cells(&current[0], x, 0..4, "the current match");
  assert_cells(&other[0], x, 10..14, "the other match");
}

#[test]
fn a_range_past_the_end_is_cut_at_it_and_an_empty_one_draws_nothing() {
  let mut app = probe_app();
  let mut tree = Tree::new();
  tree.set_root(Text::styled("aaaa", probe_style()).highlights(vec![
    TextHighlight::new(2..99, current()),
    TextHighlight::new(3..3, other()),
    TextHighlight::new(50..60, other()),
  ]));
  let snapshot = render_pass_with_app(&mut tree, &mut app);
  let x = text_x(&mut tree);
  let rects = fills(&snapshot, current());
  assert_eq!(rects.len(), 1, "{rects:?}");
  assert_cells(&rects[0], x, 2..4, "the cut fill");
  assert!(fills(&snapshot, other()).is_empty(), "empty ranges draw nothing");
}

#[test]
fn a_highlight_is_under_a_selection_of_the_same_text() {
  let selection = Color::from_hex("#00ff88");
  let mut app = probe_app();
  let mut tree = Tree::new();
  tree.set_root(
    Text::styled("aaaa", probe_style())
      .selectable(true)
      .selection_color(selection)
      .highlights(vec![TextHighlight::new(0..4, current())]),
  );
  render_pass_with_app(&mut tree, &mut app);
  let rect = tree.find_element(|_| true).unwrap().bounds();
  let y = rect.y + rect.height / 2.0;
  tree.mouse_down(rect.x, y, MouseButton::Left);
  tree.mouse_move(rect.x + 2.0 * CELL, y);
  tree.mouse_up(rect.x + 2.0 * CELL, y, MouseButton::Left);
  let snapshot = render_pass_with_app(&mut tree, &mut app);
  let fill = &fills(&snapshot, current())[0];
  let selected = &fills(&snapshot, selection)[0];
  assert!(fill.order < selected.order, "the selection is drawn over the fill");
  assert_eq!(tree.selected_text().as_deref(), Some("aa"));
}

thread_local! {
  static SIGNALS: RefCell<Option<(Signal<f32>, Signal<Vec<TextHighlight>>)>> = const { RefCell::new(None) };
}

/// A text whose width and highlights come from signals, so a change
/// re-renders it over its previous layout.
struct Highlighted;

impl Component for Highlighted {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let (width, highlights) = SIGNALS.with(|signals| signals.borrow().clone().expect("fixture installed the signals"));
    Text::styled(WORDS, probe_style())
      .width(width.get())
      .highlights(highlights.get())
  }
}

struct Fixture {
  app: App,
  tree: Tree,
  width: Signal<f32>,
  highlights: Signal<Vec<TextHighlight>>,
}

impl Fixture {
  fn new(width: f32, highlights: Vec<TextHighlight>) -> Self {
    let width = Signal::new(width);
    let highlights = Signal::new(highlights);
    SIGNALS.with(|slot| *slot.borrow_mut() = Some((width.clone(), highlights.clone())));
    let mut app = probe_app();
    let mut tree = Tree::new();
    tree.mount_root::<Highlighted>(&mut app, ());
    let mut fixture = Self {
      app,
      tree,
      width,
      highlights,
    };
    fixture.pass();
    fixture
  }

  fn pass(&mut self) -> RenderSnapshot {
    render_pass_with_app(&mut self.tree, &mut self.app);
    render_pass_with_app(&mut self.tree, &mut self.app)
  }
}

#[test]
fn a_highlight_over_a_wrap_fills_each_line_and_follows_a_rewrap() {
  // "aa aa" across the end of the first word and the start of the second.
  let mut fixture = Fixture::new(30.0, vec![TextHighlight::new(2..7, current())]);
  let snapshot = fixture.pass();
  let x = text_x(&mut fixture.tree);
  let rects = fills(&snapshot, current());
  assert_eq!(rects.len(), 2, "one fill per line: {rects:?}");
  assert_cells(&rects[0], x, 2..4, "the first line's fill");
  assert_cells(&rects[1], x, 0..2, "the second line's fill");
  assert!(rects[1].y > rects[0].y, "the fills are on two lines");

  // Wide enough for one line: the range is one run again, the space with it.
  fixture.width.set(200.0);
  let snapshot = fixture.pass();
  let rects = fills(&snapshot, current());
  assert_eq!(rects.len(), 1, "one line, one fill: {rects:?}");
  assert_cells(&rects[0], x, 2..7, "the fill on one line");
}

#[test]
fn highlights_set_on_a_text_laid_out_without_them_are_drawn_and_clear() {
  let mut fixture = Fixture::new(200.0, Vec::new());
  assert!(fills(&fixture.pass(), current()).is_empty());

  fixture.highlights.set(vec![TextHighlight::new(5..9, current())]);
  let snapshot = fixture.pass();
  let x = text_x(&mut fixture.tree);
  let rects = fills(&snapshot, current());
  assert_eq!(rects.len(), 1, "{rects:?}");
  assert_cells(&rects[0], x, 5..9, "the added fill");

  fixture.highlights.set(vec![TextHighlight::new(10..14, current())]);
  let rects = fills(&fixture.pass(), current());
  assert_eq!(rects.len(), 1, "{rects:?}");
  assert_cells(&rects[0], x, 10..14, "the moved fill");

  fixture.highlights.set(Vec::new());
  assert!(fills(&fixture.pass(), current()).is_empty(), "cleared");
}
