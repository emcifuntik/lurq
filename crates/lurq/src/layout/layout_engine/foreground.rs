//! What a node inherits while its tree is painted: the nearest ancestor's
//! `foreground`, which stands in for the color of a text that names none, and
//! whether an ancestor is disabled, which disables it.

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
    let own = node
      .effective_foreground()
      .and_then(|color| color.resolve(&self.palette.borrow()));
    if own.is_some() {
      self.foreground.set(own);
    }
    inherited
  }

  pub(super) fn leave_foreground_scope(&self, previous: Option<Color>) {
    self.foreground.set(previous);
  }

  /// Enters `node` for painting it and its children: it is disabled when it
  /// was built so or an element it is in is, and its state says so from here
  /// on. Returns the scope to restore afterwards.
  pub(super) fn enter_disabled_scope(&self, node: &Node) -> bool {
    let inherited = self.disabled.get();
    let disabled = inherited || node.is_disabled();
    node.sync_disabled_state(disabled);
    self.disabled.set(disabled);
    inherited
  }

  pub(super) fn leave_disabled_scope(&self, previous: bool) {
    self.disabled.set(previous);
  }

  /// A node painted without a scope of its own takes the state it is in.
  pub(super) fn inherit_disabled_state(&self, node: &Node) {
    node.sync_disabled_state(self.disabled.get());
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
