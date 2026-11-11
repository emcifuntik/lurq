use std::{any::Any, sync::Arc};

use parking_lot::Mutex;

use super::{Routes, guard::GuardAction, route_match::Params};
use crate::core::signal::Signal;

#[derive(Clone)]
pub struct RouterHandle {
  pub(crate) inner: Arc<RouterInner>,
}

pub(crate) struct RouterInner {
  pub(crate) routes: Routes,
  pub(crate) current_path: Signal<String>,
  pub(crate) history: Mutex<HistoryStack>,
  _watch_handle: Option<Box<dyn std::any::Any + Send + Sync>>,
}

struct HistoryEntry {
  /// Its own among every entry made, and kept as the history moves back and forward over it: what a stack of
  /// screens tells its screens apart by.
  id: u64,
  path: String,
  state: Option<Arc<dyn Any + Send + Sync>>,
}

impl HistoryEntry {
  fn new(path: String, state: Option<Arc<dyn Any + Send + Sync>>) -> Self {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    Self {
      id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
      path,
      state,
    }
  }
}

/// An entry of the history up to where it now is: a screen of a stack.
#[derive(Clone)]
pub(crate) struct StackEntry {
  pub(crate) id: u64,
  pub(crate) path: String,
  pub(crate) state: Option<Arc<dyn Any + Send + Sync>>,
}

pub(crate) struct HistoryStack {
  entries: Vec<HistoryEntry>,
  cursor: usize,
}

impl Default for HistoryStack {
  fn default() -> Self {
    Self {
      entries: Vec::new(),
      cursor: 0,
    }
  }
}

impl std::fmt::Debug for RouterHandle {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_tuple("RouterHandle").field(&Arc::as_ptr(&self.inner)).finish()
  }
}

impl PartialEq for RouterHandle {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.inner, &other.inner)
  }
}

impl RouterHandle {
  /// A [`super::Navigator`] for programmatic navigation over this router —
  /// the same handle components get from `ctx.navigator()`.
  pub fn navigator(&self) -> super::Navigator {
    super::Navigator { handle: self.clone() }
  }

  pub(crate) fn new_with_signal(routes: Routes, current_path: Signal<String>) -> Self {
    Self {
      inner: Arc::new(RouterInner {
        routes,
        current_path,
        history: Mutex::new(HistoryStack::default()),
        _watch_handle: None,
      }),
    }
  }

  pub fn push(&self, path: impl Into<String>) {
    self.push_inner(path.into(), None);
  }

  pub fn push_with_state<S: Any + Send + Sync>(&self, path: impl Into<String>, state: S) {
    let state: Arc<dyn Any + Send + Sync> = Arc::new(state);
    self.push_inner(path.into(), Some(state));
  }

  fn push_inner(&self, path: String, state: Option<Arc<dyn Any + Send + Sync>>) {
    if self.inner.current_path.get_untracked() == path {
      return;
    }

    if let Some(redirect) = self.check_guards(&path) {
      if redirect == self.inner.current_path.get_untracked() {
        return;
      }
      self.navigate_internal(redirect, None);
      return;
    }

    self.navigate_internal(path, state);
  }

  fn navigate_internal(&self, path: String, state: Option<Arc<dyn Any + Send + Sync>>) {
    {
      let mut history = self.inner.history.lock();
      let cursor = history.cursor;
      if !history.entries.is_empty() {
        history.entries.truncate(cursor + 1);
      }
      history.entries.push(HistoryEntry::new(path.clone(), state));
      history.cursor = history.entries.len() - 1;
    }
    tracing::debug!(
      target: "lurq::router",
      "[lurq/router] navigate set path={}",
      path
    );
    self.inner.current_path.set(path);
  }

  pub fn replace(&self, path: impl Into<String>) {
    self.replace_inner(path.into(), None);
  }

  pub fn replace_with_state<S: Any + Send + Sync>(&self, path: impl Into<String>, state: S) {
    let state: Arc<dyn Any + Send + Sync> = Arc::new(state);
    self.replace_inner(path.into(), Some(state));
  }

  fn replace_inner(&self, path: String, state: Option<Arc<dyn Any + Send + Sync>>) {
    if let Some(redirect) = self.check_guards(&path) {
      if redirect == self.inner.current_path.get_untracked() {
        return;
      }
      self.replace_internal(redirect, None);
      return;
    }

    self.replace_internal(path, state);
  }

  fn replace_internal(&self, path: String, state: Option<Arc<dyn Any + Send + Sync>>) {
    {
      let mut history = self.inner.history.lock();
      if history.entries.is_empty() {
        history.entries.push(HistoryEntry::new(path.clone(), state));
        history.cursor = 0;
      } else {
        let cursor = history.cursor;
        history.entries[cursor] = HistoryEntry::new(path.clone(), state);
      }
    }
    tracing::debug!(
      target: "lurq::router",
      "[lurq/router] replace set path={}",
      path
    );
    self.inner.current_path.set(path);
  }

  /// In-memory state attached to the current history entry, if any.
  pub(crate) fn current_state(&self) -> Option<Arc<dyn Any + Send + Sync>> {
    let history = self.inner.history.lock();
    history
      .entries
      .get(history.cursor)
      .and_then(|entry| entry.state.clone())
  }

  /// The history from its first entry to the current one, the current last. A path set on the router's signal
  /// by other means than this handle is not an entry.
  pub(crate) fn stack(&self) -> Vec<StackEntry> {
    let history = self.inner.history.lock();
    if history.entries.is_empty() {
      // Nothing was pushed yet: the router shows the path it started with.
      return vec![StackEntry {
        id: 0,
        path: self.inner.current_path.get_untracked(),
        state: None,
      }];
    }
    history
      .entries
      .iter()
      .take(history.cursor + 1)
      .map(|entry| StackEntry {
        id: entry.id,
        path: entry.path.clone(),
        state: entry.state.clone(),
      })
      .collect()
  }

  pub fn back(&self) -> bool {
    let new_path = {
      let mut history = self.inner.history.lock();
      if history.cursor == 0 {
        return false;
      }
      history.cursor -= 1;
      history.entries[history.cursor].path.clone()
    };
    self.inner.current_path.set(new_path);
    true
  }

  pub fn forward(&self) -> bool {
    let new_path = {
      let mut history = self.inner.history.lock();
      if history.cursor + 1 >= history.entries.len() {
        return false;
      }
      history.cursor += 1;
      history.entries[history.cursor].path.clone()
    };
    self.inner.current_path.set(new_path);
    true
  }

  /// Whether [`RouterHandle::back`] would move — there is a prior entry.
  pub fn can_back(&self) -> bool {
    self.inner.history.lock().cursor > 0
  }

  /// Whether [`RouterHandle::forward`] would move — there is a later entry.
  pub fn can_forward(&self) -> bool {
    let history = self.inner.history.lock();
    history.cursor + 1 < history.entries.len()
  }

  pub fn path(&self) -> Signal<String> {
    self.inner.current_path.clone()
  }

  pub fn params(&self) -> Params {
    let path = self.inner.current_path.get();
    let matches = self.inner.routes.resolve(&path);
    matches.last().map(|m| m.params.clone()).unwrap_or_default()
  }

  fn check_guards(&self, path: &str) -> Option<String> {
    let matches = self.inner.routes.resolve(path);
    for m in &matches {
      if let Some(guard) = self.inner.routes.guard_for(m.route_index) {
        match guard(m) {
          GuardAction::Allow => {}
          GuardAction::Deny => return Some(self.inner.current_path.get_untracked()),
          GuardAction::Redirect(target) => return Some(target),
        }
      }
    }
    None
  }
}
