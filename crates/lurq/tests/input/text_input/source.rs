//! The exact private editing operations called by runtime clipboard dispatch;
//! these tests do not read or replace the user's system clipboard.
use super::*;
use crate::{
  components::{InputToken, InputTokenStyle, InputTokens},
  node::color::Color,
};

#[test]
fn input_tokens_clipboard_payload_cut_paste_and_undo_remain_raw_source() {
  let source = Signal::new("left {{task.title}} right".to_owned());
  let state = TextInputState::new(source.clone());
  state.set_tokens(
    InputTokens::new(
      &source.get(),
      vec![InputToken::new(
        5..19,
        InputTokenStyle::new(
          crate::node::color::Color::new(1, 2, 3, 255),
          crate::node::color::Color::new(4, 5, 6, 255),
        ),
      )],
    )
    .unwrap(),
  );
  assert!(state.controller_select(5, 19));
  assert_eq!(state.selected_text().as_deref(), Some("{{task.title}}"));
  let event = KeyboardEvent::new("x", "KeyX", false, true, false, false, crate::core::NodeId::UNASSIGNED);
  let clipboard = state.cut_selection(&event).unwrap();
  assert_eq!(clipboard, "{{task.title}}");
  assert_eq!(source.get(), "left  right");
  assert!(state.insert(&clipboard, &event));
  assert_eq!(source.get(), "left {{task.title}} right");
  assert!(state.undo(&event));
  assert_eq!(source.get(), "left  right");
  assert!(state.undo(&event));
  assert_eq!(source.get(), "left {{task.title}} right");
  assert_eq!(state.selected_text().as_deref(), Some("{{task.title}}"));
}

fn filter_cluster_range(shape: &cosmic_text::ShapeLine, range: std::ops::Range<usize>) -> cosmic_text::ShapeLine {
  let mut row = shape.clone();
  for span in &mut row.spans {
    for word in &mut span.words {
      word
        .glyphs
        .retain(|glyph| glyph.start >= range.start && glyph.end <= range.end);
    }
    span.words.retain(|word| !word.glyphs.is_empty());
  }
  // Preserve the paragraph initial directional span even when this row has no glyphs there.
  row
}

#[test]
fn input_tokens_rtl_public_filtered_row_contract_probe() {
  use cosmic_text::{Align, Attrs, Buffer, FontSystem, Hinting, Metrics, Shaping, Wrap};
  let samples = [
    ("שלום {{task.title}} עולם", 60.0),
    ("مرحبا {{task.title}} world", 60.0),
    ("start שלום {{task.title}} עולם end", 80.0),
    ("שלום {{task.title}}   עולם   ", 60.0),
    ("start \u{2067}שלום {{task.title}} עולם\u{2069} end", 60.0),
    ("שלום \u{2066}{{task.title}} world\u{2069} עולם", 60.0),
  ];
  let mut fonts = FontSystem::new();
  for (source, width) in samples {
    let mut buffer = Buffer::new(&mut fonts, Metrics::new(13.0, 19.0));
    buffer.set_size(Some(width), None);
    buffer.set_wrap(Wrap::WordOrGlyph);
    buffer.set_text(source, &Attrs::new(), Shaping::Advanced, Some(Align::Left));
    buffer.shape_until_scroll(&mut fonts, false);
    let shape = buffer.lines[0].shape_opt().unwrap();
    for run in buffer.layout_runs() {
      let Some(from) = run.glyphs.iter().map(|glyph| glyph.start).min() else {
        continue;
      };
      let to = run.glyphs.iter().map(|glyph| glyph.end).max().unwrap();
      let candidate = filter_cluster_range(shape, from..to);
      let rows = candidate.layout(13.0, None, Wrap::None, Some(Align::Left), None, Hinting::Disabled);
      let signature = |glyph: &cosmic_text::LayoutGlyph| (glyph.start, glyph.end, glyph.glyph_id, glyph.level.number());
      let expected: Vec<_> = run.glyphs.iter().map(signature).collect();
      let actual: Vec<_> = rows.iter().flat_map(|row| row.glyphs.iter()).map(signature).collect();
      assert_eq!(
        actual, expected,
        "whole shape cluster filter must preserve cosmic row order for {source:?} ({from}..{to})"
      );
    }
  }
}

fn decorated_state(width: f32) -> (TextInputState, Arc<crate::layout::quad::DecoratedInputLayout>) {
  use crate::{
    app::glyph_engine::{
      GlyphEngine,
      input_tokens::{InputConfig, TokenStyle},
    },
    layout::text_style::TextStyle,
  };
  let raw = "left שלום {{task.title}} עולם right";
  let start = raw.find("{{task.title}}").unwrap();
  let source = Signal::new(raw.to_owned());
  let state = TextInputState::new(source);
  let tokens = InputTokens::new(
    raw,
    vec![InputToken::new(
      start..start + 14,
      InputTokenStyle::new(Color::new(1, 2, 3, 255), Color::new(4, 5, 6, 255)).padding(3.0),
    )],
  )
  .unwrap();
  state.set_tokens(tokens);
  let base = TextStyle {
    font_size: 13.0,
    line_height: 1.4,
    ..TextStyle::default()
  };
  let layout = GlyphEngine::new().decorated_input(InputConfig {
    source: raw.into(),
    base: base.clone(),
    width,
    min_width: width,
    wrap: true,
    styles: vec![TokenStyle {
      range: start..start + 14,
      text: base,
      padding: 3.0,
      fill: None,
      lines: Default::default(),
    }],
  });
  state.set_token_layout(Some(layout.clone()));
  (state, layout)
}

#[test]
fn input_tokens_rtl_pointer_affinity_survives_sync_keyed_reuse_and_exact_raw_edit() {
  let (state, layout) = decorated_state(70.0);
  let pair = layout
    .carets
    .iter()
    .find_map(|a| {
      layout
        .carets
        .iter()
        .find(|b| b.byte == a.byte && b.row != a.row)
        .map(|b| (*a, *b))
    })
    .unwrap();
  for position in [pair.0, pair.1] {
    state.set_caret_from_point(position.x, position.y + position.height * 0.5);
    let selected = state.controller_selection().0;
    state.sync_caret_metrics_to_position(layout.line_height);
    assert!((state.caret_x() - position.x).abs() < 0.01);
    assert_eq!(state.caret_y(), position.y);
    let reused = TextInputState::new(state.value.clone());
    reused.set_tokens(state.tokens().unwrap());
    reused.copy_runtime_state_from(&state, true);
    reused.set_token_layout(Some(layout.clone()));
    reused.sync_caret_metrics_to_position(layout.line_height);
    assert_eq!(reused.caret_y(), position.y);
    assert!((reused.caret_x() - position.x).abs() < 0.01);
    let raw = reused.value();
    let event = KeyboardEvent::new(
      "!",
      "Digit1",
      false,
      false,
      false,
      false,
      crate::core::NodeId::UNASSIGNED,
    );
    assert!(reused.insert("!", &event));
    let mut expected = raw.clone();
    expected.insert(selected, '!');
    assert_eq!(reused.value(), expected);
    assert!(reused.inner.lock().token_affinity.is_none());
    assert!(reused.undo(&event));
    assert_eq!(reused.value(), raw);
    assert!(reused.inner.lock().token_affinity.is_none());
  }
}

#[test]
fn input_tokens_rtl_drag_vertical_and_programmatic_selection_reset_affinity() {
  let (state, layout) = decorated_state(70.0);
  let last = *layout.carets.last().unwrap();
  state.begin_selection_at_point(last.x, last.y + last.height * 0.5);
  let first = layout.carets[0];
  state.update_selection_to_point(first.x, first.y + first.height * 0.5);
  state.sync_caret_metrics_to_position(layout.line_height);
  assert_eq!(state.caret_y(), first.y);
  assert!(state.move_token_vertical(true, true));
  state.sync_caret_metrics_to_position(layout.line_height);
  assert!(state.caret_y() > first.y);
  let byte = state.inner.lock().caret;
  assert!(state.controller_select(byte, byte));
  assert!(state.inner.lock().token_affinity.is_none());
  state.sync_caret_metrics_to_position(layout.line_height);
  assert_eq!(state.caret_y(), layout.carets[layout.default_caret(byte)].y);
  let mut changed = layout.config.clone();
  changed.width = 90.0;
  changed.min_width = 90.0;
  let changed = crate::app::glyph_engine::GlyphEngine::new().decorated_input(changed);
  state.set_token_layout(Some(changed));
  assert!(state.inner.lock().token_affinity.is_none());
  state.set_mask(Some('*'));
  assert!(state.token_layout().is_none());
}

#[test]
fn input_tokens_rtl_external_and_ime_entry_reset_spatial_affinity() {
  let (state, layout) = decorated_state(70.0);
  let point = layout.carets.last().unwrap();
  state.set_caret_from_point(point.x, point.y + 4.0);
  assert!(state.inner.lock().token_affinity.is_some());
  assert!(state.set_composition("語".into(), Some((3, 3))));
  assert!(state.inner.lock().token_affinity.is_none());
  assert!(state.token_layout().is_none());
  state.clear_composition();
  state.set_caret_from_point(point.x, point.y + 4.0);
  state.value.set("external value".to_owned());
  state.sync_external_value();
  assert!(state.inner.lock().token_affinity.is_none());
  assert!(state.token_layout().is_none());
}

#[test]
fn input_tokens_rtl_both_bidi_boundary_sides_retain_the_clicked_caret() {
  let (state, layout) = decorated_state(500.0);
  let pair = layout
    .carets
    .iter()
    .find_map(|a| {
      layout
        .carets
        .iter()
        .find(|b| b.byte == a.byte && b.row == a.row && (b.x - a.x).abs() > 2.0)
        .map(|b| (*a, *b))
    })
    .unwrap();
  for point in [pair.0, pair.1] {
    state.set_caret_from_point(point.x, point.y + point.height * 0.5);
    state.sync_caret_metrics_to_position(layout.line_height);
    assert!((state.caret_x() - point.x).abs() < 0.01);
    assert_eq!(state.caret_y(), point.y);
    state.set_token_layout(Some(layout.clone()));
    state.sync_caret_metrics_to_position(layout.line_height);
    assert!((state.caret_x() - point.x).abs() < 0.01);
  }
}

#[test]
fn input_tokens_scroll_raw_crlf_cut_paste_and_undo_payload_is_not_flattened() {
  let raw = "before\r\n{{task.title}}\nafter";
  let source = Signal::new(raw.to_owned());
  let state = TextInputState::new(source.clone());
  state.set_tokens(
    InputTokens::new(
      raw,
      vec![InputToken::new(
        8..22,
        InputTokenStyle::new(Color::new(1, 2, 3, 255), Color::new(4, 5, 6, 255)),
      )],
    )
    .unwrap(),
  );
  assert_eq!(state.rendered_text_for_layout(), "before  {{task.title}} after");
  assert!(state.controller_select(6, 8));
  assert_eq!(state.selected_text().as_deref(), Some("\r\n"));
  let event = KeyboardEvent::new("x", "KeyX", false, true, false, false, crate::core::NodeId::UNASSIGNED);
  assert_eq!(state.cut_selection(&event).as_deref(), Some("\r\n"));
  assert_eq!(source.get(), "before{{task.title}}\nafter");
  assert!(state.insert("\r\n", &event));
  assert_eq!(source.get(), raw);
  assert!(state.undo(&event));
  assert_eq!(source.get(), "before{{task.title}}\nafter");
  assert!(state.undo(&event));
  assert_eq!(source.get(), raw);
}
