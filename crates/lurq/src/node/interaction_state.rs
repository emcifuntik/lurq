use std::sync::{Arc, Mutex, OnceLock};

use crate::core::layout_dirty::LayoutWatch;

#[derive(Default)]
pub struct InteractionState {
  inner: OnceLock<Arc<Mutex<InteractionStateInner>>>,
}

#[derive(Default)]
struct InteractionStateInner {
  hovered: bool,
  active: bool,
  focused: bool,
  focus_visible: bool,
  drag_over: bool,
  disabled: bool,
  layout_dirty: bool,
  layout_subscribers: crate::core::layout_dirty::LayoutSubscribers,
}

impl Clone for InteractionState {
  fn clone(&self) -> Self {
    // Even an untouched clone must share subsequent interaction updates.
    Self {
      inner: OnceLock::from(self.shared_inner().clone()),
    }
  }
}

impl InteractionState {
  fn shared_inner(&self) -> &Arc<Mutex<InteractionStateInner>> {
    self.inner.get_or_init(Default::default)
  }

  pub fn new() -> Self {
    Self::default()
  }

  pub(crate) fn layout_states(&self) -> [bool; 5] {
    let Some(inner) = self.inner.get() else {
      return [false; 5];
    };
    let inner = inner.lock().unwrap();
    [
      inner.focused,
      inner.focus_visible,
      inner.hovered,
      inner.active,
      inner.drag_over,
    ]
  }

  pub fn is_hovered(&self) -> bool {
    self.inner.get().is_some_and(|inner| inner.lock().unwrap().hovered)
  }

  pub fn is_active(&self) -> bool {
    self.inner.get().is_some_and(|inner| inner.lock().unwrap().active)
  }

  pub fn is_focused(&self) -> bool {
    self.inner.get().is_some_and(|inner| inner.lock().unwrap().focused)
  }

  /// Focused and showing a focus ring, like CSS `:focus-visible`: the focus
  /// came from the keyboard (or a key was pressed since), or the node is a
  /// text input. See the focus-navigation guide for the full rules.
  pub fn is_focus_visible(&self) -> bool {
    self
      .inner
      .get()
      .is_some_and(|inner| inner.lock().unwrap().focus_visible)
  }

  /// Files from the OS are dragged over the node (`on_file_drag_enter` fired
  /// and `on_file_drag_leave` has not), like a hover during a file drag.
  pub fn is_drag_over(&self) -> bool {
    self.inner.get().is_some_and(|inner| inner.lock().unwrap().drag_over)
  }

  /// The node takes no input: it is disabled (`.disabled(true)`), or an
  /// element it is in is. Kept for every node of a tree, in view or not: it
  /// is written when the tree is built and when a live element is disabled
  /// or enabled.
  pub fn is_disabled(&self) -> bool {
    self.inner.get().is_some_and(|inner| inner.lock().unwrap().disabled)
  }

  pub(crate) fn set_disabled(&self, val: bool) {
    self.shared_inner().lock().unwrap().disabled = val;
  }

  pub(crate) fn set_drag_over(&self, val: bool) {
    self.shared_inner().lock().unwrap().drag_over = val;
  }

  pub(crate) fn set_hovered(&self, val: bool) {
    self.shared_inner().lock().unwrap().hovered = val;
  }

  pub(crate) fn set_active(&self, val: bool) {
    self.shared_inner().lock().unwrap().active = val;
  }

  /// Losing focus also loses focus-visible.
  pub(crate) fn set_focused(&self, val: bool) {
    let mut inner = self.shared_inner().lock().unwrap();
    inner.focused = val;
    inner.focus_visible &= val;
  }

  /// Only a focused state can be focus-visible.
  pub(crate) fn set_focus_visible(&self, val: bool) {
    let mut inner = self.shared_inner().lock().unwrap();
    inner.focus_visible = val && inner.focused;
  }

  /// Binds `watch` only if the shared state exists: layout tracks every node,
  /// and must not allocate the state of the many nodes nothing interacts with.
  /// Whoever creates the state later binds with [`Self::bind_layout`].
  pub(crate) fn track_layout(&self, watch: &Arc<LayoutWatch>) {
    if let Some(inner) = self.inner.get() {
      inner.lock().unwrap().layout_subscribers.bind(watch);
    }
  }

  /// Binds `watch`, creating the shared state if it does not exist yet.
  pub(crate) fn bind_layout(&self, watch: &Arc<LayoutWatch>) {
    self.shared_inner().lock().unwrap().layout_subscribers.bind(watch);
  }

  pub(crate) fn mark_layout_dirty(&self) {
    let mut inner = self.shared_inner().lock().unwrap();
    inner.layout_dirty = true;
    inner.layout_subscribers.mark();
  }

  pub(crate) fn has_layout_dirty(&self) -> bool {
    self.inner.get().is_some_and(|inner| inner.lock().unwrap().layout_dirty)
  }

  pub(crate) fn take_layout_dirty(&self) -> bool {
    let Some(inner) = self.inner.get() else {
      return false;
    };
    let mut inner = inner.lock().unwrap();
    let dirty = inner.layout_dirty;
    inner.layout_dirty = false;
    dirty
  }

  #[cfg(test)]
  pub(crate) fn has_shared_storage(&self) -> bool {
    self.inner.get().is_some()
  }
}

#[cfg(test)]
mod tests {
  use super::{InteractionState, LayoutWatch};

  #[test]
  fn tracking_layout_does_not_allocate_shared_storage() {
    let state = InteractionState::new();
    state.track_layout(&LayoutWatch::new());
    assert!(state.inner.get().is_none());
  }

  #[test]
  fn tracking_a_created_state_adds_to_its_bound_watches() {
    let state = InteractionState::new();
    let first = LayoutWatch::new();
    state.bind_layout(&first);
    let clone = state.clone();
    let second = LayoutWatch::new();
    clone.track_layout(&second);
    let generations = (first.generation(), second.generation());

    clone.mark_layout_dirty();

    assert!(first.generation() > generations.0);
    assert!(second.generation() > generations.1);
  }

  #[test]
  fn untouched_state_queries_do_not_allocate_shared_storage() {
    let state = InteractionState::new();
    assert_eq!(state.layout_states(), [false; 5]);
    assert!(!state.is_hovered());
    assert!(!state.is_active());
    assert!(!state.is_focused());
    assert!(!state.is_focus_visible());
    assert!(!state.is_drag_over());
    assert!(!state.is_disabled());
    assert!(!state.has_layout_dirty());
    assert!(!state.take_layout_dirty());
    assert!(state.inner.get().is_none());
  }

  #[test]
  fn clones_share_updates_before_and_after_first_interaction() {
    let state = InteractionState::new();
    let first = state.clone();
    first.set_hovered(true);
    assert!(state.is_hovered());
    state.set_active(true);
    state.set_focused(true);
    state.set_focus_visible(true);
    state.set_drag_over(true);
    state.mark_layout_dirty();

    let second = state.clone();
    assert_eq!(first.layout_states(), [true; 5]);
    assert_eq!(second.layout_states(), [true; 5]);
    assert!(first.take_layout_dirty());
    assert!(!second.take_layout_dirty());
    second.set_focused(false);
    assert!(!state.is_focused());
    assert!(!first.is_focus_visible());
    second.set_hovered(false);
    second.set_active(false);
    second.set_drag_over(false);
    assert_eq!(state.layout_states(), [false; 5]);
  }
}
