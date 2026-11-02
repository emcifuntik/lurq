use cosmic_text::{Buffer, LineEnding, LineIter, Metrics, Shaping};

use super::*;

const TOKEN: &str = "{{task.title}}";
const RAW: &str = "é\r\n{{task.title}}\n🙂\r\n\r\n";

fn config(raw: &str, width: f32, padding: f32) -> InputConfig {
  let base = TextStyle {
    font_size: 13.0,
    line_height: 1.4,
    ..TextStyle::default()
  };
  InputConfig {
    source: raw.into(),
    base: base.clone(),
    width,
    min_width: 0.0,
    wrap: true,
    styles: raw
      .match_indices(TOKEN)
      .map(|(start, _)| TokenStyle {
        range: start..start + TOKEN.len(),
        text: base.clone(),
        padding,
        fill: None,
        lines: Default::default(),
      })
      .collect(),
  }
}

#[test]
fn input_tokens_crlf_original_p2_has_five_canonical_rows_and_endings() {
  for padding in [0.0, 3.0] {
    let output = GlyphEngine::new().decorated_input(config(RAW, 400.0, padding));
    assert_eq!(output.rows.len(), 5, "one canonical visual break per actual CRLF");
    assert_eq!(
      output.rows.iter().map(|row| row.range.clone()).collect::<Vec<_>>(),
      [0..2, 4..18, 19..23, 25..25, 27..27]
    );
    for (byte, row) in [(2, 0), (3, 0), (4, 1), (27, 4)] {
      let caret = output.carets[output.default_caret(byte)];
      assert_eq!((caret.byte, caret.row), (byte, row));
    }
    let edge = output.carets[output.default_caret(2)];
    let interior = output.carets[output.default_caret(3)];
    assert_eq!((edge.x, edge.y), (interior.x, interior.y));
    assert_eq!(output.config.source.as_ref(), RAW);
  }
}

#[test]
fn input_tokens_crlf_canonical_endings_unicode_metadata_and_buffer_reuse() {
  let mut engine = GlyphEngine::new();
  for raw in [
    "",
    "\r\n",
    "\r\n{{task.title}}\r\n\r\n",
    "a\r\n{{task.title}}",
    "é\r\n{{task.title}}\r\n字 {{task.title}}\n🙂",
    "{{task.title}}\rnext\nlast\n\r",
  ] {
    for padding in [0.0, 3.0] {
      let mut cfg = config(raw, 800.0, padding);
      for (index, style) in cfg.styles.iter_mut().enumerate() {
        style.text.color = Color::new(10 + index as u8, 22, 33, 255);
      }
      let output = engine.decorated_input(cfg.clone());
      let mut expected: Vec<_> = LineIter::new(raw).map(|(range, _)| range).collect();
      if expected.is_empty() || raw.ends_with(['\r', '\n']) {
        expected.push(raw.len()..raw.len());
      }
      assert_eq!(
        output.rows.iter().map(|row| row.range.clone()).collect::<Vec<_>>(),
        expected,
        "{raw:?}"
      );
      for style in &cfg.styles {
        let glyphs: Vec<_> = output
          .glyphs
          .iter()
          .filter(|glyph| style.range.contains(&glyph.glyph.start))
          .collect();
        assert!(!glyphs.is_empty());
        assert!(glyphs.iter().all(|glyph| glyph.color == style.text.color));
        assert_eq!(
          glyphs.iter().map(|glyph| glyph.glyph.start).min(),
          Some(style.range.start)
        );
        assert_eq!(glyphs.iter().map(|glyph| glyph.glyph.end).max(), Some(style.range.end));
      }
      assert_eq!(output.carets[output.default_caret(raw.len())].byte, raw.len());
    }
  }
}

#[test]
fn input_tokens_crlf_wrapped_ending_uses_the_actual_last_visual_row() {
  for raw in [
    "long words words {{task.title}} words\r\n次",
    "long words {{task.title}}   \r\n次",
    "שלום {{task.title}} עולם מילים\r\n次",
  ] {
    for padding in [0.0, 3.0] {
      let ending = raw.find('\r').unwrap();
      let output = GlyphEngine::new().decorated_input(config(raw, 45.0, padding));
      let before = output.carets[output.default_caret(ending)];
      let interior = output.carets[output.default_caret(ending + 1)];
      let after = output.carets[output.default_caret(ending + 2)];
      assert!(before.row > 1, "preceding paragraph really wraps");
      assert_eq!(
        before.row,
        output.rows.iter().rposition(|row| row.ending.is_some()).unwrap()
      );
      assert_eq!((before.x, before.y, before.row), (interior.x, interior.y, interior.row));
      assert_eq!(after.row, before.row + 1);
      assert!((after.y - before.y - output.rows[before.row].height).abs() < 0.01);
      assert_eq!(output.carets[output.hit(interior.x, interior.y + 1.0)].row, before.row);
    }
  }
}

#[test]
fn input_tokens_crlf_unstyled_glyphs_match_canonical_public_buffer() {
  for raw in [RAW, "a\r\n{{task.title}}", "שלום\r\nمرحبا {{task.title}}\n\r最後"] {
    let mut engine = GlyphEngine::new();
    let mut cfg = config(raw, 500.0, 0.0);
    cfg.styles.clear();
    let output = engine.decorated_input(cfg.clone());
    let family = engine.resolve_family(&cfg.base);
    let attrs = attrs_for_style(
      &cfg.base,
      &family,
      engine.face_weight(&family, &cfg.base),
      cfg.base.font_size,
    )
    .metrics(Metrics::new(
      cfg.base.font_size,
      cfg.base.font_size * cfg.base.line_height,
    ));
    let mut buffer = Buffer::new(&mut engine.font_system, Metrics::new(cfg.base.font_size, 18.2));
    buffer.set_text(raw, &attrs, Shaping::Advanced, None);
    let mut offset = 0;
    let mut expected = Vec::new();
    for line in &mut buffer.lines {
      for row in line.shape(&mut engine.font_system, 8).layout(
        cfg.base.font_size,
        Some(500.0),
        cosmic_text::Wrap::WordOrGlyph,
        Some(cosmic_text::Align::Left),
        None,
        cosmic_text::Hinting::Disabled,
      ) {
        for glyph in row.glyphs {
          if !raw[offset + glyph.start..offset + glyph.end]
            .chars()
            .all(char::is_whitespace)
          {
            expected.push((offset + glyph.start, offset + glyph.end, glyph.glyph_id, glyph.level));
          }
        }
      }
      offset += line.text().len() + line.ending().as_str().len();
    }
    assert_eq!(
      output
        .glyphs
        .iter()
        .map(|item| (item.glyph.start, item.glyph.end, item.glyph.glyph_id, item.glyph.level))
        .collect::<Vec<_>>(),
      expected
    );
    assert_eq!(buffer.lines.last().unwrap().ending(), LineEnding::None);
  }
}
