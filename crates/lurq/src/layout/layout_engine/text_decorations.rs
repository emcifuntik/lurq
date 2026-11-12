//! The lines a text draws with its glyphs: an underline, and a line through
//! it. They follow the text as it is painted: one piece per laid-out line,
//! wrapped lines included, where the font puts them.
//!
//! A plain `Text` gets the boxes of its lines here, once its own box is
//! known, from the layout the paint draws (see `selection_layout`); rich text
//! has the boxes of its spans (`inline_links`) and uses the same geometry.

use std::sync::Arc;

use super::{LayoutEngine, TextLayoutOutput, selection_layout::painted_max_width, transformed_quad_frame};
use crate::{
  app::glyph_engine::{GlyphEngine, SpanBox},
  layout::{
    layout_result::LayoutResult,
    quad::{ClipRect, Quad, QuadContent},
    text_style::TextStyle,
  },
  node::{Node, color::Color, node_kind::NodeKind, transform::Transform2D},
};

/// Where the lines of a decorated plain text lie in its box, in logical
/// pixels.
pub(crate) struct TextDecorationLayout {
  /// The first line's offset from the top of the text's box.
  pub(crate) top: f32,
  /// One box per laid-out line that holds words.
  pub(crate) lines: Arc<[SpanBox]>,
}

impl TextDecorationLayout {
  pub(super) fn estimated_memory_bytes(&self) -> usize {
    std::mem::size_of::<Self>() + self.lines.len() * std::mem::size_of::<SpanBox>()
  }
}

/// A rectangle in a text's coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LineRect {
  pub(crate) x: f32,
  pub(crate) y: f32,
  pub(crate) width: f32,
  pub(crate) height: f32,
}

fn usable(value: Option<f32>, allow_negative: bool) -> Option<f32> {
  value.filter(|value| value.is_finite() && (allow_negative || *value >= 0.0))
}

/// The underline `style` draws under the words of `line`, whose text starts
/// `top` below its box's top.
pub(crate) fn underline_piece(style: &TextStyle, line: &SpanBox, top: f32) -> Option<(LineRect, Color)> {
  let underline = style.underline?;
  if line.width <= 0.0 {
    return None;
  }
  let thickness = usable(underline.thickness, false).unwrap_or(line.underline_thickness.max(1.0));
  let offset = usable(underline.offset, true).unwrap_or(line.underline_offset - thickness * 0.5);
  let rect = LineRect {
    x: line.x,
    y: (top + line.baseline + offset).round(),
    width: line.width,
    height: thickness,
  };
  Some((rect, underline.color.unwrap_or(style.color)))
}

/// The line `style` draws through the words of `line`.
pub(crate) fn strikethrough_piece(style: &TextStyle, line: &SpanBox, top: f32) -> Option<(LineRect, Color)> {
  let strikethrough = style.strikethrough?;
  if line.width <= 0.0 {
    return None;
  }
  let thickness = usable(strikethrough.thickness, false).unwrap_or(line.strikethrough_thickness.max(1.0));
  let rect = LineRect {
    x: line.x,
    y: (top + line.baseline + line.strikethrough_offset - thickness * 0.5).round(),
    width: line.width,
    height: thickness,
  };
  Some((rect, strikethrough.color.unwrap_or(style.color)))
}

impl LayoutEngine {
  /// Writes where the lines of a `Text` that draws an underline or a line
  /// through it lie into its result, once `result` holds its box.
  pub(super) fn lay_out_text_decorations(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    result: &mut LayoutResult,
  ) {
    let NodeKind::Text { style, .. } = node.node_kind() else {
      return;
    };
    let Some(TextLayoutOutput::Text(output)) = result.text_layout.as_deref() else {
      return;
    };
    let style = style.resolve(&self.typography.borrow(), &self.palette.borrow());
    if style.underline.is_none() && style.strikethrough.is_none() {
      return;
    }
    let content = node.text_content().unwrap_or_default();
    let text = output.display_text.as_deref().unwrap_or(content);
    let wrap = output.render_wrap;
    // As the paint lays the text out: in device pixels, at its box's width.
    let scale = self.paint_scale.get();
    let mut scaled = style.clone();
    scaled.scale_pixels(scale);
    let max_width = painted_max_width(scale, wrap, style.text_align, result.size.width);
    let boxes = glyph_engine.text_line_boxes(text, &scaled, max_width, wrap);
    let top = glyph_engine.text_vertical_align_offset(
      text,
      &scaled,
      max_width,
      wrap,
      style.vertical_align,
      result.size.height * scale,
    ) / scale;
    let lines = boxes
      .iter()
      .map(|line| SpanBox {
        span: line.span,
        x: line.x / scale,
        width: line.width / scale,
        top: line.top / scale,
        height: line.height / scale,
        baseline: line.baseline / scale,
        underline_offset: line.underline_offset / scale,
        underline_thickness: line.underline_thickness / scale,
        strikethrough_offset: line.strikethrough_offset / scale,
        strikethrough_thickness: line.strikethrough_thickness / scale,
      })
      .collect();
    let output = output.with_decoration(Arc::new(TextDecorationLayout { top, lines }));
    result.text_layout = Some(Arc::new(TextLayoutOutput::Text(output)));
  }

  /// The underline of a plain text and the line through it, in the color it
  /// is painted in, for a text whose box starts at `origin`. They go over
  /// its glyphs.
  pub(super) fn text_decoration_quads(
    result: &LayoutResult,
    style: &TextStyle,
    origin: (f32, f32),
    transform: Transform2D,
    clip: ClipRect,
    opacity: f32,
  ) -> Vec<Quad> {
    let Some(TextLayoutOutput::Text(output)) = result.text_layout.as_deref() else {
      return Vec::new();
    };
    let Some(decoration) = output.decoration.as_deref() else {
      return Vec::new();
    };
    let underlines = decoration
      .lines
      .iter()
      .filter_map(|line| underline_piece(style, line, decoration.top));
    let strikes = decoration
      .lines
      .iter()
      .filter_map(|line| strikethrough_piece(style, line, decoration.top));
    underlines
      .chain(strikes)
      .map(|(rect, color)| {
        let (x, y, quad_transform, transform_origin) =
          transformed_quad_frame(origin.0 + rect.x, origin.1 + rect.y, transform);
        Quad {
          x,
          y,
          width: rect.width,
          height: rect.height,
          opacity,
          transform: quad_transform,
          transform_origin,
          content: QuadContent::Rect { color, gradient: None },
          border_radius: None,
          border: None,
          clip,
        }
      })
      .collect()
  }
}
