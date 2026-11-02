//! A styled text input's spans as the per-range text styles of a decorated
//! input layout, each mark's look taken from the theme's text marks.
use std::sync::Arc;

use super::LayoutEngine;
use crate::{
  app::glyph_engine::input_tokens::{TextLines, TokenStyle},
  components::StyledSpan,
  layout::text_style::TextStyle,
  node::node_kind::TextInputState,
};

impl LayoutEngine {
  /// What a styled input draws: its text (with a composition in progress)
  /// and a style per span. `None` for a plain input, a masked one, and an
  /// empty one showing its placeholder.
  pub(super) fn styled_input_styles(
    &self,
    state: &TextInputState,
    base: &TextStyle,
  ) -> Option<(Arc<str>, Vec<TokenStyle>)> {
    let (text, spans) = state.styled_display()?;
    let marks = self.text_marks.borrow().clone();
    let palette = self.palette.borrow();
    let styles = spans
      .iter()
      .map(|StyledSpan { range, style }| {
        let resolved = marks.resolve(style, base, &palette);
        TokenStyle {
          range: range.clone(),
          text: resolved.text,
          padding: 0.0,
          fill: resolved.fill,
          lines: TextLines {
            underline: resolved.underline,
            strikethrough: resolved.strikethrough,
          },
        }
      })
      .collect();
    Some((text, styles))
  }

  pub(crate) fn set_text_marks(&self, marks: Arc<crate::app::theme::ThemeTextMarks>) {
    *self.text_marks.borrow_mut() = marks;
  }
}
