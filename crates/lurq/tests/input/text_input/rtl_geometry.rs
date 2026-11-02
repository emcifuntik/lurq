use super::*;

fn layout(source: &str, width: f32) -> Arc<DecoratedInputLayout> {
  let start = source.find("{{task.title}}").unwrap();
  let base = TextStyle {
    font_size: 13.0,
    line_height: 1.4,
    ..TextStyle::default()
  };
  GlyphEngine::new().decorated_input(InputConfig {
    source: source.into(),
    base: base.clone(),
    width,
    min_width: 0.0,
    wrap: true,
    styles: vec![TokenStyle {
      range: start..start + 14,
      text: base,
      padding: 3.0,
      fill: Some((Color::new(2, 3, 4, 255), 3.0)),
      lines: Default::default(),
    }],
  })
}

#[test]
fn input_tokens_rtl_geometry_trailing_crlf_source_coverage() {
  let source = "words words {{task.title}}\r\n";
  let output = layout(source, 45.0);
  let index = output.default_caret(source.len());
  let position = output.carets[index];
  assert_eq!(position.byte, source.len());
  assert_eq!(
    output.carets[output.hit(position.x + 1.0, position.y + 4.0)].byte,
    source.len()
  );
}

#[test]
fn input_tokens_rtl_metrics_tracking_are_logical_pixels() {
  let source = "a {{task.title}} b";
  let make = |tracking| {
    let base = TextStyle {
      font_size: 13.0,
      line_height: 1.4,
      ..TextStyle::default()
    };
    let token = TextStyle {
      font_size: 25.0,
      line_height: 1.2,
      letter_spacing: tracking,
      font_features: crate::layout::text_style::FontFeatures::new([
        crate::layout::text_style::FontFeature::disable(*b"liga"),
        crate::layout::text_style::FontFeature::disable(*b"calt"),
      ]),
      ..base.clone()
    };
    GlyphEngine::new().decorated_input(InputConfig {
      source: source.into(),
      base,
      width: 500.0,
      min_width: 0.0,
      wrap: true,
      styles: vec![TokenStyle {
        range: 2..16,
        text: token,
        padding: 3.0,
        fill: Some((Color::new(2, 3, 4, 255), 3.0)),
        lines: Default::default(),
      }],
    })
  };
  let normal = make(0.0);
  let spaced = make(1.0);
  let count = normal
    .glyphs
    .iter()
    .filter(|glyph| glyph.glyph.start >= 2 && glyph.glyph.end <= 16)
    .count();
  assert_eq!(count, 14);
  assert!(
    normal
      .glyphs
      .iter()
      .filter(|glyph| glyph.glyph.start >= 2 && glyph.glyph.end <= 16)
      .all(|glyph| glyph.glyph.font_size == 25.0)
  );
  assert!(
    (spaced.size.width - normal.size.width - count as f32).abs() < 0.1,
    "tracking is logical px even with metric override: normal={} spaced={}",
    normal.size.width,
    spaced.size.width
  );
}

#[test]
fn input_tokens_rtl_literals_and_mixed_levels_have_disjoint_cluster_advances() {
  for source in [
    "שלום {{task.title}} עולם",
    "مرحبا {{task.title}} world",
    "left שלום {{task.title}} עולם right",
    "שלום \u{2066}{{task.title}}\u{2069} עולם  ",
  ] {
    for width in [0.0, 35.0, 110.0, 600.0] {
      let output = layout(source, width);
      for row in &output.rows {
        let mut parts: Vec<_> = output
          .intervals
          .iter()
          .filter(|part| part.y == row.y && part.right - part.x > 0.01)
          .collect();
        parts.sort_by(|a, b| a.x.total_cmp(&b.x));
        for pair in parts.windows(2) {
          assert!(
            pair[0].right <= pair[1].x + 0.02,
            "{source} width={width}: {:?} overlaps {:?}",
            pair[0].range,
            pair[1].range
          );
        }
      }
      assert_eq!(output.carets[output.default_caret(source.len())].byte, source.len());
      assert!(output.carets.iter().all(|caret| source.is_char_boundary(caret.byte)));
      for caret in &output.carets {
        assert!(caret.x.is_finite() && caret.y.is_finite());
      }
    }
  }
}

#[test]
fn input_tokens_rtl_actual_viewport_alignment_includes_pill_and_carets() {
  use crate::layout::text_style::TextAlign;
  for source in ["שלום {{task.title}} עולם", "left {{task.title}} right"] {
    for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right, TextAlign::End] {
      let mut config = layout(source, 800.0).config.clone();
      config.min_width = 250.0;
      config.base.text_align = align;
      let output = GlyphEngine::new().decorated_input(config);
      assert_eq!(output.viewport_width, 250.0);
      let row = &output.rows[0];
      assert!(row.left >= -0.01 && row.left + row.width <= 250.01);
      let expected = match align {
        TextAlign::Center => (250.0 - row.width) * 0.5,
        TextAlign::Right => 250.0 - row.width,
        TextAlign::End if !row.rtl => 250.0 - row.width,
        _ => 0.0,
      };
      assert!((row.left - expected).abs() < 0.01);
      for glyph in &output.glyphs {
        let caret = output
          .carets
          .iter()
          .find(|caret| caret.byte == glyph.glyph.start)
          .unwrap();
        assert!(caret.x >= row.left - 0.01 && caret.x <= row.left + row.width + 0.01);
      }
      assert!(
        output
          .fragments
          .iter()
          .all(|pill| pill.x >= row.left && pill.x + pill.width <= row.left + row.width + 0.01)
      );
    }
  }
}

#[test]
fn input_tokens_rtl_spatial_candidates_and_selection_preserve_raw_cluster_coverage() {
  let output = layout("left שלום {{task.title}} עולם right", 70.0);
  let shared = output
    .carets
    .iter()
    .enumerate()
    .find(|(index, a)| {
      output.carets[*index + 1..]
        .iter()
        .any(|b| b.byte == a.byte && (b.x != a.x || b.row != a.row))
    })
    .unwrap();
  let candidates: Vec<_> = output
    .carets
    .iter()
    .enumerate()
    .filter(|(_, caret)| caret.byte == shared.1.byte)
    .collect();
  assert!(candidates.len() >= 2);
  for (_, position) in candidates {
    let hit = output.carets[output.hit(position.x, position.y + position.height * 0.5)];
    assert!((hit.x - position.x).abs() < 0.01 && hit.row == position.row);
  }
  let full = output.selection(0..output.config.source.len(), (0.0, 0.0));
  assert_eq!(full.len(), output.rows.len());
  let output = layout("left שלום {{task.title}} עולם right", 600.0);
  let start = output.config.source.find('ש').unwrap();
  let selected = output.selection(start..start + 'ש'.len_utf8(), (0.0, 0.0));
  assert_eq!(selected.len(), 1);
  let part = output
    .intervals
    .iter()
    .find(|part| part.range.contains(&start))
    .unwrap();
  assert!((selected[0].x - part.x).abs() < 0.01);
  assert!((selected[0].width - (part.right - part.x)).abs() < 0.01);
}

#[test]
fn input_tokens_rtl_cache_identity_and_large_narrow_coverage() {
  let mut engine = GlyphEngine::new();
  let config = layout("שלום {{task.title}} עולם", 100.0).config.clone();
  let first = engine.decorated_input(config.clone());
  assert!(Arc::ptr_eq(&first, &engine.decorated_input(config.clone())));
  let mut changed = config.clone();
  changed.min_width = 90.0;
  assert!(!first.same_geometry(&engine.decorated_input(changed)));
  engine.clear_text_caches();
  assert!(!first.same_geometry(&engine.decorated_input(config)));
  let source = format!("{} {} {}", "אב ".repeat(1000), "{{task.title}}", "xyz ".repeat(1000));
  let output = layout(&source, 0.0);
  assert_eq!(output.carets[output.default_caret(source.len())].byte, source.len());
  assert!(output.rows.len() <= source.chars().count());
  assert!(output.allocated_bytes() < 4 * 1024 * 1024);
  assert!(!format!("{output:?}").contains(&source));
}

#[test]
fn input_tokens_rtl_justification_matches_public_whole_paragraph_glyph_oracle() {
  use cosmic_text::{Align, Buffer, Hinting, Metrics, Shaping, Wrap};

  use crate::layout::text_style::TextAlign;
  for source in [
    "one two three four five six seven eight",
    "שלום עולם שלום עולם שלום עולם",
    "مرحبا world مرحبا world مرحبا world",
    "start \u{2067}שלום עולם\u{2069} end words words\r\nlast line  ",
    "  leading\tword\u{a0}words \u{2009}thin words trailing  ",
  ] {
    let mut engine = GlyphEngine::new();
    let base = TextStyle {
      font_size: 13.0,
      line_height: 1.4,
      text_align: TextAlign::Justified,
      ..TextStyle::default()
    };
    let output = engine.decorated_input(InputConfig {
      source: source.into(),
      base: base.clone(),
      styles: Vec::new(),
      width: 90.0,
      min_width: 90.0,
      wrap: true,
    });
    let family = engine.resolve_family(&base);
    let attrs = attrs_for_style(&base, &family, engine.face_weight(&family, &base), base.font_size)
      .metrics(Metrics::new(base.font_size, base.font_size * base.line_height));
    let mut buffer = Buffer::new(
      &mut engine.font_system,
      Metrics::new(base.font_size, base.font_size * base.line_height),
    );
    buffer.set_text(source, &attrs, Shaping::Advanced, None);
    let mut expected = Vec::new();
    let mut offset = 0;
    for line in &mut buffer.lines {
      let shape = line.shape(&mut engine.font_system, 8);
      for row in shape.layout(
        base.font_size,
        Some(90.0),
        Wrap::WordOrGlyph,
        Some(Align::Justified),
        None,
        Hinting::Disabled,
      ) {
        for mut glyph in row.glyphs {
          let raw = &source[offset + glyph.start..offset + glyph.end];
          if raw
            .chars()
            .all(|ch| ch.is_whitespace() || ch.is_control() || matches!(ch, '\u{2066}'..='\u{2069}'))
          {
            continue;
          }
          glyph.start += offset;
          glyph.end += offset;
          expected.push(glyph);
        }
      }
      offset += line.text().len() + line.ending().as_str().len();
    }
    assert_eq!(output.glyphs.len(), expected.len(), "{source}");
    for (actual, expected) in output.glyphs.iter().zip(expected) {
      let actual = &actual.glyph;
      assert_eq!(
        (actual.start, actual.end, actual.glyph_id, actual.level),
        (expected.start, expected.end, expected.glyph_id, expected.level)
      );
      assert!(
        (actual.x - expected.x).abs() < 0.02 && (actual.w - expected.w).abs() < 0.02,
        "{source}: raw={}..{} actual x/w={}/{} expected={}/{}",
        actual.start,
        actual.end,
        actual.x,
        actual.w,
        expected.x,
        expected.w
      );
    }
  }
}

#[test]
fn input_tokens_rtl_justification_word_denominator_is_distinct_from_glyph_count() {
  use crate::layout::text_style::TextAlign;
  let mut config = layout("a {{task.title}} b", 100.0).config.clone();
  config.base.text_align = TextAlign::Justified;
  let output = GlyphEngine::new().decorated_input(config.clone());
  let prototype = output.glyphs[0].glyph.clone();
  let mut line = cosmic_text::LayoutLine {
    w: 20.0,
    max_ascent: 10.0,
    max_descent: 3.0,
    line_height_opt: None,
    glyphs: vec![prototype.clone(), prototype.clone(), prototype],
    decorations: Vec::new(),
  };
  for (index, glyph) in line.glyphs.iter_mut().enumerate() {
    glyph.metadata = index;
    glyph.x = index as f32 * 10.0;
    glyph.w = 10.0;
    glyph.start = index;
    glyph.end = index + 1;
  }
  // Two original blank words: the first has two glyphs, the second one.
  super::justification::apply(&mut line, &[Some(1), Some(1), Some(2)], false, false, 2, 90.0, &config);
  assert_eq!(
    line.glyphs.iter().map(|glyph| glyph.w).collect::<Vec<_>>(),
    vec![40.0; 3]
  );
  assert_eq!(line.glyphs[2].x, 80.0);
  // Exceptional engine multiplicity overshoots the viewport; it clips rather
  // than silently changing the word denominator into a cluster denominator.
  assert_eq!(line.w, 120.0);
}

#[test]
fn input_tokens_rtl_disjoint_and_partial_combining_selection_stays_source_based() {
  let output = layout("left שלום {{task.title}} עולם right", 600.0);
  let start = output.config.source.find('ש').unwrap();
  assert_eq!(
    output.selection(0..start + 2, (0.0, 0.0)).len(),
    2,
    "left literal and the logically first RTL character are spatially disjoint"
  );
  let output = layout("e\u{301} {{task.title}}", 300.0);
  let cluster = output.intervals.iter().find(|part| part.range.start == 0).unwrap();
  assert_eq!(cluster.range, 0..3);
  let partial = output.selection(0..1, (0.0, 0.0));
  let whole = output.selection(0..3, (0.0, 0.0));
  assert_eq!(partial.len(), 1);
  assert_eq!(whole.len(), 1);
  assert_eq!((partial[0].x, partial[0].width), (whole[0].x, whole[0].width));
}

#[test]
fn input_tokens_rtl_empty_controls_hard_endings_and_nonfinite_width_keep_raw_coverage() {
  for source in ["", "  ", "\u{2066}\u{2069}", "\r\n\r\n", "a\n\n", "a\r\nb\r\n"] {
    for width in [0.0, f32::INFINITY, f32::MAX] {
      let base = TextStyle {
        font_size: 13.0,
        ..TextStyle::default()
      };
      let output = GlyphEngine::new().decorated_input(InputConfig {
        source: source.into(),
        base,
        styles: Vec::new(),
        width,
        min_width: 0.0,
        wrap: true,
      });
      assert_eq!(output.carets[output.default_caret(source.len())].byte, source.len());
      assert!(
        output
          .rows
          .windows(2)
          .all(|rows| rows[0].y + rows[0].height <= rows[1].y)
      );
      assert!(
        output
          .carets
          .iter()
          .all(|caret| caret.x.is_finite() && caret.y.is_finite())
      );
    }
  }
}

#[test]
fn input_tokens_rtl_decorated_justified_padding_metrics_and_raw_insert_boundaries() {
  use crate::layout::text_style::TextAlign;
  for source in [
    "שלום {{task.title}} עולם words words",
    "مرحبا {{task.title}} world words words",
    "left {{task.title}} right words words\r\nlast",
  ] {
    let mut config = layout(source, 100.0).config.clone();
    config.min_width = 100.0;
    config.base.text_align = TextAlign::Justified;
    let output = GlyphEngine::new().decorated_input(config);
    for row in &output.rows {
      let parts: Vec<_> = output
        .intervals
        .iter()
        .filter(|part| part.y == row.y && part.right - part.x > 0.01)
        .collect();
      let mut sorted = parts;
      sorted.sort_by(|a, b| a.x.total_cmp(&b.x));
      assert!(sorted.windows(2).all(|parts| parts[0].right <= parts[1].x + 0.02));
    }
    for part in &output.fragments {
      let candidates: Vec<_> = output
        .carets
        .iter()
        .filter(|caret| caret.row == part.row && caret.x >= part.x && caret.x <= part.x + part.width)
        .collect();
      assert!(!candidates.is_empty());
      assert!(candidates.iter().all(|caret| source.is_char_boundary(caret.byte)));
      assert!(
        candidates.iter().any(|caret| (caret.x - part.x - 3.0).abs() < 0.02)
          && candidates
            .iter()
            .any(|caret| (part.x + part.width - caret.x - 3.0).abs() < 0.02),
        "both token body edges retain padding even when a literal shares a raw boundary"
      );
    }
  }
}

#[test]
fn input_tokens_rtl_token_internal_blanks_and_negative_tracking_have_finite_raw_rows() {
  use crate::layout::text_style::TextAlign;
  let source = "שלום {{ task title }} עולם words words";
  let start = source.find("{{").unwrap();
  let end = source.find("}}").unwrap() + 2;
  for tracking in [0.0, -30.0] {
    let base = TextStyle {
      font_size: 13.0,
      line_height: 1.4,
      text_align: TextAlign::Justified,
      letter_spacing: tracking,
      ..TextStyle::default()
    };
    let output = GlyphEngine::new().decorated_input(InputConfig {
      source: source.into(),
      base: base.clone(),
      width: 100.0,
      min_width: 100.0,
      wrap: true,
      styles: vec![TokenStyle {
        range: start..end,
        text: base,
        padding: 3.0,
        fill: Some((Color::new(1, 2, 3, 255), 3.0)),
        lines: Default::default(),
      }],
    });
    assert_eq!(output.carets[output.default_caret(source.len())].byte, source.len());
    assert!(output.rows.len() <= source.chars().count());
    assert!(
      output
        .glyphs
        .iter()
        .all(|glyph| glyph.glyph.x.is_finite() && glyph.glyph.w.is_finite())
    );
    assert!(
      output
        .fragments
        .iter()
        .all(|part| part.width.is_finite() && part.width >= 0.0)
    );
    if tracking == 0.0 {
      assert!(output.fragments.len() > 1);
    }
  }
}
