use std::collections::VecDeque;

use super::*;
use crate::layout::render_list::GlyphCmd;

/// How many decorated layouts keep their glyph commands.
const RASTER_ENTRIES: usize = 8;

/// The glyph commands of one decorated layout, drawn at one scale from an
/// origin with one subpixel offset, relative to the origin's whole pixels.
/// A frame that draws the same layout again, the caret or the selection
/// having moved, copies them instead of placing every glyph again.
struct RasterEntry {
  layout: Arc<DecoratedInputLayout>,
  key: (u32, u32, u32),
  commands: Vec<GlyphCmd>,
}

#[derive(Default)]
pub(super) struct RasterCache {
  entries: VecDeque<RasterEntry>,
}

impl RasterCache {
  pub(super) fn clear(&mut self) {
    self.entries.clear();
  }

  pub(super) fn estimated_memory_bytes(&self) -> usize {
    self
      .entries
      .iter()
      .map(|entry| entry.commands.capacity() * std::mem::size_of::<GlyphCmd>())
      .sum()
  }
}

impl GlyphEngine {
  pub(crate) fn rasterize_input_glyph(
    &mut self,
    input: &InputGlyph,
    origin: (f32, f32),
    scale: f32,
    out: &mut Vec<GlyphCmd>,
  ) {
    let physical = input
      .glyph
      .physical((origin.0 * scale, (origin.1 + input.baseline) * scale), scale);
    let Some(packed) = self.get_or_pack_glyph(physical.cache_key) else {
      return;
    };
    self.push_glyph_cmd(
      out,
      physical.x as f32 + packed.left as f32,
      physical.y as f32 - packed.top as f32,
      packed,
      input.color.to_linear_f32_array(),
      false,
    );
  }

  /// Appends the glyph commands of all of `layout` drawn from `origin` at
  /// `scale`, untransformed, reusing those of the last frame that drew it.
  pub(crate) fn rasterize_input_glyphs(
    &mut self,
    layout: &Arc<DecoratedInputLayout>,
    origin: (f32, f32),
    scale: f32,
    out: &mut Vec<GlyphCmd>,
  ) {
    let (x, y) = (origin.0 * scale, origin.1 * scale);
    let (whole_x, whole_y) = (x.floor(), y.floor());
    let key = (scale.to_bits(), (x - whole_x).to_bits(), (y - whole_y).to_bits());
    let shift = |command: &GlyphCmd| GlyphCmd {
      x: command.x + whole_x,
      y: command.y + whole_y,
      ..command.clone()
    };
    let cache = &self.input_cache.raster;
    if let Some(entry) = cache
      .entries
      .iter()
      .find(|entry| entry.key == key && Arc::ptr_eq(&entry.layout, layout))
    {
      out.extend(entry.commands.iter().map(shift));
      return;
    }
    let mut commands = Vec::with_capacity(layout.glyphs.len());
    let local = ((x - whole_x) / scale, (y - whole_y) / scale);
    for glyph in &layout.glyphs {
      self.rasterize_input_glyph(glyph, local, scale, &mut commands);
    }
    out.extend(commands.iter().map(shift));
    let cache = &mut self.input_cache.raster;
    if cache.entries.len() >= RASTER_ENTRIES {
      cache.entries.pop_front();
    }
    cache.entries.push_back(RasterEntry {
      layout: layout.clone(),
      key,
      commands,
    });
  }
}
