//! Text whose characters are not one byte each, or not drawn left to right:
//! accented and Cyrillic letters, CJK, emoji outside the Basic Multilingual
//! Plane, and a right-to-left paragraph.

use lurq::components::MarkdownProps;

use super::page;
use crate::support::selection::{assert_drags_copy_their_glyphs, pairs};

const MULTIBYTE: &str = "Grüße aus Zürich: naïve café, Привет мир, 日本語のテキスト, and `code` with ß in it.";
const ASTRAL: &str = "A smile 🙂 then a rocket 🚀 and `code 🙂 span` with 𝔘𝔫𝔦𝔠𝔬𝔡𝔢 letters, then a cat 🐈 at the end.";
const HEBREW: &str = "שלום עולם, זהו משפט בעברית עם `קוד` בתוכו ועוד כמה מילים כדי שהוא יעטוף לשורה נוספת ברוחב הזה.";

#[test]
fn every_drag_over_multibyte_text_copies_the_glyphs_it_covers() {
  let mut page = page(MarkdownProps::new(MULTIBYTE), 360.0);
  assert!(page.inks.iter().any(|ink| ink.row > 0), "the text wraps");
  let count = page.inks.len();
  assert_drags_copy_their_glyphs(&mut page, pairs(count, 3));
  for (from, to) in pairs(count, 11) {
    page.drag_inks(from, to);
    page.assert_highlight_covers(from, to);
  }
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "rows are told apart by ink that does not overlap; iOS draws an emoji into the next row"
)]
fn every_drag_over_emoji_and_astral_letters_copies_the_glyphs_it_covers() {
  let mut page = page(MarkdownProps::new(ASTRAL), 360.0);
  assert!(page.inks.iter().any(|ink| ink.row > 0), "the text wraps");
  let count = page.inks.len();
  assert_drags_copy_their_glyphs(&mut page, pairs(count, 3));
  for (from, to) in pairs(count, 11) {
    page.drag_inks(from, to);
    page.assert_highlight_covers(from, to);
  }
}

/// A right-to-left paragraph: its glyphs run right to left on every row, so
/// a drag from the right edge of the first row to the left edge of the last
/// selects the whole text, and dragging from the right edge of the text to a
/// point in it selects what is written before that point.
#[test]
fn drags_over_a_right_to_left_paragraph_copy_what_they_cover() {
  let mut page = page(MarkdownProps::new(HEBREW), 320.0);
  let rows = page.inks.iter().map(|ink| ink.row).max().unwrap_or(0) + 1;
  assert!(rows >= 2, "the paragraph wraps");
  let row_edges = |page: &crate::support::selection::Page, row: usize| {
    let on_row = page.inks.iter().filter(|ink| ink.row == row);
    let left = on_row.clone().map(|ink| ink.left).fold(f32::MAX, f32::min);
    let right = on_row.clone().map(|ink| ink.right).fold(f32::MIN, f32::max);
    let y = on_row.clone().next().expect("a glyph on the row").y;
    (left, right, y)
  };
  let (_, first_right, first_y) = row_edges(&page, 0);
  let (last_left, _, last_y) = row_edges(&page, rows - 1);
  let text = page.text.clone();

  page.drag((first_right, first_y), (last_left, last_y));
  assert_eq!(page.selected().as_deref(), Some(text.as_str()), "the whole paragraph");

  // The first row alone: from its right edge to its left edge.
  let (first_left, ..) = row_edges(&page, 0);
  page.drag((first_right, first_y), (first_left, first_y));
  let first_row = page.selected().expect("the first row is selected");
  assert!(
    text.starts_with(&first_row),
    "the first row {first_row:?} starts the text"
  );
  assert!(first_row.len() < text.len(), "the first row is not the whole text");

  // The rest, from where the first row ends: the two drags cover the text.
  page.drag((first_left, first_y), (last_left, last_y));
  let rest = page.selected().expect("the rest is selected");
  assert_eq!(
    format!("{}{}", first_row.trim_end(), rest.trim_start()).replace(' ', ""),
    text.replace(' ', "")
  );

  // Into the middle of the first row: a prefix, no longer than the row.
  let (_, first_right, _) = row_edges(&page, 0);
  let middle = (first_right + first_left) / 2.0;
  page.drag((first_right, first_y), (middle, first_y));
  let prefix = page.selected().expect("a prefix is selected");
  assert!(
    text.starts_with(&prefix) && prefix.len() < first_row.len(),
    "{prefix:?} is a shorter prefix"
  );
}
