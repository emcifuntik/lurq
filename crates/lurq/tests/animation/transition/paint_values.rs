//! Transitions of what only painting reads: the foreground, the outline and
//! the box shadow of a node, and colors given as palette roles.

use std::time::{Duration, Instant};

use lurq::{
  animation::Transition,
  app::{App, Tree, theme::PaletteColor},
  components::{Button, Column, Rect, Row, Text},
  layout::{
    Constraints, Size,
    quad::{Quad, QuadContent},
  },
  node::{BoxShadow, Element, Outline, color::Color},
};

use crate::support::TestSurface;

const RED: Color = Color::new(200, 0, 0, 255);
const BLUE: Color = Color::new(0, 0, 200, 255);
const HALFWAY: Color = Color::new(100, 0, 100, 255);
const RING: Color = Color::new(40, 80, 240, 255);
const SHADE: Color = Color::new(0, 0, 0, 128);

/// A tree whose animations run on a clock the test moves.
struct Stage {
  app: App,
  tree: Tree,
  start: Instant,
}

impl Stage {
  fn new(root: impl Into<Element>) -> Self {
    let mut stage = Self {
      app: App::new(),
      tree: Tree::new(),
      start: Instant::now(),
    };
    stage.tree.set_root(root);
    stage
      .tree
      .set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 300.0))));
    stage.at(0);
    stage
  }

  /// Runs a pass `millis` after the start.
  fn at(&mut self, millis: u64) -> lurq::app::PassReport {
    self
      .tree
      .set_animation_clock(Some(self.start + Duration::from_millis(millis)));
    self.tree.request_redraw();
    self.tree.pass(&mut self.app, &TestSurface)
  }

  fn quads(&self) -> Vec<Quad> {
    self.tree.resolve_quads(self.tree.last_layout().unwrap())
  }

  fn label_color(&self) -> Color {
    self
      .quads()
      .iter()
      .find_map(|quad| match &quad.content {
        QuadContent::Text { style, .. } => Some(style.color),
        _ => None,
      })
      .expect("the label is painted")
  }

  fn fill(&self) -> Color {
    self
      .quads()
      .iter()
      .find_map(|quad| match &quad.content {
        QuadContent::Rect { color, .. } if color.a() > 0 && quad.border.is_none() => Some(*color),
        _ => None,
      })
      .expect("the fill is painted")
  }

  /// Width and color of the outline's line, if one is painted.
  fn ring(&self) -> Option<(f32, Color, f32)> {
    self.quads().iter().find_map(|quad| {
      let side = quad.border?.top?;
      matches!(&quad.content, QuadContent::Rect { color, .. } if color.a() == 0)
        .then_some((side.width, side.color, quad.x))
    })
  }

  /// Blur and color of the first box shadow, if one is painted.
  fn shadow(&self) -> Option<(f32, f32, Color)> {
    self.quads().iter().find_map(|quad| match &quad.content {
      QuadContent::BoxShadow(shadow) => Some((shadow.blur, shadow.offset_y, shadow.color)),
      _ => None,
    })
  }
}

#[test]
fn a_foreground_transition_runs_between_state_styles_without_laying_out() {
  let mut stage = Stage::new(
    Column::new().child(
      Row::new()
        .size(120.0, 40.0)
        .foreground(RED)
        .transition(Transition::foreground().duration_ms(1000).linear())
        .hovered(|style| style.foreground(BLUE))
        .child(Text::new("Save")),
    ),
  );
  assert_eq!(stage.label_color(), RED);

  stage.tree.mouse_move(60.0, 20.0);
  stage.at(0);
  assert_eq!(
    stage.label_color(),
    RED,
    "the first frame shows where the transition starts"
  );

  let report = stage.at(500);
  assert_eq!(stage.label_color(), HALFWAY);
  assert!(!report.layout_recalculated, "a foreground on its way lays nothing out");

  stage.at(1000);
  assert_eq!(stage.label_color(), BLUE);
}

#[test]
fn a_color_given_as_a_palette_role_transitions() {
  let mut stage = Stage::new(
    Column::new().child(
      Rect::new(120.0, 40.0)
        .background(PaletteColor::Accent)
        .transition(Transition::background_color().duration_ms(1000).linear())
        .hovered(|style| style.background(PaletteColor::AccentHover)),
    ),
  );
  // A role whose color changes is a change of the color: it runs the transition too. Let that one end.
  stage.app.theme().set_palette_color(PaletteColor::Accent, RED);
  stage.app.theme().set_palette_color(PaletteColor::AccentHover, BLUE);
  stage.at(0);
  stage.at(2000);
  assert_eq!(stage.fill(), RED);

  stage.tree.mouse_move(60.0, 20.0);
  stage.at(2000);
  stage.at(2500);

  assert_eq!(
    stage.fill(),
    HALFWAY,
    "the roles are read against the theme and the color moves between them"
  );
}

#[test]
fn an_outline_grows_in_and_fades_out() {
  let mut stage = Stage::new(
    Column::new().padding(20.0).child(
      Button::new("Save")
        .size(120.0, 32.0)
        .tab_index(0)
        .transition(Transition::outline().duration_ms(1000).linear())
        .focus_visible(|style| style.outline(Outline::new(4.0, RING).offset(2.0))),
    ),
  );
  assert_eq!(stage.ring(), None);

  stage
    .tree
    .key_down("Tab".to_owned(), "Tab".to_owned(), false, false, false);
  stage
    .tree
    .key_up("Tab".to_owned(), "Tab".to_owned(), false, false, false);
  stage.at(0);
  stage.at(500);
  let (width, color, x) = stage.ring().expect("the ring is on its way in");
  assert_eq!(width, 2.0, "half of its width");
  assert_eq!(color, Color::new(40, 80, 240, 128), "and half of its opacity");
  assert_eq!(x, 20.0 - 2.0 - 2.0, "at its offset from the start");

  stage.at(1000);
  assert_eq!(stage.ring(), Some((4.0, RING, 14.0)));

  stage
    .tree
    .mouse_down(300.0, 250.0, lurq::app::events::MouseButton::Left);
  stage.tree.mouse_up(300.0, 250.0, lurq::app::events::MouseButton::Left);
  stage.at(1000);
  stage.at(1500);
  let (width, color, _) = stage
    .ring()
    .expect("the ring is on its way out, though the element has none now");
  assert_eq!((width, color.a()), (2.0, 128));

  stage.at(2000);
  assert_eq!(stage.ring(), None);
}

#[test]
fn a_box_shadow_transitions_to_another_and_from_none() {
  let mut stage = Stage::new(
    Column::new().padding(40.0).child(
      Rect::new(120.0, 40.0)
        .id("card")
        .background("#ffffff")
        .transition(Transition::box_shadow().duration_ms(1000).linear())
        .hovered(|style| style.box_shadow(BoxShadow::new(0.0, 8.0, 16.0, SHADE))),
    ),
  );
  assert_eq!(stage.shadow(), None);

  stage.tree.mouse_move(100.0, 60.0);
  stage.at(0);
  stage.at(500);
  assert_eq!(
    stage.shadow(),
    Some((8.0, 4.0, Color::new(0, 0, 0, 64))),
    "from nothing: half the blur, half the offset, half the opacity"
  );

  stage.at(1000);
  assert_eq!(stage.shadow(), Some((16.0, 8.0, SHADE)));
}

#[test]
fn transition_all_covers_the_paint_values() {
  let mut stage = Stage::new(
    Column::new().child(
      Row::new()
        .size(120.0, 40.0)
        .foreground(RED)
        .transition(Transition::all().duration_ms(1000).linear())
        .hovered(|style| style.foreground(BLUE))
        .child(Text::new("Save")),
    ),
  );

  stage.tree.mouse_move(60.0, 20.0);
  stage.at(0);
  stage.at(500);

  assert_eq!(stage.label_color(), HALFWAY);
}
