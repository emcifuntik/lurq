#[cfg(feature = "devtools")]
use crate::app::ctx::{
  ComponentContextDebug, ComponentEffectDebug, ComponentMemoDebug, ComponentSignalDebug, DevtoolsInspectableDebug,
};
use crate::node::{color::Color, node::Node};

/// A built element tree. The root node lives on the heap, so an `Element` is
/// one pointer wide: render functions and the structs that hold elements
/// (window chrome, title bars, modals) move pointers, not nodes (lurq#25).
pub struct Element {
  pub(crate) node: Box<Node>,
}

impl Clone for Element {
  fn clone(&self) -> Self {
    Self::from_node(self.node.clone_for_reuse())
  }
}

#[derive(Clone, Copy)]
pub struct ElementRef<'a> {
  pub(crate) node: &'a Node,
}

pub struct ElementChildren<'a> {
  nodes: &'a [Node],
}

pub struct ElementIter<'a> {
  inner: std::slice::Iter<'a, Node>,
}

/// The boxes [`Element::into_node`] moved a node out of, for the next element
/// to take. Building a tree boxes every node and its parent moves the node out
/// of the box right away, so a build allocated and freed a box per node; with
/// these, a flat list reuses one box and a nested tree about one per level.
/// Per thread, and only a few kept: a box is over 1 KiB.
mod node_boxes {
  use std::{cell::RefCell, mem::MaybeUninit};

  use crate::node::node::Node;

  const KEPT: usize = 16;

  thread_local! {
    // The boxes themselves are kept: each goes back out as an element's box.
    #[allow(clippy::vec_box)]
    static EMPTY: RefCell<Vec<Box<MaybeUninit<Node>>>> = const { RefCell::new(Vec::new()) };
  }

  /// `Box::new(node)`, in a box an element gave back when there is one.
  pub(crate) fn boxed(node: Node) -> Box<Node> {
    // `try_with`: a thread's own destructors may still build elements after
    // its boxes are gone.
    let empty = EMPTY
      .try_with(|empty| empty.try_borrow_mut().ok().and_then(|mut empty| empty.pop()))
      .ok()
      .flatten();
    match empty {
      Some(empty) => Box::write(empty, node),
      None => Box::new(node),
    }
  }

  /// `*node`, keeping the emptied box for [`boxed`].
  pub(crate) fn unboxed(node: Box<Node>) -> Node {
    let raw = Box::into_raw(node);
    // SAFETY: `raw` comes from a live `Box<Node>`, so it holds a valid node,
    // read once here; the box is then owned again as uninitialised memory of
    // the same layout, so neither the node nor the box is dropped twice.
    let (node, empty) = unsafe { (raw.read(), Box::from_raw(raw.cast::<MaybeUninit<Node>>())) };
    let _ = EMPTY.try_with(|kept| {
      if let Ok(mut kept) = kept.try_borrow_mut()
        && kept.len() < KEPT
      {
        if kept.capacity() == 0 {
          kept.reserve_exact(KEPT);
        }
        kept.push(empty);
      }
    });
    node
  }
}

pub(crate) use node_boxes::boxed as boxed_node;

impl Element {
  pub(crate) fn from_node(node: Node) -> Self {
    Self {
      node: node_boxes::boxed(node),
    }
  }

  pub(crate) fn from_boxed_node(node: Box<Node>) -> Self {
    Self { node }
  }

  /// Rebuilds the root node with a by-value `Node` builder chain.
  pub(crate) fn map_node(self, f: impl FnOnce(Node) -> Node) -> Self {
    Self::from_node(f(self.into_node()))
  }

  /// [`Self::map_node`] with a decoration that can run again: on a
  /// component's root (an element `Ctx::mount` returned) it is also recorded,
  /// and put on the component's fresh root when the component renders again
  /// by itself. For what a wrapper (`Draggable`, `DropZone`) puts on the root
  /// of the child it is given.
  pub(crate) fn decorate_root(mut self, decorate: impl Fn(Node) -> Node + Send + Sync + 'static) -> Self {
    let decorate: crate::node::outer_decoration::NodeDecoration = std::sync::Arc::new(decorate);
    self.node.record_outer_decoration(&decorate);
    self.map_node(|node| decorate(node))
  }

  /// Moves the root node out of its box, for storing it among a parent's
  /// children; the box goes to the next element built on this thread.
  pub(crate) fn into_node(self) -> Node {
    node_boxes::unboxed(self.node)
  }

  pub fn new() -> Self {
    Self::from_node(Node::new())
  }

  pub fn node_id(&self) -> crate::core::NodeId {
    self.node.node_id()
  }

  /// On an element `Ctx::mount` returned, the key, id and classes set here
  /// stay on the component's root when the component renders again by itself.
  pub fn key(mut self, key: impl Into<std::sync::Arc<str>>) -> Self {
    let key = key.into();
    self.node.record_outer_key(&key);
    self.node.set_component_key(Some(&key));
    self
  }

  /// HTML-like `id` attribute for `Tree::get_element_by_id` lookup.
  /// Also participates in retained-node identity across sibling changes.
  /// Keep IDs stable; changing one can reset the node's runtime state.
  ///
  /// On an element `Ctx::mount` returned, the id replaces the component's own
  /// on its root, also when the component renders again by itself.
  pub fn id(mut self, id: impl Into<std::sync::Arc<str>>) -> Self {
    let id = id.into();
    self.node.record_outer_id(&id);
    crate::node::NodeUpdate::id(&mut *self.node, id);
    self
  }

  /// Appends an HTML-like class for `Tree::get_elements_by_class_name`
  /// lookup. Classes do not affect reconciliation or styling.
  ///
  /// On an element `Ctx::mount` returned, the class is appended to the
  /// component's own on its root, also when the component renders again by
  /// itself.
  pub fn class(mut self, class: impl Into<std::sync::Arc<str>>) -> Self {
    let class = class.into();
    self.node.record_outer_class(&class);
    crate::node::NodeUpdate::class(&mut *self.node, class);
    self
  }

  /// Appends several classes at once; see [`Element::class`].
  pub fn classes<C: Into<std::sync::Arc<str>>>(mut self, classes: impl IntoIterator<Item = C>) -> Self {
    for class in classes {
      self = self.class(class);
    }
    self
  }
}

impl Default for Element {
  fn default() -> Self {
    Self::new()
  }
}

impl<'a> ElementRef<'a> {
  /// Returns an owned drawing capability for an attached canvas.
  #[cfg(feature = "canvas")]
  pub fn as_canvas(&self) -> Option<crate::canvas::CanvasHandle> {
    self.node.canvas_handle().filter(|canvas| canvas.is_attached())
  }

  pub(crate) fn new(node: &'a Node) -> Self {
    Self { node }
  }

  pub fn node_id(&self) -> crate::core::NodeId {
    self.node.node_id()
  }

  pub fn tag_name(&self) -> &'a str {
    self.node.tag_name()
  }

  /// The author-supplied HTML-like `id` attribute, if any.
  pub fn id(&self) -> Option<&'a str> {
    self.node.element_id.as_deref()
  }

  /// The author-supplied HTML-like classes, in insertion order.
  pub fn classes(&self) -> impl Iterator<Item = &'a str> + use<'a> {
    self.node.class_list().iter().map(|class| class.as_ref())
  }

  pub fn has_class(&self, class: &str) -> bool {
    self.node.has_class(class)
  }

  #[allow(dead_code)]
  pub(crate) fn component_key(&self) -> Option<&'a str> {
    self.node.component_key()
  }

  pub fn text_content(&self) -> Option<&'a str> {
    self.node.text_content()
  }

  pub fn color(&self) -> Option<Color> {
    self.node.color()
  }

  #[cfg(feature = "devtools")]
  #[allow(dead_code)]
  pub(crate) fn component_props_debug(&self) -> Option<&DevtoolsInspectableDebug> {
    self.node.component_props_debug()
  }

  #[cfg(feature = "devtools")]
  #[allow(dead_code)]
  pub(crate) fn component_signals_debug(&self) -> &[ComponentSignalDebug] {
    self.node.component_signals_debug()
  }

  #[cfg(feature = "devtools")]
  #[allow(dead_code)]
  pub(crate) fn component_memos_debug(&self) -> &[ComponentMemoDebug] {
    self.node.component_memos_debug()
  }

  #[cfg(feature = "devtools")]
  #[allow(dead_code)]
  pub(crate) fn component_effects_debug(&self) -> &[ComponentEffectDebug] {
    self.node.component_effects_debug()
  }

  #[cfg(feature = "devtools")]
  #[allow(dead_code)]
  pub(crate) fn component_contexts_debug(&self) -> &[ComponentContextDebug] {
    self.node.component_contexts_debug()
  }

  #[cfg(feature = "devtools")]
  #[allow(dead_code)]
  pub(crate) fn debug_attrs(&self) -> &[(std::sync::Arc<str>, std::sync::Arc<str>)] {
    self.node.debug_attrs()
  }

  pub fn children(&self) -> ElementChildren<'a> {
    ElementChildren {
      nodes: self.node.children(),
    }
  }
}

impl<'a> ElementChildren<'a> {
  pub fn len(&self) -> usize {
    self.nodes.len()
  }

  pub fn is_empty(&self) -> bool {
    self.nodes.is_empty()
  }

  pub fn iter(&self) -> ElementIter<'a> {
    ElementIter {
      inner: self.nodes.iter(),
    }
  }
}

impl<'a> IntoIterator for ElementChildren<'a> {
  type Item = ElementRef<'a>;
  type IntoIter = ElementIter<'a>;

  fn into_iter(self) -> Self::IntoIter {
    ElementIter {
      inner: self.nodes.iter(),
    }
  }
}

impl<'a> Iterator for ElementIter<'a> {
  type Item = ElementRef<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    self.inner.next().map(ElementRef::new)
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    self.inner.size_hint()
  }
}

impl ExactSizeIterator for ElementIter<'_> {}

#[cfg(test)]
mod tests {
  use crate::{
    components::{Column, Rect, Text},
    node::Element,
  };

  #[test]
  fn element_builders_create_node_tree() {
    let node = Element::from(
      Column::new()
        .spacing(8.0)
        .child(Text::new("hello"))
        .child(Rect::new(10.0, 20.0).rounded(4.0)),
    )
    .node;

    assert_eq!(node.children().len(), 2);
  }

  /// The box a node was moved out of holds the next element built, and a
  /// node moved into one is the node that was built.
  #[test]
  fn an_element_reuses_the_box_its_predecessor_was_moved_out_of() {
    let first = Element::from(Rect::new(10.0, 20.0));
    let first_box = std::ptr::from_ref(&*first.node);
    let node = first.into_node();
    assert_eq!(node.frame.width, Some(crate::node::dimension::Dimension::Px(10.0)));

    let second = Element::from(Rect::new(30.0, 40.0).id("second"));
    assert!(std::ptr::eq(&*second.node, first_box));
    assert_eq!(second.node.element_id(), Some("second"));
    assert_eq!(
      second.node.frame.width,
      Some(crate::node::dimension::Dimension::Px(30.0))
    );
  }
}
