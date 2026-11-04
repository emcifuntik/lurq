//! What a component slot keeps of its last render (`ChildSlot::rendered` in
//! `app/ctx.rs`), and how an unchanged component's output is put together
//! from it again.
//!
//! A slot keeps its component's own output: the nodes its render built, with
//! a placeholder where it mounted a component of its own. That component's
//! output is in its own slot, so a component rendering again by itself
//! changes no cache above it, and keeping an output copies only the nodes of
//! one component, not the outputs nested in it. When the parent renders again
//! and the component is unchanged, its output is put together from its slot
//! and the slots of the components it mounted; each placeholder takes back
//! what the mounting code put on the root (`outer_decoration.rs`).
//!
//! The root of a component the render was handed (the slot children of
//! `mount_with`, the child a wrapper such as `Draggable` is given) is not a
//! component of this one: it is kept whole, with what this render put on it.
//! When its component renders again by itself afterwards, the output is put
//! together with its fresh render there, as the live tree has it.

use super::node::{Node, SyntheticNodeRole};

impl Node {
  /// A placeholder this node's slot cache holds for a child component's
  /// root; never in the live tree.
  pub(crate) fn is_component_slot_placeholder(&self) -> bool {
    self.has_synthetic_role(SyntheticNodeRole::ComponentSlot)
  }

  fn component_slot_placeholder(&self) -> Node {
    let mut placeholder = Node::new();
    placeholder.component_slot_id = self.component_slot_id;
    placeholder.component_key = self.component_key.clone();
    placeholder.outer_decoration = self.outer_decoration.clone();
    placeholder.synthetic_role = Some(SyntheticNodeRole::ComponentSlot);
    placeholder
  }

  /// A copy of this output for its slot to keep: the root of each component
  /// below it that `own` says the rendering component mounted becomes a
  /// placeholder.
  pub(crate) fn clone_own_output(&self, own: &dyn Fn(u64) -> bool) -> Box<Node> {
    Box::new(self.clone_with_component_roots(&mut |root: &Node| {
      own(root.component_slot_id?).then(|| root.component_slot_placeholder())
    }))
  }

  /// [`Self::clone_for_reuse`], except that `component_root` is asked about
  /// each component root below this node, in children and declarations: the
  /// node it returns stands for that root's whole subtree, and `None` copies
  /// the root like any other node.
  pub(crate) fn clone_with_component_roots(&self, component_root: &mut dyn FnMut(&Node) -> Option<Node>) -> Node {
    let overlay_declaration = self
      .overlay_declaration
      .as_ref()
      .map(|spec| Box::new(spec.clone_with_node(spec.node.clone_below(component_root))));
    let modal_declaration = self
      .modal_declaration
      .as_ref()
      .map(|spec| Box::new(spec.clone_with_node(spec.node.clone_below(component_root))));
    // A plain loop, as `clone_children_for_reuse` has it, for unoptimized
    // builds' stack.
    let mut children = Vec::with_capacity(self.children.len());
    for child in &self.children {
      children.push(child.clone_below(component_root));
    }
    self.clone_around(overlay_declaration, modal_declaration, children)
  }

  fn clone_below(&self, component_root: &mut dyn FnMut(&Node) -> Option<Node>) -> Node {
    if self.component_slot_id.is_some()
      && let Some(node) = component_root(self)
    {
      return node;
    }
    self.clone_with_component_roots(component_root)
  }
}
