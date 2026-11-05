//! What the code that mounts a component puts on the root of the element
//! `mount` returns: an `id`, classes or a `key` (`Ctx::mount::<C>(props).id("x")`),
//! and what a wrapper such as `Draggable` puts on the root of the child it is
//! given (`Element::decorate_root`).
//!
//! The component's root node holds them in the mounting component's rendered
//! tree, but the component's own render knows nothing of them. When the
//! component renders again by itself (a dirty refresh, which does not render
//! the mounting component again), its fresh root replaces the decorated one in
//! the live tree; the root being replaced hands its decoration to the fresh
//! one there, so a lookup by the outer id still finds the component. The
//! mounting component's cached output keeps the decoration on its placeholder
//! for the component (`component_output.rs`), and hands it to the root it is
//! put together with.

use std::sync::Arc;

use crate::node::Node;

/// A decoration that can be put on a node again, on a component's fresh root.
pub(crate) type NodeDecoration = Arc<dyn Fn(Node) -> Node + Send + Sync>;

/// The decoration put on a component's root after `mount` returned it. The
/// outer id and key replace the component's own; the outer classes are
/// appended to its own; the decorations run on it in the order they were put.
#[derive(Clone, Default)]
pub(crate) struct OuterDecoration {
  id: Option<Arc<str>>,
  key: Option<Arc<str>>,
  classes: Vec<Arc<str>>,
  decorations: Vec<NodeDecoration>,
}

impl Node {
  /// The decoration record of a component's root (a node `mount` returned),
  /// created on first use; `None` for any other node, which records nothing.
  fn outer_decoration_mut(&mut self) -> Option<&mut OuterDecoration> {
    self.component_slot_id?;
    Some(self.outer_decoration.get_or_insert_with(Box::default))
  }

  /// Records an id put on a component's root by the code that mounted it.
  pub(crate) fn record_outer_id(&mut self, id: &Arc<str>) {
    if let Some(outer) = self.outer_decoration_mut() {
      outer.id = Some(id.clone());
    }
  }

  /// Records a key put on a component's root by the code that mounted it.
  pub(crate) fn record_outer_key(&mut self, key: &Arc<str>) {
    if let Some(outer) = self.outer_decoration_mut() {
      outer.key = Some(key.clone());
    }
  }

  /// Records a class put on a component's root by the code that mounted it.
  pub(crate) fn record_outer_class(&mut self, class: &Arc<str>) {
    if let Some(outer) = self.outer_decoration_mut()
      && !outer.classes.contains(class)
    {
      outer.classes.push(class.clone());
    }
  }

  /// Records a decoration put on a component's root by a wrapper of it.
  pub(crate) fn record_outer_decoration(&mut self, decorate: &NodeDecoration) {
    if let Some(outer) = self.outer_decoration_mut() {
      outer.decorations.push(decorate.clone());
    }
  }

  /// Puts the decoration of `replaced`, the decorated root this node replaces
  /// after its component rendered again by itself, on this node, and keeps
  /// it recorded for the component's next render.
  pub(crate) fn adopt_outer_decoration(&mut self, replaced: &Node) {
    let Some(outer) = replaced.outer_decoration.as_deref() else {
      return;
    };
    if let Some(id) = &outer.id {
      self.element_id = Some(id.clone());
    }
    if let Some(key) = &outer.key {
      self.component_key = Some(key.clone());
    }
    for class in &outer.classes {
      self.push_class(class.clone());
    }
    for decorate in &outer.decorations {
      *self = decorate(std::mem::take(self));
    }
    self.outer_decoration = Some(Box::new(outer.clone()));
  }
}
