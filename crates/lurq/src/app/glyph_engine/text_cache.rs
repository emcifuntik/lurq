//! What the engine knows of a plain text laid out one way (its size, where
//! its lines sit, its carets, its glyphs), under one key and one lookup.
//!
//! A lookup hashes the text with a fast seeded hash, finds the entry by that
//! fingerprint and compares the whole key, without copying the text. A hit
//! allocates nothing; a miss stores the key once, whatever is then cached
//! under it.
//!
//! An entry remembers the pass that last used it. When the cache is full it
//! drops the entries no pass used for a while, and nothing else: what the
//! window shows stays, however many texts that is, and the cache never
//! empties at once. A window that shows more texts than the cache holds
//! makes it larger.

use std::{
  collections::HashMap,
  hash::{BuildHasher, BuildHasherDefault, Hash, Hasher},
};

use super::{
  CacheKey, CachedGlyph, TextVerticalExtents, hash_sampled_text, letter_spacing_bits, style_to_u8, text_align_to_u8,
};
use crate::{
  layout::{Size, text_style::TextStyle},
  node::text_selection::CaretPositions,
};

/// Passes an entry stays through without being used once the cache is full.
/// Several windows share the cache, and each one's pass counts.
const KEPT_PASSES: u32 = 16;
/// The cache never grows past this many times its starting size: beyond it
/// everything goes, as nothing can tell what is in use.
const MOST_GROWTH: usize = 64;

/// A fast hasher for cache fingerprints. Every lookup compares the whole key
/// after it, so a collision costs a comparison, never a wrong result. Seeded
/// per engine: what a text hashes to cannot be told from outside.
#[derive(Clone, Copy)]
pub(super) struct FingerprintHasher(u64);

/// An odd constant with no structure: 2^64 over the golden ratio.
const MULTIPLIER: u64 = 0x9E37_79B9_7F4A_7C15;

impl FingerprintHasher {
  pub(super) fn new(seed: u64) -> Self {
    Self(seed)
  }

  /// Both halves of the 128-bit product, folded: every input bit reaches
  /// every output bit.
  fn add(&mut self, word: u64) {
    let product = u128::from(self.0 ^ word) * u128::from(MULTIPLIER);
    self.0 = (product as u64) ^ ((product >> 64) as u64);
  }
}

impl Hasher for FingerprintHasher {
  fn write(&mut self, bytes: &[u8]) {
    let mut words = bytes.chunks_exact(8);
    for word in &mut words {
      self.add(u64::from_le_bytes(word.try_into().expect("eight bytes")));
    }
    let rest = words.remainder();
    if !rest.is_empty() {
      let mut last = [0_u8; 8];
      last[..rest.len()].copy_from_slice(rest);
      self.add(u64::from_le_bytes(last));
    }
    // Zero padding must not make "a" and "a\0" one input.
    self.add(bytes.len() as u64);
  }

  fn write_u8(&mut self, value: u8) {
    self.add(u64::from(value));
  }

  fn write_u16(&mut self, value: u16) {
    self.add(u64::from(value));
  }

  fn write_u32(&mut self, value: u32) {
    self.add(u64::from(value));
  }

  fn write_u64(&mut self, value: u64) {
    self.add(value);
  }

  fn write_usize(&mut self, value: usize) {
    self.add(value as u64);
  }

  fn finish(&self) -> u64 {
    let mut finished = *self;
    finished.add(0);
    finished.0
  }
}

/// Builds [`FingerprintHasher`]s with one seed: the hasher of a map whose
/// keys are small and looked up for every glyph, such as the atlas's.
#[derive(Clone, Copy)]
pub(super) struct SeededFingerprints(u64);

impl SeededFingerprints {
  pub(super) fn new() -> Self {
    Self(fingerprint_seed())
  }
}

impl BuildHasher for SeededFingerprints {
  type Hasher = FingerprintHasher;

  fn build_hasher(&self) -> FingerprintHasher {
    FingerprintHasher::new(self.0)
  }
}

/// The hasher of a map whose keys are fingerprints: the key is its own hash.
#[derive(Default)]
pub(super) struct FingerprintIsHash(u64);

impl Hasher for FingerprintIsHash {
  fn write(&mut self, bytes: &[u8]) {
    for byte in bytes {
      self.0 = self.0.rotate_left(8) ^ u64::from(*byte);
    }
  }

  fn write_u64(&mut self, value: u64) {
    self.0 = value;
  }

  fn finish(&self) -> u64 {
    self.0
  }
}

pub(super) type FingerprintMap<V> = HashMap<u64, V, BuildHasherDefault<FingerprintIsHash>>;

/// A seed no other engine or process has.
pub(super) fn fingerprint_seed() -> u64 {
  std::collections::hash_map::RandomState::new().hash_one(0_u8)
}

/// The two ways a text's glyphs are placed.
#[derive(Clone, Copy)]
pub(super) enum GlyphPlacement {
  /// At whole pixels, as text in the window is drawn.
  Snapped = 0,
  /// Where layout put them, for text drawn through a transform.
  Unsnapped = 1,
}

/// What is known of one text laid out one way. Each part is filled in by the
/// first request for it.
pub(super) struct TextEntry {
  key: CacheKey,
  /// The pass that last asked for the entry.
  used: u32,
  pub(super) size: Option<Size>,
  /// The outer `Option` is "known"; the inner one, "the text paints ink".
  pub(super) vertical_extents: Option<Option<TextVerticalExtents>>,
  pub(super) optical_extents: Option<Option<(f32, f32)>>,
  pub(super) carets: Option<CaretPositions>,
  glyphs: [Option<Vec<CachedGlyph>>; 2],
}

impl TextEntry {
  pub(super) fn glyphs(&self, placement: GlyphPlacement) -> Option<&[CachedGlyph]> {
    self.glyphs[placement as usize].as_deref()
  }

  pub(super) fn set_glyphs(&mut self, placement: GlyphPlacement, glyphs: Vec<CachedGlyph>) {
    self.glyphs[placement as usize] = Some(glyphs);
  }

  fn heap_bytes(&self) -> usize {
    self.key.text.capacity()
      + self.key.font_family.len()
      + self
        .glyphs
        .iter()
        .flatten()
        .map(|glyphs| glyphs.capacity() * std::mem::size_of::<CachedGlyph>())
        .sum::<usize>()
  }
}

pub(super) struct TextCache {
  seed: u64,
  entries: FingerprintMap<TextEntry>,
  /// Entries the cache holds before it looks for unused ones.
  room: usize,
  /// The `room` of a cache with little in use.
  least_room: usize,
  /// Counts the passes: `begin_pass`.
  pass: u32,
}

impl TextCache {
  pub(super) fn new(room: usize) -> Self {
    Self {
      seed: fingerprint_seed(),
      entries: FingerprintMap::default(),
      room,
      least_room: room,
      pass: 0,
    }
  }

  /// A pass of a window begins: what it uses from here on is in use.
  pub(super) fn begin_pass(&mut self) {
    self.pass = self.pass.wrapping_add(1);
    if self.entries.len() >= self.room {
      self.make_room();
    }
  }

  /// The fingerprint of a text laid out one way; what `get` and `entry` find
  /// its entry by.
  pub(super) fn fingerprint(&self, text: &str, style: &TextStyle, max_width: f32, wrap: bool) -> u64 {
    let mut hasher = FingerprintHasher::new(self.seed);
    hash_sampled_text(text, &mut hasher);
    style.font_family.hash(&mut hasher);
    style.font_size.to_bits().hash(&mut hasher);
    style.line_height.to_bits().hash(&mut hasher);
    letter_spacing_bits(style.letter_spacing).hash(&mut hasher);
    style.font_features.hash(&mut hasher);
    max_width.to_bits().hash(&mut hasher);
    style.weight.value().hash(&mut hasher);
    style_to_u8(style.style).hash(&mut hasher);
    text_align_to_u8(style.text_align).hash(&mut hasher);
    wrap.hash(&mut hasher);
    style.trim_line_box.hash(&mut hasher);
    hasher.finish()
  }

  /// The entry of the text, if it has one. `fingerprint` is its
  /// [`fingerprint`](Self::fingerprint).
  pub(super) fn get(
    &mut self,
    fingerprint: u64,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
  ) -> Option<&mut TextEntry> {
    let entry = self
      .entries
      .get_mut(&fingerprint)
      .filter(|entry| entry.key.matches_measure(text, style, max_width, wrap))?;
    entry.used = self.pass;
    Some(entry)
  }

  /// The entry of the text, made empty if it has none.
  pub(super) fn entry(
    &mut self,
    fingerprint: u64,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
  ) -> &mut TextEntry {
    if self.get(fingerprint, text, style, max_width, wrap).is_none() {
      if self.entries.len() >= self.room && !self.entries.contains_key(&fingerprint) {
        self.make_room();
      }
      // Two texts with one fingerprint: the newer takes the place.
      self.entries.insert(
        fingerprint,
        TextEntry {
          key: CacheKey::new(text, style, max_width, wrap),
          used: self.pass,
          size: None,
          vertical_extents: None,
          optical_extents: None,
          carets: None,
          glyphs: [None, None],
        },
      );
    }
    self
      .entries
      .get_mut(&fingerprint)
      .expect("the entry was found or just inserted")
  }

  /// Drops the entries no pass used for a while. When what is left still
  /// fills half the room, the room grows: all of it is in use.
  fn make_room(&mut self) {
    let pass = self.pass;
    self
      .entries
      .retain(|_, entry| pass.wrapping_sub(entry.used) <= KEPT_PASSES);
    if self.entries.len() >= self.least_room * MOST_GROWTH {
      self.entries.clear();
    }
    self.room = (self.entries.len() * 2).max(self.least_room);
  }

  pub(super) fn clear(&mut self) {
    self.entries.clear();
    self.room = self.least_room;
  }

  #[cfg(test)]
  pub(super) fn is_empty(&self) -> bool {
    self.entries.is_empty()
  }

  #[cfg(test)]
  pub(super) fn len(&self) -> usize {
    self.entries.len()
  }

  pub(super) fn estimated_memory_bytes(&self) -> usize {
    self.entries.capacity() * std::mem::size_of::<(u64, TextEntry)>()
      + self.entries.values().map(TextEntry::heap_bytes).sum::<usize>()
  }
}

#[cfg(test)]
mod tests {
  use std::hash::{Hash, Hasher};

  use super::{FingerprintHasher, GlyphPlacement, KEPT_PASSES, MOST_GROWTH, TextCache};
  use crate::layout::{Size, text_style::TextStyle};

  fn hash(seed: u64, value: impl Hash) -> u64 {
    let mut hasher = FingerprintHasher::new(seed);
    value.hash(&mut hasher);
    hasher.finish()
  }

  #[test]
  fn fingerprints_tell_texts_lengths_and_seeds_apart() {
    assert_eq!(hash(1, "message"), hash(1, "message"));
    assert_ne!(hash(1, "message"), hash(1, "massage"));
    assert_ne!(hash(1, "message"), hash(2, "message"));
    // Texts that differ only past a whole number of words, or only in
    // trailing zero bytes.
    assert_ne!(hash(1, "12345678"), hash(1, "123456789"));
    assert_ne!(hash(1, "a"), hash(1, "a\0"));
    assert_ne!(hash(1, ""), hash(1, "\0"));
    // The split between two strings is part of the input.
    assert_ne!(hash(1, ("ab", "c")), hash(1, ("a", "bc")));
  }

  #[test]
  fn fingerprints_spread_over_a_map() {
    // Ten thousand short texts: no two share a fingerprint, and each of the
    // 64 lowest and highest buckets is used.
    let fingerprints: std::collections::HashSet<u64> = (0..10_000).map(|index| hash(7, format!("{index}"))).collect();
    assert_eq!(fingerprints.len(), 10_000);
    let low: std::collections::HashSet<u64> = fingerprints.iter().map(|fingerprint| fingerprint & 63).collect();
    let high: std::collections::HashSet<u64> = fingerprints.iter().map(|fingerprint| fingerprint >> 58).collect();
    assert_eq!((low.len(), high.len()), (64, 64));
  }

  fn cached_size(cache: &mut TextCache, text: &str, width: f32) -> Option<Size> {
    let style = TextStyle::default();
    let fingerprint = cache.fingerprint(text, &style, width, true);
    cache.get(fingerprint, text, &style, width, true)?.size
  }

  fn cache_size(cache: &mut TextCache, text: &str, width: f32, size: Size) {
    let style = TextStyle::default();
    let fingerprint = cache.fingerprint(text, &style, width, true);
    cache.entry(fingerprint, text, &style, width, true).size = Some(size);
  }

  const SIZE: Size = Size {
    width: 1.0,
    height: 1.0,
  };

  #[test]
  fn an_entry_is_found_by_its_whole_key() {
    let mut cache = TextCache::new(8);
    cache_size(&mut cache, "hello", 100.0, Size::new(30.0, 12.0));

    assert_eq!(
      cached_size(&mut cache, "hello", 100.0).map(|size| size.width),
      Some(30.0)
    );
    assert!(cached_size(&mut cache, "hello", 101.0).is_none(), "another width");
    assert!(cached_size(&mut cache, "hallo", 100.0).is_none(), "another text");

    // Another weight or wrapping is another entry.
    let bold = TextStyle {
      weight: crate::layout::text_style::FontWeight::Bold,
      ..TextStyle::default()
    };
    let fingerprint = cache.fingerprint("hello", &bold, 100.0, true);
    assert!(cache.get(fingerprint, "hello", &bold, 100.0, true).is_none());
    let style = TextStyle::default();
    let fingerprint = cache.fingerprint("hello", &style, 100.0, false);
    assert!(cache.get(fingerprint, "hello", &style, 100.0, false).is_none());
  }

  #[test]
  fn a_fingerprint_shared_by_two_texts_serves_neither_the_other() {
    let mut cache = TextCache::new(8);
    let style = TextStyle::default();
    // The same fingerprint given for two texts, as a collision would.
    cache.entry(42, "first", &style, 100.0, true).size = Some(Size::new(1.0, 1.0));
    assert!(cache.get(42, "second", &style, 100.0, true).is_none());

    cache.entry(42, "second", &style, 100.0, true).size = Some(Size::new(2.0, 2.0));
    assert_eq!(
      cache
        .get(42, "second", &style, 100.0, true)
        .and_then(|entry| entry.size)
        .map(|size| size.width),
      Some(2.0)
    );
    assert!(cache.get(42, "first", &style, 100.0, true).is_none());
  }

  #[test]
  fn the_parts_of_an_entry_are_kept_together() {
    let mut cache = TextCache::new(8);
    let style = TextStyle::default();
    let fingerprint = cache.fingerprint("hello", &style, 100.0, true);
    cache.entry(fingerprint, "hello", &style, 100.0, true).size = Some(Size::new(30.0, 12.0));
    cache.entry(fingerprint, "hello", &style, 100.0, true).optical_extents = Some(Some((2.0, 10.0)));
    cache
      .entry(fingerprint, "hello", &style, 100.0, true)
      .set_glyphs(GlyphPlacement::Snapped, Vec::new());

    assert_eq!(cache.len(), 1);
    let entry = cache.get(fingerprint, "hello", &style, 100.0, true).expect("the entry");
    assert!(entry.size.is_some());
    assert_eq!(entry.optical_extents, Some(Some((2.0, 10.0))));
    assert!(entry.vertical_extents.is_none());
    assert!(entry.glyphs(GlyphPlacement::Snapped).is_some());
    assert!(entry.glyphs(GlyphPlacement::Unsnapped).is_none());
  }

  #[test]
  fn a_full_cache_drops_what_no_pass_used_for_a_while_and_keeps_the_rest() {
    let mut cache = TextCache::new(8);
    cache_size(&mut cache, "shown", 100.0, SIZE);
    for pass in 0..200 {
      cache.begin_pass();
      // Every pass shows one text again and one new text.
      assert!(cached_size(&mut cache, "shown", 100.0).is_some(), "pass {pass}");
      cache_size(&mut cache, &format!("scrolled past {pass}"), 100.0, SIZE);
    }

    assert!(cached_size(&mut cache, "scrolled past 0", 100.0).is_none());
    assert!(cached_size(&mut cache, "scrolled past 199", 100.0).is_some());
    // The text of every pass and those of the passes an entry is kept for,
    // twice over: a full cache makes room for as much again as is in use.
    assert!(cache.len() <= 2 * (KEPT_PASSES as usize + 2), "{} entries", cache.len());
  }

  #[test]
  fn a_pass_that_uses_more_than_the_cache_holds_keeps_all_of_it() {
    let mut cache = TextCache::new(8);
    for pass in 0..4 {
      cache.begin_pass();
      for index in 0..100 {
        let text = format!("cell {index}");
        if pass == 0 {
          cache_size(&mut cache, &text, 100.0, SIZE);
        } else {
          assert!(cached_size(&mut cache, &text, 100.0).is_some(), "pass {pass}, {text}");
        }
      }
    }
    assert_eq!(cache.len(), 100);

    // Once the window shows little again, the cache shrinks back: ten new
    // texts a pass, kept for the passes an entry is kept for, twice over.
    for pass in 0..4 * KEPT_PASSES {
      cache.begin_pass();
      for index in 0..10 {
        cache_size(&mut cache, &format!("pass {pass} label {index}"), 100.0, SIZE);
      }
    }
    assert!(cached_size(&mut cache, "cell 0", 100.0).is_none());
    assert!(
      cache.len() <= 2 * 10 * (KEPT_PASSES as usize + 2),
      "{} entries",
      cache.len()
    );
  }

  #[test]
  fn a_cache_that_no_pass_ever_ends_stays_bounded() {
    // Nothing tells what is in use: at the limit everything goes.
    let mut cache = TextCache::new(4);
    for index in 0..10_000 {
      cache_size(&mut cache, &format!("{index}"), 100.0, SIZE);
      assert!(cache.len() <= 4 * MOST_GROWTH * 2);
    }
    assert!(cached_size(&mut cache, "9999", 100.0).is_some());
  }

  #[test]
  fn clearing_drops_every_entry() {
    let mut cache = TextCache::new(2);
    for index in 0..5 {
      cache_size(&mut cache, &format!("{index}"), 100.0, SIZE);
    }
    assert!(!cache.is_empty());
    cache.clear();
    assert!(cache.is_empty());
    assert!(cached_size(&mut cache, "4", 100.0).is_none());
  }
}
