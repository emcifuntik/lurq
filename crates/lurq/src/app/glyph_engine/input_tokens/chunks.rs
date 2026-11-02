//! Long left-to-right paragraphs shaped a chunk at a time. A paragraph of
//! more than [`CHUNKED_FROM`] bytes is cut after spaces at places its own
//! words choose, so an edit moves no cut but the ones next to it; each chunk
//! is shaped on its own with only its own attributes and kept, keyed by its
//! text and attributes. An edit then shapes only the chunk it changed, and
//! the paragraph's shape is one span of the chunks' words one after another,
//! as whole shaping makes it.
//!
//! cosmic-text shapes a paragraph word by word and never across a space, so
//! cutting after a space before a letter or digit changes no glyph. Text
//! that is not plain left to right (right-to-left scripts, bidi controls)
//! and text with tabs, whose width depends on where they fall, is shaped
//! whole.
use std::{collections::HashMap, ops::Range, sync::Arc};

use cosmic_text::{AttrsList, AttrsOwned, ShapeLine, ShapeSpan, Shaping};

use super::GlyphEngine;

/// Shorter paragraphs are shaped whole.
const CHUNKED_FROM: usize = 1024;
/// A chunk is at least this long, unless it ends the paragraph.
const MIN_CHUNK: usize = 192;
/// A chunk longer than this is cut at its next possible place.
const MAX_CHUNK: usize = 1536;
/// Chunks kept in each of the cache's two generations.
const GENERATION: usize = 2048;

#[derive(Clone, PartialEq, Eq, Hash)]
struct ChunkKey {
  text: Box<str>,
  defaults: AttrsOwned,
  /// The chunk's attribute spans, counted from its start, their metadata
  /// counted from the chunk's first styled range.
  spans: Vec<(Range<usize>, AttrsOwned)>,
}

/// Shaped chunks in two generations: a lookup in the older one moves the
/// chunk to the current one, and a full current generation replaces the
/// older one, so the chunks of the text being edited stay.
#[derive(Default)]
pub(super) struct ChunkCache {
  current: HashMap<ChunkKey, Arc<ShapeLine>>,
  previous: HashMap<ChunkKey, Arc<ShapeLine>>,
  /// What each generation holds, kept as chunks come and go.
  current_bytes: usize,
  previous_bytes: usize,
}

impl ChunkCache {
  pub(super) fn clear(&mut self) {
    *self = Self::default();
  }

  pub(super) fn estimated_memory_bytes(&self) -> usize {
    self.current_bytes + self.previous_bytes
  }

  #[cfg(test)]
  pub(super) fn len(&self) -> usize {
    self.current.len() + self.previous.len()
  }

  fn get(&mut self, key: &ChunkKey) -> Option<Arc<ShapeLine>> {
    if let Some(shape) = self.current.get(key) {
      return Some(shape.clone());
    }
    let (key, shape) = self.previous.remove_entry(key)?;
    let bytes = entry_bytes(&key, &shape);
    self.previous_bytes -= bytes;
    self.insert_sized(key, shape.clone(), bytes);
    Some(shape)
  }

  fn insert(&mut self, key: ChunkKey, shape: Arc<ShapeLine>) {
    let bytes = entry_bytes(&key, &shape);
    self.insert_sized(key, shape, bytes);
  }

  fn insert_sized(&mut self, key: ChunkKey, shape: Arc<ShapeLine>, bytes: usize) {
    if self.current.len() >= GENERATION {
      self.previous = std::mem::take(&mut self.current);
      self.previous_bytes = std::mem::take(&mut self.current_bytes);
    }
    let text = key.text.len();
    if let Some(old) = self.current.insert(key, shape) {
      self.current_bytes -= text + shape_bytes(&old);
    }
    self.current_bytes += bytes;
  }
}

impl GlyphEngine {
  /// The shape of a paragraph put together from shaped chunks, or `None`
  /// when it is short or must be shaped whole.
  pub(super) fn shape_in_chunks(&mut self, text: &str, attrs: &AttrsList) -> Option<ShapeLine> {
    if self.input_cache.whole_paragraphs {
      return None;
    }
    let ends = chunk_ends(text)?;
    let mut merged: Option<ShapeLine> = None;
    let mut start = 0;
    for end in ends {
      let (key, first_style) = chunk_key(text, attrs, start..end);
      let shape = match self.input_cache.chunks.get(&key) {
        Some(shape) => shape,
        None => {
          let list = key_attrs(&key);
          let shape = Arc::new(ShapeLine::new(
            &mut self.font_system,
            &key.text,
            &list,
            Shaping::Advanced,
            8,
          ));
          self.input_cache.chunks.insert(key, shape.clone());
          shape
        }
      };
      // Plain left-to-right text shapes as one span; its words join the
      // paragraph's one span, so wrapping sees no seam between chunks.
      let [span] = shape.spans.as_slice() else {
        return None;
      };
      if shape.rtl || span.level.is_rtl() {
        return None;
      }
      let span = shifted(span, start, first_style);
      match &mut merged {
        Some(line) => {
          let joined = line.spans.first_mut()?;
          joined.words.extend(span.words);
          joined.decoration_spans.extend(span.decoration_spans);
        }
        None => {
          let mut line = ShapeLine::clone(&shape);
          line.spans = vec![span];
          merged = Some(line);
        }
      }
      start = end;
    }
    merged
  }
}

/// Where a long, plain left-to-right paragraph is cut: after a space,
/// before an ASCII letter or digit, where the word before the space hashes
/// to a multiple of four, at least [`MIN_CHUNK`] bytes after the last cut
/// (or anywhere possible past [`MAX_CHUNK`]). The ends of its chunks, the
/// last at the paragraph's end; `None` when it is not cut.
pub(super) fn chunk_ends(text: &str) -> Option<Vec<usize>> {
  if text.len() < CHUNKED_FROM || text.chars().any(shapes_whole) {
    return None;
  }
  let bytes = text.as_bytes();
  let mut ends = Vec::new();
  let (mut last, mut word, mut previous_word) = (0, FNV_OFFSET, FNV_OFFSET);
  for index in 1..bytes.len() {
    let (before, at) = (bytes[index - 1], bytes[index]);
    if before == b' ' {
      if at != b' ' && word != FNV_OFFSET {
        previous_word = word;
        word = FNV_OFFSET;
      }
      let length = index - last;
      let chosen = previous_word % 4 == 0 || length >= MAX_CHUNK;
      if at.is_ascii_alphanumeric() && length >= MIN_CHUNK && chosen {
        ends.push(index);
        last = index;
      }
    }
    if at != b' ' {
      word = (word ^ u32::from(at)).wrapping_mul(FNV_PRIME);
    }
  }
  ends.push(text.len());
  (ends.len() > 1).then_some(ends)
}

const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;

/// Characters a paragraph is shaped whole for: tabs and right-to-left text
/// or bidi controls.
fn shapes_whole(ch: char) -> bool {
  matches!(ch,
    '\t'
    | '\u{0590}'..='\u{08FF}'
    | '\u{FB1D}'..='\u{FDFF}'
    | '\u{FE70}'..='\u{FEFF}'
    | '\u{10800}'..='\u{10FFF}'
    | '\u{1E800}'..='\u{1EFFF}'
    | '\u{200E}'
    | '\u{200F}'
    | '\u{202A}'..='\u{202E}'
    | '\u{2066}'..='\u{2069}')
}

/// The cache key of `range` of a paragraph with `attrs`, and the metadata
/// its first styled range had in the paragraph, less one. Colors do not
/// change shapes and are left out, so a recolored span is not shaped again.
fn chunk_key(text: &str, attrs: &AttrsList, range: Range<usize>) -> (ChunkKey, usize) {
  let mut defaults = AttrsOwned::new(&attrs.defaults());
  defaults.color_opt = None;
  let mut spans: Vec<_> = attrs
    .spans_iter()
    .filter_map(|(span, owned)| {
      let start = span.start.max(range.start);
      let end = span.end.min(range.end);
      (start < end).then(|| {
        let mut owned = owned.clone();
        owned.color_opt = None;
        (start - range.start..end - range.start, owned)
      })
    })
    .collect();
  spans.sort_by_key(|(span, _)| span.start);
  let first_style = spans
    .iter()
    .map(|(_, owned)| owned.metadata)
    .filter(|metadata| *metadata > 0)
    .min()
    .map_or(0, |metadata| metadata - 1);
  for (_, owned) in &mut spans {
    if owned.metadata > 0 {
      owned.metadata -= first_style;
    }
  }
  let key = ChunkKey {
    text: text[range].into(),
    defaults,
    spans,
  };
  (key, first_style)
}

fn key_attrs(key: &ChunkKey) -> AttrsList {
  let mut list = AttrsList::new(&key.defaults.as_attrs());
  for (range, attrs) in &key.spans {
    list.add_span(range.clone(), &attrs.as_attrs());
  }
  list
}

/// A chunk's span placed at byte `offset` of its paragraph, its glyphs'
/// metadata counted from the paragraph's styles again.
fn shifted(span: &ShapeSpan, offset: usize, first_style: usize) -> ShapeSpan {
  let mut span = span.clone();
  for word in &mut span.words {
    for glyph in &mut word.glyphs {
      glyph.start += offset;
      glyph.end += offset;
      if glyph.metadata > 0 {
        glyph.metadata += first_style;
      }
    }
  }
  for (range, _) in &mut span.decoration_spans {
    *range = range.start + offset..range.end + offset;
  }
  span
}

fn entry_bytes(key: &ChunkKey, shape: &ShapeLine) -> usize {
  key.text.len() + shape_bytes(shape)
}

fn shape_bytes(shape: &ShapeLine) -> usize {
  shape
    .spans
    .iter()
    .flat_map(|span| &span.words)
    .map(|word| word.glyphs.capacity() * std::mem::size_of::<cosmic_text::ShapeGlyph>())
    .sum::<usize>()
    + std::mem::size_of::<ShapeLine>()
}

#[cfg(test)]
#[path = "../../../../tests/input/text_input/chunked_shaping.rs"]
mod tests;
