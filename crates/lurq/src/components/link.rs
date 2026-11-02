use std::sync::Arc;

use crate::{
  app::ctx::Ctx,
  impl_into_node,
  layout::Alignment,
  node::{CursorIcon, Element},
  router::Navigator,
};

impl_into_node!(Link);

impl Link {
  pub fn build(ctx: &mut Ctx, label: &str, to: impl Into<String>) -> Self {
    let path: Arc<str> = Arc::from(to.into());
    let navigator = ctx.use_context::<Navigator>().expect("Link must be inside a Router");
    Self::from_node(
      crate::node::Node::row(0.0, Alignment::Center, vec![crate::node::Node::text(label)])
        .link_debug_attr(path.clone())
        .cursor(CursorIcon::Pointer)
        .on_click({
          let path = path.clone();
          move |_| navigator.push(path.to_string())
        }),
    )
  }

  pub fn build_empty(ctx: &mut Ctx, to: impl Into<String>) -> Self {
    let path: Arc<str> = Arc::from(to.into());
    let navigator = ctx.use_context::<Navigator>().expect("Link must be inside a Router");
    Self::from_node(
      crate::node::Node::row(0.0, Alignment::Center, vec![])
        .link_debug_attr(path.clone())
        .cursor(CursorIcon::Pointer)
        .on_click({
          let path = path.clone();
          move |_| navigator.push(path.to_string())
        }),
    )
  }

  pub fn child(mut self, child: impl Into<Element>) -> Self {
    self.update_node(|node| node.push_child_element(child.into()));
    self
  }
}

trait LinkDebugAttr {
  fn link_debug_attr(self, path: Arc<str>) -> Self;
}

impl LinkDebugAttr for crate::node::Node {
  #[cfg(any(feature = "devtools", feature = "mcp"))]
  fn link_debug_attr(self, path: Arc<str>) -> Self {
    self.debug_attr("to", path)
  }

  #[cfg(not(any(feature = "devtools", feature = "mcp")))]
  fn link_debug_attr(self, _path: Arc<str>) -> Self {
    self
  }
}
