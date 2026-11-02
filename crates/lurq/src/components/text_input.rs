pub use crate::node::node_kind::{TextInputOverflow, TextInputOverflowAnchor};
use crate::{
  app::theme::CaretMode,
  core::Signal,
  impl_into_node,
  layout::text_style::{FontFeatures, TextAlign, TextStyle},
};

impl_into_node!(TextInput);

impl TextInput {
  /// Draw source-backed inline pills. Invalidated ranges and masked/IME text render normally.
  /// Use validated ranges for the exact current value; no display label replaces stored bytes.
  pub fn tokens(mut self, tokens: super::InputTokens) -> Self {
    self.update_node(|node| {
      if let crate::node::node_kind::NodeKind::TextInput { state, .. } = node.node_kind() {
        state.set_tokens(tokens);
      }
    });
    self
  }

  /// Makes this a styled input: `spans` holds the styled runs of the value
  /// as UTF-8 byte ranges ([`super::StyledSpan`]). The input draws them with
  /// the theme's text marks, moves them with every edit, keeps them in its
  /// undo history and writes them back to the signal; the app sets both
  /// signals to load a model. Typed text takes the style of the text before
  /// the caret, except that a link does not grow past its end. A styled
  /// input ignores [`Self::tokens`]; masked, it is plain.
  #[cfg(feature = "markdown")]
  pub fn spans(mut self, spans: Signal<Vec<super::StyledSpan>>) -> Self {
    self.update_node(|node| {
      if let crate::node::node_kind::NodeKind::TextInput { state, .. } = node.node_kind() {
        state.set_spans_signal(spans);
      }
    });
    self
  }

  /// Runs after every change a styled input makes to its text or spans
  /// (typing, deleting, pasting, styling, undo) with a [`super::StyledChange`]:
  /// the text and spans it now holds, shared rather than copied, and what the
  /// edit was. An app's own writes of the signals do not run it.
  #[cfg(feature = "markdown")]
  pub fn on_styled_change(mut self, f: impl Fn(&super::StyledChange) + Send + Sync + 'static) -> Self {
    self.update_node(|node| {
      if let crate::node::node_kind::NodeKind::TextInput { state, .. } = node.node_kind() {
        state.add_styled_change_handler(f);
      }
    });
    self
  }

  /// What copying or cutting from a styled input puts on the clipboard: `f`
  /// gets the selection with its spans (offsets from its start) and returns
  /// the text to copy, such as Markdown. Without it the plain text is copied.
  #[cfg(feature = "markdown")]
  pub fn copy_format(mut self, f: impl Fn(&super::StyledText) -> String + Send + Sync + 'static) -> Self {
    self.update_node(|node| {
      if let crate::node::node_kind::NodeKind::TextInput { state, .. } = node.node_kind() {
        state.set_copy_format(f);
      }
    });
    self
  }

  /// Runs when a person clicks a link of a styled input with the platform's
  /// shortcut modifier held (Ctrl, or Cmd on macOS); the app opens the
  /// target. A plain click places the caret, as in any text.
  #[cfg(feature = "markdown")]
  pub fn on_link_click(mut self, f: impl Fn(&super::LinkClick) + Send + Sync + 'static) -> Self {
    self.update_node(|node| {
      if let crate::node::node_kind::NodeKind::TextInput { state, .. } = node.node_kind() {
        state.add_link_click_handler(f);
      }
    });
    self
  }

  /// Retained selection/edit control for this input; use one controller per keyed value.
  pub fn controller(mut self, controller: super::TextInputController) -> Self {
    self.update_node(|node| {
      if let crate::node::node_kind::NodeKind::TextInput { state, .. } = node.node_kind() {
        controller.bind(state);
      }
    });
    self.ref_element(controller.reference())
  }

  pub fn new(value: Signal<String>) -> Self {
    Self::from_node(crate::node::Node::text_input(value))
  }

  pub fn styled(value: Signal<String>, style: TextStyle) -> Self {
    Self::from_node(crate::node::Node::text_input_styled(value, style))
  }

  pub fn text_style(mut self, style: TextStyle) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_style(node, style));
    self
  }

  pub fn placeholder_style(mut self, style: TextStyle) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_placeholder_style(node, style));
    self
  }

  pub fn text_align(mut self, align: impl Into<TextAlign>) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_align(node, align));
    self
  }

  /// Extra space after every glyph of the value and placeholder, in logical
  /// pixels (negative tightens). Like `text_align`, a later `text_style` or
  /// `placeholder_style` replaces it.
  pub fn letter_spacing(mut self, letter_spacing: f32) -> Self {
    self.update_node(|node| node.set_text_input_letter_spacing(letter_spacing));
    self
  }

  /// OpenType feature settings of the value and placeholder. Like
  /// `letter_spacing`, a later `text_style` or `placeholder_style` replaces them.
  pub fn font_features(mut self, font_features: impl Into<FontFeatures>) -> Self {
    let font_features = font_features.into();
    self.update_node(|node| node.set_text_input_font_features(font_features));
    self
  }

  pub fn placeholder(mut self, placeholder: &str) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::placeholder(node, placeholder));
    self
  }

  pub fn on_input(mut self, f: impl crate::node::IntoTextInputEventHandler) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::on_input(node, f));
    self
  }

  pub fn off_input(mut self, f: impl crate::node::IntoTextInputEventHandler) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::off_input(node, f));
    self
  }

  pub fn caret_color(mut self, color: impl Into<crate::node::TextColor>) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::caret_color(node, color));
    self
  }

  pub fn selection_color(mut self, color: impl Into<crate::node::TextColor>) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::selection_color(node, color));
    self
  }

  pub fn caret_mode(mut self, mode: CaretMode) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_caret_mode(node, mode));
    self
  }

  pub fn overflow(mut self, overflow: TextInputOverflow) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_overflow(node, overflow));
    self
  }

  /// Controls which edge of overflowing single-line text is visible while
  /// the input is not focused. Focused inputs always scroll to follow the
  /// caret.
  pub fn unfocused_overflow_anchor(mut self, anchor: TextInputOverflowAnchor) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_unfocused_overflow_anchor(node, anchor));
    self
  }

  /// Masks each value character with `•` (U+2022).
  pub fn mask(mut self) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_mask(node));
    self
  }

  /// Uses a custom mask, such as `'\u{25cf}'` for a WinUI-style heavy dot.
  pub fn mask_char(mut self, mask: char) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_mask_char(node, mask));
    self
  }

  pub fn unmask(mut self) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_unmask(node));
    self
  }

  pub fn single_line(mut self) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_overflow(node, TextInputOverflow::Scroll));
    self
  }

  pub fn multiline(mut self) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_overflow(node, TextInputOverflow::Multiline));
    self
  }

  pub fn textarea(mut self) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_rows(node, 2, 6));
    self
  }

  pub fn rows(mut self, min_rows: usize, max_rows: usize) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_rows(node, min_rows, max_rows));
    self
  }

  pub fn min_rows(mut self, min_rows: usize) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_min_rows(node, min_rows));
    self
  }

  pub fn max_rows(mut self, max_rows: usize) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_max_rows(node, max_rows));
    self
  }

  pub fn rows_exact(mut self, rows: usize) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_input_rows_exact(node, rows));
    self
  }
}

impl Default for TextInput {
  fn default() -> Self {
    Self::new(Signal::new(String::new()))
  }
}
