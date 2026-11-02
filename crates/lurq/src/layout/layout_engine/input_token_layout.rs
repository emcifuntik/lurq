//! Styled paragraph geometry and paint share the exact input layout.
use std::sync::Arc;

use super::{DEFAULT_TEXT_INPUT_WIDTH, LayoutEngine, TextInputOutput, multiline_preferred_height};
pub(crate) use crate::app::glyph_engine::input_tokens::DecoratedInputLayout as TokenLayout;
use crate::{
  app::glyph_engine::{
    GlyphEngine,
    input_tokens::{InputConfig, TextLines, TokenStyle},
  },
  components::InputTokens,
  layout::{Constraints, Size, text_style::TextStyle},
  node::node_kind::{TextInputOverflow, TextInputState},
};

impl LayoutEngine {
  pub(super) fn layout_input_tokens(
    &self,
    glyphs: &mut GlyphEngine,
    state: &TextInputState,
    base: &TextStyle,
    constraints: Constraints,
    tokens: InputTokens,
    scrollbar_lane: f32,
  ) -> crate::layout::layout_result::LayoutResult {
    let width = Self::decorated_input_width(state, constraints);
    let styles = tokens
      .tokens
      .iter()
      .map(|token| {
        let mut text = token
          .style
          .typography
          .and_then(|role| self.typography.borrow().try_get(role))
          .unwrap_or_else(|| base.clone());
        text.color = token
          .style
          .foreground
          .resolve(&self.palette.borrow())
          .unwrap_or(base.color);
        let pad = token.style.padding.resolve(&self.spacing.borrow(), width);
        let padding = if pad.is_finite() { pad.max(0.0) } else { 0.0 };
        let radius = token.style.radius.resolve(&self.radii.borrow());
        let radius = if radius.is_finite() { radius.max(0.0) } else { 0.0 };
        TokenStyle {
          range: token.range.clone(),
          text,
          padding,
          fill: token
            .style
            .background
            .resolve(&self.palette.borrow())
            .map(|color| (color, radius)),
          lines: TextLines::default(),
        }
      })
      .collect();
    self.layout_decorated_input(
      glyphs,
      state,
      base,
      constraints,
      (tokens.source, styles),
      scrollbar_lane,
    )
  }

  /// Lays out `source` with a text style per range, sharing the geometry
  /// with paint: input tokens and styled spans both draw this way.
  pub(super) fn layout_decorated_input(
    &self,
    glyphs: &mut GlyphEngine,
    state: &TextInputState,
    base: &TextStyle,
    constraints: Constraints,
    (source, styles): (Arc<str>, Vec<TokenStyle>),
    scrollbar_lane: f32,
  ) -> crate::layout::layout_result::LayoutResult {
    let wrap = state.overflow() == TextInputOverflow::Multiline;
    let width = Self::decorated_input_width(state, constraints);
    let layout = glyphs.decorated_input(InputConfig {
      source,
      base: base.clone(),
      styles,
      width,
      min_width: constraints.min_width.max(DEFAULT_TEXT_INPUT_WIDTH).min(width),
      wrap,
    });
    let preferred = Size::new(
      if wrap {
        layout.size.width.max(DEFAULT_TEXT_INPUT_WIDTH)
      } else {
        DEFAULT_TEXT_INPUT_WIDTH
      },
      if wrap {
        multiline_preferred_height(state, layout.size.height, layout.line_height)
      } else {
        layout.line_height
      },
    );
    let output = TextInputOutput {
      caret_positions: layout.positions(),
      size: constraints.constrain(preferred),
      text_width: layout.size.width,
      text_height: layout.size.height,
      line_height: layout.line_height,
      text_band: Some(layout.selection_band(0.0)),
      token_layout: Some(layout),
      scrollbar_lane,
    };
    Self::text_input_result(state, output)
  }

  /// The width a decorated input lays its text out in: the room it is given
  /// when it wraps, its own width when it scrolls.
  fn decorated_input_width(state: &TextInputState, constraints: Constraints) -> f32 {
    let max_width = if state.overflow() == TextInputOverflow::Multiline {
      constraints.max_width
    } else {
      constraints.constrain(Size::new(DEFAULT_TEXT_INPUT_WIDTH, 0.0)).width
    };
    if max_width.is_nan() { 0.0 } else { max_width.max(0.0) }
  }
}
