use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct InteractionState {
  inner: Arc<Mutex<InteractionStateInner>>,
}

#[derive(Default)]
struct InteractionStateInner {
  hovered: bool,
  active: bool,
  focused: bool,
  focus_visible: bool,
  drag_over: bool,
  layout_dirty: bool,
}

impl InteractionState {
  pub fn new() -> Self {
    Self::default()
  }

  pub(crate) fn layout_states(&self) -> [bool; 5] {
    let inner = self.inner.lock().unwrap();
    [inner.focused, inner.focus_visible, inner.hovered, inner.active, inner.drag_over]
  }

  pub fn is_hovered(&self) -> bool {
    self.inner.lock().unwrap().hovered
  }

  pub fn is_active(&self) -> bool {
    self.inner.lock().unwrap().active
  }

  pub fn is_focused(&self) -> bool {
    self.inner.lock().unwrap().focused
  }

  /// Focused and showing a focus ring, like CSS `:focus-visible`: the focus
  /// came from the keyboard (or a key was pressed since), or the node is a
  /// text input. See the focus-navigation guide for the full rules.
  pub fn is_focus_visible(&self) -> bool {
    self.inner.lock().unwrap().focus_visible
  }

  /// Files from the OS are dragged over the node (`on_file_drag_enter` fired
  /// and `on_file_drag_leave` has not), like a hover during a file drag.
  pub fn is_drag_over(&self) -> bool {
    self.inner.lock().unwrap().drag_over
  }

  pub(crate) fn set_drag_over(&self, val: bool) {
    self.inner.lock().unwrap().drag_over = val;
  }

  pub(crate) fn set_hovered(&self, val: bool) {
    self.inner.lock().unwrap().hovered = val;
  }

  pub(crate) fn set_active(&self, val: bool) {
    self.inner.lock().unwrap().active = val;
  }

  /// Losing focus also loses focus-visible.
  pub(crate) fn set_focused(&self, val: bool) {
    let mut inner = self.inner.lock().unwrap();
    inner.focused = val;
    inner.focus_visible &= val;
  }

  /// Only a focused state can be focus-visible.
  pub(crate) fn set_focus_visible(&self, val: bool) {
    let mut inner = self.inner.lock().unwrap();
    inner.focus_visible = val && inner.focused;
  }

  pub(crate) fn mark_layout_dirty(&self) {
    self.inner.lock().unwrap().layout_dirty = true;
  }

  pub(crate) fn has_layout_dirty(&self) -> bool {
    self.inner.lock().unwrap().layout_dirty
  }

  pub(crate) fn take_layout_dirty(&self) -> bool {
    let mut inner = self.inner.lock().unwrap();
    let dirty = inner.layout_dirty;
    inner.layout_dirty = false;
    dirty
  }
}
