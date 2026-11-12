//! Paint of a node's outline: a ring around its box, drawn over its content.
//!
//! Containers clip their content to their bounds by default, and a control
//! often fills its container to the edge: clipped as content is, its ring
//! would be cut there, on every side it touches. So an outline takes the clip
//! its node is under let out by how far the outline reaches. A node that is
//! itself cut by a container is still cut with its ring, a little further
//! out. A scroll viewport, and the window, cut the ring where they cut the
//! node: what scrolls out of view takes its ring with it.

use super::{DEFAULT_TRANSPARENT_COLOR, LayoutEngine, intersect_clip, transformed_quad_frame};
use crate::{
  layout::{
    layout_result::LayoutResult,
    quad::{ClipRect, Quad, QuadContent},
  },
  node::{
    border::{BorderPlacement, BorderRadius, ResolvedBorder, ResolvedBorders},
    node::Node,
    outline::ResolvedOutline,
    transform::Transform2D,
  },
};

impl LayoutEngine {
  fn resolved_outline(&self, node: &Node) -> Option<ResolvedOutline> {
    node
      .effective_outline()?
      .resolve(&self.palette.borrow(), &self.border_sizes.borrow())
  }

  /// How far `node` paints past its layout box: its outer shadows and its
  /// outline. Culling keeps a node whose box is clipped away while these
  /// still show.
  #[inline(never)]
  pub(super) fn paint_outset(&self, node: &Node) -> f32 {
    let shadow = self.box_shadow_outset(node);
    match self.resolved_outline(node) {
      Some(outline) => shadow.max(outline.outset()),
      None => shadow,
    }
  }

  /// The clip of an outline that reaches `outset` past its node, which is
  /// under `clip`.
  fn outline_clip(&self, clip: ClipRect, outset: f32) -> ClipRect {
    if !clip.active || outset <= 0.0 {
      return clip;
    }
    let corner = |radius: f32| if radius > 0.0 { radius + outset } else { 0.0 };
    let let_out = ClipRect {
      x: clip.x - outset,
      y: clip.y - outset,
      width: clip.width + outset * 2.0,
      height: clip.height + outset * 2.0,
      active: true,
      border_radius: clip.border_radius.map(|radius| BorderRadius {
        top_left: corner(radius.top_left),
        top_right: corner(radius.top_right),
        bottom_right: corner(radius.bottom_right),
        bottom_left: corner(radius.bottom_left),
      }),
    };
    intersect_clip(self.scroll_clip.get(), let_out)
  }

  /// Pushes the node's outline: a quad grown past the node's box by the
  /// outline's offset and width, with the line as its inside border. The
  /// corners follow the node's, grown by the same amount; a square corner
  /// stays square.
  #[inline(never)]
  #[allow(clippy::too_many_arguments)]
  pub(super) fn push_outline_quad(
    &self,
    node: &Node,
    result: &LayoutResult,
    abs_x: f32,
    abs_y: f32,
    opacity: f32,
    transform: Transform2D,
    clip: ClipRect,
    quads: &mut Vec<Quad>,
  ) {
    let Some(outline) = self.resolved_outline(node) else {
      return;
    };
    let grow = outline.outset();
    let width = result.size.width + grow * 2.0;
    let height = result.size.height + grow * 2.0;
    if width <= 0.0 || height <= 0.0 {
      return;
    }
    let corner = |radius: f32| if radius > 0.0 { (radius + grow).max(0.0) } else { 0.0 };
    let border_radius = node.get_border_radius(&self.radii.borrow()).map(|radius| BorderRadius {
      top_left: corner(radius.top_left),
      top_right: corner(radius.top_right),
      bottom_right: corner(radius.bottom_right),
      bottom_left: corner(radius.bottom_left),
    });
    let side = Some(ResolvedBorder {
      width: outline.width,
      color: outline.color,
      placement: BorderPlacement::Inside,
    });
    let (x, y, quad_transform, transform_origin) = transformed_quad_frame(abs_x - grow, abs_y - grow, transform);
    let clip = self.outline_clip(clip, grow);
    quads.push(Quad {
      x,
      y,
      width,
      height,
      opacity,
      transform: quad_transform,
      transform_origin,
      content: QuadContent::Rect {
        color: DEFAULT_TRANSPARENT_COLOR,
        gradient: None,
      },
      border_radius,
      border: Some(ResolvedBorders {
        top: side,
        right: side,
        bottom: side,
        left: side,
      }),
      clip,
    });
  }
}
