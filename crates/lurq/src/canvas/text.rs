use std::{collections::HashMap, sync::Arc};

use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, SwashContent, Wrap};
use tiny_skia::{Pixmap, PixmapPaint};

#[cfg(feature = "perf_profile")]
use crate::app::profiler::canvas_text::{self as profile, Stage, Timer};

mod glyph_cache;
#[cfg(all(test, feature = "perf_profile"))]
mod profile_tests;
mod shape_cache;
#[cfg(test)]
mod tests;
use glyph_cache::GlyphCache;
use shape_cache::ShapeCache;

use super::{CanvasError, MAX_PIXELS};
use crate::{
  app::glyph_engine::{FaceWeights, with_letter_spacing},
  layout::text_style::{FontFeatures, FontStyle, FontWeight, TextStyle, alias_name, cosmic_family},
  node::color::Color,
};

#[derive(Clone, PartialEq)]
pub struct CanvasFont {
  pub family: Arc<str>,
  pub size: f32,
  pub weight: FontWeight,
  pub style: FontStyle,
  /// Extra space after every glyph in logical pixels, like
  /// [`TextStyle::letter_spacing`]; scales with the canvas transform like `size`.
  pub letter_spacing: f32,
  /// OpenType feature settings, like [`TextStyle::font_features`].
  pub font_features: FontFeatures,
}
impl CanvasFont {
  pub fn new(family: impl Into<Arc<str>>, size: f32) -> Self {
    Self {
      family: family.into(),
      size,
      weight: FontWeight::Normal,
      style: FontStyle::Normal,
      letter_spacing: 0.0,
      font_features: FontFeatures::default(),
    }
  }
  pub(crate) fn from_style(style: &TextStyle) -> Self {
    Self {
      family: style.font_family.clone(),
      size: style.font_size,
      weight: style.weight,
      style: style.style,
      letter_spacing: style.letter_spacing,
      font_features: style.font_features.clone(),
    }
  }
}
impl Default for CanvasFont {
  fn default() -> Self {
    Self::from_style(&TextStyle::default())
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
  #[default]
  Left,
  Center,
  Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextBaseline {
  #[default]
  Alphabetic,
  Top,
  Middle,
  Bottom,
}

/// Logical-pixel metrics relative to the selected alignment and baseline.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
  pub width: f32,
  pub actual_bounding_box_left: f32,
  pub actual_bounding_box_right: f32,
  pub actual_bounding_box_ascent: f32,
  pub actual_bounding_box_descent: f32,
  pub font_bounding_box_ascent: f32,
  pub font_bounding_box_descent: f32,
}

pub(crate) struct CanvasTextEngine {
  fonts: FontSystem,
  aliases: HashMap<String, String>,
  // The engine owns a snapshot of the font database, so this never needs clearing.
  face_weights: FaceWeights,
  swash: GlyphCache,
  shaped: ShapeCache,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Output {
  Metrics,
  Rendered,
}

pub(super) struct ShapedText {
  pub pixels: Option<Arc<Pixmap>>,
  pub data: Arc<Vec<u8>>,
  pub asset_id: u64,
  width: f32,
  left: f32,
  top: f32,
  right: f32,
  bottom: f32,
  ascent: f32,
  descent: f32,
}

impl ShapedText {
  fn offset(&self, align: TextAlign, baseline: TextBaseline) -> (f32, f32) {
    let x = match align {
      TextAlign::Left => 0.0,
      TextAlign::Center => -self.width * 0.5,
      TextAlign::Right => -self.width,
    };
    let y = match baseline {
      TextBaseline::Alphabetic => 0.0,
      TextBaseline::Top => self.ascent,
      TextBaseline::Middle => (self.ascent - self.descent) * 0.5,
      TextBaseline::Bottom => -self.descent,
    };
    (x, y)
  }
  pub fn origin(&self, align: TextAlign, baseline: TextBaseline) -> (f32, f32) {
    let (x, y) = self.offset(align, baseline);
    (x + self.left, y + self.top)
  }
  pub fn metrics(&self, align: TextAlign, baseline: TextBaseline) -> TextMetrics {
    let (x, y) = self.offset(align, baseline);
    TextMetrics {
      width: self.width,
      actual_bounding_box_left: -(x + self.left),
      actual_bounding_box_right: x + self.right,
      actual_bounding_box_ascent: -(y + self.top),
      actual_bounding_box_descent: y + self.bottom,
      font_bounding_box_ascent: self.ascent - y,
      font_bounding_box_descent: self.descent + y,
    }
  }
}

impl CanvasTextEngine {
  pub(crate) fn new(fonts: FontSystem, aliases: HashMap<String, String>) -> Self {
    Self::with_shapes(fonts, aliases, ShapeCache::new())
  }
  /// For an engine that replaces one whose frames a GPU renderer ended. Its
  /// canvases are encoded by the same renderer, so its first frame may keep a
  /// page over the shape budget whole, like any later frame, instead of
  /// evicting the page's first texts while drawing its last ones.
  pub(crate) fn framed(fonts: FontSystem, aliases: HashMap<String, String>) -> Self {
    Self::with_shapes(fonts, aliases, ShapeCache::framed())
  }
  fn with_shapes(fonts: FontSystem, aliases: HashMap<String, String>, shaped: ShapeCache) -> Self {
    Self {
      fonts,
      aliases,
      face_weights: FaceWeights::default(),
      swash: GlyphCache::new(),
      shaped,
    }
  }
  /// Whether the engine's shaped text knows where frames are.
  pub(crate) fn is_framed(&self) -> bool {
    self.shaped.is_framed()
  }
  /// Charged bytes of the shaped text the engine keeps, for the profiler.
  #[cfg(feature = "perf_profile")]
  pub(crate) fn shape_cache_bytes(&self) -> usize {
    self.shaped.bytes()
  }

  pub(super) fn shape(
    &mut self,
    text: &str,
    font: &CanvasFont,
    scale: f32,
    color: Color,
  ) -> Result<Arc<ShapedText>, CanvasError> {
    self.prepare(text, font, scale, color, Output::Rendered)
  }
  pub(super) fn measure(
    &mut self,
    text: &str,
    font: &CanvasFont,
    color: Color,
  ) -> Result<Arc<ShapedText>, CanvasError> {
    self.prepare(text, font, 1.0, color, Output::Metrics)
  }
  fn prepare(
    &mut self,
    text: &str,
    font: &CanvasFont,
    scale: f32,
    color: Color,
    output: Output,
  ) -> Result<Arc<ShapedText>, CanvasError> {
    #[cfg(feature = "perf_profile")]
    let _total = Timer::new(Stage::Total);
    if let Some(result) = self.shaped.get(text, font, scale, color, output) {
      #[cfg(feature = "perf_profile")]
      profile::shape_hit(true);
      return Ok(result);
    }
    #[cfg(feature = "perf_profile")]
    profile::shape_hit(false);
    let result = Arc::new(self.shape_uncached(text, font, scale, color, output)?);
    #[cfg(feature = "perf_profile")]
    profile::produced(result.data.len());
    self.shaped.insert(text, font, scale, color, output, result.clone());
    Ok(result)
  }
  fn shape_uncached(
    &mut self,
    text: &str,
    font: &CanvasFont,
    scale: f32,
    color: Color,
    output: Output,
  ) -> Result<ShapedText, CanvasError> {
    if text.len() > 65_536 || font.size * scale > 4096.0 {
      return Err(CanvasError::TextTooLarge);
    }
    #[cfg(feature = "perf_profile")]
    let _buffer = Timer::new(Stage::BufferFontShape);
    let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(font.size, font.size * 1.2));
    buffer.set_size(None, None);
    buffer.set_wrap(Wrap::None);
    let family = self
      .aliases
      .get(alias_name(&font.family))
      .map(String::as_str)
      .unwrap_or(&font.family);
    let weight = self
      .face_weights
      .resolve(self.fonts.db(), family, font.weight, font.style);
    let mut attrs = Attrs::new()
      .family(cosmic_family(family))
      .weight(weight)
      .style(font.style.to_cosmic());
    if !font.font_features.is_empty() {
      attrs = attrs.font_features(font.font_features.to_cosmic());
    }
    let attrs = with_letter_spacing(attrs, font.letter_spacing, font.size);
    let text = text.replace(['\n', '\r', '\t'], " ");
    buffer.set_text(&text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut self.fonts, false);
    #[cfg(feature = "perf_profile")]
    drop(_buffer);
    #[cfg(feature = "perf_profile")]
    let _glyph = Timer::new(Stage::GlyphPrepare);
    let mut glyphs = Vec::new();
    let mut has_ink = false;
    let mut glyph_bytes = 0usize;
    let (mut width, mut ascent, mut descent) = (0.0f32, 0.0f32, 0.0f32);
    let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for run in buffer.layout_runs() {
      width = width.max(run.line_w);
      for glyph in run.glyphs {
        if let Some(face) = self.fonts.get_font(glyph.font_id, glyph.font_weight) {
          let metrics = face.as_swash().metrics(&[]).scale(font.size);
          ascent = ascent.max(metrics.ascent);
          descent = descent.max(metrics.descent.abs());
        }
        let physical = glyph.physical((0.0, 0.0), scale);
        let Some(image) = self.swash.image(&mut self.fonts, physical.cache_key).as_ref() else {
          continue;
        };
        if image.placement.width == 0 || image.placement.height == 0 {
          continue;
        }
        if u64::from(image.placement.width) * u64::from(image.placement.height) > MAX_PIXELS {
          return Err(CanvasError::TextTooLarge);
        }
        glyph_bytes += image.data.len();
        if glyph_bytes > 64 * 1024 * 1024 {
          return Err(CanvasError::TextTooLarge);
        }
        let (Some(x), Some(y)) = (
          physical.x.checked_add(image.placement.left),
          physical.y.checked_sub(image.placement.top),
        ) else {
          return Err(CanvasError::TextTooLarge);
        };
        left = left.min(x);
        top = top.min(y);
        let (Some(r), Some(b)) = (
          x.checked_add(image.placement.width as i32),
          y.checked_add(image.placement.height as i32),
        ) else {
          return Err(CanvasError::TextTooLarge);
        };
        right = right.max(r);
        bottom = bottom.max(b);
        if (i64::from(right) - i64::from(left)).saturating_mul(i64::from(bottom) - i64::from(top)) > MAX_PIXELS as i64 {
          return Err(CanvasError::TextTooLarge);
        }
        has_ink = true;
        if output == Output::Rendered {
          glyphs.push((x, y, image.clone()));
        }
      }
    }
    #[cfg(feature = "perf_profile")]
    drop(_glyph);
    if !has_ink {
      return Ok(ShapedText {
        pixels: None,
        data: Arc::new(Vec::new()),
        asset_id: 0,
        width,
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        ascent,
        descent,
      });
    }
    let (w, h) = (i64::from(right) - i64::from(left), i64::from(bottom) - i64::from(top));
    if w <= 0 || h <= 0 || w * h > MAX_PIXELS as i64 {
      return Err(CanvasError::TextTooLarge);
    }
    if output == Output::Metrics {
      return Ok(ShapedText {
        pixels: None,
        data: Arc::new(Vec::new()),
        asset_id: 0,
        width,
        left: left as f32 / scale,
        top: top as f32 / scale,
        right: right as f32 / scale,
        bottom: bottom as f32 / scale,
        ascent,
        descent,
      });
    }
    #[cfg(feature = "perf_profile")]
    let _bitmap = Timer::new(Stage::BitmapComposition);
    let mut pixels = Pixmap::new(w as u32, h as u32).ok_or(CanvasError::TextTooLarge)?;
    for (x, y, image) in glyphs {
      let mut glyph = Pixmap::new(image.placement.width, image.placement.height).ok_or(CanvasError::TextTooLarge)?;
      for (index, p) in glyph.data_mut().chunks_exact_mut(4).enumerate() {
        let (r, g, b, a) = match image.content {
          SwashContent::Mask => (color.r(), color.g(), color.b(), multiply(image.data[index], color.a())),
          SwashContent::Color => {
            let v = &image.data[index * 4..index * 4 + 4];
            (v[0], v[1], v[2], multiply(v[3], color.a()))
          }
          SwashContent::SubpixelMask => {
            let v = &image.data[index * 4..index * 4 + 4];
            (
              color.r(),
              color.g(),
              color.b(),
              multiply(v[0].max(v[1]).max(v[2]), color.a()),
            )
          }
        };
        p.copy_from_slice(&[multiply(r, a), multiply(g, a), multiply(b, a), a]);
      }
      pixels.draw_pixmap(
        x - left,
        y - top,
        glyph.as_ref(),
        &PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
      );
    }
    Ok(ShapedText {
      asset_id: super::NEXT_CANVAS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed) | (1u64 << 63),
      data: Arc::new(pixels.data().to_vec()),
      pixels: Some(Arc::new(pixels)),
      width,
      left: left as f32 / scale,
      top: top as f32 / scale,
      right: right as f32 / scale,
      bottom: bottom as f32 / scale,
      ascent,
      descent,
    })
  }
}

fn multiply(a: u8, b: u8) -> u8 {
  ((u16::from(a) * u16::from(b) + 127) / 255) as u8
}
