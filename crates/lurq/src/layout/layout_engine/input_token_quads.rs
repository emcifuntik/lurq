//! Input-only paint dispatch and geometry offsets.
use super::{
  LayoutEngine, ResolvedPadding, inset_clip_for_border, intersect_clip, text_input_vertical_offset,
  transformed_quad_frame,
};
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

  /// The box a text input shows its text, selection and caret in, before the
  /// node's transform. A single-line input shows them inside its padding. A
  /// multiline input scrolls its padding with its text, so it shows them from
  /// its top border to its bottom border, as a scroll container shows its
  /// content; across, the text stays inside the padding, which holds a
  /// reserved scrollbar's lane.
  pub(super) fn text_input_viewport(
    &self,
    node: &Node,
    state: &TextInputState,
    result: &LayoutResult,
    origin: (f32, f32),
    padding: ResolvedPadding,
  ) -> ClipRect {
    let content = ClipRect {
      x: origin.0 + padding.left,
      y: origin.1 + padding.top,
      width: (result.size.width - padding.left - padding.right).max(0.0),
      height: (result.size.height - padding.top - padding.bottom).max(0.0),
      active: true,
      border_radius: None,
    };
    if state.overflow() != TextInputOverflow::Multiline {
      return content;
    }
    let border = node.get_resolved_border(&self.palette.borrow(), &self.border_sizes.borrow());
    let inside_border = inset_clip_for_border(
      ClipRect {
        x: origin.0,
        y: origin.1,
        width: result.size.width,
        height: result.size.height,
        active: true,
        border_radius: None,
      },
      border,
    );
    ClipRect {
      y: inside_border.y,
      height: inside_border.height,
      ..content
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
    let clip = intersect_clip(
      clip,
      self
        .text_input_viewport(node, state, result, origin, padding)
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
