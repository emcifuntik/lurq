//! Selecting and copying plain `Text` outside Markdown, checked against the
//! drawn glyphs (`support::selection`): single-line, wrapped and
//! multi-paragraph texts, texts stacked in a column, and a selection
//! extended with Shift.

use lurq::{
  app::{App, Tree},
  components::{Column, Text, TextInput},
  core::Signal,
  layout::text_style::{TextStyle, VerticalAlign},
};

use crate::support::{
  TestSurface,
  selection::{Page, assert_drags_copy_their_glyphs, pairs, rows_copying_other_text, rows_highlighted_elsewhere},
};

const WRAPPED: &str = "A plain text with enough words in it to wrap onto a few lines at the width of the test, with \
accents (déjà vu), digits 2423 and punctuation; nothing else.";
const PARAGRAPHS: &str = "First line of the text.\nSecond line, a bit longer than the first one.\n\nFourth line after an \
empty one.";

fn page(root: impl Into<lurq::node::Element>, scale: f32) -> Page {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_scale_factor(scale);
  tree.resize((800.0 * scale) as u32, (600.0 * scale) as u32);
  tree.set_root(root);
  tree.pass(&mut app, &TestSurface);
  Page::new(app, tree, scale)
}

fn sweep(mut page: Page, step: usize) -> Page {
  let count = page.inks.len();
  assert_drags_copy_their_glyphs(&mut page, pairs(count, step));
  for (from, to) in pairs(count, step * 3).chain([(0, count - 1)]) {
    page.drag_inks(from, to);
    page.assert_highlight_covers(from, to);
  }
  page
}

#[test]
fn every_drag_over_a_wrapped_text_copies_the_glyphs_it_covers() {
  let page = sweep(
    page(
      Column::new().width(320.0).child(Text::new(WRAPPED).selectable(true)),
      1.0,
    ),
    5,
  );
  assert!(
    page.inks.iter().any(|ink| ink.row > 1),
    "the text wraps onto three lines"
  );
}

#[test]
fn every_drag_over_a_wrapped_text_at_a_fractional_scale_copies_the_glyphs_it_covers() {
  let page = sweep(
    page(
      Column::new().width(320.0).child(Text::new(WRAPPED).selectable(true)),
      1.5,
    ),
    5,
  );
  assert!(
    page.inks.iter().any(|ink| ink.row > 1),
    "the text wraps onto three lines"
  );
}

#[test]
fn every_drag_over_a_text_with_line_breaks_copies_the_glyphs_it_covers() {
  let mut page = page(Text::new(PARAGRAPHS).selectable(true), 1.0);
  let count = page.inks.len();
  assert_drags_copy_their_glyphs(&mut page, pairs(count, 3));
  for (from, to) in pairs(count, 9).chain([(0, count - 1)]) {
    page.drag_inks(from, to);
    page.assert_highlight_covers(from, to);
  }
}

#[test]
fn a_drag_across_stacked_texts_copies_each_on_its_own_line() {
  let mut page = page(
    Column::new()
      .spacing(8.0)
      .child(Text::new("the first text").selectable(true))
      .child(Text::new("the second text").selectable(true))
      .child(Text::new("the third text").selectable(true)),
    1.0,
  );
  let (from, _) = page.inks_of("first", 0);
  let (_, to) = page.inks_of("third", 0);
  page.drag_inks(from, to);
  assert_eq!(
    page.selected().as_deref(),
    Some("first text\nthe second text\nthe third")
  );
  page.assert_highlight_covers(from, to);
}

#[test]
fn a_shift_click_extends_a_plain_text_selection_and_a_new_drag_replaces_it() {
  let mut page = page(
    Column::new().width(320.0).child(Text::new(WRAPPED).selectable(true)),
    1.0,
  );
  let (from, to) = page.inks_of("enough words", 0);
  page.drag_inks(from, to);
  assert_eq!(page.selected().as_deref(), Some("enough words"));

  let (_, digits) = page.inks_of("2423", 0);
  let after = page.after(digits);
  page.shift_click(after.0, after.1);
  assert_eq!(page.selected().as_deref(), Some(&*page.text_between(from, digits)));

  let (from, to) = page.inks_of("nothing else", 0);
  page.drag_inks(from, to);
  assert_eq!(page.selected().as_deref(), Some("nothing else"));
  page.assert_highlight_covers(from, to);
}

#[test]
fn a_double_click_selects_a_word_and_a_triple_click_its_line() {
  let mut page = page(Text::new(PARAGRAPHS).selectable(true), 1.0);
  let (from, to) = page.inks_of("longer", 0);
  let (x, y) = page.before(from + 1);
  page.click(x, y);
  page.click(x, y);
  assert_eq!(page.selected().as_deref(), Some("longer"));
  page.assert_highlight_covers(from, to);
  page.click(x, y);
  assert_eq!(
    page.selected().as_deref(),
    Some("Second line, a bit longer than the first one.")
  );
}

#[test]
fn every_row_of_a_wrapped_text_copies_itself_at_fractional_widths_and_scales() {
  let mut failures = Vec::new();
  let mut pages = 0;
  for scale in [1.0_f32, 1.25, 1.5] {
    for step in 0..26 {
      let width = 200.17 + step as f32 * 6.37;
      let mut page = page(
        Column::new()
          .width(width)
          .child(Text::new(WRAPPED).selectable(true).width(width)),
        scale,
      );
      assert!(page.inks.iter().any(|ink| ink.row > 1), "the text wraps at {width}");
      pages += 1;
      failures.extend(rows_copying_other_text(
        &mut page,
        &format!("width {width} scale {scale}"),
      ));
    }
  }
  assert!(
    failures.is_empty(),
    "{} rows over {pages} pages copied other text:\n{}",
    failures.len(),
    failures.join("\n")
  );
}

/// A `Text`'s padding grows its box and leaves its glyphs where they are:
/// the paint draws them in the whole box, wrapped at its width and centered
/// in its height or at its top. Texts with uneven padding at fractional
/// widths and scales: every row copies itself and is highlighted over its
/// own glyphs.
#[test]
fn every_row_of_a_padded_text_copies_itself_and_is_highlighted_at_fractional_widths_and_scales() {
  let top = TextStyle {
    vertical_align: VerticalAlign::Top,
    ..TextStyle::default()
  };
  let mut failures = Vec::new();
  let mut pages = 0;
  for scale in [1.0_f32, 1.25, 1.5] {
    for step in 0..12 {
      let width = 200.17 + step as f32 * 13.37;
      for (name, text) in [
        ("centered", Text::new(WRAPPED)),
        ("top", Text::styled(WRAPPED, top.clone())),
      ] {
        let text = text
          .selectable(true)
          .width(width)
          .height(260.0)
          .padding_left(7.3)
          .padding_top(3.1)
          .padding_right(12.9)
          .padding_bottom(5.7);
        let mut page = page(Column::new().width(width + 20.0).child(text), scale);
        assert!(page.inks.iter().any(|ink| ink.row > 1), "the text wraps at {width}");
        pages += 1;
        let page_name = format!("{name} width {width} scale {scale}");
        failures.extend(rows_copying_other_text(&mut page, &page_name));
        failures.extend(rows_highlighted_elsewhere(&mut page, &page_name));
      }
    }
  }
  assert!(
    failures.is_empty(),
    "{} rows over {pages} pages copied or highlighted other glyphs than they draw:\n{}",
    failures.len(),
    failures.join("\n")
  );
}

const HEBREW: &str = "שלום עולם, זהו משפט בעברית עם עוד כמה מילים כדי שהוא יעטוף לשורה נוספת ברוחב הזה.";

/// A right-to-left text: its glyphs run right to left on every row, so a
/// drag from the right edge of the first row to the left edge of the last
/// selects the whole text, and a drag from the right edge to the middle of
/// a row selects what is written before that point.
#[test]
fn drags_over_a_right_to_left_text_copy_what_they_cover() {
  let mut page = page(
    Column::new().width(300.0).child(Text::new(HEBREW).selectable(true)),
    1.0,
  );
  let rows = page.inks.iter().map(|ink| ink.row).max().unwrap_or(0) + 1;
  assert!(rows >= 2, "the text wraps");
  let edges = |page: &Page, row: usize| {
    let on_row = page.inks.iter().filter(|ink| ink.row == row);
    let left = on_row.clone().map(|ink| ink.left).fold(f32::MAX, f32::min);
    let right = on_row.clone().map(|ink| ink.right).fold(f32::MIN, f32::max);
    let y = on_row.clone().next().expect("a glyph on the row").y;
    (left, right, y)
  };
  let (first_left, first_right, first_y) = edges(&page, 0);
  let (last_left, _, last_y) = edges(&page, rows - 1);
  let text = page.text.clone();

  page.drag((first_right, first_y), (last_left, last_y));
  assert_eq!(page.selected().as_deref(), Some(text.as_str()), "the whole text");

  page.drag((first_right, first_y), ((first_left + first_right) / 2.0, first_y));
  let prefix = page.selected().expect("a prefix is selected");
  assert!(
    text.starts_with(&prefix) && !prefix.is_empty() && prefix.len() < text.len(),
    "{prefix:?} starts the text"
  );
}

#[test]
fn a_press_in_a_text_input_forgets_where_the_selection_started() {
  let mut page = page(
    Column::new()
      .width(320.0)
      .spacing(8.0)
      .child(Text::new(WRAPPED).selectable(true))
      .child(TextInput::new(Signal::new("an input".to_owned())).id("input")),
    1.0,
  );
  let (from, to) = page.inks_of("enough words", 0);
  page.drag_inks(from, to);
  assert_eq!(page.selected().as_deref(), Some("enough words"));

  let input = page
    .tree
    .get_element_by_id_mut("input")
    .expect("the input")
    .bounds()
    .expect("laid out");
  let (x, y) = (input.x + 4.0, input.y + input.height / 2.0);
  page.click(x, y);
  assert_eq!(page.selected(), None, "the press in the input clears the selection");

  // With Shift, the next drag starts over at its pressed point instead of
  // extending from "enough words".
  let (from, to) = page.inks_of("nothing else", 0);
  let (before, after) = (page.before(from), page.after(to));
  page.shift_press(before.0, before.1);
  page
    .tree
    .mouse_move_with_modifiers(after.0, after.1, true, false, false);
  page.shift_release(after.0, after.1);
  assert_eq!(page.selected().as_deref(), Some("nothing else"));
}
