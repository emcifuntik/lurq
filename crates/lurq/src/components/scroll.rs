use crate::{impl_into_node, layout::layout_kind::ScrollState, node::Element};

impl_into_node!(ScrollVertical);
impl_into_node!(ScrollHorizontal);
impl_into_node!(ScrollBoth);

impl ScrollVertical {
  pub fn new(child: impl Into<Element>) -> Self {
    Self::from_node(crate::node::dsl::scroll_vertical(child.into().into_node()))
  }

  pub fn with_scroll_state(mut self, existing: ScrollState) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::with_scroll_state(node, existing));
    self
  }

  pub fn culling(mut self, enabled: bool) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::culling(node, enabled));
    self
  }

  /// Middle-button autoscroll, off by default; see
  /// [`Node::middle_button_autoscroll`](crate::node::node::Node::middle_button_autoscroll).
  pub fn middle_button_autoscroll(mut self, enabled: bool) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::middle_button_autoscroll(node, enabled));
    self
  }
}

impl Default for ScrollVertical {
  fn default() -> Self {
    Self::new(Element::new())
  }
}

impl ScrollHorizontal {
  pub fn new(child: impl Into<Element>) -> Self {
    Self::from_node(crate::node::dsl::scroll_horizontal(child.into().into_node()))
  }

  pub fn with_scroll_state(mut self, existing: ScrollState) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::with_scroll_state(node, existing));
    self
  }

  pub fn culling(mut self, enabled: bool) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::culling(node, enabled));
    self
  }

  /// Middle-button autoscroll, off by default; see
  /// [`Node::middle_button_autoscroll`](crate::node::node::Node::middle_button_autoscroll).
  pub fn middle_button_autoscroll(mut self, enabled: bool) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::middle_button_autoscroll(node, enabled));
    self
  }
}

impl Default for ScrollHorizontal {
  fn default() -> Self {
    Self::new(Element::new())
  }
}

impl ScrollBoth {
  pub fn new(child: impl Into<Element>) -> Self {
    Self::from_node(crate::node::dsl::scroll_both(child.into().into_node()))
  }

  pub fn with_scroll_state(mut self, existing: ScrollState) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::with_scroll_state(node, existing));
    self
  }

  pub fn culling(mut self, enabled: bool) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::culling(node, enabled));
    self
  }

  /// Middle-button autoscroll, off by default; see
  /// [`Node::middle_button_autoscroll`](crate::node::node::Node::middle_button_autoscroll).
  pub fn middle_button_autoscroll(mut self, enabled: bool) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::middle_button_autoscroll(node, enabled));
    self
  }
}

impl Default for ScrollBoth {
  fn default() -> Self {
    Self::new(Element::new())
  }
}
