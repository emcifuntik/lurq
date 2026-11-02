//! Wrapped input text laid out a paragraph at a time. Each hard paragraph is
//! laid out on its own, keyed by its text, its styles and the width, and kept;
//! an edit lays out again only the paragraph it changed and places the others
//! from the cache below each other.
use std::{
  collections::{HashMap, VecDeque},
  hash::{DefaultHasher, Hash, Hasher},
  sync::Arc,
};

use cosmic_text::LineIter;

use super::{DecoratedInputLayout, GlyphEngine, InputConfig, TokenStyle};

/// At most this many paragraphs, and this many bytes of them, are kept.
const MAX_PARAGRAPHS: usize = 2048;
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub(super) struct ParagraphCache {
  entries: HashMap<u64, Vec<Arc<DecoratedInputLayout>>>,
  /// Keys in insertion order, oldest first, for eviction.
  order: VecDeque<u64>,
  bytes: usize,
}

impl ParagraphCache {
  pub(super) fn clear(&mut self) {
    self.entries.clear();
    self.order.clear();
    self.bytes = 0;
  }

  pub(super) fn estimated_memory_bytes(&self) -> usize {
    self.bytes + self.order.capacity() * std::mem::size_of::<u64>()
  }

  fn find(&self, key: u64, config: &InputConfig) -> Option<Arc<DecoratedInputLayout>> {
    self
      .entries
      .get(&key)?
      .iter()
      .find(|piece| &piece.config == config)
      .cloned()
  }

  fn insert(&mut self, key: u64, piece: Arc<DecoratedInputLayout>) {
    let bytes = piece.allocated_bytes();
    if bytes > MAX_BYTES {
      return;
    }
    while self.order.len() >= MAX_PARAGRAPHS || self.bytes + bytes > MAX_BYTES {
      let Some(oldest) = self.order.pop_front() else {
        break;
      };
      if let Some(pieces) = self.entries.get_mut(&oldest) {
        if !pieces.is_empty() {
          self.bytes -= pieces.remove(0).allocated_bytes();
        }
        if pieces.is_empty() {
          self.entries.remove(&oldest);
        }
      }
    }
    self.bytes += bytes;
    self.order.push_back(key);
    self.entries.entry(key).or_default().push(piece);
  }
}

impl GlyphEngine {
  /// Lays out wrapped text paragraph by paragraph, unaligned, reusing every
  /// paragraph laid out before with the same text, styles and width.
  pub(super) fn shape_paragraphs(&mut self, output: &mut DecoratedInputLayout) {
    let source = output.config.source.clone();
    let mut paragraphs: Vec<_> = LineIter::new(&source).collect();
    // Like `Buffer::set_text`: one final empty paragraph after a last line
    // break, or for empty text.
    if paragraphs
      .last()
      .is_none_or(|(_, ending)| *ending != cosmic_text::LineEnding::None)
    {
      paragraphs.push((source.len()..source.len(), cosmic_text::LineEnding::None));
    }
    let mut first_style = 0;
    for (range, ending) in paragraphs {
      let styles = &output.config.styles;
      while first_style < styles.len() && styles[first_style].range.end <= range.start {
        first_style += 1;
      }
      let local = local_styles(&styles[first_style..], range.clone());
      let config = InputConfig {
        source: source[range.clone()].into(),
        base: output.config.base.clone(),
        styles: local,
        width: output.config.width,
        min_width: 0.0,
        wrap: true,
      };
      let piece = self.paragraph_piece(config);
      output.append_piece(&piece, range.start);
      let ending = ending.as_str().len();
      if ending > 0 {
        output.append_ending(range.end..range.end + ending);
      }
    }
  }

  fn paragraph_piece(&mut self, config: InputConfig) -> Arc<DecoratedInputLayout> {
    let key = piece_key(&config);
    if let Some(piece) = self.input_cache.paragraphs.find(key, &config) {
      return piece;
    }
    let mut piece = DecoratedInputLayout::empty(config, self.input_cache.revision);
    self.shape_rows(&mut piece);
    let piece = Arc::new(piece);
    self.input_cache.paragraphs.insert(key, piece.clone());
    piece
  }
}

impl DecoratedInputLayout {
  /// Places a paragraph laid out on its own below the rows so far, its
  /// source starting at byte `offset`.
  fn append_piece(&mut self, piece: &Self, offset: usize) {
    let (top, first_row) = (self.size.height, self.rows.len());
    self.glyphs.extend(piece.glyphs.iter().map(|glyph| {
      let mut layout = glyph.glyph.clone();
      layout.start += offset;
      layout.end += offset;
      super::InputGlyph {
        glyph: layout,
        baseline: glyph.baseline + top,
        row: glyph.row + first_row,
        color: glyph.color,
        shadow: glyph.shadow,
      }
    }));
    self
      .fragments
      .extend(piece.fragments.iter().map(|part| super::InputFragment {
        y: part.y + top,
        row: part.row + first_row,
        ..*part
      }));
    self.carets.extend(piece.carets.iter().map(|caret| super::InputCaret {
      byte: caret.byte + offset,
      y: caret.y + top,
      row: caret.row + first_row,
      ..*caret
    }));
    self
      .intervals
      .extend(piece.intervals.iter().map(|part| super::ClusterInterval {
        range: part.range.start + offset..part.range.end + offset,
        y: part.y + top,
        ..*part
      }));
    self.rows.extend(piece.rows.iter().map(|row| super::InputRow {
      range: row.range.start + offset..row.range.end + offset,
      ending: None,
      y: row.y + top,
      ..*row
    }));
    self.size.width = self.size.width.max(piece.size.width);
    self.size.height += piece.size.height;
    self.line_height = self.line_height.max(piece.line_height);
  }
}

/// The styles over `range`, clipped to it and counted from its start.
fn local_styles(styles: &[TokenStyle], range: std::ops::Range<usize>) -> Vec<TokenStyle> {
  styles
    .iter()
    .take_while(|style| style.range.start < range.end)
    .filter_map(|style| {
      let start = style.range.start.max(range.start);
      let end = style.range.end.min(range.end);
      (start < end).then(|| TokenStyle {
        range: start - range.start..end - range.start,
        ..style.clone()
      })
    })
    .collect()
}

/// A hash of what a paragraph's layout depends on besides its styles' looks,
/// which the cache compares in full.
fn piece_key(config: &InputConfig) -> u64 {
  let mut hasher = DefaultHasher::new();
  config.source.hash(&mut hasher);
  config.width.to_bits().hash(&mut hasher);
  for style in &config.styles {
    style.range.hash(&mut hasher);
  }
  hasher.finish()
}
