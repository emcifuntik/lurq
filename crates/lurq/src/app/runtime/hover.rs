//! Hover under a pointer that holds still. A scroll, a list that grows or any
//! other relayout moves content under the pointer without a mouse event, so
//! after each layout the hover is hit-tested again at the pointer's last
//! position, like the web does: the node that moved away gets its
//! `on_mouse_leave` (and with it, its tooltip goes), the node that moved under
//! the pointer its `on_mouse_enter`. They run with the source of the input
//! that last put the pointer there, so an agent's pointer stays synthetic.
//!
//! A hover style that moves its own node away from the pointer (a `hovered`
//! size or offset with a transition) unhovers it, which moves it back under
//! the pointer, and so on: like CSS `:hover`, such a node flickers.
//!
//! A hover that shows nothing asks for no frame. The hover moves whenever the
//! pointer comes onto another node, a text inside its row say, and most nodes
//! look the same under the pointer: a frame for each of those moves draws the
//! pixels of the frame before it. One is asked for when a node that gained or
//! lost the hover is drawn differently for it or ran a handler for it (see
//! [`set_hover`]), and for every change while DevTools shows the tree.

use super::{Tree, find_node_by_id, hovered_cursor, scrollbar_hit_at, set_node_hovered, trim_hits_to_scrollbar};
use crate::{
  app::{
    events::{InputSource, input_source, with_input_source},
    hit_test::{HitRect, hit_test_tree},
  },
  core::NodeId,
  node::{Node, VoidEventHandler},
};

/// Where the pointer last was, in physical pixels, and where the input that
/// put it there came from.
#[derive(Clone, Copy)]
pub(super) struct PointerAt {
  x: f32,
  y: f32,
  source: InputSource,
}

/// What a move of the hover did.
#[derive(Clone, Copy, Default)]
pub(super) struct HoverMove {
  /// A node gained or lost the hover.
  pub(super) changed: bool,
  /// One of those nodes is drawn differently for it, or ran a handler for it.
  pub(super) shows: bool,
}

impl Tree {
  /// Remembers where the pointer is, in physical pixels, for
  /// [`Self::refresh_hover_at_pointer`]. Called by the public pointer entry
  /// points only: a click held back for a double click is delivered later at
  /// its own position, which is no longer where the pointer is.
  pub(super) fn track_pointer(&mut self, x: f32, y: f32) {
    self.pointer = Some(PointerAt {
      x,
      y,
      source: input_source(),
    });
  }

  /// Hit-tests the hover again at the pointer's last position against the
  /// last layout. Does nothing while the pointer is outside the window or a
  /// drag holds it. Returns whether the hovered nodes changed.
  pub(super) fn refresh_hover_at_pointer(&mut self) -> bool {
    let Some(PointerAt { x, y, source }) = self.pointer else {
      return false;
    };
    if self.has_active_input_interaction() {
      return false;
    }
    let scale = self.scale_factor();
    let (lx, ly) = (x / scale, y / scale);
    let (Some(root), Some(result)) = (&self.root, &self.last_layout) else {
      return false;
    };
    let mut hits = Vec::new();
    hit_test_tree(root, result, 0.0, 0.0, lx, ly, &mut hits);
    trim_hits_to_scrollbar(&mut hits, lx, ly);

    let moved = with_input_source(source, || move_hover(root, &self.hover_path, &hits));
    let mut thumb_changed = false;
    for (node, _) in &hits {
      if let Some((state, _)) = node.scroll_container() {
        let on_thumb = scrollbar_hit_at(node, lx, ly).is_some();
        if on_thumb != state.is_thumb_hovered() {
          state.set_thumb_hovered(on_thumb);
          thumb_changed = true;
        }
      }
    }
    self.hover_path = hits.iter().map(|(node, _)| node.node_id()).collect();
    self.cursor = hovered_cursor(hits.iter().map(|(node, _)| *node));
    let draws_hover = self.draws_hover(moved);
    if draws_hover {
      self.cached_render_list = None;
    }
    if draws_hover || thumb_changed {
      self.needs_redraw = true;
    }
    moved.changed
  }

  /// Whether a move of the hover needs a frame: it shows, or DevTools is open
  /// on the tree, whose element tree and inspector show which nodes are hovered
  /// and are brought up to date after a frame.
  pub(super) fn draws_hover(&self, moved: HoverMove) -> bool {
    #[cfg(feature = "devtools")]
    if moved.changed && self.devtools_is_open() {
      return true;
    }
    moved.shows
  }
}

/// Moves the hover from the nodes in `hover_path` to the nodes in `hits`: a
/// node the pointer left is unhovered and gets its `on_mouse_leave`, then a
/// node it came onto is hovered and gets its `on_mouse_enter`.
pub(super) fn move_hover(root: &Node, hover_path: &[NodeId], hits: &[(&Node, HitRect)]) -> HoverMove {
  let mut moved = HoverMove::default();
  for old_id in hover_path {
    if hits.iter().any(|(node, _)| node.node_id() == *old_id) {
      continue;
    }
    let Some(node) = find_node_by_id(root, *old_id) else {
      continue;
    };
    moved.changed = true;
    moved.shows |= leave(node);
  }
  for (node, _) in hits {
    if hover_path.contains(&node.node_id()) {
      continue;
    }
    moved.changed = true;
    moved.shows |= set_hover(node, true, &node.events.on_mouse_enter);
  }
  moved
}

/// Takes the hover from `node` and runs its `on_mouse_leave`. Returns whether
/// that needs a frame.
pub(super) fn leave(node: &Node) -> bool {
  set_hover(node, false, &node.events.on_mouse_leave)
}

/// Sets `node`'s hover and runs the `handlers` of that change. Returns whether
/// it needs a frame:
///
/// - the node is drawn differently under the pointer ([`Node::shows_hover`]);
/// - one of its state styles sizes it, so the change dirtied its layout;
/// - a handler ran. What a handler changes mostly marks the tree by itself (a
///   signal, a scroll state, a canvas), but it may also reach what is drawn
///   through something that marks nothing, so its run is drawn whatever it did.
///
/// Nothing else of a hover is drawn. The cursor is the shell's, which reads
/// [`Tree::cursor`] after each pointer event. The hover an app reads from an
/// element ref or an interaction state is no signal: it renders no component,
/// so a frame for it would draw the frame before again.
fn set_hover(node: &Node, hovered: bool, handlers: &[VoidEventHandler]) -> bool {
  let relaid = set_node_hovered(node, hovered);
  for handler in handlers {
    handler.call();
  }
  relaid || node.shows_hover() || !handlers.is_empty()
}
