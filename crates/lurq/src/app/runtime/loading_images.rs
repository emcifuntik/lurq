//! Images decoded at runtime, resolved against the decoded-image cache.
//!
//! A node built with [`Image::from_bytes`](crate::components::Image::from_bytes)
//! or [`Image::from_file`](crate::components::Image::from_file) starts as a
//! `LoadingImage` showing its placeholder. A pass asks the cache for its
//! key: a ready image turns the node into an `Image` node (the component
//! need not render again), a failed decode puts the error view in its
//! place, and a loading one is remembered so the decode's end asks for the
//! next pass. The decode's worker wakes the window's loop when it ends.
//!
//! The walk over the tree is skipped when it cannot find anything: no
//! decode is pending for this tree, and no loading-image node was built
//! since the last walk (a counter every such node bumps).

use std::sync::atomic::{AtomicU64, Ordering};

use super::Tree;
use crate::{
  app::window::WindowWaker,
  images::{ImageKey, ImageLoadState, ImageSizeCap, decoded_images},
  layout::{Size, layout_kind::LayoutKind},
  node::{node::Node, node_kind::NodeKind},
};

/// How many loading-image nodes have been built in the process.
static LOADING_IMAGES_BUILT: AtomicU64 = AtomicU64::new(0);

/// A loading-image node was built: the next pass of every tree walks for it.
pub(crate) fn note_loading_image_built() {
  LOADING_IMAGES_BUILT.fetch_add(1, Ordering::AcqRel);
}

impl Tree {
  /// Resolves the loading images of the tree: `true` when a node changed
  /// and the layout must run again.
  pub(super) fn resolve_loading_images(&mut self) -> bool {
    let built = LOADING_IMAGES_BUILT.load(Ordering::Acquire);
    if self.pending_images.is_empty() && self.loading_images_built_seen == built {
      return false;
    }
    self.loading_images_built_seen = built;
    let Some(root) = &mut self.root else {
      return false;
    };
    // One waker for the tree's window: the cache keeps one per entry and
    // window, and tells them apart by identity.
    let wake = self
      .loading_image_waker
      .get_or_insert_with(|| self.window.loop_waker())
      .clone();
    let mut pending = Vec::new();
    let changed = resolve_recursive(root, &wake, &mut pending);
    self.pending_images = pending;
    if changed {
      self.needs_redraw = true;
    }
    changed
  }

  /// A pending decode that ended asks for a pass; called each turn of the
  /// shell's loop.
  pub(super) fn notice_decoded_images(&mut self) {
    if self.pending_images.is_empty() {
      return;
    }
    let cache = decoded_images();
    let ended = self
      .pending_images
      .iter()
      .any(|(key, cap)| !cache.state(*key, *cap).is_some_and(|state| state.is_loading()));
    if ended {
      self.needs_redraw = true;
    }
  }
}

fn resolve_recursive(node: &mut Node, wake: &WindowWaker, pending: &mut Vec<(ImageKey, Option<ImageSizeCap>)>) -> bool {
  #[cfg(test)]
  crate::core::layout_dirty::visit();
  if !node.prepare_layout_tracking().loading_images {
    return false;
  }
  let mut layout_dirty = false;
  if let NodeKind::LoadingImage { request } = &mut node.node_kind {
    let state = decoded_images().load(request.key, request.cap, || request.source.clone(), Some(wake.clone()));
    match state {
      ImageLoadState::Loading => pending.push((request.key, request.cap)),
      ImageLoadState::Ready(data) => {
        let fit = request.fit;
        node.intrinsic_size = Some(Size::new(data.width() as f32, data.height() as f32));
        node.layout_kind = LayoutKind::Leaf;
        node.children.clear();
        node.node_kind = NodeKind::Image { data, fit };
        layout_dirty = true;
      }
      ImageLoadState::Failed(error) => {
        if !request.failed {
          request.failed = true;
          node.children = request
            .error_view
            .as_ref()
            .map(|view| view(&error).into_node())
            .into_iter()
            .collect();
          layout_dirty = true;
        }
      }
    }
  }
  for child in &mut node.children {
    if resolve_recursive(child, wake, pending) {
      layout_dirty = true;
    }
  }
  if layout_dirty {
    node.layout_cache.invalidate();
  }
  layout_dirty
}
