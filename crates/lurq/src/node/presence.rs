//! The keyframe animations of a node, and how the content of a layer leaves.
//!
//! A node plays its animation from when it mounts: that is how the content
//! of an `Overlay`, a `Popup` or a `Modal` enters. A node can also name the
//! animation it leaves with. When a layer closes, the tree keeps what the
//! layer showed for as long as the longest exit animation in it lasts
//! (`app/runtime/leaving_layers.rs`): each node with one plays it, and the
//! rest stay as they were. A layer with no exit animation in it is gone in
//! the pass that closes it, as before.
//!
//! A child of a container with presence (`.presence()`) leaves the same way.
//! A render that no longer has the child among the container's keeps the
//! child's node where it was (`Node::take_leaving_children`, in the walk that
//! carries node ids over), in flow, until its exit animation ends. Only a
//! child the container can tell from its siblings is kept, one with a key or
//! an id: children without one are matched by their place, and a place whose
//! content changed and a place that was removed look the same.

use std::{
  cell::RefCell,
  sync::atomic::{AtomicBool, Ordering},
  time::Duration,
};

use super::node::{HitTestBehavior, Node, SyntheticNodeRole};
use crate::{
  animation::{Animation, AnimationFillMode, AnimationIterationCount},
  core::NodeId,
};

/// Whether any node of the process was ever given an exit animation. Until one is, reconciling a tree has
/// no leaving child to look for.
static EXIT_ANIMATIONS_IN_USE: AtomicBool = AtomicBool::new(false);

fn exit_animations_in_use() -> bool {
  EXIT_ANIMATIONS_IN_USE.load(Ordering::Relaxed)
}

thread_local! {
  /// The children that reconciling a tree found removed and kept, until the tree that reconciled takes
  /// them (`take_began_leaving`) to start their exit animations. A tree reconciles and takes on one thread,
  /// one after the other.
  static BEGAN_LEAVING: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
}

/// The children kept since the last call: see [`Node::take_leaving_children`].
pub(crate) fn take_began_leaving() -> Vec<NodeId> {
  // A process with no exit animation has kept nothing, and does not pay for the thread's list.
  if !exit_animations_in_use() {
    return Vec::new();
  }
  BEGAN_LEAVING.with(|began| std::mem::take(&mut *began.borrow_mut()))
}

/// The animation a node plays, and the one it leaves with.
#[derive(Clone)]
pub(crate) struct NodeAnimations {
  pub(crate) play: Option<Animation>,
  pub(crate) exit: Option<Animation>,
}

impl Node {
  pub(crate) fn set_animation(&mut self, spec: Animation) {
    self.animations_mut().play = Some(spec);
    self.layout_cache.invalidate();
  }

  pub(crate) fn set_exit_animation(&mut self, spec: Animation) {
    self.animations_mut().exit = Some(spec);
    EXIT_ANIMATIONS_IN_USE.store(true, Ordering::Relaxed);
  }

  /// Whether the container keeps a child that is removed from it until the child's exit animation ends:
  /// `.presence()`.
  pub(crate) fn keeps_leaving_children(&self) -> bool {
    self.rare_style.presence
  }

  /// A child a container with presence keeps when it is removed: one the container can tell from its
  /// siblings, by a key or an id, that has an exit animation of its own.
  fn leaves_with_an_animation(&self) -> bool {
    (self.component_key.is_some() || self.element_id.is_some())
      && self
        .animation
        .as_deref()
        .is_some_and(|animations| animations.exit.is_some())
  }

  /// Takes out of this node, the one a render replaces, the children that go on being drawn in `new`, the
  /// node that replaces it: the ones already leaving, and, where `new` has presence, the ones that leave
  /// with an animation and are no longer among its children. Each comes with the place it had. The second
  /// kind is noted for the tree to start their exit animations (`take_began_leaving`).
  pub(crate) fn take_leaving_children(&mut self, new: &Node) -> Vec<(usize, Node)> {
    if !exit_animations_in_use() {
      return Vec::new();
    }
    let keeps = new.keeps_leaving_children();
    let leaves = |child: &Node| {
      child.is_leaving_layer()
        || (keeps
          && child.leaves_with_an_animation()
          && !new
            .children
            .iter()
            .any(|kept| kept.component_key == child.component_key && kept.element_id == child.element_id))
    };
    if !self.children.iter().any(leaves) {
      return Vec::new();
    }
    let mut taken = Vec::new();
    let mut staying = Vec::with_capacity(self.children.len());
    for (index, child) in std::mem::take(&mut self.children).into_iter().enumerate() {
      if !leaves(&child) {
        staying.push(child);
        continue;
      }
      if !child.is_leaving_layer() {
        BEGAN_LEAVING.with(|began| began.borrow_mut().push(child.node_id));
      }
      taken.push((index, child));
    }
    self.children = staying;
    taken
  }

  /// Puts the children [`Self::take_leaving_children`] took back among this node's, each at the place it
  /// had, or at the end when fewer children are left.
  pub(crate) fn keep_leaving_children(&mut self, leaving: Vec<(usize, Node)>) {
    if leaving.is_empty() {
      return;
    }
    for (index, child) in leaving {
      self.children.insert(index.min(self.children.len()), child);
    }
    self.layout_cache.invalidate();
  }

  /// Whether a child somewhere under this node is leaving.
  pub(crate) fn holds_leaving_child(&self) -> bool {
    exit_animations_in_use()
      && self
        .children
        .iter()
        .any(|child| child.is_leaving_layer() || child.holds_leaving_child())
  }

  /// The children of this node, the one a render replaces, that the render's children are matched with:
  /// all of them but the ones already leaving, which no child continues. `None` where none is leaving.
  pub(crate) fn children_to_match(&self) -> Option<Vec<&Node>> {
    (exit_animations_in_use() && self.children.iter().any(Node::is_leaving_layer))
      .then(|| self.children.iter().filter(|child| !child.is_leaving_layer()).collect())
  }

  fn animations_mut(&mut self) -> &mut NodeAnimations {
    self
      .animation
      .get_or_insert_with(|| Box::new(NodeAnimations { play: None, exit: None }))
  }

  /// The animation the node plays now.
  pub(crate) fn playing_animation(&self) -> Option<&Animation> {
    self.animation.as_deref()?.play.as_ref()
  }

  /// Whether the node or one below it plays a keyframe animation or leaves
  /// with one.
  pub(crate) fn has_keyframe_animation(&self) -> bool {
    self.animation.is_some() || self.children.iter().any(Node::has_keyframe_animation)
  }

  /// Whether the node or one below it has an exit animation.
  pub(crate) fn has_exit_animation(&self) -> bool {
    self
      .animation
      .as_deref()
      .is_some_and(|animations| animations.exit.is_some())
      || self.children.iter().any(Node::has_exit_animation)
  }

  /// Turns the subtree to its exit animations: each node that has one plays
  /// it from its start in place of what it played, and `restarted` is told of
  /// the node, whose runs the animation engine forgets. Returns how long the
  /// longest of them takes, `None` when the subtree has none that takes any
  /// time.
  pub(crate) fn begin_exit(&mut self, restarted: &mut dyn FnMut(NodeId)) -> Option<Duration> {
    let mut longest = None;
    if let Some(animations) = self.animation.as_deref_mut()
      && let Some(mut exit) = animations.exit.take()
    {
      // Shorter exits of the layer hold their last pose until the longest ends.
      exit.fill_mode = match exit.fill_mode {
        AnimationFillMode::None | AnimationFillMode::Forwards => AnimationFillMode::Forwards,
        AnimationFillMode::Backwards | AnimationFillMode::Both => AnimationFillMode::Both,
      };
      if exit.iteration_count == AnimationIterationCount::Infinite {
        exit.iteration_count = AnimationIterationCount::Count(1.0);
      }
      longest = Some(exit.total_time());
      animations.play = Some(exit);
      restarted(self.node_id);
      self.layout_cache.invalidate();
    }
    for child in &mut self.children {
      longest = longest.max(child.begin_exit(restarted));
    }
    longest.filter(|time| !time.is_zero())
  }

  /// Makes the root of a layer that is leaving something the tree only
  /// paints: no pointer reaches it, Tab passes it by, it is no modal, no
  /// lookup by id or by element ref finds what is in it, and no component
  /// that renders again puts its output there.
  pub(crate) fn turn_into_leaving_layer(&mut self) {
    self.set_synthetic_role(SyntheticNodeRole::LeavingLayer);
    self.hit_test = HitTestBehavior::None;
    self.forget_names();
  }

  fn forget_names(&mut self) {
    self.element_id = None;
    self.component_key = None;
    // The component that rendered this may still be mounted, for the layer when it opens again; what it renders
    // next is not this layer's.
    self.component_slot_id = None;
    self.outer_decoration = None;
    // A canvas draws through its ref, which gets the canvas back when the layer is dropped.
    if !self.draws_canvas() {
      self.element_ref = None;
    }
    for child in &mut self.children {
      child.forget_names();
    }
  }

  fn draws_canvas(&self) -> bool {
    #[cfg(feature = "canvas")]
    {
      matches!(self.node_kind(), super::node_kind::NodeKind::Canvas { .. })
    }
    #[cfg(not(feature = "canvas"))]
    {
      false
    }
  }

  /// A layer the tree keeps on screen while its exit animations play.
  pub(crate) fn is_leaving_layer(&self) -> bool {
    self.has_synthetic_role(SyntheticNodeRole::LeavingLayer)
  }
}
