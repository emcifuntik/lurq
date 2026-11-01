use crate::{
  layout::layout_kind::LayoutKind,
  node::{Node, layout_cache::LayoutSubtree, node_kind::NodeKind},
};

impl Node {
  pub(crate) fn prepare_layout_tracking(&self) -> LayoutSubtree {
    let cache = &self.layout_cache;
    if cache.tracking_generation.get() == cache.watch.generation() {
      return cache.subtree.get();
    }
    #[cfg(test)]
    crate::core::layout_dirty::visit();

    let watch = &cache.watch;
    self.text_content.track_layout(watch);
    self.color.track_layout(watch);
    self.gradient.track_layout(watch);
    self.border_radius.track_layout(watch);
    self.border.track_layout(watch);
    self.box_shadow.track_layout(watch);
    self.caret_color.track_layout(watch);
    self.selection_color.track_layout(watch);
    self.caret_mode.track_layout(watch);
    #[cfg(feature = "raster")]
    self.background_image.track_layout(watch);
    self.scrollbar_style.track_layout(watch);
    self.style_state.track_layout(watch);
    if let Some(reference) = &self.element_ref {
      reference.track_layout(watch);
    }
    let mut subtree = LayoutSubtree {
      timeline: !self.transitions.is_empty() || self.animation.is_some() || !self.animation_overrides.is_empty(),
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
      _ => {}
    }
    if let LayoutKind::ScrollModifier { state, .. } = self.layout_kind() {
      state.track_layout(watch);
      subtree.scroll = true;
    }
    for child in self.children() {
      child.layout_cache.watch.set_parent(watch);
      let child = child.prepare_layout_tracking();
      subtree.text_input |= child.text_input;
      subtree.select |= child.select;
      subtree.timeline |= child.timeline;
      subtree.scroll |= child.scroll;
      subtree.canvas |= child.canvas;
    }
    cache.subtree.set(subtree);
    cache.tracking_generation.set(watch.generation());
    subtree
  }
}
