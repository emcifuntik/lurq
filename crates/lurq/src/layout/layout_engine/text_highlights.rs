//! The fills behind highlighted ranges of a `Text` or `RichText` leaf
//! (`Text::highlights`): one per laid-out line a range covers, from the
//! carets of the painted layout, as a selection is drawn. They go under the
//! glyphs and under a selection.

use super::{LayoutEngine, transformed_quad_frame};
use crate::{
  layout::{
    layout_result::LayoutResult,
    quad::{ClipRect, Quad, QuadContent},
  },
  node::{border::BorderRadius, node::Node, node_kind::NodeKind, transform::Transform2D},
};

impl LayoutEngine {
  #[allow(clippy::too_many_arguments)]
  pub(super) fn push_text_highlights(
    &self,
    node: &Node,
    result: &LayoutResult,
    origin: (f32, f32),
    transform: Transform2D,
    clip: ClipRect,
    opacity: f32,
    quads: &mut Vec<Quad>,
  ) {
    let (state, value, line_height) = match node.node_kind() {
      NodeKind::Text { state, style, .. } => {
        let style = style.resolve(&self.typography.borrow(), &self.palette.borrow());
        (
          state,
          node.text_content().unwrap_or_default().to_owned(),
          style.font_size * style.line_height,
        )
      }
      #[cfg(feature = "markdown")]
      NodeKind::RichText { state, spans, .. } => (
        state,
        node.rich_text_value().unwrap_or_default(),
        spans
          .first()
          .map_or(16.0, |span| span.style.font_size * span.style.line_height),
      ),
      _ => return,
    };
    let highlights = state.highlight_ranges(&value);
    if highlights.is_empty() {
      return;
    }
    let height = line_height.min(result.size.height).max(1.0);
    #[cfg(feature = "markdown")]
    let generated = Self::generated_span_rects(node, result);
    let palette = self.palette.borrow();
    for (highlight, lines) in highlights {
      let Some(color) = highlight.color.resolve(&palette) else {
        continue;
      };
      let radius = highlight.drawn_radius();
      // A rich text's presentation spans (a link's isolate marks) are not
      // its text: a highlight leaves them out, as a selection does.
      #[cfg(feature = "markdown")]
      let pieces = lines.into_iter().flat_map(|line| {
        super::inline_links::without_rects(line.x, line.y, line.width, height, &generated)
          .into_iter()
          .map(move |(x, width)| (x, line.y, width))
      });
      #[cfg(not(feature = "markdown"))]
      let pieces = lines.into_iter().map(|line| (line.x, line.y, line.width));
      for (x, y, width) in pieces {
        let (x, y, transform, transform_origin) = transformed_quad_frame(origin.0 + x, origin.1 + y, transform);
        quads.push(Quad {
          x,
          y,
          width,
          height,
          opacity,
          transform,
          transform_origin,
          content: QuadContent::Rect { color, gradient: None },
          border_radius: (radius > 0.0).then(|| BorderRadius::all(radius).clamped_to_rect(width, height)),
          border: None,
          clip,
        });
      }
    }
  }
}
