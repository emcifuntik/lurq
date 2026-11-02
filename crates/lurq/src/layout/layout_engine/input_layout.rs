use super::*;
use crate::node::text_selection::{trailing_break_len, trailing_break_row_y};

impl LayoutEngine {
  pub(super) fn layout_text_input(
    &self,
    glyph_engine: &mut GlyphEngine,
    state: &crate::node::node_kind::TextInputState,
    text: &str,
    style: &TextStyle,
    placeholder_style: Option<&TextStyle>,
    constraints: Constraints,
    scrollbar_lane: f32,
  ) -> LayoutResult {
    #[cfg(feature = "markdown")]
    if let Some(styled) = self.styled_input_styles(state, style) {
      return self.layout_decorated_input(glyph_engine, state, style, constraints, styled, scrollbar_lane);
    }
    if let Some(tokens) = state.tokens().filter(|tokens| !tokens.tokens.is_empty()) {
      return self.layout_input_tokens(glyph_engine, state, style, constraints, tokens, scrollbar_lane);
    }
    let display_style = text_input_display_style(state, style, placeholder_style);
    let overflow = state.overflow();
    let text_constraints = Constraints {
      min_width: 0.0,
      min_height: 0.0,
      ..constraints
    };
    let text_result = self.layout_text(
      glyph_engine,
      text,
      display_style,
      text_constraints,
      overflow == crate::node::node_kind::TextInputOverflow::Multiline,
    );
    let line_height = (style.font_size * style.line_height).max(1.0);
    let wraps = overflow == crate::node::node_kind::TextInputOverflow::Multiline;
    let preferred_width = if wraps {
      text_result.size.width.max(DEFAULT_TEXT_INPUT_WIDTH)
    } else {
      DEFAULT_TEXT_INPUT_WIDTH
    };
    let width = constraints.constrain(Size::new(preferred_width, 0.0)).width;
    // Caret positions must wrap exactly like the rendered text, otherwise
    // soft-wrapped rows collapse onto the first row's y and pointer hit-testing
    // can never reach them. Single-line (Scroll) inputs never wrap, but aligned
    // text still needs the finite content width so caret/selection geometry
    // follows centered or right-aligned glyphs.
    let caret_width = if wraps || display_style.text_align != crate::layout::text_style::TextAlign::Left {
      width
    } else {
      f32::MAX
    };
    let caret_source = state.caret_layout_text();
    let mut caret_positions = glyph_engine.caret_positions(&caret_source, style, caret_width, wraps);
    state.remap_caret_positions(&mut caret_positions);
    let mut text_height = text_result
      .size
      .height
      .max(explicit_multiline_height(text, line_height));
    // Text layout ends at the row a final line break ends; the caret after it sits on the empty row below, which
    // the input must hold too. Counting line breaks alone misses the rows a paragraph wrapped into.
    if wraps && trailing_break_len(&caret_source) > 0 {
      let empty_row = trailing_break_row_y(&caret_positions, &caret_source, line_height);
      text_height = text_height.max(empty_row + line_height);
    }
    let preferred = if wraps {
      Size::new(
        preferred_width,
        multiline_preferred_height(state, text_height, line_height),
      )
    } else {
      Size::new(preferred_width, line_height)
    };
    let size = constraints.constrain(preferred);
    let output = TextInputOutput {
      token_layout: None,
      caret_positions,
      size,
      text_width: text_result.size.width,
      text_height,
      line_height,
      text_band: glyph_engine.line_content_band(style),
      scrollbar_lane,
    };
    Self::text_input_result(state, output)
  }

  /// The width a multiline text input keeps free right of its text for a
  /// vertical scrollbar placed [`ScrollBarPlacement::Reserved`], or 0 when
  /// the bar overlays the text. Layout and paint count it in the input's
  /// right padding, so the text wraps short of the bar, which runs in the
  /// lane. Like a reserved scroll container's gutter, the lane stays while
  /// the text fits, so the text never re-wraps as it starts or stops
  /// overflowing; it fits the wider of the bar at rest and hovered, so
  /// hovering the bar never moves the text.
  pub(crate) fn text_input_scrollbar_lane(&self, node: &Node) -> f32 {
    let NodeKind::TextInput { state, .. } = node.node_kind() else {
      return 0.0;
    };
    if state.overflow() != TextInputOverflow::Multiline {
      return 0.0;
    }
    let theme_style = self.scrollbar.borrow().clone();
    let resting = node.scrollbar_style_when(theme_style.clone(), false);
    if !should_reserve_scrollbar(&resting, ScrollDirection::Vertical, ScrollAxis::Vertical) {
      return 0.0;
    }
    let hovered = node.scrollbar_style_when(theme_style, true);
    reserved_scrollbar_size(&resting).max(reserved_scrollbar_size(&hovered))
  }
}
