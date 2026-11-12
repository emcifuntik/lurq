//! Runtime state of a `Select`: its per-render configuration (labels,
//! selection, style) and the state that survives re-renders (open, keyboard
//! highlight, menu scroll, type-ahead).

use std::{
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
  },
  time::{Duration, Instant},
};

use crate::{
  app::ctx::OutsidePress,
  layout::layout_kind::ScrollState,
  node::{SelectStyle, select_icon::SelectAdornment},
};

pub(crate) type SelectChangeCallback = Arc<dyn Fn(usize) + Send + Sync>;

/// Typed characters within this interval extend the type-ahead search.
const TYPE_AHEAD_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub(crate) struct SelectState {
  inner: Arc<Mutex<SelectInner>>,
  layout_dirty: Arc<AtomicBool>,
  layout_subscribers: Arc<crate::core::layout_dirty::LayoutSubscribers>,
}

struct SelectInner {
  // Per-render config, rebuilt by the generic `Select<T>` wrapper each render.
  labels: Vec<Arc<str>>,
  details: Vec<Option<Arc<str>>>,
  disabled: Vec<bool>,
  /// The builders of the elements before and after each option's label.
  leading: Vec<Option<SelectAdornment>>,
  trailing: Vec<Option<SelectAdornment>>,
  selected: Vec<usize>,
  multiple: bool,
  placeholder: Option<Arc<str>>,
  style: SelectStyle,
  on_change: Option<SelectChangeCallback>,
  /// The id of the signal the select is bound to: its identity across
  /// re-renders, like a text input's value signal.
  binding: Option<usize>,
  outside_press: OutsidePress,
  // Runtime state, preserved across re-renders via `copy_runtime_state_from`.
  open: bool,
  highlighted: Option<usize>,
  /// The menu's scroll position; fresh each time the menu opens.
  menu_scroll: ScrollState,
  /// An option the next menu layout scrolls into view.
  reveal: Option<usize>,
  type_ahead: String,
  type_ahead_at: Option<Instant>,
}

impl SelectInner {
  fn is_enabled(&self, index: usize) -> bool {
    index < self.labels.len() && !self.disabled.get(index).copied().unwrap_or(false)
  }

  fn first_enabled(&self) -> Option<usize> {
    (0..self.labels.len()).find(|index| self.is_enabled(*index))
  }

  fn last_enabled(&self) -> Option<usize> {
    (0..self.labels.len()).rev().find(|index| self.is_enabled(*index))
  }

  fn first_selected_enabled(&self) -> Option<usize> {
    self.selected.iter().copied().find(|index| self.is_enabled(*index))
  }

  /// Where the highlight goes when a key moves it from nowhere: the selected
  /// option, else the first (or, moving up, last) enabled option.
  fn initial_highlight(&self, forward: bool) -> Option<usize> {
    self.first_selected_enabled().or_else(|| {
      if forward {
        self.first_enabled()
      } else {
        self.last_enabled()
      }
    })
  }

  /// The next enabled option from `from` in `delta` steps' direction; stays
  /// on `from` at either end.
  fn step_from(&self, from: usize, delta: i32) -> usize {
    let count = self.labels.len() as i64;
    let direction = i64::from(delta.signum());
    let mut remaining = delta.unsigned_abs();
    let mut current = from;
    let mut probe = from as i64;
    while remaining > 0 {
      probe += direction;
      if probe < 0 || probe >= count {
        break;
      }
      if self.is_enabled(probe as usize) {
        current = probe as usize;
        remaining -= 1;
      }
    }
    current
  }

  fn open_menu(&mut self, highlighted: Option<usize>) {
    self.open = true;
    self.highlighted = highlighted;
    self.menu_scroll = ScrollState::new();
    self.reveal = highlighted.or_else(|| self.selected.first().copied());
    self.type_ahead.clear();
  }
}

impl SelectState {
  pub(crate) fn new() -> Self {
    Self {
      inner: Arc::new(Mutex::new(SelectInner {
        labels: Vec::new(),
        details: Vec::new(),
        disabled: Vec::new(),
        leading: Vec::new(),
        trailing: Vec::new(),
        selected: Vec::new(),
        multiple: false,
        placeholder: None,
        style: SelectStyle::new(),
        on_change: None,
        binding: None,
        outside_press: OutsidePress::default(),
        open: false,
        highlighted: None,
        menu_scroll: ScrollState::new(),
        reveal: None,
        type_ahead: String::new(),
        type_ahead_at: None,
      })),
      layout_dirty: Arc::new(AtomicBool::new(false)),
      layout_subscribers: Arc::default(),
    }
  }

  pub(crate) fn set_labels(&self, labels: Vec<Arc<str>>) {
    self.inner.lock().unwrap().labels = labels;
  }

  pub(crate) fn set_details(&self, details: Vec<Option<Arc<str>>>) {
    self.inner.lock().unwrap().details = details;
  }

  pub(crate) fn set_disabled(&self, disabled: Vec<bool>) {
    self.inner.lock().unwrap().disabled = disabled;
  }

  pub(crate) fn set_adornments(&self, leading: Vec<Option<SelectAdornment>>, trailing: Vec<Option<SelectAdornment>>) {
    let mut inner = self.inner.lock().unwrap();
    inner.leading = leading;
    inner.trailing = trailing;
  }

  pub(crate) fn set_selected(&self, selected: Vec<usize>) {
    self.inner.lock().unwrap().selected = selected;
  }

  pub(crate) fn set_multiple(&self, multiple: bool) {
    self.inner.lock().unwrap().multiple = multiple;
  }

  pub(crate) fn set_placeholder(&self, placeholder: Option<Arc<str>>) {
    self.inner.lock().unwrap().placeholder = placeholder;
  }

  pub(crate) fn set_style(&self, style: SelectStyle) {
    self.inner.lock().unwrap().style = style;
  }

  pub(crate) fn set_on_change(&self, on_change: SelectChangeCallback) {
    self.inner.lock().unwrap().on_change = Some(on_change);
  }

  pub(crate) fn set_outside_press(&self, outside_press: OutsidePress) {
    self.inner.lock().unwrap().outside_press = outside_press;
  }

  pub(crate) fn outside_press(&self) -> OutsidePress {
    self.inner.lock().unwrap().outside_press
  }

  pub(crate) fn set_binding(&self, signal_id: usize) {
    self.inner.lock().unwrap().binding = Some(signal_id);
  }

  pub(crate) fn has_binding(&self) -> bool {
    self.inner.lock().unwrap().binding.is_some()
  }

  /// Whether both selects are bound to the same signal. Unbound selects
  /// (a bare `Node::select`) match any unbound select.
  pub(crate) fn same_binding(&self, other: &SelectState) -> bool {
    if Arc::ptr_eq(&self.inner, &other.inner) {
      return true;
    }
    let binding = self.inner.lock().unwrap().binding;
    binding == other.inner.lock().unwrap().binding
  }

  pub(crate) fn labels(&self) -> Vec<Arc<str>> {
    self.inner.lock().unwrap().labels.clone()
  }

  pub(crate) fn detail(&self, index: usize) -> Option<Arc<str>> {
    self.inner.lock().unwrap().details.get(index).cloned().flatten()
  }

  /// The builders of the elements before and after the label of option
  /// `index`. Returned, not called here: a builder is app code and runs
  /// without the state locked.
  pub(crate) fn adornments(&self, index: usize) -> (Option<SelectAdornment>, Option<SelectAdornment>) {
    let inner = self.inner.lock().unwrap();
    let builder = |slots: &[Option<SelectAdornment>]| slots.get(index).cloned().flatten();
    (builder(&inner.leading), builder(&inner.trailing))
  }

  pub(crate) fn is_disabled(&self, index: usize) -> bool {
    self.inner.lock().unwrap().disabled.get(index).copied().unwrap_or(false)
  }

  pub(crate) fn multiple(&self) -> bool {
    self.inner.lock().unwrap().multiple
  }

  pub(crate) fn style(&self) -> SelectStyle {
    self.inner.lock().unwrap().style.clone()
  }

  pub(crate) fn is_open(&self) -> bool {
    self.inner.lock().unwrap().open
  }

  pub(crate) fn highlighted(&self) -> Option<usize> {
    self.inner.lock().unwrap().highlighted
  }

  pub(crate) fn is_selected(&self, index: usize) -> bool {
    self.inner.lock().unwrap().selected.contains(&index)
  }

  #[cfg(feature = "mcp")]
  pub(crate) fn selected_indices(&self) -> Vec<usize> {
    self.inner.lock().unwrap().selected.clone()
  }

  pub(crate) fn selected_labels(&self) -> Vec<Arc<str>> {
    let inner = self.inner.lock().unwrap();
    inner
      .selected
      .iter()
      .filter_map(|index| inner.labels.get(*index).cloned())
      .collect()
  }

  pub(crate) fn menu_scroll(&self) -> ScrollState {
    self.inner.lock().unwrap().menu_scroll.clone()
  }

  /// The option to scroll into view on the next menu layout, once.
  pub(crate) fn take_reveal(&self) -> Option<usize> {
    self.inner.lock().unwrap().reveal.take()
  }

  /// Enter or Space on the open menu: commit the highlighted option, or close
  /// without a change when nothing is highlighted.
  pub(crate) fn activate(&self) {
    match self.highlighted() {
      Some(index) => self.commit(index),
      None => self.set_open(false),
    }
  }

  /// Open from a pointer press (no highlight) or close.
  pub(crate) fn set_open(&self, open: bool) {
    let mut inner = self.inner.lock().unwrap();
    let changed = inner.open != open;
    if open {
      if changed {
        inner.open_menu(None);
      }
    } else {
      inner.open = false;
      inner.highlighted = None;
      inner.reveal = None;
      inner.type_ahead.clear();
    }
    drop(inner);
    if changed {
      self.mark_layout_dirty();
    }
  }

  /// Open from the keyboard, highlighting the selected option (or the first
  /// enabled one).
  pub(crate) fn open_with_highlight(&self) {
    let mut inner = self.inner.lock().unwrap();
    let highlighted = inner.initial_highlight(true);
    inner.open_menu(highlighted);
    drop(inner);
    self.mark_layout_dirty();
  }

  pub(crate) fn toggle_open(&self) {
    let open = self.inner.lock().unwrap().open;
    self.set_open(!open);
  }

  /// Move the highlight `delta` enabled options, stopping at either end.
  /// From no highlight the first move lands on the selected option, or the
  /// first (moving up: last) enabled option.
  pub(crate) fn move_highlight(&self, delta: i32) {
    let mut inner = self.inner.lock().unwrap();
    let next = match inner.highlighted {
      Some(current) => Some(inner.step_from(current, delta)),
      None => inner.initial_highlight(delta >= 0),
    };
    self.set_highlight(&mut inner, next);
  }

  /// Home / End: highlight the first or last enabled option.
  pub(crate) fn highlight_edge(&self, last: bool) {
    let mut inner = self.inner.lock().unwrap();
    let next = if last {
      inner.last_enabled()
    } else {
      inner.first_enabled()
    };
    self.set_highlight(&mut inner, next);
  }

  /// Whether a type-ahead search is in progress, so Space extends it instead
  /// of committing.
  pub(crate) fn is_typing_ahead(&self, now: Instant) -> bool {
    let inner = self.inner.lock().unwrap();
    !inner.type_ahead.is_empty()
      && inner
        .type_ahead_at
        .is_some_and(|at| now.duration_since(at) < TYPE_AHEAD_TIMEOUT)
  }

  /// Extend the type-ahead search with `ch` and highlight the first enabled
  /// option whose label starts with it. Repeating one character cycles
  /// through the options starting with it.
  pub(crate) fn type_ahead(&self, ch: char, now: Instant) {
    let mut inner = self.inner.lock().unwrap();
    if inner
      .type_ahead_at
      .is_none_or(|at| now.duration_since(at) >= TYPE_AHEAD_TIMEOUT)
    {
      inner.type_ahead.clear();
    }
    inner.type_ahead.extend(ch.to_lowercase());
    inner.type_ahead_at = Some(now);
    let query = inner.type_ahead.clone();
    let mut chars = query.chars();
    let first = chars.next();
    let repeated = first.is_some_and(|first| chars.all(|ch| ch == first));
    let (prefix, skip_current) = match first {
      Some(first) if repeated => (first.to_string(), true),
      _ => (query, false),
    };
    let count = inner.labels.len();
    let start = inner.highlighted.or_else(|| inner.selected.first().copied());
    let offset = match (start, skip_current) {
      (Some(start), true) => start + 1,
      (Some(start), false) => start,
      (None, _) => 0,
    };
    let found = (0..count)
      .map(|step| (offset + step) % count.max(1))
      .find(|index| inner.is_enabled(*index) && inner.labels[*index].to_lowercase().starts_with(prefix.as_str()));
    if found.is_some() {
      self.set_highlight(&mut inner, found);
    }
  }

  fn set_highlight(&self, inner: &mut SelectInner, next: Option<usize>) {
    if next.is_none() || inner.highlighted == next {
      return;
    }
    inner.highlighted = next;
    inner.reveal = next;
    self.mark_layout_dirty();
  }

  pub(crate) fn track_layout(&self, watch: &Arc<crate::core::layout_dirty::LayoutWatch>) {
    self.layout_subscribers.bind(watch);
  }

  pub(crate) fn take_layout_dirty(&self) -> bool {
    self.layout_dirty.swap(false, Ordering::Relaxed)
  }

  pub(crate) fn has_layout_dirty(&self) -> bool {
    self.layout_dirty.load(Ordering::Relaxed)
  }

  fn mark_layout_dirty(&self) {
    self.layout_dirty.store(true, Ordering::Relaxed);
    self.layout_subscribers.mark();
  }

  /// Commit option `index`: fire the change callback and, for single-select,
  /// close the menu. Multi-select keeps the menu open. Disabled options do
  /// nothing.
  pub(crate) fn commit(&self, index: usize) {
    let (callback, multiple) = {
      let inner = self.inner.lock().unwrap();
      if !inner.is_enabled(index) {
        return;
      }
      (inner.on_change.clone(), inner.multiple)
    };
    if let Some(callback) = callback {
      callback(index);
    }
    if !multiple {
      self.set_open(false);
    }
  }

  pub(crate) fn copy_runtime_state_from(&self, old: &SelectState) {
    if Arc::ptr_eq(&self.inner, &old.inner) {
      return;
    }
    let old_inner = old.inner.lock().unwrap();
    let mut inner = self.inner.lock().unwrap();
    inner.open = old_inner.open;
    // The highlight names an option, not a row: it survives only while that
    // row still shows the same enabled option, so Enter never commits an
    // option the user did not move to.
    let highlighted = old_inner
      .highlighted
      .filter(|index| inner.is_enabled(*index) && inner.labels.get(*index) == old_inner.labels.get(*index));
    inner.highlighted = highlighted;
    inner.menu_scroll = old_inner.menu_scroll.clone();
    inner.reveal = old_inner.reveal;
    inner.type_ahead = old_inner.type_ahead.clone();
    inner.type_ahead_at = old_inner.type_ahead_at;
  }
}
