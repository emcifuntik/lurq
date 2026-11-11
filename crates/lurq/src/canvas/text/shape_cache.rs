//! Shaped Canvas text kept across frames by the shared frame-stamped policy.
//!
//! A page draws the same labels in the same order every frame. The former
//! 256-entry least-recently-used list missed on every one of them once a page
//! had more keys than that, and each label shaped again was rasterized into a
//! new asset that the GPU renderer then uploaded again.
use std::{
  hash::{DefaultHasher, Hash, Hasher},
  sync::Arc,
};

use super::{CanvasFont, Output, ShapedText};
#[cfg(feature = "perf_profile")]
use crate::app::profiler::canvas_text as profile;
use crate::{
  canvas::frame_cache::{FrameCache, Limits},
  node::color::Color,
};

/// The former 8 MiB, which a frame's own text may stretch to twice.
pub(super) const LIMITS: Limits = Limits {
  budget: 8 * 1024 * 1024,
  ceiling: 16 * 1024 * 1024,
};
/// Charged per entry beyond its text and pixels, for the key, the table and
/// the eviction queue, so that many empty measurements stay bounded too.
const ENTRY_CHARGE: usize = 256;

struct Key {
  text: String,
  font: CanvasFont,
  scale: f32,
  color: Color,
  output: Output,
}

impl Key {
  /// The equality shaping has always used, including float `==`.
  fn matches(&self, text: &str, font: &CanvasFont, scale: f32, color: Color, output: Output) -> bool {
    self.text == text && self.font == *font && self.scale == scale && self.color == color && self.output == output
  }
}

/// Equal keys hash equally: `-0.0 == 0.0`, so zeros hash alike. A collision is
/// harmless, because a hit also compares the whole key.
fn identity(text: &str, font: &CanvasFont, scale: f32, color: Color, output: Output) -> u64 {
  let float = |value: f32| if value == 0.0 { 0 } else { value.to_bits() };
  let mut hasher = DefaultHasher::new();
  text.hash(&mut hasher);
  font.family.hash(&mut hasher);
  float(font.size).hash(&mut hasher);
  font.weight.hash(&mut hasher);
  font.style.hash(&mut hasher);
  float(font.letter_spacing).hash(&mut hasher);
  font.font_features.hash(&mut hasher);
  float(scale).hash(&mut hasher);
  [color.r(), color.g(), color.b(), color.a()].hash(&mut hasher);
  matches!(output, Output::Rendered).hash(&mut hasher);
  hasher.finish()
}

struct Shape {
  key: Key,
  result: Arc<ShapedText>,
}

pub(super) struct ShapeCache {
  shapes: FrameCache<Shape>,
}

impl ShapeCache {
  pub(super) fn new() -> Self {
    Self::with_limits(LIMITS)
  }
  pub(super) fn with_limits(limits: Limits) -> Self {
    Self {
      shapes: FrameCache::new(limits),
    }
  }
  /// For an engine whose frames a GPU renderer ends from the first one.
  pub(super) fn framed() -> Self {
    Self {
      shapes: FrameCache::framed(LIMITS),
    }
  }
  pub(super) fn is_framed(&self) -> bool {
    self.shapes.is_framed()
  }

  #[cfg(any(test, feature = "perf_profile"))]
  pub(super) fn bytes(&self) -> usize {
    self.shapes.bytes()
  }
  #[cfg(test)]
  pub(super) fn len(&self) -> usize {
    self.shapes.len()
  }
  #[cfg(test)]
  pub(super) fn results(&self) -> impl Iterator<Item = &Arc<ShapedText>> {
    self.shapes.values().map(|shape| &shape.result)
  }

  pub(super) fn get(
    &mut self,
    text: &str,
    font: &CanvasFont,
    scale: f32,
    color: Color,
    output: Output,
  ) -> Option<Arc<ShapedText>> {
    let shape = self.shapes.get(identity(text, font, scale, color, output))?;
    shape
      .key
      .matches(text, font, scale, color, output)
      .then(|| shape.result.clone())
  }

  /// Keeps `result` unless the text of the current and previous frame has
  /// filled the ceiling, in which case it is only returned to its caller.
  pub(super) fn insert(
    &mut self,
    text: &str,
    font: &CanvasFont,
    scale: f32,
    color: Color,
    output: Output,
    result: Arc<ShapedText>,
  ) {
    // The pixels are held twice, by the pixmap and by the upload data.
    let bytes = result.data.len() * 2 + text.len() + ENTRY_CHARGE;
    let key = Key {
      text: text.to_owned(),
      font: font.clone(),
      scale,
      color,
      output,
    };
    let _kept = self.shapes.insert(
      identity(text, font, scale, color, output),
      Shape { key, result },
      bytes,
      evicted,
    );
    #[cfg(feature = "perf_profile")]
    {
      if _kept.is_err() {
        profile::uncached();
      }
      profile::stretched(self.shapes.bytes().saturating_sub(self.shapes.limits().budget));
    }
  }

  #[cfg(any(test, feature = "wgpu", all(feature = "dx12", target_os = "windows")))]
  pub(super) fn finish_frame(&mut self) {
    // A frame whose text was all cached still holds a stretched cache.
    #[cfg(feature = "perf_profile")]
    profile::stretched(self.shapes.bytes().saturating_sub(self.shapes.limits().budget));
    self.shapes.close_frame(evicted);
  }
}

fn evicted(_shape: Shape) {
  #[cfg(feature = "perf_profile")]
  profile::evicted();
}

/// Text frames end with a GPU renderer's cache frames, so only the GPU
/// backends (and tests) close them.
#[cfg(any(test, feature = "wgpu", all(feature = "dx12", target_os = "windows")))]
mod frames {
  use crate::canvas::{CanvasHandle, CanvasTextEngine};

  impl CanvasHandle {
    /// Ends the text frame of the engine this canvas draws with. The GPU
    /// renderers call it, when a cache frame ends, for every canvas they
    /// encoded in that frame (see `FrameBoundary`); the engine counts one frame
    /// however many of its canvases end it together. A busy engine is skipped
    /// rather than waited for, which only merges two frames into one.
    pub(crate) fn finish_text_frame(&self) {
      let engine = self.inner.lock().text.clone();
      if let Some(engine) = engine
        && let Some(mut text) = engine.try_lock()
      {
        text.finish_frame();
      }
    }

    /// Charged bytes of the shaped text kept by the engine this canvas draws
    /// with, for the GPU renderers' residency tests.
    #[cfg(all(test, any(feature = "wgpu", all(feature = "dx12", target_os = "windows"))))]
    pub(crate) fn shaped_text_bytes(&self) -> usize {
      let engine = self.inner.lock().text.clone();
      engine.map_or(0, |engine| engine.lock().shaped.bytes())
    }
  }

  impl CanvasTextEngine {
    pub(crate) fn finish_frame(&mut self) {
      self.shaped.finish_frame();
    }
  }
}
