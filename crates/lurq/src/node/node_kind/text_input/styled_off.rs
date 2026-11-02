//! Without the `markdown` feature no input is styled. These stand in for the
//! styled-span hooks of the editing paths and compile to nothing.
use std::{convert::Infallible, ops::Range};

use super::*;

#[derive(Clone, Default)]
pub(super) struct StyledState;

impl StyledState {
  pub(super) fn is_bound(&self) -> bool {
    false
  }

  pub(super) fn copy_runtime_state_from(&mut self, _old: &Self) {}
}

pub(super) enum SpansBeforeEdit {}

pub(super) type SnapshotSpans = Infallible;

pub(super) enum InsertedText {}

impl TextInputState {
  #[inline]
  pub(super) fn sync_external_spans(&self) {}

  #[inline]
  pub(super) fn current_spans(&self) -> Option<SnapshotSpans> {
    None
  }

  #[inline]
  pub(super) fn styled_before_edit(&self) -> Option<SpansBeforeEdit> {
    None
  }

  #[inline]
  pub(super) fn styled_after_edit(&self, _edit: Option<SpansBeforeEdit>, _range: Range<usize>, _inserted: usize) {}

  #[inline]
  pub(super) fn styled_after_insert(
    &self,
    _edit: Option<SpansBeforeEdit>,
    _range: Range<usize>,
    inserted: &InsertedText,
  ) {
    match *inserted {}
  }

  #[inline]
  pub(super) fn restore_spans(&self, spans: SnapshotSpans) {
    match spans {}
  }

  /// What copying the selection puts on the clipboard: the selected text.
  pub(crate) fn clipboard_selection(&self) -> Option<String> {
    self.selected_text()
  }

  /// No input has links without the `markdown` feature.
  pub(crate) fn click_link_at_point(&self, _x: f32, _y: f32) -> bool {
    false
  }
}
