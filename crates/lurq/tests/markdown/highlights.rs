//! `MarkdownProps::highlights`: fills behind ranges of a document's
//! rendered text (lurq#75), checked against the glyphs the document draws
//! (`support::selection`): a highlight of some words covers their glyphs,
//! row by row, and nothing else, whatever paragraph, link, code span or
//! list they are in.

use std::ops::Range;

use lurq::{
  components::{MarkdownProps, TextHighlight},
  node::color::Color,
};

use super::selection::{page, update};
use crate::support::selection::Page;

/// The color the page reads as its highlight (the default selection
/// color); nothing is selected in these tests, so it is the fills'.
const FILL: Color = Color::new(191, 219, 254, 255);
const OTHER: Color = Color::new(250, 204, 21, 255);

const DOCUMENT: &str = "# Title\n\nSome **bold** and `code` with [a link](https://example.com) here.\n\n- one\n- two\n\n\
1. first\n\n```rust\nlet x = 1;\n```\n\n| a | b |\n|---|---|\n| c | d |\n\nNote[^1].\n\n[^1]: The note.\n";

/// A paragraph that wraps at 240px, with inline code and a link in it.
const PARAGRAPH: &str = "Search a long conversation for a word: the `current` match and [the others](https://example.com) \
are drawn behind the words, across the lines they wrap onto, in their own colors.";

/// The bytes of `needle`'s `nth` occurrence in `text`.
fn range_of(text: &str, needle: &str, nth: usize) -> Range<usize> {
  let start = text
    .match_indices(needle)
    .nth(nth)
    .unwrap_or_else(|| panic!("{needle:?} is not in {text:?}"))
    .0;
  start..start + needle.len()
}

/// `props` with `needle` (its `nth` occurrence in the rendered text) filled.
fn highlighting(props: MarkdownProps, needle: &str, nth: usize) -> MarkdownProps {
  let range = range_of(&props.rendered_text(), needle, nth);
  props.highlights(vec![TextHighlight::new(range, FILL)])
}

fn assert_fill_covers(page: &mut Page, needle: &str) {
  let (from, to) = page.inks_of(needle, 0);
  if let Some(mismatch) = page.highlight_mismatch(from, to) {
    panic!("the fill of {needle:?}: {mismatch}");
  }
}

#[test]
fn the_rendered_text_is_the_documents_words_without_markers_or_labels() {
  assert_eq!(
    MarkdownProps::new(DOCUMENT).rendered_text(),
    "Title\nSome bold and code with a link here.\none\ntwo\nfirst\nlet x = 1;\na\nb\nc\nd\nNote[^1].\nThe note."
  );
}

#[test]
fn a_highlight_covers_its_words_in_each_kind_of_block() {
  for needle in [
    "Title",
    "bold and code",
    "with a link here",
    "two",
    "first",
    "x = 1",
    "d",
    "The note",
  ] {
    // "d" is the last cell; in the rendered text it is the line "d".
    let nth = if needle == "d" { 3 } else { 0 };
    let props = highlighting(MarkdownProps::new(DOCUMENT), needle, nth);
    let mut page = page(props, 560.0);
    if needle == "d" {
      let start = page.text.rfind("\nd").expect("the last cell") + 1;
      let ink = page.inks.iter().position(|ink| ink.start == start).expect("its glyph");
      page.assert_highlight_covers(ink, ink);
      continue;
    }
    assert_fill_covers(&mut page, needle);
  }
}

#[test]
fn a_highlight_over_two_paragraphs_covers_both_parts() {
  let props = highlighting(MarkdownProps::new(DOCUMENT), "Title\nSome bold", 0);
  let mut page = page(props, 560.0);
  let from = page.ink_at("Title", 0);
  let (_, to) = page.inks_of("Some bold", 0);
  page.assert_highlight_covers(from, to);
}

#[test]
fn highlights_each_draw_in_their_own_color() {
  let props = MarkdownProps::new(DOCUMENT);
  let text = props.rendered_text();
  let props = props.highlights(vec![
    TextHighlight::new(range_of(&text, "one", 0), FILL),
    TextHighlight::new(range_of(&text, "two", 0), OTHER).radius(3.0),
  ]);
  let mut page = page(props, 560.0);
  assert_fill_covers(&mut page, "one");
  let snapshot = crate::support::render_pass_with_app(&mut page.tree, &mut page.app);
  let others: Vec<_> = snapshot.rects.iter().filter(|rect| rect.color == OTHER).collect();
  assert_eq!(others.len(), 1, "one fill in the other color: {others:?}");
  let (from, to) = page.inks_of("two", 0);
  assert!(
    others[0].x <= page.inks[from].left + 3.0 && others[0].x + others[0].width >= page.inks[to].right - 3.0,
    "the other fill covers \"two\": {:?}",
    others[0]
  );
}

#[test]
fn every_word_of_a_wrapping_paragraph_is_covered_by_its_highlight() {
  let props = MarkdownProps::new(PARAGRAPH);
  let text = props.rendered_text();
  let mut page = page(props.clone(), 240.0);
  assert_eq!(page.text, text, "one paragraph: the page's text is the rendered text");
  assert!(
    page.inks.iter().map(|ink| ink.row).max().unwrap_or(0) >= 3,
    "the paragraph wraps onto several lines"
  );
  let words: Vec<Range<usize>> = text
    .split(' ')
    .scan(0, |at, word| {
      let range = *at..*at + word.len();
      *at = range.end + 1;
      Some(range)
    })
    .collect();
  for range in &words {
    update(
      &mut page,
      props.clone().highlights(vec![TextHighlight::new(range.clone(), FILL)]),
    );
    let from = page.inks.iter().position(|ink| ink.start == range.start).unwrap();
    let to = page.inks.iter().rposition(|ink| ink.end <= range.end).unwrap();
    if let Some(mismatch) = page.highlight_mismatch(from, to) {
      panic!("the fill of {:?}: {mismatch}", &text[range.clone()]);
    }
  }
  // Every second word at once, over the wraps between them.
  let spans: Vec<Range<usize>> = words
    .chunks(2)
    .map(|pair| pair[0].start..pair[pair.len() - 1].end)
    .collect();
  for span in spans {
    update(
      &mut page,
      props.clone().highlights(vec![TextHighlight::new(span.clone(), FILL)]),
    );
    let from = page.inks.iter().position(|ink| ink.start == span.start).unwrap();
    let to = page.inks.iter().rposition(|ink| ink.end <= span.end).unwrap();
    if let Some(mismatch) = page.highlight_mismatch(from, to) {
      panic!("the fill of {:?}: {mismatch}", &text[span.clone()]);
    }
  }
}

#[test]
fn highlights_cleared_on_a_rerender_draw_nothing() {
  let props = MarkdownProps::new(PARAGRAPH);
  let mut page = page(highlighting(props.clone(), "current", 0), 240.0);
  assert!(!page.highlights().is_empty());
  update(&mut page, props);
  assert!(page.highlights().is_empty(), "cleared: {:?}", page.highlights());
}
