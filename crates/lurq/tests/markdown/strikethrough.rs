//! `~~text~~` is drawn with a line through its words.

use lurq::{components::MarkdownProps, layout::quad::QuadContent};

use super::links::{mount, quads};

/// The thin filled rectangles of the paint: the lines drawn with text.
fn text_lines(source: &str) -> Vec<(f32, f32)> {
  let (_app, tree) = mount(MarkdownProps::new(source));
  quads(&tree)
    .iter()
    .filter_map(|quad| match &quad.content {
      QuadContent::Rect { color, .. } if color.a() > 0 && quad.height <= 3.0 && quad.width > 0.0 => {
        Some((quad.width, quad.height))
      }
      _ => None,
    })
    .collect()
}

#[test]
fn struck_text_has_a_line_through_its_words() {
  assert!(text_lines("Keep this and that.").is_empty());

  let lines = text_lines("Keep ~~this~~ and that.");
  assert_eq!(lines.len(), 1, "one line through the struck word");

  let longer = text_lines("Keep ~~this and that~~.");
  assert!(longer[0].0 > lines[0].0, "the line spans the struck words");
}
