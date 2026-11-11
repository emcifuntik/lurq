//! An input method composition shown inside a `TextInput`.
//!
//! The composition text (the *preedit*) replaces the selection, or sits at the
//! caret, in what the input displays; the value is unchanged until the input
//! method commits. While composing, the input's caret positions are laid out
//! over the displayed text, so the caret can sit inside the preedit; pointer
//! hit-testing maps them back to the value.

use super::{TextInputInner, TextInputState};
use crate::node::text_selection::{
  TextSelectionRange, clamp_to_char_boundary, selection_range_indices, selection_ranges_for_positions,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Composition {
  text: String,
  /// The input method's cursor in `text` as byte offsets, clamped to char
  /// boundaries. `None` when it hides its caret.
  cursor: Option<(usize, usize)>,
}

/// Where a composition sits in the displayed text.
struct Span {
  /// Byte offset in the value where the composition starts (the caret, or
  /// the selection's start).
  start: usize,
  /// Byte offset in the value where the text after the composition resumes
  /// (the selection's end, or the caret).
  end: usize,
  len: usize,
}

impl TextInputInner {
  fn composition_span(&self, value: &str) -> Option<Span> {
    let composition = self.composition.as_ref()?;
    let (start, end) =
      selection_range_indices(value, self.selection_anchor, self.caret).unwrap_or((self.caret, self.caret));
    let start = clamp_to_char_boundary(value, start);
    let end = clamp_to_char_boundary(value, end).max(start);
    Some(Span {
      start,
      end,
      len: composition.text.len(),
    })
  }
}

impl TextInputState {
  /// Shows `text` as the input method's composition, or ends the
  /// composition when it is empty. Masked inputs take no composition.
  /// Returns whether what the input shows changed.
  pub(crate) fn set_composition(&self, text: String, cursor: Option<(usize, usize)>) -> bool {
    if text.is_empty() {
      return self.clear_composition();
    }
    if self.is_masked() {
      return false;
    }
    let cursor = cursor.map(|(start, end)| {
      let start = clamp_to_char_boundary(&text, start);
      (start, clamp_to_char_boundary(&text, end).max(start))
    });
    let composition = Composition { text, cursor };
    let mut inner = self.inner.lock();
    if inner.composition.as_ref() == Some(&composition) {
      return false;
    }
    inner.composition = Some(composition);
    inner.token_affinity = None;
    drop(inner);
    self.mark_layout_dirty();
    true
  }

  /// Ends the composition without inserting it. Returns whether one was
  /// shown.
  pub(crate) fn clear_composition(&self) -> bool {
    let cleared = self.inner.lock().composition.take().is_some();
    if cleared {
      self.mark_layout_dirty();
    }
    cleared
  }

  pub(crate) fn is_composing(&self) -> bool {
    self.inner.lock().composition.is_some()
  }

  pub(crate) fn composition_text(&self) -> Option<String> {
    let inner = self.inner.lock();
    inner.composition.as_ref().map(|composition| composition.text.clone())
  }

  /// The value as displayed while composing: the composition in place of the
  /// selection, or at the caret.
  pub(crate) fn composed_text(&self) -> Option<String> {
    let value = self.value();
    let inner = self.inner.lock();
    let span = inner.composition_span(&value)?;
    let composition = inner.composition.as_ref()?;
    let mut text = String::with_capacity(value.len() + span.len);
    text.push_str(&value[..span.start]);
    text.push_str(&composition.text);
    text.push_str(&value[span.end..]);
    Some(text)
  }

  /// The caret's index in the displayed text while composing: at the end of
  /// the input method's cursor, or after the composition when it has none.
  pub(super) fn composed_caret(inner: &TextInputInner, value: &str) -> Option<usize> {
    let span = inner.composition_span(value)?;
    let composition = inner.composition.as_ref()?;
    let offset = composition.cursor.map_or(span.len, |(_, end)| end);
    Some(span.start + offset)
  }

  /// Maps an index in the displayed text back to the value: inside the
  /// composition it is where the composition starts.
  pub(super) fn composed_to_value_index(inner: &TextInputInner, value: &str, index: usize) -> usize {
    let Some(span) = inner.composition_span(value) else {
      return index;
    };
    if index <= span.start {
      index
    } else if index < span.start + span.len {
      span.start
    } else {
      clamp_to_char_boundary(value, index - span.start - span.len + span.end)
    }
  }

  /// Where the composition is in the displayed text, [`composed_text`](Self::composed_text), in bytes.
  pub(crate) fn composition_range(&self) -> Option<std::ops::Range<usize>> {
    let value = self.value();
    let inner = self.inner.lock();
    let span = inner.composition_span(&value)?;
    Some(span.start..span.start + span.len)
  }

  /// The caret in the displayed text while composing.
  pub(crate) fn composition_caret(&self) -> Option<usize> {
    let value = self.value();
    let inner = self.inner.lock();
    Self::composed_caret(&inner, &value)
  }

  /// Rows under the composition, for its underline, in the input's content
  /// coordinates like [`TextInputState::selection_ranges`].
  pub(crate) fn composition_ranges(&self) -> Vec<TextSelectionRange> {
    let value = self.value();
    let inner = self.inner.lock();
    let Some(span) = inner.composition_span(&value) else {
      return Vec::new();
    };
    selection_ranges_for_positions(
      &inner.caret_positions,
      None,
      span.start,
      span.start + span.len,
      inner.scroll_x,
      inner.scroll_y(),
    )
  }
}
