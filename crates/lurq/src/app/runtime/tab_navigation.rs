//! Tab / Shift+Tab traversal. Revealing the newly focused node is [`super::scroll_into_view`].
//!
//! The Tab scope is the topmost open modal, or the whole window when no modal is open, and Tab wraps around at its
//! ends. Only nodes with an explicit `tab_index >= 0` are stops, except inside forms, where controls are stops without
//! one. A form is part of its scope's order like in a browser: Tab from its last control moves on to the next stop.
//!
//! An open modal traps Tab: focus behind it is never a stop, and Tab stays put when the modal has no stops.
//!
//! A child its Row/Column dropped to make room (`ShrinkLimit::Drop`) is not drawn, so nothing inside it is a stop.

use super::{FocusTarget, find_node_by_path};
use crate::{
  core::NodeId,
  layout::layout_result::LayoutResult,
  node::{Node, SyntheticNodeRole},
};

pub(super) enum TabMove {
  Focus(FocusTarget),
  /// An open modal has no stops: Tab is consumed and focus stays.
  Stay,
  /// Nothing to traverse; the key falls through to other handlers.
  Unhandled,
}

/// Sort key of a stop: positive tab indices first in ascending order, then
/// `0` (and unset form controls) in tree order.
type StopKey = (u8, i32, usize);

struct Stop {
  target: FocusTarget,
  key: StopKey,
}

struct Walk {
  focused: Option<NodeId>,
  focused_order: Option<usize>,
  visited: usize,
  stops: Vec<Stop>,
}

/// `dropped` holds the nodes the last layout dropped from their Row/Column
/// (see [`dropped_node_ids`]); nothing inside them is a stop.
pub(super) fn tab_move(root: &Node, dropped: &[NodeId], focused: Option<(NodeId, &[usize])>, reverse: bool) -> TabMove {
  let modal = top_modal_path(root);
  let focused = focused.filter(|(_, path)| modal.as_deref().is_none_or(|modal| path.starts_with(modal)));

  let scope = match &modal {
    Some(path) => find_node_by_path(root, path),
    None => Some(root),
  };
  match scope.and_then(|scope| next_stop(scope, dropped, focused.map(|(id, _)| id), reverse)) {
    Some(target) => TabMove::Focus(target),
    None if modal.is_some() => TabMove::Stay,
    None => TabMove::Unhandled,
  }
}

/// Path of the topmost open modal. Modals are direct children of the overlay
/// host, after the base tree, in stacking order. `WindowChrome`'s layer is
/// built like a modal but has no modal role, so it is never the scope.
fn top_modal_path(root: &Node) -> Option<Vec<usize>> {
  if !root.has_synthetic_role(SyntheticNodeRole::OverlayHost) {
    return None;
  }
  root
    .children()
    .iter()
    .rposition(|child| child.has_synthetic_role(SyntheticNodeRole::Modal))
    .map(|index| vec![index])
}

/// The nodes `layout` (the layout of `root`) dropped from their Row/Column,
/// outermost only: what lies inside them is dropped with them.
pub(super) fn dropped_node_ids(root: &Node, layout: &LayoutResult) -> Vec<NodeId> {
  fn collect(node: &Node, layout: &LayoutResult, dropped: &mut Vec<NodeId>) {
    if layout.dropped {
      dropped.push(node.node_id());
      return;
    }
    for (child, child_layout) in node.children().iter().zip(&layout.children) {
      collect(child, &child_layout.result, dropped);
    }
  }
  let mut dropped = Vec::new();
  collect(root, layout, &mut dropped);
  dropped
}

/// Whether the node at `path` (child indices from `root`) is one of the
/// `dropped` nodes or lies inside one.
/// Whether the node at `path`, or an element it is in, is disabled.
pub(super) fn path_is_disabled(root: &Node, path: &[usize]) -> bool {
  let mut node = root;
  if node.is_disabled() {
    return true;
  }
  for &index in path {
    let Some(child) = node.children().get(index) else {
      return false;
    };
    node = child;
    if node.is_disabled() {
      return true;
    }
  }
  false
}

pub(super) fn path_is_dropped(root: &Node, dropped: &[NodeId], path: &[usize]) -> bool {
  if dropped.is_empty() {
    return false;
  }
  let mut node = root;
  for &index in path {
    let Some(child) = node.children().get(index) else {
      return false;
    };
    node = child;
    if dropped.contains(&node.node_id()) {
      return true;
    }
  }
  false
}

fn next_stop(scope: &Node, dropped: &[NodeId], focused: Option<NodeId>, reverse: bool) -> Option<FocusTarget> {
  let mut walk = Walk {
    focused,
    focused_order: None,
    visited: 0,
    stops: Vec::new(),
  };
  collect_stops(scope, dropped, None, false, &mut walk);
  let stops = &mut walk.stops;
  if stops.is_empty() {
    return None;
  }
  stops.sort_by_key(|stop| stop.key);

  let last = stops.len() - 1;
  let current = focused.and_then(|id| stops.iter().position(|stop| stop.target.input_id == id));
  let next = match (current, walk.focused_order) {
    (Some(index), _) if reverse => index.checked_sub(1).unwrap_or(last),
    (Some(index), _) => (index + 1) % stops.len(),
    // Focus is on a node that is not a stop (a clicked button without a tab
    // index): continue from its place in tree order, like a browser.
    (None, Some(order)) => {
      let key = stop_key(0, order);
      if reverse {
        stops.iter().rposition(|stop| stop.key < key).unwrap_or(last)
      } else {
        stops.iter().position(|stop| stop.key > key).unwrap_or(0)
      }
    }
    (None, None) if reverse => last,
    (None, None) => 0,
  };
  Some(stops[next].target)
}

fn collect_stops(node: &Node, dropped: &[NodeId], focus_event_id: Option<NodeId>, in_form: bool, walk: &mut Walk) {
  // Nothing in a disabled element takes focus, so Tab passes all of it by.
  if dropped.contains(&node.node_id()) || node.is_disabled() {
    return;
  }
  let order = walk.visited;
  walk.visited += 1;
  if walk.focused == Some(node.node_id()) {
    walk.focused_order = Some(order);
  }
  let focus_event_id = if !node.events.on_focus.is_empty() || !node.events.on_blur.is_empty() {
    Some(node.node_id())
  } else {
    focus_event_id
  };
  let in_form = in_form || is_form(node);
  let tab_index = node.tab_index_value().or(in_form.then_some(0));
  if let Some(tab_index) = tab_index
    && tab_index >= 0
    && node.is_focusable()
  {
    walk.stops.push(Stop {
      target: FocusTarget {
        input_id: node.node_id(),
        event_id: focus_event_id.unwrap_or_else(|| node.node_id()),
      },
      key: stop_key(tab_index, order),
    });
    // A button's content is part of the button, not separate stops.
    if node.button_kind_value().is_some() {
      return;
    }
  }

  for child in node.children() {
    collect_stops(child, dropped, focus_event_id, in_form, walk);
  }
}

fn stop_key(tab_index: i32, order: usize) -> StopKey {
  if tab_index > 0 {
    (0, tab_index, order)
  } else {
    (1, 0, order)
  }
}

#[cfg(feature = "form")]
fn is_form(node: &Node) -> bool {
  node.events.on_submit.is_some()
}

#[cfg(not(feature = "form"))]
fn is_form(_: &Node) -> bool {
  false
}
