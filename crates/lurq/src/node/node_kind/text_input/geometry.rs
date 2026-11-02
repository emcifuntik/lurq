use super::*;

impl TextInputState {
  pub(crate) fn copy_runtime_state_from(&self, old: &Self, layout_signature_matches: bool) {
    if Arc::ptr_eq(&self.inner, &old.inner) {
      return;
    }
    let old_inner = old.inner.lock();
    let old_caret = old_inner.caret;
    let old_affinity = old_inner.token_affinity.clone();
    let old_last_value = old_inner.last_value.clone();
    let old_selection_anchor = old_inner.selection_anchor;
    let old_caret_x = old_inner.caret_x;
    let old_caret_y = old_inner.caret_y;
    let old_caret_height = old_inner.caret_height;
    let old_text_band = old_inner.text_band;
    let old_caret_positions = old_inner.caret_positions.clone();
    let old_scroll_x = old_inner.scroll_x;
    let old_scroll = old_inner.scroll.clone();
    let old_follow_caret = old_inner.follow_caret;
    let old_vertical_goal = old_inner.vertical_goal;
    let old_focused = old_inner.focused;
    let old_composition = old_inner.composition.clone();
    let old_undo_stack = old_inner.undo_stack.clone();
    let old_redo_stack = old_inner.redo_stack.clone();
    let old_styled = old_inner.styled.clone();
    let layout_dirty = if layout_signature_matches {
      old.layout_dirty.load(Ordering::Relaxed)
    } else {
      self.layout_dirty.load(Ordering::Relaxed) || old.layout_dirty.load(Ordering::Relaxed)
    };
    let len = self.value().len();
    let mut inner = self.inner.lock();
    inner.last_value = old_last_value;
    inner.caret = old_caret.min(len);
    inner.token_affinity = old_affinity;
    inner.selection_anchor = old_selection_anchor.map(|anchor| anchor.min(len));
    inner.caret_x = old_caret_x;
    inner.caret_y = old_caret_y;
    inner.caret_height = old_caret_height;
    inner.text_band = old_text_band;
    inner.caret_positions = old_caret_positions;
    inner.scroll_x = old_scroll_x;
    inner.scroll = old_scroll;
    inner.follow_caret = old_follow_caret;
    inner.vertical_goal = old_vertical_goal;
    inner.focused = old_focused;
    inner.composition = old_composition;
    inner.undo_stack = old_undo_stack;
    inner.redo_stack = old_redo_stack;
    inner.styled.copy_runtime_state_from(&old_styled);
    self.layout_dirty.store(layout_dirty, Ordering::Relaxed);
  }

  pub(crate) fn sync_caret_metrics_to_position(&self, line_height: f32) {
    if self.sync_token_caret() {
      return;
    }
    let value = self.value();
    let composed = self.composed_text();
    let mut inner = self.inner.lock();
    // While composing, caret positions index the displayed text and the
    // caret sits at the input method's cursor inside the composition.
    let (text, caret) = match composed {
      Some(composed) => {
        let caret = Self::composed_caret(&inner, &value).unwrap_or(inner.caret);
        (composed, caret)
      }
      None => (value, inner.caret),
    };
    let caret = clamp_to_char_boundary(&text, caret);
    if inner.overflow == TextInputOverflow::Multiline
      && caret == text.len()
      && trailing_break_len(&text[..caret]) > 0
      && inner.token_layout.is_none()
    {
      inner.caret_x = 0.0;
      inner.caret_y = trailing_break_row_y(&inner.caret_positions, &text, line_height);
      return;
    }
    inner.caret_x = caret_x_for_index(&inner.caret_positions, caret);
    inner.caret_y = caret_y_for_index(&inner.caret_positions, caret).unwrap_or_else(|| {
      if inner.overflow == TextInputOverflow::Multiline {
        text[..caret].chars().filter(|ch| *ch == '\n').count() as f32 * line_height
      } else {
        0.0
      }
    });
  }

  pub(crate) fn caret_x(&self) -> f32 {
    let inner = self.inner.lock();
    inner.caret_x - inner.scroll_x
  }

  /// Whether the caret is at the left edge of its line's text box (the start
  /// of a left-aligned line), with no glyph before it.
  pub(crate) fn caret_at_line_start(&self) -> bool {
    self.inner.lock().caret_x <= 0.0
  }

  pub(crate) fn caret_y(&self) -> f32 {
    let inner = self.inner.lock();
    inner.caret_y - inner.scroll_y()
  }

  pub(crate) fn set_caret_height(&self, caret_height: f32) {
    self.inner.lock().caret_height = caret_height;
  }

  pub(crate) fn caret_height(&self) -> f32 {
    self.inner.lock().caret_height
  }

  pub(crate) fn set_text_band(&self, band: Option<(f32, f32)>) {
    self.inner.lock().text_band = band;
  }

  /// `(top, height)` of the glyphs' band within a line box: the font's ascent
  /// and descent, centered in the line height. The whole line box until the
  /// input has been laid out.
  pub(crate) fn text_band(&self) -> (f32, f32) {
    let inner = self.inner.lock();
    inner.text_band.unwrap_or((0.0, inner.caret_height))
  }

  pub(crate) fn set_caret_positions(&self, positions: CaretPositions) {
    self.inner.lock().caret_positions = positions;
  }

  pub(crate) fn set_scroll_x(&self, scroll_x: f32) {
    self.inner.lock().scroll_x = scroll_x.max(0.0);
  }

  pub(crate) fn scroll_x(&self) -> f32 {
    self.inner.lock().scroll_x
  }

  /// Records where layout put the text: a `viewport_width` by
  /// `viewport_height` box showing `content_height` of text from `scroll_y`,
  /// with a `scrollbar_lane` wide lane for the bar right of the box.
  pub(crate) fn set_vertical_scroll(
    &self,
    viewport_width: f32,
    viewport_height: f32,
    content_height: f32,
    scroll_y: f32,
    scrollbar_lane: f32,
  ) {
    self.inner.lock().scroll.set_text_layout(
      viewport_width,
      viewport_height,
      content_height,
      scroll_y,
      scrollbar_lane,
    );
  }

  pub(crate) fn scroll_y(&self) -> f32 {
    self.inner.lock().scroll_y()
  }

  /// The scroll a multiline input shows its text with, for the wheel and the
  /// scrollbar; a single-line input scrolls only with its caret.
  pub(crate) fn scroll_state(&self) -> Option<ScrollState> {
    let inner = self.inner.lock();
    (inner.overflow == TextInputOverflow::Multiline).then(|| inner.scroll.clone())
  }

  /// Takes a scroll by wheel or scrollbar since the last layout: from then
  /// on, layout leaves the text where it was scrolled instead of bringing
  /// the caret back, until the next edit or caret move. Returns whether the
  /// input was scrolled.
  pub(crate) fn take_user_scroll(&self) -> bool {
    let mut inner = self.inner.lock();
    let scrolled = inner.scroll.take_scroll_dirty();
    if scrolled {
      inner.follow_caret = false;
    }
    scrolled
  }

  pub(crate) fn follows_caret(&self) -> bool {
    self.inner.lock().follow_caret
  }

  pub(crate) fn set_unfocused_overflow_anchor(&self, anchor: TextInputOverflowAnchor) {
    let mut inner = self.inner.lock();
    if inner.unfocused_overflow_anchor == anchor {
      return;
    }
    inner.unfocused_overflow_anchor = anchor;
    drop(inner);
    self.mark_layout_dirty_keeping_scroll();
  }

  pub(crate) fn unfocused_overflow_anchor(&self) -> TextInputOverflowAnchor {
    self.inner.lock().unfocused_overflow_anchor
  }

  pub(crate) fn set_overflow(&self, overflow: TextInputOverflow) {
    let mut inner = self.inner.lock();
    if inner.overflow == overflow {
      return;
    }
    inner.overflow = overflow;
    drop(inner);
    self.mark_layout_dirty_keeping_scroll();
  }

  pub(crate) fn overflow(&self) -> TextInputOverflow {
    self.inner.lock().overflow
  }

  pub(crate) fn set_rows(&self, min_rows: usize, max_rows: usize) {
    let min_rows = min_rows.max(1);
    let max_rows = max_rows.max(min_rows);
    let mut inner = self.inner.lock();
    if inner.overflow == TextInputOverflow::Multiline
      && inner.min_rows == Some(min_rows)
      && inner.max_rows == Some(max_rows)
    {
      return;
    }
    inner.overflow = TextInputOverflow::Multiline;
    inner.min_rows = Some(min_rows);
    inner.max_rows = Some(max_rows);
    drop(inner);
    self.mark_layout_dirty_keeping_scroll();
  }

  pub(crate) fn set_min_rows(&self, min_rows: usize) {
    let min_rows = min_rows.max(1);
    let mut inner = self.inner.lock();
    let next_max_rows = inner.max_rows.map(|max_rows| max_rows.max(min_rows));
    if inner.overflow == TextInputOverflow::Multiline
      && inner.min_rows == Some(min_rows)
      && inner.max_rows == next_max_rows
    {
      return;
    }
    inner.overflow = TextInputOverflow::Multiline;
    inner.min_rows = Some(min_rows);
    inner.max_rows = next_max_rows;
    drop(inner);
    self.mark_layout_dirty_keeping_scroll();
  }

  pub(crate) fn set_max_rows(&self, max_rows: usize) {
    let max_rows = max_rows.max(1);
    let mut inner = self.inner.lock();
    let next_min_rows = inner.min_rows.map(|min_rows| min_rows.min(max_rows));
    if inner.overflow == TextInputOverflow::Multiline
      && inner.max_rows == Some(max_rows)
      && inner.min_rows == next_min_rows
    {
      return;
    }
    inner.overflow = TextInputOverflow::Multiline;
    inner.max_rows = Some(max_rows);
    inner.min_rows = next_min_rows;
    drop(inner);
    self.mark_layout_dirty_keeping_scroll();
  }

  pub(crate) fn set_rows_exact(&self, rows: usize) {
    let rows = rows.max(1);
    let mut inner = self.inner.lock();
    if inner.overflow == TextInputOverflow::Multiline && inner.min_rows == Some(rows) && inner.max_rows == Some(rows) {
      return;
    }
    inner.overflow = TextInputOverflow::Multiline;
    inner.min_rows = Some(rows);
    inner.max_rows = Some(rows);
    drop(inner);
    self.mark_layout_dirty_keeping_scroll();
  }

  pub(crate) fn row_limits(&self) -> (Option<usize>, Option<usize>) {
    let inner = self.inner.lock();
    (inner.min_rows, inner.max_rows)
  }

  pub(crate) fn selection_ranges(&self) -> Vec<TextSelectionRange> {
    let value = self.value();
    let decorated = self.token_layout();
    let inner = self.inner.lock();
    // A composition is shown in place of the selection.
    if inner.composition.is_some() {
      return Vec::new();
    }
    let Some((start, end)) = selection_range_indices(&value, inner.selection_anchor, inner.caret) else {
      return Vec::new();
    };
    if let Some(layout) = decorated {
      return layout.selection(start..end, (inner.scroll_x, inner.scroll_y()));
    }
    selection_ranges_for_positions(
      &inner.caret_positions,
      None,
      start,
      end,
      inner.scroll_x,
      inner.scroll_y(),
    )
  }

  pub(crate) fn set_focused(&self, focused: bool) {
    let mut inner = self.inner.lock();
    if inner.focused == focused {
      return;
    }
    inner.focused = focused;
    // A composition belongs to the focused input; leaving it cancels it.
    if !focused {
      inner.composition = None;
    }
    drop(inner);
    // Taking the focus shows the caret; losing it leaves the text where it is.
    if focused {
      self.mark_layout_dirty();
    } else {
      self.mark_layout_dirty_keeping_scroll();
    }
  }

  pub(crate) fn is_focused(&self) -> bool {
    self.inner.lock().focused
  }
}
