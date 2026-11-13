//! A wrapped text cut to a number of lines, like CSS `line-clamp`: what fits
//! in the lines it may take, ending in an ellipsis on the last of them.
//!
//! A text is laid out twice. Layout measures it in logical pixels at the
//! width it is given; the paint shapes it in device pixels at its box's
//! width (see `selection_layout`). At a fractional display scale the two can
//! break a line at different words, so a text that takes two lines in one
//! can take three in the other. The limit holds in both: the text kept is
//! one that neither layout needs more lines for.

use super::{LayoutEngine, bounded_text_width, selection_layout::painted_max_width};
use crate::{
  app::glyph_engine::GlyphEngine,
  layout::{Constraints, text_style::TextStyle},
};

const ELLIPSIS: char = '…';

impl LayoutEngine {
  /// How many lines the paint draws `text` in, in the box layout gives it
  /// under `constraints`.
  fn painted_line_count(
    &self,
    glyph_engine: &mut GlyphEngine,
    text: &str,
    style: &TextStyle,
    constraints: Constraints,
  ) -> usize {
    let scale = self.paint_scale.get();
    let width = self
      .layout_text(glyph_engine, text, style, constraints, true)
      .size
      .width;
    let mut scaled = style.clone();
    scaled.scale_pixels(scale);
    glyph_engine.wrapped_line_count(text, &scaled, painted_max_width(scale, true, style.text_align, width))
  }

  /// Whether `text` takes at most `max_lines` lines, where layout measures
  /// it and where the paint draws it.
  fn fits_lines(
    &self,
    glyph_engine: &mut GlyphEngine,
    text: &str,
    style: &TextStyle,
    constraints: Constraints,
    max_lines: usize,
  ) -> bool {
    glyph_engine.wrapped_line_count(text, style, constraints.max_width) <= max_lines
      // At a scale of one the paint breaks the lines layout measured.
      && (self.paint_scale.get() == 1.0 || self.painted_line_count(glyph_engine, text, style, constraints) <= max_lines)
  }

  /// `text` cut to `max_lines` lines when wrapped under `constraints`: its
  /// start, as much of it as leaves room for an ellipsis on the last line,
  /// and the ellipsis. `None` when it fits as it is, or has no limit.
  /// Whitespace before the ellipsis is dropped, and a text that cannot keep
  /// a character still shows the ellipsis.
  pub(super) fn text_cut_to_lines(
    &self,
    glyph_engine: &mut GlyphEngine,
    text: &str,
    style: &TextStyle,
    constraints: Constraints,
    max_lines: usize,
  ) -> Option<String> {
    if max_lines == 0 || !bounded_text_width(constraints.max_width) {
      return None;
    }
    if self.fits_lines(glyph_engine, text, style, constraints, max_lines) {
      return None;
    }
    // Start from where the last line layout allows ends; a text only the paint needs more lines for starts whole.
    let mut end = glyph_engine
      .end_of_wrapped_line(text, style, constraints.max_width, max_lines)
      .unwrap_or(text.len());
    loop {
      let kept = text[..end].trim_end();
      let candidate = format!("{kept}{ELLIPSIS}");
      if kept.is_empty() || self.fits_lines(glyph_engine, &candidate, style, constraints, max_lines) {
        return Some(candidate);
      }
      // One character fewer, and try again: the ellipsis took the last one's room.
      end = kept.char_indices().next_back().map_or(0, |(index, _)| index);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::layout::Size;

  const TEXT: &str = "The quick brown fox jumps over the lazy dog while seven wizards quietly boxed";

  /// The lines the paint draws `shown` in, in the box layout gave it.
  fn painted_lines(
    engine: &LayoutEngine,
    glyph_engine: &mut GlyphEngine,
    shown: &str,
    constraints: Constraints,
  ) -> usize {
    engine.painted_line_count(glyph_engine, shown, &TextStyle::default(), constraints)
  }

  #[test]
  fn a_cut_text_is_painted_in_its_limit_of_lines_at_any_scale_and_width() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let style = TextStyle::default();
    let mut cut = 0;
    for scale in [1.0_f32, 1.1, 1.25, 1.33, 1.5, 1.75, 2.0, 2.25] {
      engine.set_paint_scale(scale);
      for step in 0..140 {
        let width = 70.0 + step as f32 * 0.5;
        let constraints = Constraints::loose(Size::new(width, 400.0));
        for max_lines in [1_usize, 2, 3] {
          let shown = engine.text_cut_to_lines(&mut glyph_engine, TEXT, &style, constraints, max_lines);
          let Some(shown) = shown else {
            assert!(glyph_engine.wrapped_line_count(TEXT, &style, width) <= max_lines);
            assert!(painted_lines(&engine, &mut glyph_engine, TEXT, constraints) <= max_lines);
            continue;
          };
          cut += 1;
          assert!(shown.ends_with(ELLIPSIS));
          assert!(
            glyph_engine.wrapped_line_count(&shown, &style, width) <= max_lines,
            "layout: {shown:?} at {width} x{scale}"
          );
          assert!(
            painted_lines(&engine, &mut glyph_engine, &shown, constraints) <= max_lines,
            "paint: {shown:?} at {width} x{scale}"
          );
        }
      }
    }
    assert!(cut > 0, "the widths cut the text");
  }

  #[test]
  fn the_paint_needs_no_more_lines_than_layout_measured() {
    // What the limit rests on at a fractional scale; the cut checks both layouts all the same. The paint wraps
    // at the box's width rounded up to a device pixel, so it has the room layout had and up to a pixel more:
    // with some fonts a word fits in that pixel, and the paint then takes a line fewer than layout measured
    // (DejaVu Sans at 78 and 120.5, at x1.1, x1.25 and x1.33).
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let style = TextStyle::default();
    for scale in [1.1_f32, 1.25, 1.33, 1.5, 1.75, 2.25] {
      engine.set_paint_scale(scale);
      for step in 0..300 {
        let width = 60.0 + step as f32 * 0.5;
        let constraints = Constraints::loose(Size::new(width, 400.0));
        let painted = painted_lines(&engine, &mut glyph_engine, TEXT, constraints);
        let measured = glyph_engine.wrapped_line_count(TEXT, &style, width);
        assert!(
          painted <= measured,
          "painted in {painted} lines, measured in {measured}, at {width} x{scale}"
        );
      }
    }
  }
}
