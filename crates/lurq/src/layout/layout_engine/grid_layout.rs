//! Layout of a grid container: where its items go and how large its tracks
//! are is `layout/grid.rs`; this lays the items out in their cells.
//!
//! An item is laid out up to three times, each for what only that pass can
//! tell. Its natural width, when a column it is in is sized by content. Its
//! height at the width of its columns, which sizes the rows. And, where it
//! is stretched to a cell taller than itself, in that cell. The last layout
//! an item gets is always the one it is drawn with.

use std::sync::Arc;

use super::{ChildLayoutOverride, LayoutEngine};
use crate::{
  app::glyph_engine::GlyphEngine,
  layout::{
    Alignment, Constraints, Offset, Size,
    grid::{GridSpec, GridTrack, Placed, place_items, size_tracks},
    layout_kind::Position,
    layout_result::{ChildLayout, LayoutResult, SubtreeHolds},
  },
  node::node::Node,
};

/// Where each track of an axis starts, and the length of them all with their gaps.
fn track_starts(sizes: &[f32], gap: f32) -> (Vec<f32>, f32) {
  let mut starts = Vec::with_capacity(sizes.len());
  let mut at = 0.0;
  for (index, size) in sizes.iter().enumerate() {
    if index > 0 {
      at += gap;
    }
    starts.push(at);
    at += size;
  }
  (starts, at)
}

/// The length of `span` tracks from `start`, gaps between them included.
fn span_length(sizes: &[f32], gap: f32, start: usize, span: usize) -> f32 {
  let end = (start + span).min(sizes.len());
  let tracks = &sizes[start.min(end)..end];
  tracks.iter().sum::<f32>() + gap * tracks.len().saturating_sub(1) as f32
}

fn resolved_gap(gap: f32) -> f32 {
  if gap.is_finite() { gap.max(0.0) } else { 0.0 }
}

impl LayoutEngine {
  pub(super) fn layout_grid(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    spec: &GridSpec,
    child_overrides: Option<&[Option<ChildLayoutOverride>]>,
  ) -> LayoutResult {
    let children = node.children();
    let (column_gap, row_gap) = {
      let spacing = self.spacing.borrow();
      (
        resolved_gap(spec.column_gap.resolve(&spacing, constraints.max_width)),
        resolved_gap(spec.row_gap.resolve(&spacing, constraints.max_height)),
      )
    };

    // The children that take cells: an overlay's declaration takes no room, and an absolutely positioned
    // child is placed over the grid, as in a stack.
    let items: Vec<usize> = (0..children.len())
      .filter(|index| {
        let child = &children[*index];
        !child.is_overlay_declaration() && !matches!(child.position(), Position::Absolute { .. })
      })
      .collect();
    let cells: Vec<_> = items.iter().map(|index| children[*index].grid_cell()).collect();
    let placement = place_items(&cells, spec.columns.len(), spec.rows.len());
    let mut results: Vec<Option<Arc<LayoutResult>>> = vec![None; children.len()];

    // Columns. A grid in room of a known width divides it; in unbounded room its columns are their content's.
    let known_width = constraints.max_width.is_finite().then_some(constraints.max_width);
    let column_by_content = |column: usize| match spec.columns.get(column) {
      None | Some(GridTrack::Auto) => true,
      Some(GridTrack::Px(_)) => false,
      Some(GridTrack::Fr(share)) => known_width.is_none() || !(share.is_finite() && *share > 0.0),
    };
    let mut widths_of_items = Vec::new();
    for (placed, index) in placement.items.iter().zip(&items) {
      if !(placed.column..placed.column + placed.column_span).any(column_by_content) {
        continue;
      }
      let natural = Constraints {
        min_width: 0.0,
        max_width: constraints.max_width,
        min_height: 0.0,
        max_height: f32::INFINITY,
      };
      let result = self.layout_child_node(glyph_engine, child_overrides, *index, &children[*index], natural);
      widths_of_items.push((placed.column, placed.column_span, result.size.width));
    }
    let widths = size_tracks(
      &spec.columns,
      placement.columns,
      column_gap,
      known_width,
      &widths_of_items,
    );
    let (column_starts, columns_length) = track_starts(&widths, column_gap);

    // Rows: each item at the width of its columns says how tall it is.
    let cell_width = |placed: &Placed| span_length(&widths, column_gap, placed.column, placed.column_span);
    let stretched_across = spec.justify == Alignment::Stretch;
    let mut heights_of_items = Vec::with_capacity(items.len());
    for (placed, index) in placement.items.iter().zip(&items) {
      let width = cell_width(placed);
      let at_its_width = Constraints {
        min_width: if stretched_across { width } else { 0.0 },
        max_width: width,
        min_height: 0.0,
        max_height: f32::INFINITY,
      };
      let result = self.layout_child_node(glyph_engine, child_overrides, *index, &children[*index], at_its_width);
      heights_of_items.push((placed.row, placed.row_span, result.size.height));
      results[*index] = Some(result);
    }
    // A grid given its height divides it; one as tall as its content has rows as tall as theirs.
    let known_height = (constraints.max_height.is_finite() && constraints.min_height >= constraints.max_height)
      .then_some(constraints.max_height);
    let heights = size_tracks(&spec.rows, placement.rows, row_gap, known_height, &heights_of_items);
    let (row_starts, rows_length) = track_starts(&heights, row_gap);

    // An item stretched to a cell taller than itself is laid out in the cell.
    let cell_height = |placed: &Placed| span_length(&heights, row_gap, placed.row, placed.row_span);
    if spec.align == Alignment::Stretch {
      for (placed, index) in placement.items.iter().zip(&items) {
        let (width, height) = (cell_width(placed), cell_height(placed));
        let laid_out = results[*index].as_ref().map_or(0.0, |result| result.size.height);
        if (laid_out - height).abs() < 0.01 {
          continue;
        }
        let in_its_cell = Constraints {
          min_width: if stretched_across { width } else { 0.0 },
          max_width: width,
          min_height: height,
          max_height: height,
        };
        results[*index] =
          Some(self.layout_child_node(glyph_engine, child_overrides, *index, &children[*index], in_its_cell));
      }
    }

    // The grid fills the width it divides among shares; otherwise it is as wide as its columns.
    let shares_width = known_width.is_some()
      && (0..placement.columns)
        .any(|column| matches!(spec.columns.get(column), Some(GridTrack::Fr(_))) && !column_by_content(column));
    let width = if shares_width {
      constraints.max_width
    } else {
      columns_length
    };
    let size = constraints.constrain(Size::new(width, known_height.unwrap_or(rows_length)));

    // Children that take no cell: overlays' declarations have no size, and absolutely positioned ones are
    // sized against the grid's room, as a stack sizes them.
    for (index, child) in children.iter().enumerate() {
      if results[index].is_some() {
        continue;
      }
      results[index] = Some(match child.position() {
        Position::Absolute { width, height, .. } if !child.is_overlay_declaration() => {
          let resolved_width = width.and_then(|size| Self::resolve_dimension(size, constraints.max_width));
          let resolved_height = height.and_then(|size| Self::resolve_dimension(size, constraints.max_height));
          let positioned = Constraints {
            min_width: resolved_width.unwrap_or(0.0),
            max_width: resolved_width.unwrap_or(constraints.max_width),
            min_height: resolved_height.unwrap_or(0.0),
            max_height: resolved_height.unwrap_or(constraints.max_height),
          };
          let mut result = self.layout_child_node(glyph_engine, child_overrides, index, child, positioned);
          if let Some(width) = resolved_width {
            Arc::make_mut(&mut result).size.width = width;
          }
          if let Some(height) = resolved_height {
            Arc::make_mut(&mut result).size.height = height;
          }
          result
        }
        _ => Arc::new(LayoutResult {
          size: Size::new(0.0, 0.0),
          children: Vec::new(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
          generation: 0,
          sticky_shifts: None,
        }),
      });
    }

    let mut cell_of_child = vec![None; children.len()];
    for (placed, index) in placement.items.iter().zip(&items) {
      cell_of_child[*index] = Some(*placed);
    }
    let child_layouts = children
      .iter()
      .zip(results)
      .zip(cell_of_child)
      .map(|((child, result), placed)| {
        let result = result.expect("every child is laid out above");
        let offset = match (placed, child.position()) {
          (Some(placed), _) => Offset::new(
            column_starts[placed.column] + spec.justify.cross_offset(cell_width(&placed), result.size.width),
            row_starts[placed.row] + spec.align.cross_offset(cell_height(&placed), result.size.height),
          ),
          (None, Position::Absolute { x, y, .. }) => Offset::new(x, y),
          (None, _) => Offset::default(),
        };
        ChildLayout {
          offset: if child.is_overlay_declaration() {
            Offset::default()
          } else {
            Self::apply_relative_position(child, offset)
          },
          result,
        }
      })
      .collect();

    LayoutResult {
      size,
      children: child_layouts,
      text_layout: None,
      dropped: false,
      holds: SubtreeHolds::UNKNOWN,
      generation: 0,
      sticky_shifts: None,
    }
  }
}
