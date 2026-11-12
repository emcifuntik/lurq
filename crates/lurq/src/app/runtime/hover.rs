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

use super::{
  Tree, find_node_by_id, hovered_cursor, scrollbar_hit_at, set_node_hovered, take_disabled_hit, trim_hits_to_scrollbar,
};
use crate::{
  app::{
    events::{InputSource, input_source, with_input_source},
    hit_test::{HitRect, hit_test_tree},
  },
  core::NodeId,
  node::Node,
};

/// Where the pointer last was, in physical pixels, and where the input that
/// put it there came from.
#[derive(Clone, Copy)]
pub(super) struct PointerAt {
  x: f32,
  y: f32,
  source: InputSource,
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
    // A disabled element is not hovered; the elements it is in are.
    let disabled = take_disabled_hit(&mut hits);
    trim_hits_to_scrollbar(&mut hits, lx, ly);

    let changed = with_input_source(source, || move_hover(root, &self.hover_path, &hits));
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
    self.cursor = hovered_cursor(disabled.into_iter().chain(hits.iter().map(|(node, _)| *node)));
    if changed {
      self.cached_render_list = None;
    }
    if changed || thumb_changed {
      self.needs_redraw = true;
    }
    changed
  }
}

/// Moves the hover from the nodes in `hover_path` to the nodes in `hits`: a
/// node the pointer left is unhovered and gets its `on_mouse_leave`, then a
/// node it came onto is hovered and gets its `on_mouse_enter`. Returns
/// whether any node changed.
pub(super) fn move_hover(root: &Node, hover_path: &[NodeId], hits: &[(&Node, HitRect)]) -> bool {
  let mut changed = false;
  for old_id in hover_path {
    if hits.iter().any(|(node, _)| node.node_id() == *old_id) {
      continue;
    }
    let Some(node) = find_node_by_id(root, *old_id) else {
      continue;
    };
    set_node_hovered(node, false);
    changed = true;
    for handler in &node.events.on_mouse_leave {
      handler.call();
    }
  }
  for (node, _) in hits {
    if hover_path.contains(&node.node_id()) {
      continue;
    }
    set_node_hovered(node, true);
    changed = true;
    for handler in &node.events.on_mouse_enter {
      handler.call();
    }
  }
  changed
}
