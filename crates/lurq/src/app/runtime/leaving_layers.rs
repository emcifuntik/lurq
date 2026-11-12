//! Layers that closed and are still on screen while their exit animations
//! play (`node/presence.rs`).
//!
//! The overlay host is built again in every pass from the layers the layout
//! declares, and a layer no declaration continues is dropped. One with an
//! exit animation in it is kept instead, as it was when it closed: the host
//! holds its nodes at the place the layer had among the others, the
//! animations play on them, and the pass that comes when the longest has
//! ended drops it. The app does not render it any more, so it shows what it
//! showed when it closed.
//!
//! A leaving layer is only painted. Its root takes no pointer and has the
//! `LeavingLayer` role, so it is no modal and Tab passes it by; focus, hover
//! and a press that were in it are let go when it begins to leave; its nodes
//! lose their ids and element refs, so no lookup finds them. It is not taken
//! over either: a layer opened again while it leaves is a new one, which
//! enters over it.
//!
//! A layer hidden because its anchor went out of view does not leave: it is
//! still open, and comes back with its anchor.

use std::time::Instant;

use super::{
  OverlayHostReuse, Tree, find_node_by_id, overlay_layers::OverlayLayers, reset_element_ref_flags_recursive,
};
use crate::{core::NodeId, node::Node};

/// A layer the overlay host keeps while its exit animations play.
pub(super) struct LeavingLayer {
  /// The id of the layer's root, a child of the overlay host.
  root: NodeId,
  /// When the longest of its exit animations ends and the layer is dropped.
  pub(super) until: Instant,
}

impl Tree {
  /// The layers the host draws on their way out in the pass at `now`, each
  /// with the place it had among the old host's layers, in that order: the
  /// ones that were leaving and still are, and the old layers no layer of
  /// this pass continues that have an exit animation, which begin to leave.
  /// The layers whose time is up are left in `old_parts` to be dropped.
  pub(super) fn leaving_layers(
    &mut self,
    old_parts: &mut OverlayHostReuse,
    layers: &OverlayLayers,
    now: Instant,
  ) -> Vec<(usize, Node)> {
    let mut records = Vec::new();
    let mut leaving = Vec::new();
    for (position, layer) in std::mem::take(&mut old_parts.leaving) {
      let record = self.leaving_layers.iter().find(|record| record.root == layer.node_id());
      match record {
        Some(record) if now < record.until => {
          records.push(LeavingLayer {
            root: record.root,
            until: record.until,
          });
          leaving.push((position, layer));
        }
        _ => old_parts.dropped.push(layer),
      }
    }

    for index in 0..old_parts.old_overlays.len() {
      if !old_parts.is_free(index) || !old_parts.old_overlays[index].has_exit_animation() {
        continue;
      }
      let position = old_parts.old_overlay_positions[index];
      let hidden = self
        .overlay_layer_anchors
        .get(position)
        .and_then(Option::as_ref)
        .is_some_and(|anchor| layers.is_hidden(anchor));
      if hidden {
        continue;
      }
      let layer = &mut old_parts.old_overlays[index];
      let animations = &mut self.animation_engine;
      let until = layer
        .begin_exit(&mut |node| animations.forget_node(node))
        .and_then(|lasts| now.checked_add(lasts));
      let Some(until) = until else {
        continue;
      };
      let mut layer = std::mem::take(layer);
      old_parts.old_overlay_used[index] = true;
      reset_element_ref_flags_recursive(&layer);
      layer.turn_into_leaving_layer();
      records.push(LeavingLayer {
        root: layer.node_id(),
        until,
      });
      self.layers_left.push(layer.node_id());
      leaving.push((position, layer));
    }

    self.leaving_layers = records;
    leaving.sort_by_key(|(position, _)| *position);
    leaving
  }

  /// Whether a leaving layer's time is up at `now`: the pass drops it.
  pub(super) fn leaving_layer_due(&self, now: Instant) -> bool {
    self.leaving_layers.iter().any(|layer| now >= layer.until)
  }

  /// Lets go of the focus, the hover and the press that are in a layer that
  /// began to leave in this pass: what is only painted holds none of them.
  pub(super) fn release_input_in_leaving_layers(&mut self) {
    let left = std::mem::take(&mut self.layers_left);
    if left.is_empty() {
      return;
    }
    let Some(root) = self.root.as_ref() else {
      return;
    };
    let inside = |id: &NodeId| {
      root
        .children()
        .iter()
        .filter(|layer| left.contains(&layer.node_id()))
        .any(|layer| find_node_by_id(layer, *id).is_some())
    };
    let loses_focus = self.focused_node.iter().chain(&self.focused_event_node).any(inside);
    let hover_path = self.hover_path.iter().copied().filter(|id| !inside(id)).collect();
    let active_path = self.active_path.iter().copied().filter(|id| !inside(id)).collect();
    self.hover_path = hover_path;
    self.active_path = active_path;
    if loses_focus {
      self.blur_focus();
    }
  }
}

/// The host's layers in the order it stacks them: `overlays`, the open ones,
/// with each of `leaving` back at the place it had among the old host's
/// layers, or on top when fewer layers are left. What `layers` knows of each
/// open layer by its place (its anchor, the node that declares it, which
/// layer a press or Escape dismisses) follows it to its new place.
pub(super) fn stack_layers(overlays: Vec<Node>, leaving: Vec<(usize, Node)>, layers: &mut OverlayLayers) -> Vec<Node> {
  if leaving.is_empty() {
    return overlays;
  }
  let leaving_count = leaving.len();
  let mut stack = Vec::with_capacity(overlays.len() + leaving_count);
  let mut anchors = Vec::with_capacity(stack.capacity());
  let mut origins = Vec::with_capacity(stack.capacity());
  // Where each open layer is in the stack.
  let mut places = Vec::with_capacity(overlays.len());
  let mut leaving = leaving.into_iter().peekable();
  let mut open = overlays
    .into_iter()
    .zip(std::mem::take(&mut layers.anchors))
    .zip(std::mem::take(&mut layers.origins))
    .peekable();
  loop {
    let leaving_is_next = match (leaving.peek(), open.peek()) {
      (Some((position, _)), Some(_)) => *position <= stack.len(),
      (Some(_), None) => true,
      (None, Some(_)) => false,
      (None, None) => break,
    };
    if leaving_is_next {
      let (_, layer) = leaving.next().expect("peeked");
      stack.push(layer);
      anchors.push(None);
      origins.push(None);
    } else {
      let ((layer, anchor), origin) = open.next().expect("peeked");
      places.push(stack.len());
      stack.push(layer);
      anchors.push(anchor);
      origins.push(origin);
    }
  }
  layers.anchors = anchors;
  layers.origins = origins;
  // An entry's layer counts the host's children, the page first. A hidden layer's is where it would be.
  for entry in &mut layers.dismiss_entries {
    entry.layer = match entry.layer.checked_sub(1).and_then(|place| places.get(place)) {
      Some(place) => place + 1,
      None => entry.layer + leaving_count,
    };
  }
  stack
}
