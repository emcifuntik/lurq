//! Input method support of the focused `TextInput` in paint: the underline
//! under a composition in progress, and the caret area an input method places
//! its candidate window at.

use super::{
  DEFAULT_QUAD_OPACITY, LayoutEngine, TEXT_INPUT_CARET_WIDTH, intersect_clip, text_input_vertical_offset,
  transformed_quad_frame,
};
use crate::{
  app::events::ImeCursorArea,
  layout::{
    layout_result::LayoutResult,
    quad::{ClipRect, Quad, QuadContent},
    text_style::TextStyle,
  },
  node::{node::Node, node_kind::TextInputState, transform::Transform2D},
};

/// Where the focused text input was painted, in logical pixels from the window's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextInputFrame {
  /// The input's box: x, y, width, height.
  pub(crate) bounds: (f32, f32, f32, f32),
  /// Where the input's text coordinates start: a caret at `(x, y)` in them is painted at this plus that.
  pub(crate) text_origin: (f32, f32),
  pub(crate) line_height: f32,
}

/// Thickness of the line under a composition, in logical pixels.
const COMPOSITION_UNDERLINE_WIDTH: f32 = 1.0;

impl LayoutEngine {
  /// Records the focused input's caret as the IME cursor area and underlines
  /// its composition, if any, in the text colour at the bottom of the glyph
  /// band.
  #[allow(clippy::too_many_arguments)]
  pub(super) fn push_focused_text_input_ime_quads(
    &self,
    node: &Node,
    state: &TextInputState,
    style: &TextStyle,
    result: &LayoutResult,
    (abs_x, abs_y): (f32, f32),
    transform: Transform2D,
    clip: ClipRect,
    quads: &mut Vec<Quad>,
  ) {
    let padding = self.resolved_padding_for_size(node, result.size);
    let content_height = (result.size.height - padding.top - padding.bottom).max(0.0);
    let vertical_offset = padding.top + text_input_vertical_offset(state, content_height);
    let (band_top, band_height) = state.text_band();
    // Where the input is painted, which an ancestor's transform moves from where it is laid out: a sheet's
    // content is laid out at the window's top and moved down to where the sheet stands.
    let painted = |x: f32, y: f32| {
      if transform.is_identity() {
        (x, y)
      } else {
        transform.transform_point(x, y)
      }
    };
    let caret = painted(
      abs_x + padding.left + state.caret_x(),
      abs_y + vertical_offset + state.caret_y() + band_top,
    );
    self.ime_cursor_area.set(Some(ImeCursorArea {
      x: caret.0,
      y: caret.1,
      width: TEXT_INPUT_CARET_WIDTH,
      height: band_height.max(1.0),
    }));
    let corner = painted(abs_x, abs_y);
    self.text_input_frame.set(Some(TextInputFrame {
      bounds: (corner.0, corner.1, result.size.width, result.size.height),
      text_origin: painted(abs_x + padding.left, abs_y + vertical_offset + band_top),
      line_height: band_height.max(1.0),
    }));

    let ranges = state.composition_ranges();
    if ranges.is_empty() {
      return;
    }
    let underline_clip = intersect_clip(
      clip,
      self
        .text_input_viewport(node, state, result, (abs_x, abs_y), padding)
        .transformed(transform),
    );
    let underline_top = band_top + band_height.min(content_height) - COMPOSITION_UNDERLINE_WIDTH;
    for range in ranges {
      let x = abs_x + padding.left + range.x;
      let y = abs_y + vertical_offset + range.y + underline_top;
      let (x, y, quad_transform, transform_origin) = transformed_quad_frame(x, y, transform);
      quads.push(Quad {
        x,
        y,
        width: range.width,
        height: COMPOSITION_UNDERLINE_WIDTH,
        opacity: DEFAULT_QUAD_OPACITY,
        transform: quad_transform,
        transform_origin,
        content: QuadContent::Rect {
          color: style.color,
          gradient: None,
        },
        border_radius: None,
        border: None,
        clip: underline_clip,
      });
    }
  }

  /// Where the focused text input was painted by the last quad resolution.
  pub(crate) fn text_input_frame(&self) -> Option<TextInputFrame> {
    self.text_input_frame.get()
  }

  /// The focused text input's caret recorded by the last quad resolution.
  pub(crate) fn ime_cursor_area(&self) -> Option<ImeCursorArea> {
    self.ime_cursor_area.get()
  }
}
