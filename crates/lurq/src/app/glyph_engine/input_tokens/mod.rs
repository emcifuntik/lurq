//! Whole-paragraph input shaping; cached geometry is also the paint payload.
use std::{borrow::Cow, collections::VecDeque, ops::Range, sync::Arc};

use cosmic_text::LayoutGlyph;

use super::GlyphEngine;
#[cfg(test)]
use super::attrs_for_style;
use crate::{
  layout::{
    Size,
    text_style::{TextShadow, TextStyle},
  },
  node::{
    color::Color,
    text_selection::{CaretPosition, CaretPositions, TextSelectionRange},
  },
};

mod alignment;
mod chunks;
#[cfg(test)]
#[path = "../../../../tests/input/text_input/crlf_geometry.rs"]
mod crlf_tests;
mod geometry;
mod justification;
mod paint;
#[cfg(test)]
#[path = "../../../../tests/input/text_input/paint_oracle.rs"]
pub(crate) mod paint_oracle;
mod paragraphs;
mod pieces;
mod rows;
#[cfg(test)]
#[path = "../../../../tests/input/text_input/scroll_geometry.rs"]
mod scroll_tests;
#[cfg(test)]
#[path = "../../../../tests/input/text_input/rtl_geometry.rs"]
mod tests;

#[derive(Clone, PartialEq)]
pub(crate) struct TokenStyle {
  pub(crate) range: Range<usize>,
  pub(crate) text: TextStyle,
  pub(crate) padding: f32,
  pub(crate) fill: Option<(Color, f32)>,
  /// Lines under and through the range's text, in its color.
  pub(crate) lines: TextLines,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextLines {
  pub(crate) underline: bool,
  pub(crate) strikethrough: bool,
}

#[derive(Clone, PartialEq)]
pub(crate) struct InputConfig {
  pub(crate) source: Arc<str>,
  pub(crate) base: TextStyle,
  pub(crate) styles: Vec<TokenStyle>,
  pub(crate) width: f32,
  pub(crate) min_width: f32,
  pub(crate) wrap: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct InputCaret {
  pub(crate) byte: usize,
  pub(crate) x: f32,
  pub(crate) y: f32,
  pub(crate) height: f32,
  pub(crate) font_size: f32,
  pub(crate) row: usize,
}

pub(crate) struct InputGlyph {
  pub(crate) glyph: LayoutGlyph,
  pub(crate) baseline: f32,
  row: usize,
  pub(crate) color: Color,
  pub(crate) shadow: Option<TextShadow>,
}

pub(crate) struct InputFragment {
  pub(crate) x: f32,
  pub(crate) y: f32,
  pub(crate) width: f32,
  pub(crate) height: f32,
  pub(crate) color: Color,
  pub(crate) radius: f32,
  row: usize,
}

pub(crate) struct ClusterInterval {
  range: Range<usize>,
  x: f32,
  right: f32,
  y: f32,
}

/// Opaque validated input paint geometry. Only the toolkit can construct or change it.
#[doc(hidden)]
pub struct DecoratedInputLayout {
  pub(crate) config: InputConfig,
  pub(crate) revision: u64,
  pub(crate) glyphs: Vec<InputGlyph>,
  pub(crate) fragments: Vec<InputFragment>,
  pub(crate) carets: Vec<InputCaret>,
  rows: Vec<InputRow>,
  intervals: Vec<ClusterInterval>,
  pub(crate) size: Size,
  pub(crate) line_height: f32,
  pub(crate) viewport_width: f32,
}

struct InputRow {
  range: Range<usize>,
  ending: Option<Range<usize>>,
  y: f32,
  height: f32,
  band_top: f32,
  band_height: f32,
  left: f32,
  width: f32,
  rtl: bool,
}

impl DecoratedInputLayout {
  fn empty(config: InputConfig, revision: u64) -> Self {
    Self {
      config,
      revision,
      glyphs: Vec::new(),
      fragments: Vec::new(),
      carets: Vec::new(),
      rows: Vec::new(),
      intervals: Vec::new(),
      size: Size::default(),
      line_height: 1.0,
      viewport_width: 0.0,
    }
  }
}

impl std::fmt::Debug for DecoratedInputLayout {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("DecoratedInputLayout").finish_non_exhaustive()
  }
}

#[derive(Default)]
pub(crate) struct InputCache {
  revision: u64,
  entries: VecDeque<Arc<DecoratedInputLayout>>,
  /// Laid-out paragraphs, so an edit lays out only the paragraph it changed.
  paragraphs: pieces::ParagraphCache,
  /// Glyph commands of the layouts drawn last.
  raster: paint::RasterCache,
  /// Shaped chunks of long paragraphs, so an edit shapes only its chunk.
  chunks: chunks::ChunkCache,
  /// Shapes every paragraph whole, for tests that compare the two.
  whole_paragraphs: bool,
}

impl InputCache {
  pub(super) fn clear(&mut self) {
    self.revision = self.revision.wrapping_add(1);
    self.entries.clear();
    self.paragraphs.clear();
    self.raster.clear();
    self.chunks.clear();
  }
  fn find(&self, key: &InputConfig) -> Option<Arc<DecoratedInputLayout>> {
    self.entries.iter().find(|layout| &layout.config == key).cloned()
  }
  pub(super) fn estimated_memory_bytes(&self) -> usize {
    self.entries.capacity() * std::mem::size_of::<Arc<DecoratedInputLayout>>()
      + self
        .entries
        .iter()
        .map(|layout| layout.allocated_bytes())
        .sum::<usize>()
      + self.paragraphs.estimated_memory_bytes()
      + self.raster.estimated_memory_bytes()
      + self.chunks.estimated_memory_bytes()
  }
  fn insert(&mut self, layout: Arc<DecoratedInputLayout>) {
    // A fixed entry count alone is not a text-memory bound.
    while self.entries.len() >= 16
      || self.entries.iter().map(|entry| entry.allocated_bytes()).sum::<usize>() + layout.allocated_bytes()
        > 8 * 1024 * 1024
    {
      if self.entries.pop_front().is_none() {
        break;
      }
    }
    if layout.allocated_bytes() <= 8 * 1024 * 1024 {
      self.entries.push_back(layout);
    }
  }
}

impl GlyphEngine {
  pub(crate) fn decorated_input(&mut self, config: InputConfig) -> Arc<DecoratedInputLayout> {
    if let Some(layout) = self.input_cache.find(&config) {
      return layout;
    }
    let mut output = DecoratedInputLayout::empty(config, self.input_cache.revision);
    self.shape_input(&mut output);
    let output = Arc::new(output);
    self.input_cache.insert(output.clone());
    output
  }

  fn shape_input(&mut self, output: &mut DecoratedInputLayout) {
    // Wrapped text is laid out a paragraph at a time from the paragraph
    // cache. Justified rows stretch to the widest paragraph, and a single
    // scrolling line has no paragraphs, so those lay out whole.
    if output.config.wrap && output.config.base.text_align != crate::layout::text_style::TextAlign::Justified {
      self.shape_paragraphs(output);
    } else {
      self.shape_rows(output);
    }
    output.finish_carets();
    output.align_viewport();
  }

  /// Lays out the rows of all of `output`'s text, unaligned.
  fn shape_rows(&mut self, output: &mut DecoratedInputLayout) {
    let config = &output.config;
    // CR and LF each become one ASCII space in Scroll, preserving every raw
    // UTF-8 byte offset. Only presentation is flattened; config.source stays raw.
    let presentation = if config.wrap {
      Cow::Borrowed(config.source.as_ref())
    } else {
      Cow::Owned(config.source.replace(['\r', '\n'], " "))
    };
    let mut buffer = self.input_paragraphs(config, &presentation);
    let mut offset = 0;
    let mut prepared = Vec::new();
    for line in &mut buffer.lines {
      let bytes = line.text().len();
      let chunked = self.shape_in_chunks(line.text(), line.attrs_list());
      let shape = match &chunked {
        Some(shape) => shape,
        None => line.shape(&mut self.font_system, 8),
      };
      rows::paragraph(&mut prepared, shape, offset, bytes, &output.config);
      let ending = offset + bytes..offset + bytes + line.ending().as_str().len();
      rows::set_ending(&mut prepared, ending.clone());
      offset = ending.end;
    }
    rows::finish(output, prepared);
    self.buffer_pool.push(buffer);
  }
}
