use parking_lot::Mutex;

use super::*;

#[path = "../text_input_composition.rs"]
mod composition;
mod controller_access;
mod editing;
mod geometry;
#[cfg(feature = "markdown")]
mod styled;
#[cfg(not(feature = "markdown"))]
#[path = "styled_off.rs"]
mod styled;
pub(crate) use controller_access::WeakTextInputState;
#[cfg(test)]
#[path = "../../../../tests/input/text_input/source.rs"]
mod source_tests;
mod token_affinity;
mod tokens;
use composition::Composition;
use token_affinity::TokenAffinity;

const MAX_TEXT_INPUT_HISTORY: usize = 128;
type TextInputCallback = EventHandler<TextInputEvent>;

#[derive(Clone)]
pub(crate) struct TextInputState {
  value: Signal<String>,
  inner: Arc<Mutex<TextInputInner>>,
  on_input: Arc<Mutex<Vec<TextInputCallback>>>,
  layout_dirty: Arc<AtomicBool>,
  layout_subscribers: Arc<crate::core::layout_dirty::LayoutSubscribers>,
}

struct TextInputInner {
  tokens: Option<crate::components::InputTokens>,
  token_layout: Option<Arc<crate::layout::layout_engine::input_token_layout::TokenLayout>>,
  token_affinity: Option<TokenAffinity>,
  placeholder: Option<String>,
  last_value: String,
  caret: usize,
  selection_anchor: Option<usize>,
  caret_x: f32,
  caret_y: f32,
  caret_height: f32,
  /// Where a line's glyphs sit in its line box by the font's ascent and
  /// descent, as `(top, height)`; selections cover it.
  text_band: Option<(f32, f32)>,
  caret_positions: CaretPositions,
  scroll_x: f32,
  /// The vertical scroll of a multiline input: the offset it shows its text
  /// from and the scrollbar, wheel and scrollbar drags move.
  scroll: ScrollState,
  /// Whether layout keeps the caret's line in view. An edit or a caret move
  /// sets it; scrolling by wheel or scrollbar clears it, so the text stays
  /// where the person scrolled it until the next edit.
  follow_caret: bool,
  /// The column ArrowUp and ArrowDown keep: the x the first of a run of
  /// vertical moves started from, and the caret that run left. Any other
  /// caret move or edit ends the run, and so does a caret found anywhere
  /// else.
  vertical_goal: Option<(usize, f32)>,
  unfocused_overflow_anchor: TextInputOverflowAnchor,
  overflow: TextInputOverflow,
  min_rows: Option<usize>,
  max_rows: Option<usize>,
  mask: Option<char>,
  focused: bool,
  /// An input method composition in progress, shown at the caret.
  composition: Option<Composition>,
  undo_stack: Vec<TextInputSnapshot>,
  redo_stack: Vec<TextInputSnapshot>,
  /// The spans of a styled input and its hooks; empty for a plain one.
  styled: styled::StyledState,
}

#[derive(Clone, PartialEq)]
struct TextInputSnapshot {
  value: String,
  caret: usize,
  selection_anchor: Option<usize>,
  /// A styled input's spans, so undo restores styles with the text.
  spans: Option<styled::SnapshotSpans>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct TextInputLayoutSignature {
  tokens: Option<crate::components::InputTokens>,
  placeholder: Option<String>,
  unfocused_overflow_anchor: TextInputOverflowAnchor,
  overflow: TextInputOverflow,
  min_rows: Option<usize>,
  max_rows: Option<usize>,
  mask: Option<char>,
  styled: bool,
}

impl TextInputState {
  pub(crate) fn same_value(&self, other: &Self) -> bool {
    self.value.id() == other.value.id()
  }

  /// Id of the bound value signal, which identifies the input across
  /// re-renders.
  pub(crate) fn value_signal_id(&self) -> usize {
    self.value.id()
  }
  pub(crate) fn new(value: Signal<String>) -> Self {
    let initial_value = value.get_untracked();
    let caret = initial_value.len();
    Self {
      value,
      inner: Arc::new(Mutex::new(TextInputInner {
        tokens: None,
        token_layout: None,
        token_affinity: None,
        placeholder: None,
        last_value: initial_value,
        caret,
        selection_anchor: None,
        caret_x: 0.0,
        caret_y: 0.0,
        caret_height: 0.0,
        text_band: None,
        caret_positions: vec![CaretPosition {
          index: 0,
          x: 0.0,
          y: 0.0,
        }]
        .into(),
        scroll_x: 0.0,
        scroll: ScrollState::new(),
        follow_caret: true,
        vertical_goal: None,
        unfocused_overflow_anchor: TextInputOverflowAnchor::default(),
        overflow: TextInputOverflow::default(),
        min_rows: None,
        max_rows: None,
        mask: None,
        focused: false,
        composition: None,
        undo_stack: Vec::new(),
        redo_stack: Vec::new(),
        styled: Default::default(),
      })),
      on_input: Arc::new(Mutex::new(Vec::new())),
      layout_dirty: Arc::new(AtomicBool::new(false)),
      layout_subscribers: Arc::default(),
    }
  }

  pub(crate) fn value(&self) -> String {
    self.value.get_untracked()
  }

  /// Programmatic value write, DOM `el.value = x` semantics: updates the
  /// backing signal (the same one the app holds) and clamps caret/selection
  /// via [`TextInputState::sync_external_value`], but does NOT fire
  /// `on_input` handlers.
  pub(crate) fn set_value(&self, value: impl Into<String>) {
    let value = value.into();
    if self.value.get_untracked() == value {
      return;
    }
    self.value.set(value);
    self.sync_external_value();
  }

  pub(crate) fn set_on_input(&self, f: impl IntoTextInputEventHandler) {
    self.on_input.lock().push(f.into_event_handler());
  }

  pub(crate) fn clear_on_input(&self, f: impl IntoTextInputEventHandler) {
    let handler = f.into_event_handler();
    self.on_input.lock().retain(|existing| !existing.same_handler(&handler));
  }

  /// Replace the value from outside the keyboard path (automation tooling).
  /// Writes the app's value signal and reconciles caret/selection the same
  /// way any external signal write is reconciled; `on_input` handlers do not
  /// fire, matching a controlled update rather than a keystroke.
  #[cfg(feature = "mcp")]
  pub(crate) fn set_value_external(&self, value: String) {
    self.value.set(value);
    self.sync_external_value();
  }

  pub(crate) fn sync_external_value(&self) -> bool {
    self.sync_external_spans();
    let value = self.value();
    let mut inner = self.inner.lock();
    if inner.last_value == value {
      return false;
    }

    let previous_len = inner.last_value.len();
    if inner.selection_anchor.is_none() && inner.caret == previous_len {
      inner.caret = value.len();
    } else {
      inner.caret = clamp_to_char_boundary(&value, inner.caret.min(value.len()));
      inner.selection_anchor = inner
        .selection_anchor
        .map(|anchor| clamp_to_char_boundary(&value, anchor.min(value.len())));
    }
    inner.last_value = value;
    inner.token_affinity = None;
    drop(inner);
    self.mark_layout_dirty();
    true
  }

  pub(crate) fn set_placeholder(&self, placeholder: impl Into<String>) {
    let placeholder = placeholder.into();
    let mut inner = self.inner.lock();
    if inner.placeholder.as_deref() == Some(placeholder.as_str()) {
      return;
    }
    inner.placeholder = Some(placeholder);
    drop(inner);
    self.mark_layout_dirty_keeping_scroll();
  }

  pub(crate) fn placeholder(&self) -> Option<String> {
    self.inner.lock().placeholder.clone()
  }

  pub(crate) fn rendered_text(&self) -> Option<String> {
    if let Some(composed) = self.composed_text() {
      return Some(composed);
    }
    let value = self.value();
    if value.is_empty() {
      self.placeholder()
    } else {
      Some(value)
    }
  }

  pub(crate) fn is_showing_placeholder(&self) -> bool {
    self.value().is_empty() && self.placeholder().is_some() && !self.is_composing()
  }

  pub(crate) fn rendered_text_for_layout(&self) -> String {
    let text = self.rendered_text().unwrap_or_default();
    let text = match self.overflow() {
      TextInputOverflow::Multiline => text,
      TextInputOverflow::Scroll => text.replace(['\r', '\n'], " "),
    };
    match self.mask() {
      Some(mask) if !self.is_showing_placeholder() => mask_text(&text, mask),
      _ => text,
    }
  }

  /// Text used to compute caret geometry. Masked the same way the rendered text
  /// is, so caret x-coordinates line up with the displayed mask glyphs.
  pub(crate) fn caret_source_text(&self) -> String {
    let value = self.value();
    match self.mask() {
      Some(mask) => mask_text(&value, mask),
      None => value,
    }
  }

  /// Text the caret positions are laid out over: the displayed text while an
  /// input method composes (indices then point into it, see
  /// `text_input_composition`), else [`TextInputState::caret_source_text`].
  /// Unmasked Scroll breaks use one space per byte, matching paint without changing source offsets.
  pub(crate) fn caret_layout_text(&self) -> String {
    let text = self.composed_text().unwrap_or_else(|| self.caret_source_text());
    if self.overflow() == TextInputOverflow::Scroll && !self.is_masked() {
      text.replace(['\r', '\n'], " ")
    } else {
      text
    }
  }

  /// Caret positions are computed over the masked string, so their `index` is a
  /// byte offset into the mask — not the real value. Remap each back to the real
  /// value's char boundaries so editing and hit-testing stay correct.
  pub(crate) fn remap_caret_positions(&self, positions: &mut CaretPositions) {
    let Some(mask) = self.mask() else {
      return;
    };
    let mask_len = mask.len_utf8();
    let value = self.value();
    let boundaries: Vec<usize> = value
      .char_indices()
      .map(|(index, _)| index)
      .chain(std::iter::once(value.len()))
      .collect();
    for position in positions.make_mut().iter_mut() {
      let char_index = position.index / mask_len;
      position.index = boundaries.get(char_index).copied().unwrap_or(value.len());
    }
  }

  pub(crate) fn set_mask(&self, mask: Option<char>) {
    let mut inner = self.inner.lock();
    if inner.mask == mask {
      return;
    }
    inner.mask = mask;
    inner.token_affinity = None;
    drop(inner);
    self.mark_layout_dirty_keeping_scroll();
  }

  /// The configured display mask, if this is a masked input.
  pub fn mask(&self) -> Option<char> {
    self.inner.lock().mask
  }

  pub fn is_masked(&self) -> bool {
    self.mask().is_some()
  }
}

impl TextInputState {
  fn closest_caret_to_point(&self, x: f32, y: f32) -> usize {
    if let Some(byte) = self.token_hit(x, y) {
      return byte;
    }
    let value = self.value();
    let inner = self.inner.lock();
    let content_x = x + inner.scroll_x;
    let content_y = y + inner.scroll_y();
    let index = closest_caret_to_point(&inner.caret_positions, content_x, content_y);
    Self::composed_to_value_index(&inner, &value, index)
  }

  fn delete_selection_if_present(&self) -> Option<usize> {
    let mut selected = None;
    {
      let value = self.value();
      let inner = self.inner.lock();
      if let Some(range) = selection_range_indices(&value, inner.selection_anchor, inner.caret) {
        selected = Some(range);
      }
    }

    let (start, end) = selected?;
    let styled = self.styled_before_edit();
    self.value.update(|value| {
      value.replace_range(start..end, "");
    });
    let mut inner = self.inner.lock();
    inner.caret = start;
    inner.selection_anchor = None;
    drop(inner);
    self.styled_after_edit(styled, start..end, 0);
    self.mark_layout_dirty();
    Some(start)
  }

  pub(crate) fn layout_signature(&self) -> TextInputLayoutSignature {
    let inner = self.inner.lock();
    TextInputLayoutSignature {
      tokens: inner.tokens.clone(),
      placeholder: inner.placeholder.clone(),
      unfocused_overflow_anchor: inner.unfocused_overflow_anchor,
      overflow: inner.overflow,
      min_rows: inner.min_rows,
      max_rows: inner.max_rows,
      mask: inner.mask,
      styled: inner.styled.is_bound(),
    }
  }

  pub(crate) fn track_layout(&self, watch: &Arc<crate::core::layout_dirty::LayoutWatch>) {
    self.layout_subscribers.bind(watch);
    self.inner.lock().scroll.track_layout(watch);
  }

  pub(crate) fn take_layout_dirty(&self) -> bool {
    self.layout_dirty.swap(false, Ordering::Relaxed)
  }

  pub(crate) fn has_layout_dirty(&self) -> bool {
    self.layout_dirty.load(Ordering::Relaxed)
  }

  /// Marks an edit or a caret move: lay the input out again and bring the
  /// caret's line into view. It ends a run of vertical moves.
  fn mark_layout_dirty(&self) {
    let mut inner = self.inner.lock();
    inner.follow_caret = true;
    inner.vertical_goal = None;
    drop(inner);
    self.layout_dirty.store(true, Ordering::Relaxed);
    self.layout_subscribers.mark();
  }

  /// Marks a change of how the input looks (its placeholder, mode, rows or
  /// focus) that leaves the scroll where it is.
  fn mark_layout_dirty_keeping_scroll(&self) {
    self.layout_dirty.store(true, Ordering::Relaxed);
    self.layout_subscribers.mark();
  }

  /// Fire `on_input` handlers with the text this edit is about to produce.
  /// `new_value` is only evaluated when handlers are attached.
  fn fire_input(&self, keyboard: &KeyboardEvent, new_value: impl FnOnce() -> String) -> bool {
    let handlers = self.on_input.lock().clone();
    if handlers.is_empty() {
      return true;
    }

    let event = TextInputEvent::new(self.value.clone(), new_value(), keyboard.clone());
    for handler in handlers {
      handler.call(&event);
    }
    self.sync_external_value();
    !event.default_prevented()
  }

  /// The text as it will read after the current selection is removed, plus
  /// the caret position after that removal. The value is unchanged (and the
  /// caret current) when nothing is selected.
  fn value_without_selection(&self) -> (String, usize) {
    let mut value = self.value();
    let inner = self.inner.lock();
    match selection_range_indices(&value, inner.selection_anchor, inner.caret) {
      Some((start, end)) => {
        value.replace_range(start..end, "");
        (value, start)
      }
      None => {
        let caret = inner.caret;
        (value, caret)
      }
    }
  }

  pub(crate) fn has_selection(&self) -> bool {
    let value = self.value();
    let inner = self.inner.lock();
    selection_range_indices(&value, inner.selection_anchor, inner.caret).is_some()
  }

  fn snapshot(&self) -> TextInputSnapshot {
    let value = self.value();
    let spans = self.current_spans();
    let inner = self.inner.lock();
    TextInputSnapshot {
      value,
      caret: inner.caret,
      selection_anchor: inner.selection_anchor,
      spans,
    }
  }

  fn push_undo_snapshot(&self) {
    let snapshot = self.snapshot();
    let mut inner = self.inner.lock();
    push_limited(&mut inner.undo_stack, snapshot);
    inner.redo_stack.clear();
  }

  fn push_undo_snapshot_value(&self, snapshot: TextInputSnapshot) {
    let mut inner = self.inner.lock();
    push_limited(&mut inner.undo_stack, snapshot);
  }

  fn push_redo_snapshot(&self, snapshot: TextInputSnapshot) {
    let mut inner = self.inner.lock();
    push_limited(&mut inner.redo_stack, snapshot);
  }

  fn restore_snapshot(&self, snapshot: TextInputSnapshot) {
    let value = snapshot.value;
    let len = value.len();
    let caret = clamp_to_char_boundary(&value, snapshot.caret.min(len));
    let selection_anchor = snapshot
      .selection_anchor
      .map(|anchor| clamp_to_char_boundary(&value, anchor.min(len)));
    self.value.set(value.clone());
    let mut inner = self.inner.lock();
    inner.last_value = value;
    inner.token_affinity = None;
    inner.caret = caret;
    inner.selection_anchor = selection_anchor;
    drop(inner);
    if let Some(spans) = snapshot.spans {
      self.restore_spans(spans);
    }
  }
}

impl TextInputInner {
  fn scroll_y(&self) -> f32 {
    self.scroll.scroll_y()
  }
}

fn mask_text(text: &str, mask: char) -> String {
  let mut out = String::with_capacity(text.chars().count() * mask.len_utf8());
  for _ in text.chars() {
    out.push(mask);
  }
  out
}

fn push_limited(stack: &mut Vec<TextInputSnapshot>, snapshot: TextInputSnapshot) {
  if stack.last() == Some(&snapshot) {
    return;
  }
  stack.push(snapshot);
  if stack.len() > MAX_TEXT_INPUT_HISTORY {
    stack.remove(0);
  }
}

fn move_inner_to(inner: &mut TextInputInner, target: usize, selecting: bool) {
  if selecting {
    if inner.selection_anchor.is_none() {
      inner.selection_anchor = Some(inner.caret);
    }
  } else {
    inner.selection_anchor = None;
  }
  inner.caret = target;
}
