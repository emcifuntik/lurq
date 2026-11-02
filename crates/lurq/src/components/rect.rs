use crate::{impl_into_node, layout::layout_kind::FrameConstraints, node::dimension::Dimension};

impl_into_node!(Rect);

impl Rect {
  pub fn new(width: impl Into<Dimension>, height: impl Into<Dimension>) -> Self {
    Self::sized(width.into(), height.into())
  }

  /// Never inlined: the nodes this builds by value (over 1 KiB each) would
  /// otherwise sit in the frame of a caller that builds a tree recursively,
  /// once per level; see the note above `Node::push_child_element` (LUR-13).
  #[inline(never)]
  fn sized(width: Dimension, height: Dimension) -> Self {
    Self::from_node(crate::node::Node::new().frame(FrameConstraints {
      width: Some(width),
      height: Some(height),
      ..FrameConstraints::default()
    }))
  }
}

impl Default for Rect {
  fn default() -> Self {
    Self::new(Dimension::Auto, Dimension::Auto)
  }
}
