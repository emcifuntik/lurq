//! State styles change what is painted without a render: the foreground of
//! the text under an element, and the opacity of the element.

use lurq::{
  animation::Transition,
  app::{App, Tree, events::MouseButton},
  components::{Column, Rect, Row, Text},
  layout::{Constraints, Size, quad::QuadContent},
  node::{Element, color::Color},
};

use crate::support::TestSurface;

const RED: Color = Color::new(200, 30, 30, 255);
const BLUE: Color = Color::new(30, 60, 200, 255);
const GREEN: Color = Color::new(20, 160, 80, 255);
const FILL: Color = Color::new(240, 240, 240, 255);

fn painted(app: &mut App, root: impl Into<Element>) -> Tree {
  let mut tree = Tree::new();
  tree.set_root(root);
  tree.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 300.0))));
  tree.pass(app, &TestSurface);
  tree
}

fn label_color(tree: &Tree) -> Color {
  tree
    .resolve_quads(tree.last_layout().unwrap())
    .iter()
    .find_map(|quad| match &quad.content {
      QuadContent::Text { style, .. } => Some(style.color),
      _ => None,
    })
    .expect("the label is painted")
}

fn fill_opacity(tree: &Tree) -> f32 {
  tree
    .resolve_quads(tree.last_layout().unwrap())
    .iter()
    .find_map(|quad| match &quad.content {
      QuadContent::Rect { color, .. } if *color == FILL => Some(quad.opacity),
      _ => None,
    })
    .expect("the fill is painted")
}

fn button() -> Row {
  Row::new()
    .size(120.0, 40.0)
    .foreground(RED)
    .hovered(|style| style.foreground(BLUE))
    .active(|style| style.foreground(GREEN))
    .child(Text::new("Save"))
}

#[test]
fn a_hovered_style_sets_the_foreground_of_the_text_under_it() {
  let mut app = App::new();
  let mut tree = painted(&mut app, Column::new().child(button()));
  assert_eq!(label_color(&tree), RED);

  tree.mouse_move(60.0, 20.0);
  let report = tree.pass(&mut app, &TestSurface);
  assert_eq!(label_color(&tree), BLUE);
  assert!(!report.layout_recalculated, "a foreground lays nothing out");

  tree.mouse_move(300.0, 200.0);
  tree.pass(&mut app, &TestSurface);
  assert_eq!(label_color(&tree), RED);
}

#[test]
fn an_active_style_wins_over_a_hovered_one() {
  let mut app = App::new();
  let mut tree = painted(&mut app, Column::new().child(button()));

  tree.mouse_move(60.0, 20.0);
  tree.mouse_down(60.0, 20.0, MouseButton::Left);
  tree.pass(&mut app, &TestSurface);
  assert_eq!(label_color(&tree), GREEN);

  tree.mouse_up(60.0, 20.0, MouseButton::Left);
  tree.pass(&mut app, &TestSurface);
  assert_eq!(label_color(&tree), BLUE);
}

#[test]
fn a_state_foreground_does_not_reach_a_text_that_names_its_color() {
  let mut app = App::new();
  let mut tree = painted(
    &mut app,
    Column::new().child(
      Row::new()
        .size(120.0, 40.0)
        .hovered(|style| style.foreground(BLUE))
        .child(Text::new("Save").color(GREEN)),
    ),
  );

  tree.mouse_move(60.0, 20.0);
  tree.pass(&mut app, &TestSurface);
  assert_eq!(label_color(&tree), GREEN);
}

#[test]
fn a_hovered_style_sets_the_opacity_of_its_element() {
  let mut app = App::new();
  let mut tree = painted(
    &mut app,
    Column::new().child(
      Rect::new(120.0, 40.0)
        .background(FILL)
        .hovered(|style| style.opacity(0.5)),
    ),
  );
  assert_eq!(fill_opacity(&tree), 1.0);

  tree.mouse_move(60.0, 20.0);
  let report = tree.pass(&mut app, &TestSurface);
  assert_eq!(fill_opacity(&tree), 0.5);
  assert!(!report.layout_recalculated, "an opacity lays nothing out");

  tree.mouse_move(300.0, 200.0);
  tree.pass(&mut app, &TestSurface);
  assert_eq!(fill_opacity(&tree), 1.0);
}

#[test]
fn a_state_opacity_replaces_the_opacity_of_its_element() {
  let mut app = App::new();
  let mut tree = painted(
    &mut app,
    Column::new().child(
      Rect::new(120.0, 40.0)
        .background(FILL)
        .opacity(0.8)
        .hovered(|style| style.opacity(0.4)),
    ),
  );
  assert_eq!(fill_opacity(&tree), 0.8);

  tree.mouse_move(60.0, 20.0);
  tree.pass(&mut app, &TestSurface);
  assert_eq!(fill_opacity(&tree), 0.4);
}

#[test]
fn an_opacity_transition_runs_between_state_styles() {
  let mut app = App::new();
  let mut tree = painted(
    &mut app,
    Column::new().child(
      Rect::new(120.0, 40.0)
        .background(FILL)
        .transition(Transition::opacity().duration_ms(1000).linear())
        .hovered(|style| style.opacity(0.0)),
    ),
  );

  tree.mouse_move(60.0, 20.0);
  tree.pass(&mut app, &TestSurface);
  assert_eq!(
    fill_opacity(&tree),
    1.0,
    "the first frame shows where the transition starts"
  );

  std::thread::sleep(std::time::Duration::from_millis(50));
  tree.request_redraw();
  tree.pass(&mut app, &TestSurface);
  let during = fill_opacity(&tree);
  assert!(during < 1.0 && during > 0.0, "the opacity is on its way: {during}");
}
