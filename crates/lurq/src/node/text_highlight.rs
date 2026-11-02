//! Fills behind ranges of a text, such as the matches of a search.

use std::ops::Range;

use crate::node::TextColor;

/// A fill behind a range of a [`Text`](crate::components::Text::highlights)
/// or a `Markdown` document (`MarkdownProps::highlights`): one rounded
/// rectangle per laid-out line the range covers, under the glyphs and under
/// a selection. It does not change how the text is shaped, so it follows the
/// text when it re-wraps.
///
/// `range` is in bytes of the text: of a `Text`'s content, or of a
/// document's rendered text (`MarkdownProps::rendered_text`). Ends inside a
/// character are moved back to its start; a range past the text's end is
/// cut at it.
///
/// ```
/// use lurq::{components::TextHighlight, node::color::Color};
///
/// let current = TextHighlight::new(4..9, Color::new(255, 200, 0, 255)).radius(3.0);
/// let other = TextHighlight::new(20..25, Color::new(255, 200, 0, 90));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct TextHighlight {
  pub range: Range<usize>,
  /// The fill: a color or a palette role.
  pub color: TextColor,
  /// Corner radius of each line's fill, in logical pixels.
  pub radius: f32,
}

impl TextHighlight {
  pub fn new(range: Range<usize>, color: impl Into<TextColor>) -> Self {
    Self {
      range,
      color: color.into(),
      radius: 0.0,
    }
  }

  /// Corner radius of each line's fill, in logical pixels.
  pub fn radius(mut self, radius: f32) -> Self {
    self.radius = radius;
    self
  }

  /// The radius to draw: a negative or non-finite one draws square corners.
  pub(crate) fn drawn_radius(&self) -> f32 {
    if self.radius.is_finite() {
      self.radius.max(0.0)
    } else {
      0.0
    }
  }
}

impl crate::app::component::DevtoolsInspectable for TextHighlight {
  fn inspect(&self, formatter: &mut crate::app::component::DevtoolsFormatter<'_>) {
    formatter.value_debug(self);
  }
}
