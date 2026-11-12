//! A wrapped text cut to a number of lines, like CSS `line-clamp`: what fits
//! in the lines it may take, ending in an ellipsis on the last of them.

use cosmic_text::LineIter;

use super::GlyphEngine;
use crate::layout::text_style::TextStyle;

const ELLIPSIS: char = '…';

impl GlyphEngine {
  /// How many lines `text` is laid out in when wrapped at `max_width`.
  fn wrapped_line_count(&mut self, text: &str, style: &TextStyle, max_width: f32) -> usize {
    let buffer = self.full_text_buffer(text, style, max_width, true);
    let count = buffer.layout_runs().count();
    self.retain_plain_buffer(buffer);
    count
  }

  /// Where in `text` its line number `max_lines` ends when wrapped at
  /// `max_width`, if lines follow it.
  fn end_of_line(&mut self, text: &str, style: &TextStyle, max_width: f32, max_lines: usize) -> Option<usize> {
    let buffer = self.full_text_buffer(text, style, max_width, true);
    let mut runs = buffer.layout_runs();
    let last = runs.nth(max_lines - 1).map(|run| {
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

  /// `text` cut to `max_lines` lines at `max_width`: its start, as much of it
  /// as leaves room for an ellipsis on the last line, and the ellipsis.
  /// `None` when it fits as it is. Whitespace before the ellipsis is dropped,
  /// and a text that cannot keep a character still shows the ellipsis.
  pub(crate) fn text_clamped_to_lines(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    max_lines: usize,
  ) -> Option<String> {
    if max_lines == 0 {
      return None;
    }
    let mut end = self.end_of_line(text, style, max_width, max_lines)?;
    loop {
      let kept = text[..end].trim_end();
      let candidate = format!("{kept}{ELLIPSIS}");
      if kept.is_empty() || self.wrapped_line_count(&candidate, style, max_width) <= max_lines {
        return Some(candidate);
      }
      // One character fewer, and try again: the ellipsis took the last one's room.
      end = kept.char_indices().next_back().map_or(0, |(index, _)| index);
    }
  }
}
