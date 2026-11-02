//! Flex shrink for single-line Rows and Columns: distributing the overflow
//! over shrinkable children and laying each shrunk child out again at its
//! final main size, so its inner layout (scroll viewport, nested flex
//! distribution, alignment) matches the box it is given.

use std::sync::Arc;

use super::{
  ChildLayoutOverride, LayoutEngine,
  shrink_distribution::{LineSpace, ShrinkOutcome, distribute},
};
use crate::{
  app::glyph_engine::GlyphEngine,
  layout::{
    Constraints, Size,
    layout_kind::{FlexWrap, LayoutKind, ShrinkRule},
    layout_result::{ChildLayout, LayoutResult},
  },
  node::node::Node,
};

/// Inputs of one flex line's shrink step.
pub(super) struct FlexShrinkLine<'a> {
  pub(super) children: &'a [Node],
  pub(super) constraints: Constraints,
  pub(super) max_main: f32,
  /// What the children take beyond `max_main` ([`LineOverflow`]).
  pub(super) overflow: f32,
  pub(super) spacing: f32,
  pub(super) shrink_total: f32,
  pub(super) vertical: bool,
  /// Its parent collapsed this line ([`LayoutEngine::is_collapsed`]).
  pub(super) collapsed: bool,
}

/// The room a Row/Column grants its growing children.
#[derive(Clone, Copy)]
pub(super) struct FreeRoom {
  /// The main size left after the spacing and the children sized on their
  /// own; negative when those overflow the line, infinite on an unbounded
  /// main axis.
  pub(super) remaining: f32,
  pub(super) grow_total: f32,
}

/// What a line's children take beyond its main size, before any of them
/// shrinks, summed up as the line lays out each child it grants a main size
/// to; a child sized on its own is already out of the free room.
///
/// The growing children are granted the free room in shares, so together
/// they take exactly that room: it is not summed back up from their sizes.
/// In `f32` that sum can come out a few micro-pixels over the line, and the
/// rounding would pass for an overflow: the line would shrink a text by it
/// and ellipsize the text, with the room still free beside it. What the room
/// did not account for still counts: a flex basis, and whatever a child takes
/// beyond its grant.
pub(super) struct LineOverflow {
  room: FreeRoom,
  vertical: bool,
  granted: f32,
}

impl LineOverflow {
  pub(super) fn new(room: FreeRoom, vertical: bool) -> Self {
    Self {
      room,
      vertical,
      granted: 0.0,
    }
  }

  fn shares_room(&self) -> bool {
    self.room.remaining > 0.0 && self.room.grow_total > 0.0
  }

  /// Counts a flex child laid out as `result` at the main size `grant`.
  pub(super) fn add_granted(&mut self, result: &LayoutResult, grant: f32, basis: Option<f32>) {
    let size = main_size(result, self.vertical);
    self.granted += if self.shares_room() {
      size - grant + basis.unwrap_or(0.0)
    } else {
      size
    };
  }

  pub(super) fn total(&self) -> f32 {
    if !self.room.remaining.is_finite() {
      0.0
    } else if self.shares_room() {
      self.granted
    } else {
      self.granted - self.room.remaining
    }
  }
}

/// Whether a child takes up space in its flex line: overlay declarations and
/// children the shrink step dropped do not, and get no spacing either.
pub(super) fn occupies_line(child: &Node, result: &LayoutResult) -> bool {
  !child.is_overlay_declaration() && !result.dropped
}

impl LayoutEngine {
  /// Whether a dirty child of this Row/Column was last laid out at a size the
  /// shrink step chose. Its cached tight main-axis constraint is an output of
  /// the parent's previous distribution, so repairing the child under it would
  /// pin the old size; the parent must measure it naturally and shrink again.
  pub(super) fn has_dirty_shrunk_child(node: &Node) -> bool {
    let vertical = match node.layout_kind() {
      LayoutKind::Column { wrap, .. } if *wrap != FlexWrap::Wrap => true,
      LayoutKind::Row { wrap, .. } if *wrap != FlexWrap::Wrap => false,
      _ => return false,
    };
    node.children().iter().any(|child| {
      !child.is_overlay_declaration()
        && child.layout_cache.is_dirty()
        && child.state_flex().is_some_and(|params| params.shrink > 0.0)
        && child.layout_cache.constraints().is_some_and(|constraints| {
          if vertical {
            constraints.min_height == constraints.max_height
          } else {
            constraints.min_width == constraints.max_width
          }
        })
    })
  }

  /// Shrinks overflowing children and re-lays out every child whose main size
  /// changed, with that size as a tight main-axis constraint. A dropped child
  /// is laid out at zero size and its result marked dropped; a collapsed child
  /// is laid out with its own line collapsed (see [`Self::is_collapsed`]).
  /// Returns which children collapsed, so a later layout of them (stretching)
  /// collapses them again; empty when none did, as on almost every line.
  pub(super) fn shrink_flex_line(
    &self,
    glyph_engine: &mut GlyphEngine,
    line: &FlexShrinkLine<'_>,
    results: &mut [ChildLayout],
    child_overrides: Option<&[Option<ChildLayoutOverride>]>,
  ) -> Vec<bool> {
    let mut collapsed = Vec::new();
    for (index, outcome) in self.shrink_outcomes(line, results) {
      let child = &line.children[index];
      match outcome {
        ShrinkOutcome::Keep => {}
        ShrinkOutcome::Resize(new_main) => {
          if new_main == main_size(&results[index].result, line.vertical) {
            continue;
          }
          let child_constraints = shrunk_constraints(line, new_main);
          results[index].result =
            self.layout_child_node(glyph_engine, child_overrides, index, child, child_constraints);
        }
        ShrinkOutcome::Collapsed(new_main) => {
          collapsed.resize(results.len(), false);
          collapsed[index] = true;
          results[index].result = self.layout_collapsed(glyph_engine, child, shrunk_constraints(line, new_main));
        }
        ShrinkOutcome::Drop => {
          let zero = Constraints::tight(Size::default());
          let mut dropped = self.layout_child_node(glyph_engine, child_overrides, index, child, zero);
          Arc::make_mut(&mut dropped).size = Size::default();
          Arc::make_mut(&mut dropped).dropped = true;
          results[index].result = dropped;
        }
      }
    }
    collapsed
  }

  /// Lays `child` out with its line (through logical wrappers) collapsed:
  /// every child of it that can drop is dropped and every child that can
  /// collapse is collapsed, whatever its size, as its parent decided. The
  /// layout cache is not read for it, since it holds layouts without that
  /// decision.
  pub(super) fn layout_collapsed(
    &self,
    glyph_engine: &mut GlyphEngine,
    child: &Node,
    constraints: Constraints,
  ) -> Arc<LayoutResult> {
    let depth = self.collapsed_lines.borrow().len();
    let mut node = child;
    loop {
      self.collapsed_lines.borrow_mut().push(node_key(node));
      match (node.layout_kind(), node.children().first()) {
        (LayoutKind::LogicalModifier, Some(inner)) => node = inner,
        _ => break,
      }
    }
    let result = self.layout_node(glyph_engine, child, constraints);
    self.collapsed_lines.borrow_mut().truncate(depth);
    result
  }

  /// Whether `node`'s parent collapsed it for the layout in progress.
  pub(super) fn is_collapsed(&self, node: &Node) -> bool {
    self.collapsed_lines.borrow().contains(&node_key(node))
  }

  /// What the overflow does to each shrinking child, by child index.
  fn shrink_outcomes(&self, line: &FlexShrinkLine<'_>, results: &[ChildLayout]) -> Vec<(usize, ShrinkOutcome)> {
    if line.shrink_total <= 0.0 || !line.max_main.is_finite() {
      return Vec::new();
    }
    let occupied = line
      .children
      .iter()
      .filter(|child| !child.is_overlay_declaration())
      .count();
    let overflow = line.overflow;
    // A collapsed line drops and collapses its children even when it fits.
    if overflow <= 0.0 && !line.collapsed {
      return Vec::new();
    }

    let mut indices = Vec::new();
    let mut items = Vec::new();
    // Whole-pixel sharing is part of the give-way rules: a line without
    // orders or limits shares exactly as before them.
    let mut whole_pixels = false;
    for (index, child) in line.children.iter().enumerate() {
      if child.is_overlay_declaration() {
        continue;
      }
      let factor = child.state_flex().map_or(0.0, |params| params.shrink);
      if factor <= 0.0 {
        continue;
      }
      whole_pixels |= child.shrink_rule() != ShrinkRule::default();
      indices.push(index);
      items.push(self.shrink_item(child, &results[index].result, factor, line.vertical));
    }
    let space = LineSpace {
      overflow,
      gap: line.spacing,
      occupied,
      whole_pixels,
    };
    indices
      .into_iter()
      .zip(distribute(&items, space, line.collapsed))
      .collect()
  }
}

/// A node's identity for [`LayoutEngine::is_collapsed`]: its address, which
/// does not move while the tree is laid out (node ids may be unassigned).
fn node_key(node: &Node) -> usize {
  std::ptr::from_ref(node) as usize
}

pub(super) fn main_size(result: &LayoutResult, vertical: bool) -> f32 {
  if vertical {
    result.size.height
  } else {
    result.size.width
  }
}

fn shrunk_constraints(line: &FlexShrinkLine<'_>, new_main: f32) -> Constraints {
  if line.vertical {
    Constraints {
      min_width: 0.0,
      max_width: line.constraints.max_width,
      min_height: new_main,
      max_height: new_main,
    }
  } else {
    Constraints {
      min_width: new_main,
      max_width: new_main,
      min_height: 0.0,
      max_height: line.constraints.max_height,
    }
  }
}
