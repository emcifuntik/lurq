use cosmic_text::{Align, Attrs, Buffer, Hinting, Metrics, ShapeSpan, Shaping, Wrap};

use super::*;

fn config(source: &str, wrap: bool) -> InputConfig {
  let start = source.find("{{task.title}}").unwrap();
  let base = TextStyle {
    font_size: 13.0,
    line_height: 1.4,
    ..TextStyle::default()
  };
  InputConfig {
    source: source.into(),
    base: base.clone(),
    width: 300.0,
    min_width: 300.0,
    wrap,
    styles: vec![TokenStyle {
      range: start..start + 14,
      text: base,
      padding: 3.0,
      fill: None,
      lines: Default::default(),
    }],
  }
}

#[test]
fn input_tokens_scroll_single_line_presentation_preserves_every_raw_break_byte() {
  let mut engine = GlyphEngine::new();
  let raw = "before\r\n{{task.title}}\nafter\n";
  let output = engine.decorated_input(config(raw, false));
  assert_eq!(output.rows.len(), 1);
  assert_eq!(output.rows[0].range, 0..raw.len());
  assert!(output.carets.iter().all(|caret| caret.row == 0 && caret.y == 0.0));
  for byte in [6, 7, 8, 22, 23, raw.len()] {
    assert_eq!(output.carets[output.default_caret(byte)].byte, byte);
  }
  assert_eq!(output.config.source.as_ref(), raw);
  let multiline = engine.decorated_input(config(raw, true));
  assert!(multiline.rows.len() >= 4);
  assert_eq!(multiline.carets[multiline.default_caret(raw.len())].byte, raw.len());
}

#[test]
fn input_tokens_scroll_overwide_rtl_geometry_is_positive_and_still_byte_mapped() {
  use crate::layout::text_style::TextAlign;
  let raw = "שלום שלום {{task.title}} עולם עולם";
  for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right, TextAlign::End] {
    let mut config = config(raw, false);
    config.width = 70.0;
    config.min_width = 70.0;
    config.base.text_align = align;
    let output = GlyphEngine::new().decorated_input(config);
    assert!(output.size.width > output.viewport_width);
    assert_eq!(output.rows[0].left, 0.0);
    assert!(
      output
        .carets
        .iter()
        .all(|caret| caret.x >= 0.0 && raw.is_char_boundary(caret.byte))
    );
    for byte in [0, raw.len()] {
      let point = output.carets[output.default_caret(byte)];
      assert_eq!(output.carets[output.hit(point.x, point.height * 0.5)].byte, byte);
    }
  }
}

#[test]
fn input_tokens_context_empty_intermediate_span_keeps_public_shaped_order() {
  let raw = "left שלום {{task.title}} world";
  let config = config(raw, true);
  let mut engine = GlyphEngine::new();
  let mut buffer = Buffer::new(&mut engine.font_system, Metrics::new(13.0, 18.2));
  buffer.set_text(raw, &Attrs::new(), Shaping::Advanced, None);
  let mut shape = buffer.lines[0].shape(&mut engine.font_system, 8).clone();
  assert!(shape.spans.len() >= 3);
  let level = shape.spans[1].level;
  shape.spans.insert(
    1,
    ShapeSpan {
      level,
      words: Vec::new(),
      decoration_spans: Vec::new(),
    },
  );
  let signature = |glyph: &cosmic_text::LayoutGlyph| (glyph.start, glyph.end, glyph.glyph_id, glyph.level);
  let visible = |glyph: &cosmic_text::LayoutGlyph| !raw[glyph.start..glyph.end].chars().all(char::is_whitespace);
  let expected: Vec<_> = shape
    .layout(
      13.0,
      Some(300.0),
      Wrap::None,
      Some(Align::Left),
      None,
      Hinting::Disabled,
    )
    .into_iter()
    .flat_map(|row| row.glyphs)
    .filter(visible)
    .map(|glyph| signature(&glyph))
    .collect();
  let mut prepared = Vec::new();
  rows::paragraph(&mut prepared, &shape, 0, raw.len(), &config);
  let mut output = DecoratedInputLayout {
    config,
    revision: 0,
    glyphs: Vec::new(),
    fragments: Vec::new(),
    carets: Vec::new(),
    rows: Vec::new(),
    intervals: Vec::new(),
    size: Size::default(),
    line_height: 1.0,
    viewport_width: 0.0,
  };
  rows::finish(&mut output, prepared);
  output.finish_carets();
  output.align_viewport();
  assert_eq!(
    output
      .glyphs
      .iter()
      .map(|glyph| signature(&glyph.glyph))
      .collect::<Vec<_>>(),
    expected
  );
  assert_eq!(output.config.source.as_ref(), raw);
}
