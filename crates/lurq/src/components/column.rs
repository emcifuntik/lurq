use crate::{impl_into_node, layout::Alignment, node::Element};

impl_into_node!(Column);

impl Column {
  pub fn new() -> Self {
    Self::from_node(crate::node::Node::column(0.0, Alignment::Start, vec![]))
  }

  pub fn with(
    spacing: impl Into<crate::node::SpacingValue>,
    align: Alignment,
    children: impl IntoIterator<Item = impl Into<Element>>,
  ) -> Self {
    Self::new().spacing(spacing).align_items(align).with_children(children)
  }

  pub fn child(mut self, child: impl Into<Element>) -> Self {
    self.update_node(|node| node.push_child_element(child.into()));
    self
  }

  pub fn with_children(mut self, children: impl IntoIterator<Item = impl Into<Element>>) -> Self {
    self.update_node(|node| node.extend_child_elements(children.into_iter().map(Into::into)));
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

  pub fn wrap(mut self) -> Self {
    self.update_node(crate::node::NodeUpdate::wrap);
    self
  }
}

impl Default for Column {
  fn default() -> Self {
    Self::new()
  }
}
