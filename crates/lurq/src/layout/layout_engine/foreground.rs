//! The foreground a text inherits while its tree is painted: the nearest
//! ancestor's `foreground`, which stands in for the color of a text that
//! names none.

use super::LayoutEngine;
use crate::{
  layout::text_style::TextStyle,
  node::{color::Color, node::Node, node_kind::TextStyleSource},
};

impl LayoutEngine {
  /// Enters `node` for painting it and its children: its foreground, if it
  /// sets one the palette resolves, is theirs. Returns the scope to restore
  /// afterwards.
  pub(super) fn enter_foreground_scope(&self, node: &Node) -> Option<Color> {
    let inherited = self.foreground.get();
    // A transition or an animation of the foreground draws its own color this frame.
    let animated = match node.animation_override(crate::animation::AnimatableProperty::Foreground) {
      Some(crate::animation::AnimatableValue::Color(color)) => Some(color),
      _ => None,
    };
    let own = animated.or_else(|| {
      node
        .effective_foreground()
        .and_then(|color| color.resolve(&self.palette.borrow()))
    });
    if own.is_some() {
      self.foreground.set(own);
    }
    inherited
  }

  pub(super) fn leave_foreground_scope(&self, previous: Option<Color>) {
    self.foreground.set(previous);
  }

  /// The style a text is painted in: its own, in the inherited foreground
  /// when it names no color.
  pub(super) fn painted_text_style(&self, style: &TextStyleSource) -> TextStyle {
    let mut resolved = style.resolve(&self.typography.borrow(), &self.palette.borrow());
    if !style.names_color()
      && let Some(foreground) = self.foreground.get()
    {
      resolved.color = foreground;
    }
    resolved
  }
}
