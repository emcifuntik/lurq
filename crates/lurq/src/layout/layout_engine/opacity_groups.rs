//! Opacity groups. A node with `opacity < 1` fades its subtree as one layer
//! (see [`crate::layout::opacity_layer`]): its quads, its children's and its
//! overlay quads are emitted at full opacity and recorded as a group. A group
//! that paints one primitive folds its opacity into that quad instead, which
//! paints the same pixels without a layer.
//!
//! A node with a blur is a group as well, and always a layer: the blur is
//! applied to the layer as it is composited.

use super::LayoutEngine;
use crate::layout::{
  opacity_layer::OpacityGroup,
  quad::{ClipRect, Quad, QuadContent},
};

/// `clip` for the content of a node blurred by `radius`: grown by as far as
/// the blur moves colour. Content just outside the clip the node is under is
/// not shown, but it bleeds into what is, so it has to be painted into the
/// node's layer; the layer itself is cut to the clip when it is composited.
/// A rounded clip loses its corners here, and the layer is cut to its box.
pub(super) fn blur_content_clip(clip: ClipRect, radius: f32) -> ClipRect {
  if !clip.active {
    return clip;
  }
  let reach = crate::layout::opacity_layer::blur_reach(radius);
  ClipRect {
    x: clip.x - reach,
    y: clip.y - reach,
    width: clip.width + 2.0 * reach,
    height: clip.height + 2.0 * reach,
    active: true,
    border_radius: None,
  }
}

impl LayoutEngine {
  /// Closes the group a node with `opacity < 1` or a blur opened when its
  /// first quad would have been pushed at `start`. `blur` is the node's blur
  /// radius (0 for none) and `clip` the clip the node itself is under.
  #[inline(never)]
  pub(super) fn close_opacity_group(&self, opacity: f32, blur: f32, clip: ClipRect, start: usize, quads: &mut [Quad]) {
    let opacity = opacity.max(0.0);
    let end = quads.len();
    if start >= end {
      return;
    }
    if blur <= 0.0 && end - start == 1 && paints_one_primitive(&quads[start]) {
      quads[start].opacity *= opacity;
      return;
    }
    let mut groups = self.opacity_groups.borrow_mut();
    // A layer composited into an otherwise empty layer is that layer at the
    // product of both opacities. A blurred layer is kept apart: it is
    // painted for other pixels than the layer around it.
    if let Some(inner) = groups.last_mut()
      && inner.start == start
      && inner.end == end
      && inner.blur <= 0.0
      && blur <= 0.0
    {
      inner.opacity *= opacity;
      return;
    }
    groups.push(OpacityGroup {
      start,
      end,
      opacity,
      blur: blur.max(0.0),
      clip: (blur > 0.0 && clip.active).then_some([clip.x, clip.y, clip.width, clip.height]),
    });
  }

  /// Moves the groups recorded by the last quad resolution into `groups`.
  pub(crate) fn take_opacity_groups(&self, groups: &mut Vec<OpacityGroup>) {
    groups.clear();
    std::mem::swap(groups, &mut self.opacity_groups.borrow_mut());
  }
}

/// Whether a quad paints a single primitive, so fading it and fading a layer
/// holding it give the same pixels. A rect with a border paints its fill and
/// then the border over the fill's edge, and a text shadow paints under its
/// text, so those overlap.
///
/// Glyphs of one text run are treated as one primitive, as browsers do for a
/// text run under `opacity`: they overlap only where outlines touch.
fn paints_one_primitive(quad: &Quad) -> bool {
  match &quad.content {
    QuadContent::Rect { .. } => quad.border.is_none(),
    QuadContent::Text { style, .. } => style.shadow.is_none(),
    QuadContent::DecoratedInput { layout } => layout.glyphs.iter().all(|glyph| glyph.shadow.is_none()),
    QuadContent::RichText { spans, .. } => spans.first().is_none_or(|span| span.style.shadow.is_none()),
    #[cfg(feature = "raster")]
    QuadContent::Image { .. } | QuadContent::Video { .. } => true,
    // Rasterized, an SVG is one image; tessellated, its paths overlap.
    #[cfg(all(feature = "svg", feature = "raster"))]
    QuadContent::Svg { .. } => true,
    #[cfg(all(feature = "svg", not(feature = "raster")))]
    QuadContent::Svg { .. } => false,
    QuadContent::BoxShadow(_) | QuadContent::None => true,
  }
}
