//! The keyframe animations of a node, and how the content of a layer leaves.
//!
//! A node plays its animation from when it mounts: that is how the content
//! of an `Overlay`, a `Popup` or a `Modal` enters. A node can also name the
//! animation it leaves with. When a layer closes, the tree keeps what the
//! layer showed for as long as the longest exit animation in it lasts
//! (`app/runtime/leaving_layers.rs`): each node with one plays it, and the
//! rest stay as they were. A layer with no exit animation in it is gone in
//! the pass that closes it, as before.

use std::time::Duration;

use super::node::{HitTestBehavior, Node, SyntheticNodeRole};
use crate::{
  animation::{Animation, AnimationFillMode, AnimationIterationCount},
  core::NodeId,
};

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

  #[cfg(test)]
  pub(crate) fn exit_animation_value(&self) -> Option<&Animation> {
    self.animation.as_deref()?.exit.as_ref()
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
  /// paints: no pointer reaches it, Tab passes it by, it is no modal, and no
  /// lookup by id or by element ref finds what is in it.
  pub(crate) fn turn_into_leaving_layer(&mut self) {
    self.set_synthetic_role(SyntheticNodeRole::LeavingLayer);
    self.hit_test = HitTestBehavior::None;
    self.forget_names();
  }

  fn forget_names(&mut self) {
    self.element_id = None;
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
