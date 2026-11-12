use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    theme::{PaletteColor, TypographyStyle},
  },
  components::{Column, Popup, Row, Text},
  core::ElementRef,
  layout::{Constraints, Size, quad::QuadContent, text_style::TextStyle},
  node::{Element, color::Color},
};

use crate::support::TestSurface;

const RED: Color = Color::new(200, 30, 30, 255);
const BLUE: Color = Color::new(30, 60, 200, 255);
const GREEN: Color = Color::new(20, 160, 80, 255);
const BLACK: Color = Color::new(0, 0, 0, 255);

fn painted(app: &mut App, root: impl Into<Element>) -> Tree {
  let mut tree = Tree::new();
  tree.set_root(root);
  tree.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 300.0))));
  tree.pass(app, &TestSurface);
  tree
}

fn text_colors(tree: &Tree) -> Vec<(String, Color)> {
  tree
    .resolve_quads(tree.last_layout().unwrap())
    .iter()
    .filter_map(|quad| match &quad.content {
      QuadContent::Text { text, style, .. } => Some((text.clone(), style.color)),
      _ => None,
    })
    .collect()
}

fn color_of(tree: &Tree, text: &str) -> Color {
  text_colors(tree)
    .into_iter()
    .find_map(|(painted, color)| (painted == text).then_some(color))
    .unwrap_or_else(|| panic!("no text {text:?} was painted"))
}

#[test]
fn text_is_drawn_in_the_foreground_of_its_nearest_ancestor() {
  let mut app = App::new();
  let tree = painted(
    &mut app,
    Column::new()
      .foreground(RED)
      .child(Text::new("outer"))
      .child(Row::new().foreground(BLUE).child(Row::new().child(Text::new("inner")))),
  );

  assert_eq!(color_of(&tree, "outer"), RED);
  assert_eq!(color_of(&tree, "inner"), BLUE);
}

#[test]
fn a_foreground_ends_with_the_element_that_set_it() {
  let mut app = App::new();
  let tree = painted(
    &mut app,
    Column::new()
      .child(Text::new("before"))
      .child(Row::new().foreground(RED).child(Text::new("inside")))
      .child(Text::new("after")),
  );

  assert_eq!(color_of(&tree, "before"), BLACK);
  assert_eq!(color_of(&tree, "inside"), RED);
  assert_eq!(color_of(&tree, "after"), BLACK);
}

#[test]
fn text_with_no_foreground_above_keeps_its_typography_color() {
  let mut app = App::new();
  app.theme().set_typography_style(
    TypographyStyle::Caption,
    TextStyle {
      color: GREEN,
      ..TextStyle::default()
    },
  );
  let tree = painted(
    &mut app,
    Column::new()
      .child(Text::new("body"))
      .child(Text::new("caption").variant(TypographyStyle::Caption)),
  );

  assert_eq!(color_of(&tree, "body"), BLACK);
  assert_eq!(color_of(&tree, "caption"), GREEN);
}

#[test]
fn a_foreground_replaces_the_color_of_a_typography_style() {
  let mut app = App::new();
  app.theme().set_typography_style(
    TypographyStyle::Caption,
    TextStyle {
      color: GREEN,
      ..TextStyle::default()
    },
  );
  let tree = painted(
    &mut app,
    Column::new()
      .foreground(RED)
      .child(Text::new("caption").variant(TypographyStyle::Caption)),
  );

  assert_eq!(color_of(&tree, "caption"), RED);
}

#[test]
fn a_color_the_text_names_wins_over_the_foreground() {
  let mut app = App::new();
  let styled = TextStyle {
    color: BLUE,
    ..TextStyle::default()
  };
  let tree = painted(
    &mut app,
    Column::new()
      .foreground(RED)
      .child(Text::new("colored").color(GREEN))
      .child(Text::styled("styled", styled)),
  );

  assert_eq!(color_of(&tree, "colored"), GREEN);
  assert_eq!(color_of(&tree, "styled"), BLUE);
}

#[test]
fn a_text_can_set_its_own_foreground() {
  let mut app = App::new();
  let tree = painted(&mut app, Column::new().child(Text::new("label").foreground(RED)));

  assert_eq!(color_of(&tree, "label"), RED);
}

#[test]
fn a_palette_role_as_foreground_follows_the_theme() {
  let mut app = App::new();
  app.theme().set_palette_color(PaletteColor::Accent, RED);
  let mut tree = painted(
    &mut app,
    Column::new().foreground(PaletteColor::Accent).child(Text::new("label")),
  );
  assert_eq!(color_of(&tree, "label"), RED);

  app.theme().set_palette_color(PaletteColor::Accent, BLUE);
  tree.pass(&mut app, &TestSurface);

  assert_eq!(color_of(&tree, "label"), BLUE);
}

#[test]
fn setting_a_foreground_on_a_live_element_repaints_it_in_place() {
  let mut app = App::new();
  let mut tree = painted(&mut app, Column::new().id("panel").child(Text::new("label")));
  let size_before = tree.last_layout().unwrap().size;

  tree.get_element_by_id_mut("panel").unwrap().set_foreground(RED);
  let report = tree.pass(&mut app, &TestSurface);

  assert!(report.required, "the new color is drawn");
  assert_eq!(color_of(&tree, "label"), RED);
  assert_eq!(tree.last_layout().unwrap().size, size_before);
}

struct PageWithPopup {
  anchor: ElementRef,
}

impl Component for PageWithPopup {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self {
      anchor: ElementRef::new(),
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .foreground(RED)
      .child(Text::new("anchor").ref_element(self.anchor.clone()))
      .child(Popup::new(self.anchor.clone(), Column::new().child(Text::new("in popup"))).open_when(true))
  }
}

#[test]
fn the_content_of_a_popup_starts_from_no_foreground() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<PageWithPopup>(&mut app, ());
  tree.pass(&mut app, &TestSurface);
  tree.pass(&mut app, &TestSurface);

  assert_eq!(color_of(&tree, "anchor"), RED);
  assert_eq!(color_of(&tree, "in popup"), BLACK);
}
