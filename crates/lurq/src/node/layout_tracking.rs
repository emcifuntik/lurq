use crate::{
  layout::layout_kind::LayoutKind,
  node::{Node, layout_cache::LayoutSubtree, node_kind::NodeKind},
};

impl Node {
  pub(crate) fn prepare_layout_tracking(&self) -> LayoutSubtree {
    let cache = &self.layout_cache;
    if cache.tracking_generation.get() == cache.watch().generation() {
      return cache.subtree.get();
    }
    #[cfg(test)]
    crate::core::layout_dirty::visit();

    let watch = cache.watch();
    self.text_content.track_layout(watch);
    self.color.track_layout(watch);
    self.border_radius.track_layout(watch);
    self.border.track_layout(watch);
    self.rare_style.track_layout(watch);
    #[cfg(feature = "raster")]
    self.background_image.track_layout(watch);
    self.scrollbar_style.track_layout(watch);
    self.style_state.track_layout(watch);
    if let Some(reference) = &self.element_ref {
      reference.track_layout(watch);
    }
    let mut subtree = LayoutSubtree {
      timeline: !self.transitions.is_empty()
        || self.playing_animation().is_some()
        || !self.animation_overrides.is_empty(),
      ..LayoutSubtree::default()
    };
    match self.node_kind() {
      NodeKind::TextInput { state, .. } => {
        state.track_layout(watch);
        subtree.text_input = true;
      }
      NodeKind::Select { state } => {
        state.track_layout(watch);
        subtree.select = true;
      }
      #[cfg(feature = "canvas")]
      NodeKind::Canvas { .. } => subtree.canvas = true,
      #[cfg(all(feature = "image", feature = "resources"))]
      NodeKind::ResourceImage { .. } => subtree.resource_images = true,
      // Part styles are shared and can change without a layout notification.
      // Keep controls pollable for resource images in any of those styles.
      #[cfg(all(feature = "image", feature = "resources"))]
      NodeKind::Checkbox { .. } | NodeKind::Slider { .. } => subtree.resource_images = true,
      #[cfg(all(feature = "svg", feature = "resources"))]
      NodeKind::ResourceSvg { .. } => subtree.resource_svgs = true,
      #[cfg(feature = "image")]
      NodeKind::LoadingImage { .. } => subtree.loading_images = true,
      _ => {}
    }
    #[cfg(all(feature = "image", feature = "resources"))]
    {
      subtree.resource_images |= self.background_resource_image.is_some();
    }
    if let LayoutKind::ScrollModifier { state, .. } = self.layout_kind() {
      state.track_layout(watch);
      subtree.scroll = true;
    }
    let mut attached = false;
    for child in self.children() {
      attached |= child.layout_cache.watch().set_parent(watch);
      let child = child.prepare_layout_tracking();
      subtree.text_input |= child.text_input;
      subtree.select |= child.select;
      subtree.timeline |= child.timeline;
      subtree.scroll |= child.scroll;
      subtree.canvas |= child.canvas;
      subtree.resource_images |= child.resource_images;
      subtree.resource_svgs |= child.resource_svgs;
      subtree.loading_images |= child.loading_images;
    }
    // A newly attached subtree must be inspected even if its old parent has
    // already consumed its dirty marks (the overlay host lays its base first).
    // One mark covers every child attached above: a mark per child cost a
    // first layout a walk up the ancestors for each node.
    if attached {
      watch.mark_descendant();
    }
    cache.subtree.set(subtree);
    cache.tracking_generation.set(watch.generation());
    subtree
  }
}
