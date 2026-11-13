//! Files dragged into the window from the OS, delivered to the element under
//! the pointer.
//!
//! A shell starts a session with [`Tree::file_drag_enter`] when the OS
//! reports a drag over the window, moves it with [`Tree::file_drag_over`],
//! and ends it with [`Tree::file_drag_leave`] or [`Tree::file_drop`]. The
//! elements under the pointer get `on_file_drag_enter` when the drag comes
//! over them, `on_file_drag_over` on every move, `on_file_drag_leave` when it
//! moves off them or leaves the window, and `on_file_drop` on the drop; each
//! event bubbles from the innermost element to the root. While the drag is
//! over an element, its `drag_over` style applies, like `hovered` under the
//! pointer. Overlays and modals are hit-tested as for the mouse, so a drag
//! over a popup reaches the popup.
//!
//! Positions are physical pixels, as the shell's pointer events are; the
//! events carry logical pixels.

use super::{Tree, find_node_by_id, hit_test_tree};
use crate::{
  app::events::{DraggedPaths, FileDragEvent, FileDropEvent},
  core::NodeId,
  node::node::Node,
};

/// One drag of files over the window.
pub(super) struct FileDragSession {
  paths: DraggedPaths,
  /// The elements under the pointer, innermost first, that have the drag
  /// over them.
  over_path: Vec<NodeId>,
  /// The elements that refused the drag when it entered them, until it
  /// leaves them.
  refused: Vec<NodeId>,
  accepted: bool,
  /// Where the drag last was, in physical pixels.
  position: (f32, f32),
}

impl Tree {
  /// Whether files from the OS are being dragged over the window.
  pub fn has_file_drag(&self) -> bool {
    self.file_drag.is_some()
  }

  /// The paths of the drag over the window, if one is.
  pub fn file_drag_paths(&self) -> Option<&DraggedPaths> {
    self.file_drag.as_ref().map(|session| &session.paths)
  }

  /// Whether the elements under the pointer accept the drag: `Some(false)`
  /// once a handler called [`FileDragEvent::refuse`] at the current
  /// position, `None` without a drag.
  pub fn file_drag_accepted(&self) -> Option<bool> {
    self.file_drag.as_ref().map(|session| session.accepted)
  }

  /// Files from the OS come over the window at physical `(x, y)`: starts the
  /// drag and delivers `on_file_drag_enter` and `on_file_drag_over` to the
  /// elements under the pointer. A drag already in progress gets the new
  /// paths.
  pub fn file_drag_enter(&mut self, paths: DraggedPaths, x: f32, y: f32) {
    match &mut self.file_drag {
      Some(session) => session.paths = paths,
      None => {
        self.file_drag = Some(FileDragSession {
          paths,
          over_path: Vec::new(),
          refused: Vec::new(),
          accepted: true,
          position: (x, y),
        });
      }
    }
    self.file_drag_move(x, y);
  }

  /// The drag moves to physical `(x, y)`: elements it leaves get
  /// `on_file_drag_leave`, elements it comes over get `on_file_drag_enter`,
  /// and every element under the pointer gets `on_file_drag_over`. Nothing
  /// without a drag.
  pub fn file_drag_over(&mut self, x: f32, y: f32) {
    if self.file_drag.is_none() {
      return;
    }
    self.file_drag_move(x, y);
  }

  /// The drag leaves the window without a drop: the elements it was over
  /// get `on_file_drag_leave` and lose their `drag_over` style.
  pub fn file_drag_leave(&mut self) {
    let Some(session) = self.file_drag.take() else {
      return;
    };
    self.rebuild_if_dirty();
    if let Some(root) = &self.root {
      for node_id in &session.over_path {
        if let Some(node) = find_node_by_id(root, *node_id) {
          node.set_style_drag_over(false);
          for handler in &node.events.on_file_drag_leave {
            handler.call();
          }
        }
      }
    }
    self.cached_render_list = None;
    self.needs_redraw = true;
    self.apply_reactive_updates_after_event();
  }

  /// Files are dropped at physical `(x, y)`: the elements under the pointer
  /// get `on_file_drop`, innermost first, until one stops the propagation,
  /// and the drag ends (their `drag_over` style goes, without
  /// `on_file_drag_leave`). A drop without an enter first (a synthetic one)
  /// enters at the same point. `true` when an element under the pointer had
  /// an `on_file_drop` handler.
  pub fn file_drop(&mut self, paths: DraggedPaths, x: f32, y: f32) -> bool {
    // A drag already at the point gets no second `on_file_drag_over`.
    match &mut self.file_drag {
      Some(session) if session.position == (x, y) => session.paths = paths.clone(),
      _ => self.file_drag_enter(paths.clone(), x, y),
    }
    let scale = self.scale_factor();
    let (lx, ly) = (x / scale, y / scale);
    let mut handled = false;
    if let (Some(root), Some(layout)) = (&self.root, &self.last_layout) {
      let mut hits = Vec::new();
      hit_test_tree(root, layout, 0.0, 0.0, lx, ly, &mut hits);
      super::take_disabled_hit(&mut hits);
      let event = FileDropEvent::new(lx, ly, paths, NodeId::UNASSIGNED);
      'drop: for (node, _) in &hits {
        let event = event.at(node.node_id());
        for handler in &node.events.on_file_drop {
          handled = true;
          handler.call(&event);
          if event.immediate_propagation_stopped() {
            break 'drop;
          }
        }
        if event.propagation_stopped() {
          break;
        }
      }
    }
    if let Some(session) = self.file_drag.take()
      && let Some(root) = &self.root
    {
      for node_id in &session.over_path {
        if let Some(node) = find_node_by_id(root, *node_id) {
          node.set_style_drag_over(false);
        }
      }
    }
    self.cached_render_list = None;
    self.needs_redraw = true;
    self.apply_reactive_updates_after_event();
    handled
  }

  fn file_drag_move(&mut self, x: f32, y: f32) {
    self.rebuild_if_dirty();
    let scale = self.scale_factor();
    let (lx, ly) = (x / scale, y / scale);
    let Some(session) = self.file_drag.as_mut() else {
      return;
    };
    session.position = (x, y);
    let (Some(root), Some(layout)) = (&self.root, &self.last_layout) else {
      return;
    };
    let mut hits = Vec::new();
    hit_test_tree(root, layout, 0.0, 0.0, lx, ly, &mut hits);
    super::take_disabled_hit(&mut hits);
    let current_ids: Vec<NodeId> = hits.iter().map(|(node, _)| node.node_id()).collect();

    for old_id in &session.over_path {
      if !current_ids.contains(old_id)
        && let Some(node) = find_node_by_id(root, *old_id)
      {
        node.set_style_drag_over(false);
        for handler in &node.events.on_file_drag_leave {
          handler.call();
        }
      }
    }
    // A refusal lasts while the drag is over the element; leaving forgets it.
    session.refused.retain(|id| current_ids.contains(id));

    let entered: Vec<&Node> = hits
      .iter()
      .map(|(node, _)| *node)
      .filter(|node| !session.over_path.contains(&node.node_id()))
      .collect();
    let enter = FileDragEvent::new(lx, ly, session.paths.clone(), NodeId::UNASSIGNED);
    let refused = dispatch_file_drag(&entered, &enter, |node| &node.events.on_file_drag_enter);
    remember_refused(&mut session.refused, refused);
    let over: Vec<&Node> = hits.iter().map(|(node, _)| *node).collect();
    let moved = FileDragEvent::new(lx, ly, session.paths.clone(), NodeId::UNASSIGNED);
    let refused = dispatch_file_drag(&over, &moved, |node| &node.events.on_file_drag_over);
    remember_refused(&mut session.refused, refused);

    session.accepted = session.refused.is_empty();
    for node in &over {
      node.set_style_drag_over(session.accepted);
    }
    session.over_path = current_ids;
    self.cached_render_list = None;
    self.needs_redraw = true;
    self.apply_reactive_updates_after_event();
  }

  /// Puts the `drag_over` flags back on the elements of the drag after the
  /// tree was rebuilt.
  pub(super) fn refresh_file_drag_state(&mut self) {
    let Some(session) = self.file_drag.as_mut() else {
      return;
    };
    let Some(root) = self.root.as_ref() else {
      session.over_path.clear();
      return;
    };
    session
      .over_path
      .retain(|node_id| match find_node_by_id(root, *node_id) {
        Some(node) => {
          node.set_style_drag_over(session.accepted);
          true
        }
        None => false,
      });
  }
}

fn remember_refused(refused: &mut Vec<NodeId>, nodes: Vec<NodeId>) {
  for node_id in nodes {
    if !refused.contains(&node_id) {
      refused.push(node_id);
    }
  }
}

/// Runs one kind of file-drag handler on `nodes`, innermost first, until one
/// stops the propagation. Returns the nodes whose own handlers refused the
/// drag: a refusal is the refusing element's, not its ancestors' or
/// siblings'.
fn dispatch_file_drag<'a>(
  nodes: &[&'a Node],
  event: &FileDragEvent,
  handlers: impl Fn(&'a Node) -> &'a [crate::node::EventHandler<FileDragEvent>],
) -> Vec<NodeId> {
  let mut refused = Vec::new();
  for node in nodes {
    let event = event.at(node.node_id());
    let mut stopped = false;
    for handler in handlers(node) {
      handler.call(&event);
      if event.immediate_propagation_stopped() {
        stopped = true;
        break;
      }
    }
    if !event.is_accepted() {
      refused.push(node.node_id());
    }
    if stopped || event.propagation_stopped() {
      break;
    }
  }
  refused
}
