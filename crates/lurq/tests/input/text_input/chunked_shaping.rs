//! A long paragraph shaped a chunk at a time lays out exactly as when it is
//! shaped whole, styled or not, and after an edit in one chunk.
use super::*;
use crate::{
  app::glyph_engine::{
    GlyphEngine,
    input_tokens::{DecoratedInputLayout, InputConfig, TokenStyle},
  },
  layout::text_style::{FontStyle, FontWeight, TextStyle},
};

const WORDS: &str = "lorem ipsum dolor sit amet, consectetur adipiscing elit; sed do (eiusmod) tempor 42 ";

fn base() -> TextStyle {
  TextStyle {
    font_size: 14.0,
    line_height: 1.4,
    ..TextStyle::default()
  }
}

/// A bold, italic or monospace style every few words.
fn styles(text: &str) -> Vec<TokenStyle> {
  let mut styles = Vec::new();
  let mut start = 0;
  for (index, word) in text.split(' ').enumerate() {
    let range = start..start + word.len();
    start += word.len() + 1;
    if index % 4 != 1 || word.is_empty() {
      continue;
    }
    let mut style = base();
    match index % 3 {
      0 => style.weight = FontWeight::Bold,
      1 => style.style = FontStyle::Italic,
      _ => style.font_family = "monospace".into(),
    }
    styles.push(TokenStyle {
      range,
      text: style,
      padding: 0.0,
      fill: None,
      lines: Default::default(),
    });
  }
  styles
}

fn config(text: &str, styled: bool) -> InputConfig {
  InputConfig {
    source: text.into(),
    base: base(),
    styles: if styled { styles(text) } else { Vec::new() },
    width: 420.0,
    min_width: 0.0,
    wrap: true,
  }
}

/// Glyph places, carets and rows of a layout, rounded to hundredths.
fn geometry(layout: &DecoratedInputLayout) -> Vec<String> {
  let round = |value: f32| (value * 100.0).round() as i64;
  let glyphs = layout.glyphs.iter().map(|glyph| {
    format!(
      "g {}..{} {} {} {} {}",
      glyph.glyph.start,
      glyph.glyph.end,
      round(glyph.glyph.x),
      round(glyph.baseline),
      glyph.glyph.glyph_id,
      glyph.row
    )
  });
  let carets = layout
    .carets
    .iter()
    .map(|caret| format!("c {} {} {} {}", caret.byte, round(caret.x), round(caret.y), caret.row));
  let rows = layout
    .rows
    .iter()
    .map(|row| format!("r {:?} {} {}", row.range, round(row.y), round(row.width)));
  glyphs.chain(carets).chain(rows).collect()
}

fn whole_and_chunked(text: &str, styled: bool) -> (Vec<String>, Vec<String>) {
  let mut whole = GlyphEngine::new();
  whole.input_cache.whole_paragraphs = true;
  let mut chunked = GlyphEngine::new();
  (
    geometry(&whole.decorated_input(config(text, styled))),
    geometry(&chunked.decorated_input(config(text, styled))),
  )
}

#[test]
fn chunked_shaping_of_a_long_paragraph_matches_whole_shaping() {
  let text = WORDS.repeat(40);
  assert!(chunk_ends(&text).is_some_and(|ends| ends.len() > 4));
  for styled in [false, true] {
    let (whole, chunked) = whole_and_chunked(&text, styled);
    if let Some(index) = whole.iter().zip(&chunked).position(|(a, b)| a != b) {
      panic!(
        "styled {styled}: first difference at {index}: whole {:?} chunked {:?} (lengths {} {})",
        &whole[index.saturating_sub(2)..index + 3],
        &chunked[index.saturating_sub(2)..index + 3],
        whole.len(),
        chunked.len()
      );
    }
    assert_eq!(whole.len(), chunked.len());
  }
}

#[test]
fn chunked_shaping_reuses_unchanged_chunks_after_an_edit() {
  let text = WORDS.repeat(40);
  let before = config(&text, true);
  let mut engine = GlyphEngine::new();
  engine.decorated_input(before.clone());
  let shaped = engine.input_cache.chunks.len();
  // Insert a word in the middle; the styles after it move with the text, as
  // a styled input moves its spans.
  let at = text[..text.len() / 2].rfind(' ').unwrap() + 1;
  let inserted = "inserted ";
  let mut edited = text.clone();
  edited.insert_str(at, inserted);
  let mut after = config(&edited, false);
  after.styles = before
    .styles
    .iter()
    .map(|style| {
      let shift = if style.range.start >= at { inserted.len() } else { 0 };
      TokenStyle {
        range: style.range.start + shift..style.range.end + shift,
        ..style.clone()
      }
    })
    .collect();
  let layout = engine.decorated_input(after.clone());
  let new_chunks = engine.input_cache.chunks.len() - shaped;
  assert!(
    new_chunks <= 2,
    "an edit shapes one or two chunks again, not {new_chunks}"
  );
  let mut whole = GlyphEngine::new();
  whole.input_cache.whole_paragraphs = true;
  assert_eq!(geometry(&layout), geometry(&whole.decorated_input(after)));
}

#[test]
fn chunked_shaping_leaves_right_to_left_and_tabbed_paragraphs_whole() {
  let long = WORDS.repeat(20);
  assert!(chunk_ends(&format!("{long}שלום {long}")).is_none());
  assert!(chunk_ends(&format!("{long}\t{long}")).is_none());
  assert!(
    chunk_ends(&WORDS.repeat(5)).is_none(),
    "short paragraphs are shaped whole"
  );
}
