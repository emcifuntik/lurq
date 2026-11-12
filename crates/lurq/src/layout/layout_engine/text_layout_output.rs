//! What laying out a text leaf writes into its node's runtime state.
//!
//! Text layout has side effects that the [`LayoutResult`] size does not
//! capture: the ellipsized string and the wrap decision the renderer draws,
//! and the caret geometry that selection, caret placement and pointer
//! hit-testing read. They live in the node's text state, which keeps whatever
//! the last real layout wrote. A layout served from a node's cache skips text
//! layout, so each text leaf's result carries the output that produced it, and
//! serving a cached result writes that output back. Without this, a layout
//! cached at one width and served again after a layout at another width keeps
//! the other width's ellipsis, wrapping and caret geometry.

use std::sync::Arc;

use super::{LayoutEngine, ResolvedPadding};
use crate::{
  layout::{
    Size,
    layout_result::{LayoutResult, SubtreeHolds},
  },
  node::{
    node::Node,
    node_kind::{NodeKind, PaintedCarets, TextInputOverflow, TextInputOverflowAnchor, TextInputState, TextState},
    text_selection::CaretPositions,
  },
};

pub(crate) enum TextLayoutOutput {
  /// A `Text` or `RichText` leaf.
  Text(TextOutput),
  /// A `TextInput` leaf.
  TextInput(TextInputOutput),
  /// A link of a rich-text paragraph, placed by the paragraph.
  #[cfg(feature = "markdown")]
  InlineLink(super::inline_links::InlineLinkOutput),
}

/// Output of laying out a `Text` or `RichText` leaf.
pub(crate) struct TextOutput {
  /// The string drawn instead of the content (the ellipsized text, or the
  /// concatenated spans of selectable rich text).
  pub(super) display_text: Option<Arc<str>>,
  pub(super) render_wrap: bool,
  /// Only selectable text measures caret geometry, from the layout the
  /// paint draws ([`LayoutEngine::lay_out_text_selection`]); other text
  /// leaves the state's carets as they are.
  pub(super) carets: Option<PaintedCarets>,
  /// Where the text's first line sits below the top of its box, as the
  /// paint places it (`VerticalAlign`); measured with the carets, so a
  /// selection meets the glyphs.
  pub(super) text_top: f32,
  /// A plain text that draws an underline or a line through it: where its
  /// lines lie.
  pub(super) decoration: Option<Arc<super::text_decorations::TextDecorationLayout>>,
  /// Rich text that paints underlines or links: where its text lies.
  #[cfg(feature = "markdown")]
  pub(super) inline: Option<Arc<super::inline_links::InlineTextLayout>>,
}

impl TextLayoutOutput {
  /// The output's allocation and display text, plus its share of the caret
  /// data it holds.
  pub(crate) fn estimated_memory_bytes(&self) -> usize {
    let heap = match self {
      Self::Text(output) => {
        output.display_text.as_ref().map_or(0, |text| text.len())
          + output
            .carets
            .as_ref()
            .map_or(0, |carets| carets.positions.estimated_shared_memory_bytes())
          + output
            .decoration
            .as_ref()
            .map_or(0, |decoration| decoration.estimated_memory_bytes())
          + output.inline_memory_bytes()
      }
      #[cfg(feature = "markdown")]
      Self::InlineLink(output) => output.estimated_memory_bytes(),
      Self::TextInput(output) => {
        output.caret_positions.estimated_shared_memory_bytes()
          + output
            .token_layout
            .as_ref()
            .map_or(0, super::input_token_layout::TokenLayout::estimated_shared_memory_bytes)
      }
    };
    // Reference counts of the `Arc` holding the output.
    2 * std::mem::size_of::<usize>() + std::mem::size_of::<Self>() + heap
  }
}

impl TextOutput {
  /// This output, painting underlines or links as `inline` lays them out.
  #[cfg(feature = "markdown")]
  pub(super) fn with_inline(&self, inline: Arc<super::inline_links::InlineTextLayout>) -> Self {
    Self {
      display_text: self.display_text.clone(),
      render_wrap: self.render_wrap,
      carets: self.carets.clone(),
      text_top: inline.top,
      decoration: self.decoration.clone(),
      inline: Some(inline),
    }
  }

  /// This output, drawing its underline or the line through it as `decoration` lays them out.
  pub(super) fn with_decoration(&self, decoration: Arc<super::text_decorations::TextDecorationLayout>) -> Self {
    Self {
      display_text: self.display_text.clone(),
      render_wrap: self.render_wrap,
      carets: self.carets.clone(),
      text_top: self.text_top,
      decoration: Some(decoration),
      #[cfg(feature = "markdown")]
      inline: self.inline.clone(),
    }
  }

  /// This output with the carets of the painted layout and where the
  /// text's first line sits in its box.
  pub(super) fn with_selection(&self, carets: PaintedCarets, text_top: f32) -> Self {
    Self {
      display_text: self.display_text.clone(),
      render_wrap: self.render_wrap,
      carets: Some(carets),
      text_top,
      decoration: self.decoration.clone(),
      #[cfg(feature = "markdown")]
      inline: self.inline.clone(),
    }
  }

  fn inline_memory_bytes(&self) -> usize {
    #[cfg(feature = "markdown")]
    {
      self.inline.as_ref().map_or(0, |inline| inline.estimated_memory_bytes())
    }
    #[cfg(not(feature = "markdown"))]
    {
      0
    }
  }

  fn apply(&self, state: &TextState) {
    state.set_layout_output(
      self.display_text.as_ref(),
      self.render_wrap,
      self.carets.as_ref(),
      self.text_top,
    );
  }
}

/// Measurements of a `TextInput` leaf. Caret metrics and scroll offsets also
/// depend on the caret, focus and scroll position, which change between
/// layouts, so they are derived from these measurements each time the output
/// is applied instead of being stored.
pub(crate) struct TextInputOutput {
  pub(super) token_layout: Option<Arc<super::input_token_layout::TokenLayout>>,
  pub(super) caret_positions: CaretPositions,
  /// The box the text is laid out in: the input's box inside `padding`.
  pub(super) size: Size,
  /// Width of the laid-out text, which bounds horizontal scrolling.
  pub(super) text_width: f32,
  /// Height of the laid-out text, which bounds vertical scrolling.
  pub(super) text_height: f32,
  pub(super) line_height: f32,
  pub(super) text_band: Option<(f32, f32)>,
  /// The input's padding around `size`, with a reserved scrollbar's lane
  /// ([`LayoutEngine::text_input_scrollbar_lane`]) in its right side. A
  /// multiline input scrolls it with the text.
  pub(super) padding: ResolvedPadding,
}

impl TextInputOutput {
  fn apply(&self, state: &TextInputState) {
    state.set_token_layout(self.token_layout.clone());
    state.set_caret_positions(self.caret_positions.clone());
    state.set_caret_height(self.line_height);
    state.set_text_band(self.text_band);
    state.sync_caret_metrics_to_position(self.line_height);
    let (width, height) = (self.size.width, self.size.height);
    match state.overflow() {
      TextInputOverflow::Scroll => {
        state.set_scroll_x(self.horizontal_scroll(state));
        state.set_vertical_scroll(width, height, height, 0.0);
      }
      TextInputOverflow::Multiline => {
        state.set_scroll_x(0.0);
        let scroll_y = self.vertical_scroll(state);
        // The scroll's viewport is the input's whole box and its content the
        // text with the padding around it, as a scroll container's are, so
        // the scrollbar is placed from the box's edges and the offsets range
        // as far as they do over the text in the padded box.
        let padding = self.padding;
        let vertical_padding = padding.top + padding.bottom;
        state.set_vertical_scroll(
          width + padding.left + padding.right,
          height + vertical_padding,
          self.text_height + vertical_padding,
          scroll_y,
        );
      }
    }
  }

  /// Keeps the caret of a focused single-line input in view; an unfocused one
  /// shows its configured end.
  fn horizontal_scroll(&self, state: &TextInputState) -> f32 {
    let caret_x = state.caret_x() + state.scroll_x();
    let caret_width = 1.0;
    let max_scroll = (self.text_width + caret_width - self.size.width).max(0.0);
    if !state.is_focused() {
      return match state.unfocused_overflow_anchor() {
        TextInputOverflowAnchor::Start => 0.0,
        TextInputOverflowAnchor::End => max_scroll,
      };
    }
    let scroll_x = state.scroll_x().min(max_scroll);
    if caret_x < scroll_x {
      caret_x
    } else if caret_x + caret_width > scroll_x + self.size.width {
      (caret_x + caret_width - self.size.width).min(max_scroll)
    } else {
      scroll_x
    }
  }

  /// Keeps a multiline input's offset within its text and, unless the input
  /// was scrolled since its last edit or caret move, its caret's line in view:
  /// inside the padding, so the first line shows the top padding above it and
  /// the last line the bottom padding below it.
  fn vertical_scroll(&self, state: &TextInputState) -> f32 {
    let caret_y = state.caret_y() + state.scroll_y();
    let line_height = state.caret_height();
    let max_scroll = (self.text_height - self.size.height).max(0.0);
    let scroll_y = state.scroll_y().min(max_scroll);
    if !state.follows_caret() {
      scroll_y
    } else if caret_y < scroll_y {
      caret_y
    } else if caret_y + line_height > scroll_y + self.size.height {
      (caret_y + line_height - self.size.height).min(max_scroll)
    } else {
      scroll_y
    }
  }
}

impl LayoutEngine {
  /// Writes back the text output of every text leaf in `result`, a layout of
  /// `node` that is reused without laying the subtree out again.
  pub(super) fn restore_text_layout_tree(node: &Node, result: &LayoutResult) {
    Self::restore_text_layout_output(node, result);
    for (child, child_layout) in node.children().iter().zip(&result.children) {
      if child_layout.result.holds.text {
        Self::restore_text_layout_tree(child, &child_layout.result);
      }
    }
  }

  /// Writes the text output a cached result was laid out with back into the
  /// node's state.
  pub(super) fn restore_text_layout_output(node: &Node, result: &LayoutResult) {
    let Some(output) = result.text_layout.as_deref() else {
      return;
    };
    match (node.node_kind(), output) {
      (NodeKind::Text { state, .. }, TextLayoutOutput::Text(output)) => output.apply(state),
      #[cfg(feature = "markdown")]
      (NodeKind::RichText { state, .. }, TextLayoutOutput::Text(output)) => output.apply(state),
      (NodeKind::TextInput { state, .. }, TextLayoutOutput::TextInput(output)) => output.apply(state),
      _ => {}
    }
  }

  /// Applies a freshly computed text output to the node's state and attaches
  /// it to the result, so a later cache hit can restore it.
  pub(super) fn text_result(state: &TextState, size: Size, output: TextOutput) -> LayoutResult {
    output.apply(state);
    Self::leaf_with_output(size, TextLayoutOutput::Text(output))
  }

  pub(super) fn text_input_result(state: &TextInputState, output: TextInputOutput) -> LayoutResult {
    output.apply(state);
    Self::leaf_with_output(output.size, TextLayoutOutput::TextInput(output))
  }

  fn leaf_with_output(size: Size, output: TextLayoutOutput) -> LayoutResult {
    LayoutResult {
      size,
      children: vec![],
      text_layout: Some(Arc::new(output)),
      dropped: false,
      holds: SubtreeHolds::UNKNOWN,
      generation: 0,
      sticky_shifts: None,
    }
  }
}
