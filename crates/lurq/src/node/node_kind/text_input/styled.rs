//! Styled spans of a text input: kept in the app's spans signal, moved with
//! every edit, recorded in the undo history with the text, and reported to
//! the app after every change the input makes.
use std::ops::Range;

use super::*;
use crate::components::{
  LinkClick, SelectionStyle, SpanStyle, StyledChange, StyledEdit, StyledSpan, StyledText, TextMark,
  styled_text as spans_ops,
};

type ChangeHandler = Arc<dyn Fn(&StyledChange) + Send + Sync>;
type CopyFormat = Arc<dyn Fn(&StyledText) -> String + Send + Sync>;
type LinkHandler = Arc<dyn Fn(&LinkClick) + Send + Sync>;

/// The spans in canonical form for one version of the value and of the
/// spans signal, so reading them copies nothing until either changes.
#[derive(Clone)]
struct SpansCache {
  value_version: u64,
  spans_version: u64,
  spans: Arc<Vec<StyledSpan>>,
}

/// What makes an input styled. Its hooks come from the builder on every
/// render; the rest is runtime state kept across renders.
#[derive(Clone, Default)]
pub(super) struct StyledState {
  spans: Option<Signal<Vec<StyledSpan>>>,
  /// The spans signal's version the input last wrote or read, to notice an
  /// app's write and lay the input out again.
  synced_version: u64,
  cache: Option<SpansCache>,
  /// The style the next typed text takes, set by the app, while the caret
  /// stays at the offset it was set at with nothing selected.
  pending: Option<(usize, SpanStyle)>,
  on_change: Vec<ChangeHandler>,
  copy_format: Option<CopyFormat>,
  on_link_click: Vec<LinkHandler>,
}

impl StyledState {
  pub(super) fn is_bound(&self) -> bool {
    self.spans.is_some()
  }

  pub(super) fn copy_runtime_state_from(&mut self, old: &Self) {
    if let (Some(new), Some(previous)) = (&self.spans, &old.spans)
      && new.id() == previous.id()
    {
      self.synced_version = old.synced_version;
      self.cache = old.cache.clone();
    }
    self.pending = old.pending.clone();
  }
}

/// The spans and the typing style captured before an edit changes the text.
pub(super) struct SpansBeforeEdit {
  spans: Arc<Vec<StyledSpan>>,
  style: SpanStyle,
}

/// The spans a snapshot of the undo history keeps, shared.
pub(super) type SnapshotSpans = Arc<Vec<StyledSpan>>;

/// Text inserted with its own spans.
pub(super) type InsertedText = StyledText;

impl TextInputState {
  pub(crate) fn set_spans_signal(&self, spans: Signal<Vec<StyledSpan>>) {
    let mut inner = self.inner.lock();
    if inner
      .styled
      .spans
      .as_ref()
      .is_some_and(|bound| bound.id() == spans.id())
    {
      return;
    }
    inner.styled.synced_version = spans.version();
    inner.styled.cache = None;
    inner.styled.spans = Some(spans);
    drop(inner);
    self.mark_layout_dirty_keeping_scroll();
  }

  pub(crate) fn add_styled_change_handler(&self, handler: impl Fn(&StyledChange) + Send + Sync + 'static) {
    self.inner.lock().styled.on_change.push(Arc::new(handler));
  }

  pub(crate) fn set_copy_format(&self, format: impl Fn(&StyledText) -> String + Send + Sync + 'static) {
    self.inner.lock().styled.copy_format = Some(Arc::new(format));
  }

  pub(crate) fn add_link_click_handler(&self, handler: impl Fn(&LinkClick) + Send + Sync + 'static) {
    self.inner.lock().styled.on_link_click.push(Arc::new(handler));
  }

  /// Whether the input edits styled text: it has a spans signal and is not
  /// masked.
  pub(crate) fn is_styled(&self) -> bool {
    let inner = self.inner.lock();
    inner.styled.spans.is_some() && inner.mask.is_none()
  }

  /// The value. A copy: the value signal's own allocation is never shared,
  /// since `Signal::update` writes it in place.
  fn shared_value(&self) -> String {
    self.value()
  }

  /// The spans of the current text in canonical form, shared; `None` for a
  /// plain input. They are normalized again only after the value or the
  /// spans signal changed.
  pub(crate) fn current_spans(&self) -> Option<SnapshotSpans> {
    let (signal, cache) = {
      let inner = self.inner.lock();
      if inner.mask.is_some() {
        return None;
      }
      (inner.styled.spans.clone()?, inner.styled.cache.clone())
    };
    let versions = (self.value.version(), signal.version());
    if let Some(cache) = cache.filter(|cache| (cache.value_version, cache.spans_version) == versions) {
      return Some(cache.spans);
    }
    let value = self.shared_value();
    let spans = Arc::new(signal.with_untracked(|spans| spans_ops::normalize(&value, spans.clone())));
    self.inner.lock().styled.cache = Some(SpansCache {
      value_version: versions.0,
      spans_version: versions.1,
      spans: spans.clone(),
    });
    Some(spans)
  }

  pub(crate) fn styled_text(&self) -> Option<StyledText> {
    let spans = self.current_spans()?;
    Some(StyledText {
      text: self.value(),
      spans: spans.to_vec(),
    })
  }

  /// What the input draws: the text, with a composition in progress in the
  /// style typing would give it, and its spans. `None` for a plain input
  /// and for an empty one showing its placeholder.
  pub(crate) fn styled_display(&self) -> Option<(Arc<str>, SnapshotSpans)> {
    let spans = self.current_spans()?;
    let Some(composed) = self.composed_text() else {
      if self.is_showing_placeholder() {
        return None;
      }
      return Some((self.shared_value().into(), spans));
    };
    let value = self.shared_value();
    let (start, end) = self.selection_or_caret(&value);
    let inserted = composed.len() + (end - start) - value.len();
    let style = self.typing_style_for(&value, &spans);
    let spans = spans_ops::splice(&spans, start..end, inserted, &style);
    Some((composed.into(), Arc::new(spans)))
  }

  /// Notices an app's write of the spans signal: the input lays out again
  /// and drops a pending typing style.
  pub(super) fn sync_external_spans(&self) {
    let mut inner = self.inner.lock();
    let Some(version) = inner.styled.spans.as_ref().map(Signal::version) else {
      return;
    };
    if version == inner.styled.synced_version {
      return;
    }
    inner.styled.synced_version = version;
    inner.styled.pending = None;
    drop(inner);
    self.mark_layout_dirty_keeping_scroll();
  }

  /// Captures what an edit needs before it changes the text.
  pub(super) fn styled_before_edit(&self) -> Option<SpansBeforeEdit> {
    let spans = self.current_spans()?;
    let style = self.typing_style_for(&self.shared_value(), &spans);
    Some(SpansBeforeEdit { spans, style })
  }

  /// Moves the spans over an edit that replaced `range` with `inserted`
  /// bytes typed in the captured style, and reports the change.
  pub(super) fn styled_after_edit(&self, edit: Option<SpansBeforeEdit>, range: Range<usize>, inserted: usize) {
    let Some(edit) = edit else {
      return;
    };
    let spans = spans_ops::splice(&edit.spans, range.clone(), inserted, &edit.style);
    self.commit_spans(
      spans,
      StyledEdit::Text {
        replaced: range,
        inserted,
      },
    );
  }

  /// Like [`Self::styled_after_edit`] for inserted text that brings its own
  /// spans, relative to its start.
  pub(super) fn styled_after_insert(
    &self,
    edit: Option<SpansBeforeEdit>,
    range: Range<usize>,
    inserted: &InsertedText,
  ) {
    let Some(edit) = edit else {
      return;
    };
    let start = range.start;
    let mut spans = spans_ops::splice(&edit.spans, range.clone(), inserted.text.len(), &SpanStyle::default());
    spans.extend(
      inserted
        .spans
        .iter()
        .map(|span| StyledSpan::new(span.range.start + start..span.range.end + start, span.style.clone())),
    );
    let spans = spans_ops::normalize(&self.shared_value(), spans);
    self.commit_spans(
      spans,
      StyledEdit::Text {
        replaced: range,
        inserted: inserted.text.len(),
      },
    );
  }

  /// Puts back the spans of an undo snapshot.
  pub(super) fn restore_spans(&self, spans: SnapshotSpans) {
    self.commit_spans(spans.to_vec(), StyledEdit::Replaced);
  }

  /// Writes `spans` for the current text and reports the change to the
  /// app's change handlers, sharing the spans with them.
  fn commit_spans(&self, spans: Vec<StyledSpan>, edit: StyledEdit) {
    let (signal, handlers) = {
      let mut inner = self.inner.lock();
      let Some(signal) = inner.styled.spans.clone() else {
        return;
      };
      inner.styled.pending = None;
      (signal, inner.styled.on_change.clone())
    };
    let shared = Arc::new(spans);
    if signal.with_untracked(|current| current != shared.as_ref()) {
      signal.set(shared.to_vec());
    }
    {
      let mut inner = self.inner.lock();
      inner.styled.synced_version = signal.version();
      inner.styled.cache = Some(SpansCache {
        value_version: self.value.version(),
        spans_version: signal.version(),
        spans: shared.clone(),
      });
    }
    if handlers.is_empty() {
      return;
    }
    let change = StyledChange {
      value: self.value.clone(),
      spans: shared,
      edit,
    };
    for handler in handlers {
      handler(&change);
    }
  }

  /// The marks over the selection, or of the next typed text.
  pub(crate) fn selection_style(&self) -> Option<SelectionStyle> {
    let spans = self.current_spans()?;
    let value = self.shared_value();
    let (start, end) = self.selection_or_caret(&value);
    if start < end {
      return Some(spans_ops::summarize(&spans, value.len(), start..end));
    }
    Some(SelectionStyle::of_style(&self.typing_style_for(&value, &spans)))
  }

  /// Turns `mark` on over the selection unless all of it has it, then off;
  /// with nothing selected, for the next typed text. `false` for a plain
  /// input.
  pub(crate) fn toggle_mark(&self, mark: TextMark) -> bool {
    let Some(style) = self.selection_style() else {
      return false;
    };
    let on = style.mark(mark) != crate::components::MarkPresence::All;
    self.set_mark(mark, on)
  }

  pub(crate) fn set_mark(&self, mark: TextMark, on: bool) -> bool {
    self.restyle_selection(|style| *style = std::mem::take(style).with(mark, on))
  }

  /// Links the selection to `target`, or unlinks it with `None`. With
  /// nothing selected it changes the link the caret is in, else the next
  /// typed text.
  pub(crate) fn set_link(&self, target: Option<Arc<str>>) -> bool {
    let Some(spans) = self.current_spans() else {
      return false;
    };
    let value = self.shared_value();
    let (start, end) = self.selection_or_caret(&value);
    if start == end
      && let Some(range) = link_range(&spans, start).filter(|range| range.start < start && start < range.end)
    {
      return self.restyle_range(&spans, range, |style| style.link = target.clone());
    }
    self.restyle_selection(|style| style.link = target.clone())
  }

  /// Sets the style the next typed text takes at the caret; `None` goes
  /// back to the style of the text before the caret.
  pub(crate) fn set_pending_style(&self, style: Option<SpanStyle>) -> bool {
    if !self.is_styled() {
      return false;
    }
    let mut inner = self.inner.lock();
    inner.styled.pending = style.map(|style| (inner.caret, style));
    true
  }

  /// The selection with its spans, offsets from its start.
  pub(crate) fn selected_styled_text(&self) -> Option<StyledText> {
    let spans = self.current_spans()?;
    let value = self.shared_value();
    let (start, end) = self.selection_or_caret(&value);
    Some(StyledText {
      text: value[start..end].to_owned(),
      spans: spans_ops::slice(&spans, start..end),
    })
  }

  /// What copying the selection puts on the clipboard: the app's format of
  /// the styled selection when it gave one, else the selected text.
  pub(crate) fn clipboard_selection(&self) -> Option<String> {
    let selected = self.selected_text()?;
    let format = self.inner.lock().styled.copy_format.clone();
    match (format, self.selected_styled_text()) {
      (Some(format), Some(styled)) => Some(format(&styled)),
      _ => Some(selected),
    }
  }

  /// Reports the link under a point of the content box to the input's link
  /// handlers. `false` when no handler or no link is there.
  pub(crate) fn click_link_at_point(&self, x: f32, y: f32) -> bool {
    let handlers = self.inner.lock().styled.on_link_click.clone();
    if handlers.is_empty() {
      return false;
    }
    let Some(click) = self.link_at_point(x, y) else {
      return false;
    };
    for handler in handlers {
      handler(&click);
    }
    true
  }

  /// The link drawn under a point of the content box.
  fn link_at_point(&self, x: f32, y: f32) -> Option<LinkClick> {
    let spans = self.current_spans()?;
    let layout = self.token_layout()?;
    let (scroll_x, scroll_y) = {
      let inner = self.inner.lock();
      (inner.scroll_x, inner.scroll_y())
    };
    let cluster = layout.cluster_at(x + scroll_x, y + scroll_y)?;
    let range = link_range(&spans, cluster.start)?;
    let target = spans_ops::style_at(&spans, cluster.start).link?;
    Some(LinkClick { target, range })
  }

  fn restyle_selection(&self, change: impl Fn(&mut SpanStyle)) -> bool {
    let Some(spans) = self.current_spans() else {
      return false;
    };
    let value = self.shared_value();
    let (start, end) = self.selection_or_caret(&value);
    if start == end {
      let mut style = self.typing_style_for(&value, &spans);
      change(&mut style);
      let mut inner = self.inner.lock();
      inner.styled.pending = Some((inner.caret, style));
      return true;
    }
    self.restyle_range(&spans, start..end, change)
  }

  fn restyle_range(&self, spans: &[StyledSpan], range: Range<usize>, change: impl Fn(&mut SpanStyle)) -> bool {
    let restyled = spans_ops::restyle(spans, self.shared_value().len(), range.clone(), change);
    if restyled.as_slice() == spans {
      return false;
    }
    self.push_undo_snapshot();
    self.commit_spans(restyled, StyledEdit::Style { range });
    self.mark_layout_dirty();
    true
  }

  /// The style typed text takes over the selection or at the caret.
  fn typing_style_for(&self, value: &str, spans: &[StyledSpan]) -> SpanStyle {
    let (start, end) = self.selection_or_caret(value);
    let inner = self.inner.lock();
    if let Some((caret, style)) = &inner.styled.pending
      && start == end
      && *caret == start
    {
      return style.clone();
    }
    drop(inner);
    spans_ops::insertion_style(value, spans, start, end)
  }

  fn selection_or_caret(&self, value: &str) -> (usize, usize) {
    let inner = self.inner.lock();
    selection_range_indices(value, inner.selection_anchor, inner.caret).unwrap_or_else(|| {
      let caret = clamp_to_char_boundary(value, inner.caret);
      (caret, caret)
    })
  }
}

/// The whole link around `offset`: the spans next to each other that carry
/// the link of the span at `offset` (or ending there).
fn link_range(spans: &[StyledSpan], offset: usize) -> Option<Range<usize>> {
  let index = spans
    .iter()
    .position(|span| span.range.start <= offset && offset < span.range.end && span.style.link.is_some())
    .or_else(|| {
      spans
        .iter()
        .position(|span| span.range.end == offset && span.style.link.is_some())
    })?;
  let target = spans[index].style.link.clone();
  let mut first = index;
  while first > 0 && spans[first - 1].range.end == spans[first].range.start && spans[first - 1].style.link == target {
    first -= 1;
  }
  let mut last = index;
  while last + 1 < spans.len()
    && spans[last + 1].range.start == spans[last].range.end
    && spans[last + 1].style.link == target
  {
    last += 1;
  }
  Some(spans[first].range.start..spans[last].range.end)
}
