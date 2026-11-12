//! Short plain texts kept shaped, found by what shapes them.
//!
//! Shaping depends on a text and its face: the family, weight, slant,
//! features and tracking in ems. The size, the width, the wrapping and the
//! alignment only place the shaped glyphs. So a text is shaped once and laid
//! out again when its width or size changes: a resized window, a scale
//! factor, a measurement in logical pixels and the paint in physical ones.
//!
//! The cache holds a budget of bytes and drops the texts put longest ago, one
//! at a time, as newer ones come.

use std::{
  collections::{VecDeque, hash_map::Entry},
  hash::{Hash, Hasher},
};

use super::{
  CacheKey, CachedPlainBuffer, letter_spacing_bits, style_to_u8,
  text_cache::{FingerprintHasher, FingerprintMap, fingerprint_seed},
};
use crate::layout::text_style::{FontFeatures, TextStyle};

/// Texts shorter than this are kept here. Longer ones are documents: their
/// paragraphs are kept and reused one by one (`GlyphEngine::plain_buffers`).
pub(super) const SHORT_TEXT_BYTES: usize = 1024;

/// The tracking that reaches shaping: in ems, as cosmic-text takes it.
fn tracking_bits(letter_spacing_bits: u32, font_size_bits: u32) -> u32 {
  if letter_spacing_bits == 0 {
    return 0;
  }
  (f32::from_bits(letter_spacing_bits) / f32::from_bits(font_size_bits)).to_bits()
}

/// Whether the buffer of `key` holds `text` shaped as `style` shapes it.
fn shaped_alike(key: &CacheKey, text: &str, style: &TextStyle) -> bool {
  key.weight == style.weight.value()
    && key.style == style_to_u8(style.style)
    && tracking_bits(key.letter_spacing_bits, key.font_size_bits)
      == tracking_bits(letter_spacing_bits(style.letter_spacing), style.font_size.to_bits())
    && key.font_family == style.font_family
    && key.font_features == style.font_features
    && key.text == text
}

pub(super) struct ShapedTexts {
  seed: u64,
  /// Each buffer with the stamp of its last `put`.
  entries: FingerprintMap<(u64, CachedPlainBuffer)>,
  /// Fingerprints in the order they were put, oldest first, each with the
  /// stamp of that put. A text taken and put again leaves its older place
  /// behind; the stamp tells that place from its current one.
  order: VecDeque<(u64, u64)>,
  stamp: u64,
  bytes: usize,
  budget: usize,
}

impl ShapedTexts {
  pub(super) fn new(budget: usize) -> Self {
    Self {
      seed: fingerprint_seed(),
      entries: FingerprintMap::default(),
      order: VecDeque::new(),
      stamp: 0,
      bytes: 0,
      budget,
    }
  }

  fn fingerprint(
    &self,
    text: &str,
    family: &str,
    weight: u16,
    style: u8,
    features: &FontFeatures,
    tracking: u32,
  ) -> u64 {
    let mut hasher = FingerprintHasher::new(self.seed);
    text.hash(&mut hasher);
    family.hash(&mut hasher);
    weight.hash(&mut hasher);
    style.hash(&mut hasher);
    features.hash(&mut hasher);
    tracking.hash(&mut hasher);
    hasher.finish()
  }

  fn fingerprint_of_style(&self, text: &str, style: &TextStyle) -> u64 {
    self.fingerprint(
      text,
      &style.font_family,
      style.weight.value(),
      style_to_u8(style.style),
      &style.font_features,
      tracking_bits(letter_spacing_bits(style.letter_spacing), style.font_size.to_bits()),
    )
  }

  fn fingerprint_of_key(&self, key: &CacheKey) -> u64 {
    self.fingerprint(
      &key.text,
      &key.font_family,
      key.weight,
      key.style,
      &key.font_features,
      tracking_bits(key.letter_spacing_bits, key.font_size_bits),
    )
  }

  /// Takes `text` shaped as `style` shapes it out of the cache. It may be
  /// laid out for another size, width, wrapping or alignment.
  pub(super) fn take(&mut self, text: &str, style: &TextStyle) -> Option<CachedPlainBuffer> {
    let fingerprint = self.fingerprint_of_style(text, style);
    let Entry::Occupied(entry) = self.entries.entry(fingerprint) else {
      return None;
    };
    if !shaped_alike(&entry.get().1.key, text, style) {
      return None;
    }
    let (_, buffer) = entry.remove();
    self.bytes -= buffer.bytes;
    Some(buffer)
  }

  /// Puts a shaped text into the cache, as the newest. The texts put longest
  /// ago go until the budget holds.
  pub(super) fn put(&mut self, buffer: CachedPlainBuffer) {
    let fingerprint = self.fingerprint_of_key(&buffer.key);
    self.stamp += 1;
    self.bytes += buffer.bytes;
    // The same text put twice, or two texts with one fingerprint: the newer
    // takes the place.
    if let Some((_, replaced)) = self.entries.insert(fingerprint, (self.stamp, buffer)) {
      self.bytes -= replaced.bytes;
    }
    self.order.push_back((fingerprint, self.stamp));
    while self.bytes > self.budget {
      let Some((oldest, stamp)) = self.order.pop_front() else {
        break;
      };
      if let Entry::Occupied(entry) = self.entries.entry(oldest) {
        if entry.get().0 == stamp {
          self.bytes -= entry.remove().1.bytes;
        }
      }
    }
    // Places left behind by texts taken and put again.
    if self.order.len() > self.entries.len() * 2 + 64 {
      let entries = &self.entries;
      self
        .order
        .retain(|(fingerprint, stamp)| entries.get(fingerprint).is_some_and(|(current, _)| current == stamp));
    }
  }

  pub(super) fn clear(&mut self) {
    self.entries.clear();
    self.order.clear();
    self.bytes = 0;
  }

  /// Bytes the kept buffers are charged for.
  pub(super) fn bytes(&self) -> usize {
    self.bytes
  }

  #[cfg(test)]
  pub(super) fn len(&self) -> usize {
    self.entries.len()
  }

  /// Places in the order of puts, those left behind included.
  #[cfg(test)]
  pub(super) fn places(&self) -> usize {
    self.order.len()
  }

  /// The cache's own storage, without the buffers `bytes` counts.
  pub(super) fn estimated_overhead_bytes(&self) -> usize {
    self.entries.capacity() * std::mem::size_of::<(u64, (u64, CachedPlainBuffer))>()
      + self.order.capacity() * std::mem::size_of::<(u64, u64)>()
  }
}
