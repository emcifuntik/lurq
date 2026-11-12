use lurq::{
  app::Tree,
  layout::{Constraints, layout_result::LayoutResult},
};

use crate::support::run_pass;

pub(super) trait PassLayoutExt {
  fn pass_layout(&mut self, constraints: Constraints) -> Option<LayoutResult>;
}

impl PassLayoutExt for Tree {
  fn pass_layout(&mut self, constraints: Constraints) -> Option<LayoutResult> {
    self.set_layout_constraints_override(Some(constraints));
    run_pass(self);
    let result = self.last_layout().cloned();
    self.set_layout_constraints_override(None);
    result
  }
}

mod absolute_position;
mod borders;
mod box_shadow;
mod box_shadow_math;
mod column;
mod constraints;
mod edge_cases;
mod ellipsis_fit;
mod flex;
mod flex_constraints;
mod flex_shrink_relayout;
mod font_features;
mod font_weight;
mod foreground;
mod frame;
mod frame_constraints;
mod generic_family;
mod letter_spacing;
mod max_lines;
mod nested;
mod node_ids;
mod offscreen_clipping;
mod offset;
mod outline;
mod padding;
mod ported_flex;
mod quads;
mod row;
mod scroll;
mod scroll_padding;
mod scrollbar_placement;
mod shrink_give_way;
mod stack;
mod sticky;
mod state_style_paint;
mod text_centering;
mod text_decorations;
mod text_layout_cache;
mod theme_typography;
mod wrapped_row;
