pub use crate::node::node_kind::TextOverflow;
use crate::{
  app::theme::TypographyStyle,
  impl_into_node,
  layout::text_style::{FontFeatures, TextAlign, TextStyle},
  node::{TextColor, TextTransformMode},
};

impl_into_node!(Text);

impl Text {
  pub fn new(content: &str) -> Self {
    Self::from_node(crate::node::Node::text(content))
  }

  pub fn styled(content: &str, style: TextStyle) -> Self {
    Self::from_node(crate::node::Node::text_styled(content, style))
  }

  pub fn nowrap(mut self) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_wrap(node, false));
    self
  }

  pub fn selectable(mut self, selectable: bool) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::selectable(node, selectable));
    self
  }

  /// Keep this text out of inspection: it is laid out and painted on screen
  /// as usual, but DevTools (tree, inspector, snapshots) and the MCP tools
  /// (`lurq_read_tree`, `lurq_inspect`, the `lurq_find*` lookups, and
  /// accessible names derived from it) show [`REDACTED`](crate::core::REDACTED)
  /// in its place, with the attribute `sensitive=true`, and their screenshots
  /// (`lurq_screenshot`, a DevTools node screenshot) show an opaque grey bar
  /// where its glyphs are painted. The app's own screenshots
  /// (`WindowHandle::screenshot`) show the text. For a one-time code or
  /// a secret the user must read. Selection follows `selectable` as for any
  /// text (off by default); a selectable sensitive text can be selected and
  /// copied with Ctrl+C/Cmd+C, which no inspector sees. Keep the value itself
  /// in a [`Sensitive`](crate::core::Sensitive) so that it does not reach
  /// DevTools' signal values and history either.
  pub fn sensitive(mut self) -> Self {
    self.update_node(crate::node::node::Node::set_text_sensitive);
    self
  }

  pub fn text_transform_mode(mut self, mode: TextTransformMode) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_transform_mode(node, mode));
    self
  }

  pub fn variant(mut self, style: impl Into<TypographyStyle>) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_variant(node, style));
    self
  }

  pub fn text_align(mut self, align: impl Into<TextAlign>) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_align(node, align));
    self
  }

  /// Extra space after every glyph in logical pixels (negative tightens),
  /// overriding the style's [`TextStyle::letter_spacing`].
  pub fn letter_spacing(mut self, letter_spacing: f32) -> Self {
    self.update_node(|node| node.set_text_letter_spacing(letter_spacing));
    self
  }

  /// OpenType feature settings, overriding the style's
  /// [`TextStyle::font_features`] (typography roles included).
  pub fn font_features(mut self, font_features: impl Into<FontFeatures>) -> Self {
    let font_features = font_features.into();
    self.update_node(|node| node.set_text_font_features(font_features));
    self
  }

  pub fn text_overflow(mut self, overflow: TextOverflow) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_overflow(node, overflow));
    self
  }

  pub fn color(mut self, color: impl Into<TextColor>) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_color(node, color));
    self
  }

  pub fn text_shadow(mut self, shadow: crate::layout::text_style::TextShadow) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::text_shadow(node, shadow));
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

  /// Fills behind ranges of the text, such as the matches of a search: one
  /// per laid-out line a range covers, each in its own color, under the
  /// glyphs and under a selection. Ranges are in bytes of the content and
  /// stay on their words when the text re-wraps; the text is shaped as
  /// without them. Pass an empty list to clear them.
  ///
  /// ```
  /// use lurq::{
  ///   components::{Text, TextHighlight},
  ///   node::color::Color,
  /// };
  ///
  /// let current = Color::new(250, 204, 21, 255);
  /// let other = Color::new(250, 204, 21, 90);
  /// Text::new("one match, two match")
  ///   .highlights(vec![
  ///     TextHighlight::new(4..9, current).radius(3.0),
  ///     TextHighlight::new(15..20, other).radius(3.0),
  ///   ]);
  /// ```
  pub fn highlights(
    mut self,
    highlights: impl Into<std::sync::Arc<[crate::node::text_highlight::TextHighlight]>>,
  ) -> Self {
    let highlights = highlights.into();
    self.update_node(|node| node.set_text_highlights(highlights));
    self
  }
}

impl Default for Text {
  fn default() -> Self {
    Self::new("")
  }
}
