use crate::{
  impl_into_node,
  layout::Alignment,
  node::{ButtonKind, Element},
};

impl_into_node!(Button);

impl Button {
  pub fn new(label: &str) -> Self {
    Self::from_node(
      crate::node::Node::row(0.0, Alignment::Center, vec![crate::node::Node::text(label)])
        .button_kind(ButtonKind::Button),
    )
  }

  pub fn empty() -> Self {
    Self::from_node(crate::node::Node::row(0.0, Alignment::Center, vec![]).button_kind(ButtonKind::Button))
  }

  pub fn child(mut self, child: impl Into<Element>) -> Self {
    self.update_node(|node| node.push_child_element(child.into()));
    self
  }

  pub fn with_children(mut self, children: impl IntoIterator<Item = impl Into<Element>>) -> Self {
    self.update_node(|node| node.extend_child_elements(children.into_iter().map(Into::into)));
    self
  }

  #[cfg(feature = "form")]
  pub fn submit(mut self) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::button_kind(node, ButtonKind::Submit));
    self
  }

  pub fn button(mut self) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::button_kind(node, ButtonKind::Button));
    self
  }

  pub fn kind(mut self, kind: ButtonKind) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::button_kind(node, kind));
    self
  }

  pub fn spacing(mut self, spacing: impl Into<crate::node::SpacingValue>) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::spacing(node, spacing));
    self
  }

  pub fn align_items(mut self, align: Alignment) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::align_items(node, align));
    self
  }

  pub fn justify(mut self, justify: crate::layout::layout_kind::Justify) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::justify(node, justify));
    self
  }
}
