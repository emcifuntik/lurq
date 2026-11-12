use crate::{
  layout::{
    layout_kind::{LayoutKind, Overflow},
    layout_result::LayoutResult,
  },
  node::{HitTestBehavior, node::Node, transform::Transform2D},
};

pub struct HitRect {
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  pub local_x: f32,
  pub local_y: f32,
  pub transform: Transform2D,
}

impl HitRect {
  pub fn contains(&self, px: f32, py: f32) -> bool {
    px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
  }
}

pub(crate) fn hit_test_tree<'a>(
  node: &'a Node,
  result: &LayoutResult,
  abs_x: f32,
  abs_y: f32,
  px: f32,
  py: f32,
  hits: &mut Vec<(&'a Node, HitRect)>,
) {
  hit_test_tree_with_transform(
    node,
    result,
    abs_x,
    abs_y,
    px,
    py,
    Transform2D::IDENTITY,
    true,
    true,
    hits,
  );
}

/// Like [`hit_test_tree`], with what is inside a disabled element listed as
/// if it were enabled: for tools that inspect the tree, and to find the
/// scroll containers in a disabled element, which still scroll.
pub(crate) fn hit_test_tree_inspecting<'a>(
  node: &'a Node,
  result: &LayoutResult,
  abs_x: f32,
  abs_y: f32,
  px: f32,
  py: f32,
  hits: &mut Vec<(&'a Node, HitRect)>,
) {
  hit_test_tree_with_transform(
    node,
    result,
    abs_x,
    abs_y,
    px,
    py,
    Transform2D::IDENTITY,
    true,
    false,
    hits,
  );
}

pub(crate) fn hit_test_tree_all<'a>(
  node: &'a Node,
  result: &LayoutResult,
  abs_x: f32,
  abs_y: f32,
  px: f32,
  py: f32,
  hits: &mut Vec<(&'a Node, HitRect)>,
) {
  hit_test_tree_with_transform(
    node,
    result,
    abs_x,
    abs_y,
    px,
    py,
    Transform2D::IDENTITY,
    false,
    true,
    hits,
  );
}

fn hit_test_tree_with_transform<'a>(
  node: &'a Node,
  result: &LayoutResult,
  abs_x: f32,
  abs_y: f32,
  px: f32,
  py: f32,
  inherited_transform: Transform2D,
  occlude_stack_siblings: bool,
  fold_disabled: bool,
  hits: &mut Vec<(&'a Node, HitRect)>,
) {
  let behavior = node.hit_test_behavior();
  if behavior == HitTestBehavior::None || result.dropped {
    return;
  }
  let own_hits = hits.len();

  let rect = HitRect {
    x: abs_x,
    y: abs_y,
    width: result.size.width,
    height: result.size.height,
    local_x: px,
    local_y: py,
    transform: Transform2D::IDENTITY,
  };

  let local_transform = node.effective_transform();
  let local_transform_origin = [rect.x + rect.width * 0.5, rect.y + rect.height * 0.5];
  let local_affine = if local_transform.is_identity() {
    Transform2D::IDENTITY
  } else {
    local_transform.around_origin(local_transform_origin)
  };
  let transform = if inherited_transform.is_identity() {
    local_affine
  } else if local_affine.is_identity() {
    inherited_transform
  } else if local_transform.is_identity() {
    inherited_transform
  } else {
    inherited_transform.then(&local_affine)
  };
  let (local_x, local_y) = inverse_transform_point(px, py, transform);
  let rect = HitRect {
    local_x,
    local_y,
    transform,
    ..rect
  };
  let inside = contains_point(result, &rect, local_x, local_y);
  let can_have_visible_children = inside || node.overflow == Overflow::Visible;

  if can_have_visible_children {
    // What sticks is painted over its siblings, the last of them on top, so it is hit before them.
    let children = || result.children.iter().zip(node.children().iter());
    let sticky = children().any(|(_, child_node)| child_node.is_sticky());
    let occludes = occlude_stack_siblings && matches!(node.layout_kind(), LayoutKind::Stack { .. });
    let mut hit = |child_layout: &crate::layout::layout_result::ChildLayout, child_node: &'a Node| {
      let hit_count = hits.len();
      hit_test_tree_with_transform(
        child_node,
        &child_layout.result,
        abs_x + child_layout.offset.x,
        abs_y + child_layout.offset.y,
        px,
        py,
        transform,
        occlude_stack_siblings,
        fold_disabled,
        hits,
      );
      hits.len() > hit_count
    };
    // A sticky child that is hit covers the siblings under it, as a child of a stack covers the ones before it.
    let mut covered = false;
    if sticky {
      for (child_layout, child_node) in children().rev().filter(|(_, child_node)| child_node.is_sticky()) {
        if hit(child_layout, child_node) && occlude_stack_siblings {
          covered = true;
          break;
        }
      }
    }
    if !covered && occludes {
      for (child_layout, child_node) in children().rev().filter(|(_, child_node)| !child_node.is_sticky()) {
        if hit(child_layout, child_node) {
          break;
        }
      }
    } else if !covered {
      for (child_layout, child_node) in children().filter(|(_, child_node)| !child_node.is_sticky()) {
        hit(child_layout, child_node);
      }
    }
  }

  // A disabled element stands for everything in it: where the point is on it or on any of its content, it is the
  // one hit, and the innermost. Input reads that as a target that takes nothing (`runtime::take_disabled_hit`).
  if fold_disabled && node.is_disabled() && hits.len() > own_hits {
    hits.truncate(own_hits);
    hits.push((node, rect));
  } else if inside && behavior == HitTestBehavior::Auto {
    hits.push((node, rect));
  }
}

/// Whether the node laid out as `result` at `rect` is under `(x, y)`. A link
/// inside a paragraph is under the point only on its words and what follows
/// them, line by line, not on its box.
#[cfg_attr(not(feature = "markdown"), allow(unused_variables))]
fn contains_point(result: &LayoutResult, rect: &HitRect, x: f32, y: f32) -> bool {
  #[cfg(feature = "markdown")]
  if let Some(crate::layout::layout_engine::TextLayoutOutput::InlineLink(link)) = result.text_layout.as_deref() {
    return link.hits(x - rect.x, y - rect.y);
  }
  rect.contains(x, y)
}

fn inverse_transform_point(px: f32, py: f32, transform: Transform2D) -> (f32, f32) {
  if transform.is_identity() {
    return (px, py);
  }

  let Some(inverse) = transform.inverse_affine() else {
    return (px, py);
  };
  inverse.transform_point(px, py)
}
