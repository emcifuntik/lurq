//! A Markdown document whose style names no color is drawn in the theme's
//! foreground.

use lurq::{
  app::{App, Tree, theme::PaletteColor},
  components::MarkdownProps,
  layout::{quad::QuadContent, text_style::TextStyle},
  node::color::Color,
};

use super::links::{MarkdownRoot, quads};
use crate::support::TestSurface;

const BLUE: Color = Color::new(30, 60, 200, 255);
const RED: Color = Color::new(200, 30, 30, 255);

/// The color of the first words painted.
fn words_color(props: MarkdownProps) -> Color {
  let mut app = App::new();
  app.theme().set_palette_color(PaletteColor::Foreground, BLUE);
  let mut tree = Tree::new();
  tree.mount_root::<MarkdownRoot>(&mut app, props);
  tree.pass(&mut app, &TestSurface);
  quads(&tree)
    .iter()
    .find_map(|quad| match &quad.content {
      QuadContent::Text { style, .. } => Some(style.color),
      QuadContent::RichText { spans, .. } => spans.first().map(|span| span.style.color),
      _ => None,
    })
    .expect("the document paints its words")
}

#[test]
fn a_document_that_names_no_color_is_drawn_in_the_themes_foreground() {
  assert_eq!(words_color(MarkdownProps::new("Some words.")), BLUE);
}

#[test]
fn a_document_keeps_the_color_its_style_names() {
  let style = TextStyle {
    color: RED,
    ..TextStyle::default()
  };
  assert_eq!(words_color(MarkdownProps::styled("Some words.", style)), RED);
}
