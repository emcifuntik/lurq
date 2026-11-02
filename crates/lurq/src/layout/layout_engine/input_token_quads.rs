//! Input-only paint dispatch and geometry offsets.
use super::{LayoutEngine, intersect_clip, text_input_vertical_offset, transformed_quad_frame};
use crate::{
  layout::{
    layout_result::LayoutResult,
    quad::{ClipRect, Quad, QuadContent},
    text_style::{TextStyle, VerticalAlign},
  },
  node::{
    TextTransformMode,
    border::BorderRadius,
    node::Node,
    node_kind::{TextInputOverflow, TextInputState},
    transform::Transform2D,
  },
};

impl LayoutEngine {
  pub(super) fn input_content(
    &self,
    state: &TextInputState,
    style: &TextStyle,
    placeholder: Option<&TextStyle>,
  ) -> QuadContent {
    if let Some(layout) = state.painted_layout() {
      return QuadContent::DecoratedInput { layout };
    }
    let display = super::text_input_display_style(state, style, placeholder).clone();
    let vertical_align = if state.overflow() == TextInputOverflow::Multiline {
      VerticalAlign::LineBox
    } else {
      display.vertical_align
    };
    QuadContent::Text {
      text: state.rendered_text_for_layout(),
      style: display,
      wrap: state.overflow() == TextInputOverflow::Multiline,
      vertical_align,
      transform_mode: TextTransformMode::Bitmap,
    }
  }

  pub(super) fn push_input_tokens(
    &self,
    node: &Node,
    state: &TextInputState,
    result: &LayoutResult,
    origin: (f32, f32),
    transform: Transform2D,
    clip: ClipRect,
    opacity: f32,
    quads: &mut Vec<Quad>,
  ) {
    let Some(layout) = state.painted_layout() else {
      return;
    };
    let padding = self.resolved_padding_for_size(node, result.size);
    let height = (result.size.height - padding.top - padding.bottom).max(0.0);
    let width = (result.size.width - padding.left - padding.right).max(0.0);
    let clip = intersect_clip(
      clip,
      ClipRect {
        x: origin.0 + padding.left,
        y: origin.1 + padding.top,
        width,
        height,
        active: true,
        border_radius: None,
      }
      .transformed(transform),
    );
    let start = (
      origin.0 + padding.left - state.scroll_x(),
      origin.1 + padding.top + text_input_vertical_offset(state, height) - state.scroll_y(),
    );
    for part in &layout.fragments {
      let (x, y, xf, center) = transformed_quad_frame(start.0 + part.x, start.1 + part.y, transform);
      quads.push(Quad {
        x,
        y,
        width: part.width,
        height: part.height,
        opacity,
        transform: xf,
        transform_origin: center,
        content: QuadContent::Rect {
          color: part.color,
          gradient: None,
        },
        border_radius: Some(BorderRadius::all(part.radius).clamped_to_rect(part.width, part.height)),
        border: None,
        clip,
      });
    }
  }
}
