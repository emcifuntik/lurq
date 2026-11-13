//! Where each span of rich text lies once laid out: one box per span and
//! laid-out line, with the line's baseline and the underline and strikeout
//! metrics of the span's font. Underlines, lines through text, link fills,
//! focus rings and link hit areas are drawn and tested against these boxes,
//! so they follow wrapping. A plain text has one box per laid-out line.

use std::{collections::HashMap, sync::Arc};

use cosmic_text::{Buffer, FontSystem, LayoutGlyph, LayoutRun, fontdb};

/// The font's underline when it gives none, in ems (cosmic-text's fallback).
const FALLBACK_UNDERLINE_OFFSET_EM: f32 = -0.125;
const FALLBACK_UNDERLINE_THICKNESS_EM: f32 = 1.0 / 14.0;
/// The font's strikeout when it gives none, in ems (cosmic-text's fallback).
const FALLBACK_STRIKEOUT_OFFSET_EM: f32 = 0.3;
const FALLBACK_STRIKEOUT_THICKNESS_EM: f32 = 1.0 / 14.0;

/// The part of one span on one laid-out line, in the text's coordinates (the
/// first line box starts at `y = 0`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SpanBox {
  /// Index of the span in the rich text's spans.
  pub(crate) span: u32,
  pub(crate) x: f32,
  pub(crate) width: f32,
  /// Top of the line box.
  pub(crate) top: f32,
  /// Height of the line box.
  pub(crate) height: f32,
  /// The line's baseline.
  pub(crate) baseline: f32,
  /// Where the font puts its underline's center, below the baseline.
  pub(crate) underline_offset: f32,
  /// The font's underline thickness.
  pub(crate) underline_thickness: f32,
  /// Where the font puts its strikeout's center, from the baseline: negative, as it is above it.
  pub(crate) strikethrough_offset: f32,
  /// The font's strikeout thickness.
  pub(crate) strikethrough_thickness: f32,
}

/// The underline and the strikeout of a face in ems: each one's offset above
/// the baseline, then its thickness.
type FaceLines = [f32; 4];

/// Font underline and strikeout metrics in ems, per face.
#[derive(Default)]
pub(super) struct UnderlineMetrics {
  faces: HashMap<(fontdb::ID, u16), FaceLines>,
}

const FALLBACK_LINES: FaceLines = [
  FALLBACK_UNDERLINE_OFFSET_EM,
  FALLBACK_UNDERLINE_THICKNESS_EM,
  FALLBACK_STRIKEOUT_OFFSET_EM,
  FALLBACK_STRIKEOUT_THICKNESS_EM,
];

impl UnderlineMetrics {
  /// In pixels, from the baseline down: the underline's offset and thickness,
  /// then the strikeout's.
  fn of(&mut self, font_system: &mut FontSystem, glyph: &LayoutGlyph) -> FaceLines {
    let key = (glyph.font_id, glyph.font_weight.0);
    let [underline, underline_thickness, strikeout, strikeout_thickness] =
      *self.faces.entry(key).or_insert_with(|| {
        font_system
          .get_font(glyph.font_id, glyph.font_weight)
          .and_then(|font| {
            let metrics = font.metrics();
            let upem = f32::from(metrics.units_per_em);
            (upem > 0.0).then(|| {
              let [underline, underline_thickness] = metrics.underline.map_or(
                [FALLBACK_UNDERLINE_OFFSET_EM, FALLBACK_UNDERLINE_THICKNESS_EM],
                |line| [line.offset / upem, line.thickness / upem],
              );
              let [strikeout, strikeout_thickness] = metrics.strikeout.map_or(
                [FALLBACK_STRIKEOUT_OFFSET_EM, FALLBACK_STRIKEOUT_THICKNESS_EM],
                |line| [line.offset / upem, line.thickness / upem],
              );
              [underline, underline_thickness, strikeout, strikeout_thickness]
            })
          })
          .unwrap_or(FALLBACK_LINES)
      });
    [
      -underline * glyph.font_size,
      underline_thickness * glyph.font_size,
      -strikeout * glyph.font_size,
      strikeout_thickness * glyph.font_size,
    ]
  }

  pub(super) fn estimated_memory_bytes(&self) -> usize {
    self.faces.capacity() * std::mem::size_of::<((fontdb::ID, u16), FaceLines)>()
  }
}

/// One box per laid-out line of a shaped plain text: the words of the line,
/// without the whitespace that hangs past its end, with the metrics of the
/// font of its first glyph. A line without words has no box.
pub(super) fn line_boxes(
  buffer: &Buffer,
  font_system: &mut FontSystem,
  metrics: &mut UnderlineMetrics,
) -> Arc<[SpanBox]> {
  let mut boxes = Vec::new();
  for run in buffer.layout_runs() {
    let end = run
      .glyphs
      .iter()
      .rposition(|glyph| !super::glyph_cluster_is_whitespace(run.text, glyph))
      .map_or(0, |index| index + 1);
    let Some(first) = run.glyphs[..end].first() else {
      continue;
    };
    let (left, right) = run.glyphs[..end]
      .iter()
      .fold((f32::MAX, f32::MIN), |(left, right), glyph| {
        (left.min(glyph.x), right.max(glyph.x + glyph.w))
      });
    let [
      underline_offset,
      underline_thickness,
      strikethrough_offset,
      strikethrough_thickness,
    ] = metrics.of(font_system, first);
    boxes.push(SpanBox {
      span: 0,
      x: left,
      width: right - left,
      top: run.line_top,
      height: run.line_height,
      baseline: run.line_y,
      underline_offset,
      underline_thickness,
      strikethrough_offset,
      strikethrough_thickness,
    });
  }
  boxes.into()
}

/// The span boxes of a shaped buffer whose glyphs carry their span index as
/// metadata.
pub(super) fn span_boxes(
  buffer: &Buffer,
  font_system: &mut FontSystem,
  metrics: &mut UnderlineMetrics,
) -> Arc<[SpanBox]> {
  let mut boxes = Vec::new();
  for run in buffer.layout_runs() {
    push_run_boxes(&run, font_system, metrics, &mut boxes);
  }
  boxes.into()
}

fn push_run_boxes(
  run: &LayoutRun<'_>,
  font_system: &mut FontSystem,
  metrics: &mut UnderlineMetrics,
  boxes: &mut Vec<SpanBox>,
) {
  // Whitespace that ends a wrapped line hangs past it and is not part of any
  // run of words, so nothing is drawn under it.
  let end = run
    .glyphs
    .iter()
    .rposition(|glyph| !super::glyph_cluster_is_whitespace(run.text, glyph))
    .map_or(0, |index| index + 1);
  let run_start = boxes.len();
  for glyph in &run.glyphs[..end] {
    let span = u32::try_from(glyph.metadata).unwrap_or(u32::MAX);
    if let Some(last) = boxes[run_start..].last_mut()
      && last.span == span
    {
      let right = (last.x + last.width).max(glyph.x + glyph.w);
      last.x = last.x.min(glyph.x);
      last.width = right - last.x;
      continue;
    }
    let [
      underline_offset,
      underline_thickness,
      strikethrough_offset,
      strikethrough_thickness,
    ] = metrics.of(font_system, glyph);
    boxes.push(SpanBox {
      span,
      x: glyph.x,
      width: glyph.w,
      top: run.line_top,
      height: run.line_height,
      baseline: run.line_y,
      underline_offset,
      underline_thickness,
      strikethrough_offset,
      strikethrough_thickness,
    });
  }
}
