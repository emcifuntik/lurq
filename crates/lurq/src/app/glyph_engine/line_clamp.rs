//! The laid-out lines of a wrapped plain text: how many there are, and where
//! one of them ends. A text cut to a number of lines (`Text::max_lines`) is
//! found with these, in the layout that sizes it and in the one that paints
//! it (`layout_engine::line_limit`).

use cosmic_text::LineIter;

use super::GlyphEngine;
use crate::layout::text_style::TextStyle;

impl GlyphEngine {
  /// How many lines `text` is laid out in when wrapped at `max_width`.
  pub(crate) fn wrapped_line_count(&mut self, text: &str, style: &TextStyle, max_width: f32) -> usize {
    let buffer = self.full_text_buffer(text, style, max_width, true);
    let count = buffer.layout_runs().count();
    self.retain_plain_buffer(buffer);
    count
  }

  /// Where in `text` its line number `line` (from 1) ends when wrapped at
  /// `max_width`, if lines follow it.
  pub(crate) fn end_of_wrapped_line(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    line: usize,
  ) -> Option<usize> {
    let buffer = self.full_text_buffer(text, style, max_width, true);
    let mut runs = buffer.layout_runs();
    let last = runs.nth(line.checked_sub(1)?).map(|run| {
      // A run's glyphs index its paragraph; an empty line ends where it starts.
      let end = run.glyphs.iter().map(|glyph| glyph.end).max();
      (run.line_i, end)
    });
    let more = runs.next().is_some();
    drop(runs);
    self.retain_plain_buffer(buffer);
    let (paragraph, end) = last.filter(|_| more)?;
    let range = LineIter::new(text).nth(paragraph)?.0;
    Some(end.map_or(range.start, |end| (range.start + end).min(range.end)))
  }
}
