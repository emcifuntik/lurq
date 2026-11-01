use std::{
  cell::Cell,
  ops::{Deref, DerefMut},
};

pub struct Guard<T> {
  value: T,
  changed: Cell<bool>,
  layout_watch: Cell<std::sync::Weak<super::layout_dirty::LayoutWatch>>,
}

impl<T> Guard<T> {
  pub fn new(value: T) -> Self {
    Self {
      value,
      changed: Cell::new(true),
      layout_watch: Default::default(),
    }
  }

  pub(crate) fn track_layout(&self, watch: &std::sync::Arc<super::layout_dirty::LayoutWatch>) {
    self.layout_watch.set(std::sync::Arc::downgrade(watch));
  }

  fn mark_changed(&self) {
    self.changed.set(true);
    let weak = self.layout_watch.take();
    if let Some(watch) = weak.upgrade() {
      watch.mark();
    }
    self.layout_watch.set(weak);
  }

  pub fn is_changed(&self) -> bool {
    self.changed.get()
  }

  pub fn clear_changed(&self) {
    self.changed.set(false);
  }

  pub fn set(&mut self, value: T) {
    self.value = value;
    self.mark_changed();
  }

  pub fn into_inner(self) -> T {
    self.value
  }
}

impl<T> Deref for Guard<T> {
  type Target = T;

  fn deref(&self) -> &T {
    &self.value
  }
}

impl<T> DerefMut for Guard<T> {
  fn deref_mut(&mut self) -> &mut T {
    self.mark_changed();
    &mut self.value
  }
}

impl<T: Clone> Clone for Guard<T> {
  fn clone(&self) -> Self {
    Self {
      value: self.value.clone(),
      changed: Cell::new(self.changed.get()),
      layout_watch: Default::default(),
    }
  }
}

impl<T: Default> Default for Guard<T> {
  fn default() -> Self {
    Self::new(T::default())
  }
}
