#[cfg(feature = "perf_profile")]
use std::time::Instant;
use std::{
  borrow::Cow,
  collections::{HashMap, hash_map::DefaultHasher},
  hash::{Hash, Hasher},
  path::Path,
};

use cosmic_text::{
  Attrs, Buffer, CacheKey as GlyphCacheKey, Color as CosmicColor, Font, FontSystem, LayoutGlyph, Metrics, Shaping,
  SwashContent, SwashImage, Wrap,
};
use swash::{
  scale::{Render, ScaleContext, Source, StrikeWith},
  zeno::{Angle, Format, Transform as SwashTransform, Vector},
};

mod cap_height;
mod carets;
mod face_weight;
mod generic_families;
mod hinted;
pub(crate) mod input_tokens;
#[cfg(target_os = "ios")]
mod ios_emoji;
#[cfg(target_os = "ios")]
mod ios_fallback;
mod letter_spacing;
mod line_clamp;
mod reflow;
mod scaler;
mod shaped_texts;
mod span_boxes;
mod text_cache;
mod vertical_align;
use cap_height::GlyphFace;
pub(crate) use face_weight::FaceWeights;
use letter_spacing::letter_spacing_bits;
pub(crate) use letter_spacing::with_letter_spacing;
use reflow::ReflowRange;
use shaped_texts::{SHORT_TEXT_BYTES, ShapedTexts};
pub(crate) use span_boxes::SpanBox;
use text_cache::{GlyphPlacement, SeededFingerprints, TextCache};

use crate::{
  app::profile_types::GlyphEngineProfile,
  layout::{
    Size,
    quad::{ClipRect, RichTextSpan},
    render_list::{GlyphAtlas, GlyphAtlasDirtyRect, GlyphCmd},
    text_style::{FontFeatures, FontStyle, TextAlign, TextStyle, VerticalAlign, alias_name, cosmic_family},
  },
  node::{
    color::Color,
    text_selection::{CaretPositions, CaretPositionsBuilder, ParagraphCarets},
    transform::Transform2D,
  },
};

const GLYPH_LAYOUT_CACHE_LIMIT: usize = 1024;
const PLAIN_BUFFER_CACHE_LIMIT: usize = 64;
// Keep logical and physical document layouts together at fractional display scales.
const PLAIN_BUFFER_CACHE_BYTES: usize = 48 * 1024 * 1024;
/// Passes, or texts laid out anew, after which the words cosmic-text keeps
/// shaped are looked through for unused ones, whichever comes first.
const SHAPED_WORDS_TRIM_INTERVAL: usize = 256;
/// Looks a shaped word stays through without being used again.
const SHAPED_WORDS_KEPT_TRIMS: u64 = 2;
/// What the short texts kept shaped may hold: a few screens of labels and
/// messages, so that a resize or a new scale factor lays them out again
/// without shaping them again.
const SHAPED_TEXT_CACHE_BYTES: usize = 4 * 1024 * 1024;
/// Transparent texels the atlas keeps around every glyph it packs.
pub(crate) const GLYPH_ATLAS_PADDING: u32 = 2;
const GLYPH_ATLAS_BYTES_PER_PIXEL: usize = 4;
const DIRTY_RECT_MAX_HORIZONTAL_GAP: u32 = GLYPH_ATLAS_PADDING * 4;
const DIRTY_RECT_MERGE_WASTE_NUMERATOR: u64 = 3;
const DIRTY_RECT_MERGE_WASTE_DENOMINATOR: u64 = 2;

#[derive(Clone, PartialEq)]
struct CacheKey {
  text: String,
  font_family: std::sync::Arc<str>,
  font_size_bits: u32,
  line_height_bits: u32,
  letter_spacing_bits: u32,
  font_features: FontFeatures,
  max_width_bits: u32,
  weight: u16,
  style: u8,
  text_align: u8,
  wrap: bool,
  trim_line_box: bool,
  raster_mode: u8,
}

impl Eq for CacheKey {}

impl Hash for CacheKey {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.text.hash(state);
    self.font_family.hash(state);
    self.font_size_bits.hash(state);
    self.line_height_bits.hash(state);
    self.letter_spacing_bits.hash(state);
    self.font_features.hash(state);
    self.max_width_bits.hash(state);
    self.weight.hash(state);
    self.style.hash(state);
    self.text_align.hash(state);
    self.wrap.hash(state);
    self.trim_line_box.hash(state);
    self.raster_mode.hash(state);
  }
}

impl CacheKey {
  fn new(text: &str, style: &TextStyle, max_width: f32, wrap: bool) -> Self {
    Self {
      text: text.to_owned(),
      font_family: style.font_family.clone(),
      font_size_bits: style.font_size.to_bits(),
      line_height_bits: style.line_height.to_bits(),
      letter_spacing_bits: letter_spacing_bits(style.letter_spacing),
      font_features: style.font_features.clone(),
      max_width_bits: max_width.to_bits(),
      weight: style.weight.value(),
      style: style_to_u8(style.style),
      text_align: text_align_to_u8(style.text_align),
      wrap,
      trim_line_box: style.trim_line_box,
      raster_mode: 0,
    }
  }

  fn new_for_raster(text: &str, style: &TextStyle, max_width: f32, wrap: bool, snap_to_pixel: bool) -> Self {
    let mut key = Self::new(text, style, max_width, wrap);
    key.raster_mode = if snap_to_pixel { 0 } else { 1 };
    key
  }

  fn new_for_baked_transform(text: &str, style: &TextStyle, max_width: f32, wrap: bool) -> Self {
    let mut key = Self::new(text, style, max_width, wrap);
    key.raster_mode = 2;
    key
  }

  /// The key of the same text in the same face, laid out as `style`,
  /// `max_width` and `wrap` lay it out.
  fn lay_out_as(&mut self, style: &TextStyle, max_width: f32, wrap: bool) {
    self.font_size_bits = style.font_size.to_bits();
    self.line_height_bits = style.line_height.to_bits();
    self.letter_spacing_bits = letter_spacing_bits(style.letter_spacing);
    self.max_width_bits = max_width.to_bits();
    self.text_align = text_align_to_u8(style.text_align);
    self.wrap = wrap;
    self.trim_line_box = style.trim_line_box;
  }

  fn matches_measure(&self, text: &str, style: &TextStyle, max_width: f32, wrap: bool) -> bool {
    self.raster_mode == 0
      && self.text == text
      && self.font_family == style.font_family
      && self.font_size_bits == style.font_size.to_bits()
      && self.line_height_bits == style.line_height.to_bits()
      && self.letter_spacing_bits == letter_spacing_bits(style.letter_spacing)
      && self.font_features == style.font_features
      && self.max_width_bits == max_width.to_bits()
      && self.weight == style.weight.value()
      && self.style == style_to_u8(style.style)
      && self.text_align == text_align_to_u8(style.text_align)
      && self.wrap == wrap
      && self.trim_line_box == style.trim_line_box
  }
}

/// Bytes in each of the three samples a long text is hashed by.
const TEXT_SAMPLE_BYTES: usize = 128;

/// Whether [`hash_sampled_text`] reads only samples of `text`. Two such texts
/// of one length that differ between the samples hash alike.
fn text_is_sampled(text: &str) -> bool {
  text.len() > TEXT_SAMPLE_BYTES * 3
}

/// Hashes a long text by three samples of it and its length: hashing a whole
/// document on every lookup would cost more than the lookup saves.
fn hash_sampled_text<H: Hasher>(text: &str, hasher: &mut H) {
  const SAMPLE: usize = TEXT_SAMPLE_BYTES;
  text.len().hash(hasher);
  if !text_is_sampled(text) {
    text.hash(hasher);
    return;
  }

  hasher.write(&text.as_bytes()[..SAMPLE]);
  let middle = text.len() / 2;
  let middle_start = text_floor_char_boundary(text, middle.saturating_sub(SAMPLE / 2));
  let middle_end = text_floor_char_boundary(text, (middle_start + SAMPLE).min(text.len()));
  hasher.write(&text.as_bytes()[middle_start..middle_end]);
  let tail_start = text_floor_char_boundary(text, text.len() - SAMPLE);
  hasher.write(&text.as_bytes()[tail_start..]);
}

fn text_floor_char_boundary(text: &str, mut index: usize) -> usize {
  while index > 0 && !text.is_char_boundary(index) {
    index -= 1;
  }
  index
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ClippedCacheKey {
  base: CacheKey,
  clip_x_bits: u32,
  clip_y_bits: u32,
  clip_width_bits: u32,
  clip_height_bits: u32,
}

impl ClippedCacheKey {
  fn new(base: CacheKey, origin_x: f32, origin_y: f32, clip: ClipRect) -> Option<Self> {
    clip.active.then_some(Self {
      base,
      clip_x_bits: (clip.x - origin_x).to_bits(),
      clip_y_bits: (clip.y - origin_y).to_bits(),
      clip_width_bits: clip.width.to_bits(),
      clip_height_bits: clip.height.to_bits(),
    })
  }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct RichTextCacheKey {
  spans: Vec<RichTextSpanCacheKey>,
  max_width_bits: u32,
  wrap: bool,
  raster_mode: u8,
}

impl RichTextCacheKey {
  fn new_for_raster(spans: &[RichTextSpan], max_width: f32, wrap: bool, snap_to_pixel: bool) -> Self {
    Self {
      spans: spans.iter().map(RichTextSpanCacheKey::new).collect(),
      max_width_bits: max_width.to_bits(),
      wrap,
      raster_mode: if snap_to_pixel { 0 } else { 1 },
    }
  }

  fn matches(&self, spans: &[RichTextSpan], max_width: f32, wrap: bool, snap_to_pixel: bool) -> bool {
    self.max_width_bits == max_width.to_bits()
      && self.wrap == wrap
      && self.raster_mode == if snap_to_pixel { 0 } else { 1 }
      && spans_match(&self.spans, spans, 1.0)
  }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct RichTextShapeKey {
  spans: Vec<RichTextSpanCacheKey>,
  max_width_bits: u32,
  wrap: bool,
}

impl RichTextShapeKey {
  fn new(spans: &[RichTextSpan], max_width: f32, wrap: bool) -> Self {
    Self {
      spans: spans.iter().map(RichTextSpanCacheKey::new).collect(),
      max_width_bits: max_width.to_bits(),
      wrap,
    }
  }

  /// Whether this key is `spans` with their pixel sizes scaled by `scale`
  /// (see [`spans_match`]).
  fn matches(&self, spans: &[RichTextSpan], scale: f32, max_width: f32, wrap: bool) -> bool {
    self.max_width_bits == max_width.to_bits() && self.wrap == wrap && spans_match(&self.spans, spans, scale)
  }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct RichTextSpanCacheKey {
  text: String,
  font_family: std::sync::Arc<str>,
  font_size_bits: u32,
  line_height_bits: u32,
  letter_spacing_bits: u32,
  font_features: FontFeatures,
  weight: u16,
  style: u8,
  text_align: u8,
  color: [u8; 4],
}

impl RichTextSpanCacheKey {
  fn new(span: &RichTextSpan) -> Self {
    let style = &span.style;
    Self {
      text: span.text.clone(),
      font_family: style.font_family.clone(),
      font_size_bits: style.font_size.to_bits(),
      line_height_bits: style.line_height.to_bits(),
      letter_spacing_bits: letter_spacing_bits(style.letter_spacing),
      font_features: style.font_features.clone(),
      weight: style.weight.value(),
      style: style_to_u8(style.style),
      text_align: text_align_to_u8(style.text_align),
      color: [style.color.r(), style.color.g(), style.color.b(), style.color.a()],
    }
  }

  fn matches(&self, span: &RichTextSpan, scale: f32) -> bool {
    let style = &span.style;
    self.text == span.text
      && self.font_family == style.font_family
      && self.font_size_bits == (style.font_size * scale).to_bits()
      && self.line_height_bits == style.line_height.to_bits()
      && self.letter_spacing_bits == letter_spacing_bits(style.letter_spacing * scale)
      && self.font_features == style.font_features
      && self.weight == style.weight.value()
      && self.style == style_to_u8(style.style)
      && self.text_align == text_align_to_u8(style.text_align)
      && self.color == [style.color.r(), style.color.g(), style.color.b(), style.color.a()]
  }
}

/// Whether `cached` keys `spans` with their pixel sizes scaled by `scale`:
/// the key of the spans `TextStyle::scale_pixels(scale)` makes of them,
/// without making them. Layout looks a painted layout up with the logical
/// spans and the paint's scale; the paint, with the scaled spans and 1.
fn spans_match(cached: &[RichTextSpanCacheKey], spans: &[RichTextSpan], scale: f32) -> bool {
  cached.len() == spans.len()
    && cached
      .iter()
      .zip(spans)
      .all(|(cached, span)| cached.matches(span, scale))
}

/// The spans with their pixel sizes scaled by `scale`; the spans themselves
/// at 1, so a lookup at the paint's own scale copies nothing.
fn scaled_spans(spans: &[RichTextSpan], scale: f32) -> Cow<'_, [RichTextSpan]> {
  if scale == 1.0 {
    return Cow::Borrowed(spans);
  }
  let mut scaled = spans.to_vec();
  for span in &mut scaled {
    span.style.scale_pixels(scale);
  }
  Cow::Owned(scaled)
}

fn rich_text_shape_fingerprint(spans: &[RichTextSpan], scale: f32, max_width: f32, wrap: bool) -> u64 {
  let mut hasher = DefaultHasher::new();
  max_width.to_bits().hash(&mut hasher);
  wrap.hash(&mut hasher);
  hash_rich_text_spans(spans, scale, &mut hasher);
  hasher.finish()
}

fn rich_text_raster_fingerprint(spans: &[RichTextSpan], max_width: f32, wrap: bool, snap_to_pixel: bool) -> u64 {
  let mut hasher = DefaultHasher::new();
  max_width.to_bits().hash(&mut hasher);
  wrap.hash(&mut hasher);
  (if snap_to_pixel { 0u8 } else { 1u8 }).hash(&mut hasher);
  hash_rich_text_spans(spans, 1.0, &mut hasher);
  hasher.finish()
}

/// Hashes `spans` with their pixel sizes scaled by `scale`, as
/// [`spans_match`] compares them.
fn hash_rich_text_spans(spans: &[RichTextSpan], scale: f32, hasher: &mut DefaultHasher) {
  spans.len().hash(hasher);
  for span in spans {
    let style = &span.style;
    span.text.hash(hasher);
    style.font_family.hash(hasher);
    (style.font_size * scale).to_bits().hash(hasher);
    style.line_height.to_bits().hash(hasher);
    letter_spacing_bits(style.letter_spacing * scale).hash(hasher);
    style.font_features.hash(hasher);
    style.weight.value().hash(hasher);
    style_to_u8(style.style).hash(hasher);
    text_align_to_u8(style.text_align).hash(hasher);
    [style.color.r(), style.color.g(), style.color.b(), style.color.a()].hash(hasher);
  }
}

fn style_to_u8(s: FontStyle) -> u8 {
  match s {
    FontStyle::Normal => 0,
    FontStyle::Italic => 1,
  }
}

fn text_align_to_u8(align: TextAlign) -> u8 {
  match align {
    TextAlign::Left => 0,
    TextAlign::Center => 1,
    TextAlign::Right => 2,
    TextAlign::Justified => 3,
    TextAlign::End => 4,
  }
}

/// Buffer setters only mark the buffer dirty; readers shape it with `shape_until_scroll`.
fn set_buffer_text(buffer: &mut Buffer, text: &str, attrs: Attrs, text_align: TextAlign) {
  buffer.set_text(text, &attrs, Shaping::Advanced, Some(text_align.to_cosmic()));
  // Cosmic appends an empty line after a trailing line break. Paragraph caches,
  // rich text and caret extraction all end at the last line break instead.
  if buffer.lines.len() > 1
    && buffer
      .lines
      .last()
      .is_some_and(|line| line.text().is_empty() && line.ending() == cosmic_text::LineEnding::None)
  {
    buffer.lines.pop();
  }
}

/// Vertical extents of shaped text, relative to the render origin
/// (a glyph placed at origin_y `Y` puts these at `Y + value`). `ink_*` is the
/// tight bound of the visible glyph ink; `optical_*` is the descender-agnostic
/// box used for centering — from the first line's cap height to the final
/// line's baseline for real text, falling back to the ink bounds when the font
/// exposes no usable cap height (e.g. icon fonts). For one line this is the
/// usual `[baseline - cap_height, baseline]` box. Centering the optical box
/// keeps text with and without descenders — and icons next to it — visually
/// aligned, instead of letting descenders drag the visible mass upward.
#[derive(Clone, Copy)]
pub(crate) struct TextVerticalExtents {
  pub(crate) ink_top: f32,
  pub(crate) ink_bottom: f32,
  pub(crate) optical_top: f32,
  pub(crate) optical_bottom: f32,
}

struct CachedPlainBuffer {
  key: CacheKey,
  buffer: Buffer,
  paragraphs: Vec<PlainParagraph>,
  bytes: usize,
  layout_ready: bool,
}

#[derive(Clone, Default)]
struct PlainParagraph {
  carets: Option<std::sync::Arc<ParagraphCarets>>,
  reflow: Option<ReflowRange>,
  fingerprint: u64,
  shape_bytes: usize,
  layout_bytes: usize,
}

impl CachedPlainBuffer {
  fn refresh_bytes(&mut self) {
    self.bytes = std::mem::size_of::<Self>()
      + self.key.text.len()
      + self.buffer.lines.capacity() * std::mem::size_of::<cosmic_text::BufferLine>()
      + self.paragraphs.capacity() * std::mem::size_of::<PlainParagraph>()
      + self
        .paragraphs
        .iter()
        .map(|line| line.shape_bytes + line.layout_bytes + line.carets.as_ref().map_or(0, |carets| carets.bytes()))
        .sum::<usize>();
  }
}

// Callers can inspect runs, but mutations must also update the retained allocation charge.
impl std::ops::Deref for CachedPlainBuffer {
  type Target = Buffer;

  fn deref(&self) -> &Buffer {
    &self.buffer
  }
}

pub(crate) struct GlyphEngine {
  input_cache: input_tokens::InputCache,
  #[cfg(feature = "canvas")]
  canvas_text: Option<std::sync::Arc<parking_lot::Mutex<crate::canvas::CanvasTextEngine>>>,
  /// A GPU renderer ended frames of a canvas text engine that a cleared cache
  /// dropped, so its replacements are created framed.
  #[cfg(feature = "canvas")]
  canvas_text_framed: bool,
  font_system: FontSystem,
  swash_context: ScaleContext,
  transformed_scale_context: ScaleContext,
  hinted_fonts: hinted::HintedFonts,
  font_aliases: HashMap<String, String>,
  face_weights: FaceWeights,
  bold_text: bool,
  /// What is known of each plain text laid out one way: its size, extents,
  /// carets and glyphs.
  texts: TextCache,
  rich_shaped_layout_cache: HashMap<u64, Vec<(RichTextShapeKey, CachedRichShapedLayout)>>,
  underline_metrics: span_boxes::UnderlineMetrics,
  clipped_glyph_layout_cache: HashMap<ClippedCacheKey, Vec<CachedGlyph>>,
  rich_glyph_layout_cache: HashMap<u64, Vec<(RichTextCacheKey, Vec<CachedRichGlyph>)>>,
  transformed_glyph_layout_cache: HashMap<CacheKey, Vec<CachedTransformedGlyph>>,
  atlas_packer: AtlasPacker,
  /// Where each rasterised glyph is in the atlas; asked for every glyph of
  /// every text laid out to be painted.
  atlas_entries: HashMap<GlyphCacheKey, PackedGlyph, SeededFingerprints>,
  transformed_atlas_entries: HashMap<TransformedGlyphKey, PackedGlyph, SeededFingerprints>,
  buffer_pool: Vec<Buffer>,
  // Full-height layouts of documents (`SHORT_TEXT_BYTES` and longer) shared by
  // measurement, ink bounds, and clipped painting.
  // Oldest entries are evicted first; taking and returning an entry refreshes its age.
  plain_buffers: Vec<CachedPlainBuffer>,
  /// The same for shorter texts, found by what shapes them.
  shaped_texts: ShapedTexts,
  /// Passes and newly laid out texts since the shaped words were last looked
  /// through (`trim_shaped_words`).
  shaped_words_age: usize,
  plain_buffer_bytes: usize,
  plain_buffer_budget: usize,
  pub(crate) measure_hits: usize,
  pub(crate) measure_misses: usize,
  pub(crate) glyph_hits: usize,
  pub(crate) glyph_misses: usize,
  #[cfg(feature = "perf_profile")]
  profile: GlyphEngineProfile,
}

impl GlyphEngine {
  pub(crate) fn new() -> Self {
    let mut font_system = FontSystem::new();
    load_platform_fonts(&mut font_system);
    generic_families::set_generic_families(font_system.db_mut());
    // The same fonts, with the fallback lists cosmic-text has no iOS for.
    #[cfg(target_os = "ios")]
    let font_system = {
      let (locale, db) = font_system.into_locale_and_db();
      FontSystem::new_with_locale_and_db_and_fallback(locale, db, ios_fallback::IosFallback)
    };

    Self {
      font_system,
      #[cfg(feature = "canvas")]
      canvas_text: None,
      #[cfg(feature = "canvas")]
      canvas_text_framed: false,
      swash_context: ScaleContext::new(),
      transformed_scale_context: ScaleContext::new(),
      hinted_fonts: hinted::HintedFonts::default(),
      font_aliases: HashMap::new(),
      input_cache: input_tokens::InputCache::default(),
      face_weights: FaceWeights::default(),
      bold_text: false,
      texts: TextCache::new(GLYPH_LAYOUT_CACHE_LIMIT),
      rich_shaped_layout_cache: HashMap::new(),
      underline_metrics: span_boxes::UnderlineMetrics::default(),
      clipped_glyph_layout_cache: HashMap::new(),
      rich_glyph_layout_cache: HashMap::new(),
      transformed_glyph_layout_cache: HashMap::new(),
      atlas_packer: AtlasPacker::new(),
      atlas_entries: HashMap::with_hasher(SeededFingerprints::new()),
      transformed_atlas_entries: HashMap::with_hasher(SeededFingerprints::new()),
      buffer_pool: Vec::new(),
      plain_buffers: Vec::new(),
      shaped_texts: ShapedTexts::new(SHAPED_TEXT_CACHE_BYTES),
      shaped_words_age: 0,
      plain_buffer_bytes: 0,
      plain_buffer_budget: plain_buffer_cache_budget(),
      measure_hits: 0,
      measure_misses: 0,
      glyph_hits: 0,
      glyph_misses: 0,
      #[cfg(feature = "perf_profile")]
      profile: GlyphEngineProfile::default(),
    }
  }

  /// Does ahead of time what the first text drawn with `style` makes
  /// cosmic-text do: score every face of the font database for the style's
  /// family, weight and slant (once per combination, and milliseconds with a
  /// desktop's fonts), and load the face that wins. Size, colour and spacing
  /// do not matter.
  pub(crate) fn warm_style(&mut self, style: &TextStyle) {
    self.measure_text("Ag", style, f32::MAX);
  }

  pub(crate) fn load_font(&mut self, data: Vec<u8>) {
    self.font_system.db_mut().load_font_data(data);
    self.clear_text_caches();
  }

  /// Loads a font whose glyphs are drawn grid-fitted: at whole pixel sizes and
  /// positions, with the font's hints applied. Meant for icon fonts; see `hinted`.
  pub(crate) fn load_hinted_font(&mut self, data: Vec<u8>) {
    let source = cosmic_text::fontdb::Source::Binary(std::sync::Arc::new(data));
    let faces = self.font_system.db_mut().load_font_source(source);
    self.hinted_fonts.add(faces);
    self.clear_text_caches();
  }

  pub(crate) fn load_font_file(&mut self, path: &Path) {
    self.font_system.db_mut().load_font_file(path).ok();
    self.clear_text_caches();
  }

  pub(crate) fn load_fonts_dir(&mut self, path: &Path) {
    self.font_system.db_mut().load_fonts_dir(path);
    self.clear_text_caches();
  }

  pub(crate) fn register_font(&mut self, name: &str, family: &str) {
    let name = alias_name(name);
    if self.font_aliases.get(name).is_some_and(|current| current == family) {
      return;
    }
    self.font_aliases.insert(name.to_owned(), family.to_owned());
    self.clear_text_caches();
  }

  pub(crate) fn install_fonts<I, A, N, F>(&mut self, fonts: I, aliases: A)
  where
    I: IntoIterator<Item = Vec<u8>>,
    A: IntoIterator<Item = (N, F)>,
    N: AsRef<str>,
    F: AsRef<str>,
  {
    let mut changed = false;
    let mut fonts = fonts.into_iter().peekable();
    if fonts.peek().is_some() {
      let database = self.font_system.db_mut();
      for data in fonts {
        database.load_font_data(data);
        changed = true;
      }
    }
    for (name, family) in aliases {
      let name = alias_name(name.as_ref());
      let family = family.as_ref();
      if self.font_aliases.get(name).is_some_and(|current| current == family) {
        continue;
      }
      self.font_aliases.insert(name.to_owned(), family.to_owned());
      changed = true;
    }
    if changed {
      self.clear_text_caches();
    }
  }

  pub(crate) fn clear_cache(&mut self) {
    self.clear_text_caches();
  }

  #[cfg(feature = "canvas")]
  pub(crate) fn canvas_text_engine(&mut self) -> std::sync::Arc<parking_lot::Mutex<crate::canvas::CanvasTextEngine>> {
    self
      .canvas_text
      .get_or_insert_with(|| {
        let fonts = platform_font_system(self.font_system.locale().to_owned(), self.font_system.db().clone());
        let aliases = self.font_aliases.clone();
        let engine = if self.canvas_text_framed {
          crate::canvas::CanvasTextEngine::framed(fonts, aliases)
        } else {
          crate::canvas::CanvasTextEngine::new(fonts, aliases)
        };
        std::sync::Arc::new(parking_lot::Mutex::new(engine))
      })
      .clone()
  }

  /// The shared Canvas text engine, if one was made, without making one.
  #[cfg(all(feature = "canvas", feature = "perf_profile"))]
  pub(crate) fn existing_canvas_text_engine(
    &self,
  ) -> Option<std::sync::Arc<parking_lot::Mutex<crate::canvas::CanvasTextEngine>>> {
    self.canvas_text.clone()
  }

  fn clear_text_caches(&mut self) {
    // The next layout binds a replacement to the same canvases. If a GPU
    // renderer ended frames of this engine, it ends the replacement's too.
    // Canvas drawing holds an engine only to shape and never takes this one.
    #[cfg(feature = "canvas")]
    if let Some(engine) = self.canvas_text.take() {
      self.canvas_text_framed |= engine.lock().is_framed();
    }
    self.face_weights.clear();
    self.input_cache.clear();
    self.plain_buffers.clear();
    self.shaped_texts.clear();
    // Words shaped with the fonts there were: another font may now cover them.
    self.font_system.shape_run_cache = cosmic_text::ShapeRunCache::default();
    self.shaped_words_age = 0;
    self.plain_buffer_bytes = 0;
    self.texts.clear();
    self.rich_shaped_layout_cache.clear();
    self.underline_metrics = span_boxes::UnderlineMetrics::default();
    self.clipped_glyph_layout_cache.clear();
    self.rich_glyph_layout_cache.clear();
    self.transformed_glyph_layout_cache.clear();
  }

  /// A pass of a window begins: its counters start from zero, and what the
  /// text caches are asked for from here on counts as in use.
  pub(crate) fn begin_pass(&mut self) {
    self.trim_shaped_words();
    self.reset_stats();
    self.texts.begin_pass();
  }

  /// cosmic-text keeps the glyphs of the words it shaped, so a word of one
  /// text is not shaped again in the next. It drops none on its own: every so
  /// many passes or newly laid out texts, the words not used since a few
  /// looks ago go. Typing shapes a new word a keystroke and scrolling a few
  /// a row; both stay bounded.
  fn trim_shaped_words(&mut self) {
    self.shaped_words_age += 1 + self.measure_misses;
    if self.shaped_words_age >= SHAPED_WORDS_TRIM_INTERVAL {
      self.shaped_words_age = 0;
      self.font_system.shape_run_cache.trim(SHAPED_WORDS_KEPT_TRIMS);
    }
  }

  pub(crate) fn reset_stats(&mut self) {
    self.measure_hits = 0;
    self.measure_misses = 0;
    self.glyph_hits = 0;
    self.glyph_misses = 0;
    #[cfg(feature = "perf_profile")]
    {
      self.profile = GlyphEngineProfile::default();
    }
  }

  #[cfg_attr(not(feature = "perf_profile"), allow(dead_code))]
  pub(crate) fn profile(&self) -> GlyphEngineProfile {
    #[cfg(feature = "perf_profile")]
    {
      GlyphEngineProfile {
        plain_cache_bytes: self.plain_buffer_bytes + self.shaped_texts.bytes(),
        plain_cache_budget: self.plain_buffer_budget + SHAPED_TEXT_CACHE_BYTES,
        ..self.profile
      }
    }
    #[cfg(not(feature = "perf_profile"))]
    {
      GlyphEngineProfile::default()
    }
  }

  pub(crate) fn measure_text(&mut self, text: &str, style: &TextStyle, max_width: f32) -> Size {
    let wrap = is_bounded_text_width(max_width);
    let fingerprint = self.texts.fingerprint(text, style, max_width, wrap);
    if let Some(cached) = self
      .texts
      .get(fingerprint, text, style, max_width, wrap)
      .and_then(|entry| entry.size)
    {
      self.measure_hits += 1;
      return cached;
    }
    self.measure_misses += 1;
    let size = self.shape_and_measure(text, style, max_width, wrap);
    self.texts.entry(fingerprint, text, style, max_width, wrap).size = Some(size);
    size
  }

  /// Vertical extents of `text` relative to the render origin used by
  /// `rasterize_text_*`. Whitespace-only or empty text yields `None`. See
  /// [`TextVerticalExtents`] for how the ink vs. optical (cap-height) boxes are
  /// used by the different vertical-align modes.
  pub(crate) fn text_vertical_extents(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
  ) -> Option<TextVerticalExtents> {
    let fingerprint = self.texts.fingerprint(text, style, max_width, wrap);
    if let Some(cached) = self
      .texts
      .get(fingerprint, text, style, max_width, wrap)
      .and_then(|entry| entry.vertical_extents)
    {
      self.measure_hits += 1;
      return cached;
    }
    self.measure_misses += 1;
    #[cfg(feature = "perf_profile")]
    let extents_start = Instant::now();
    let extents = self.compute_text_vertical_extents(text, style, max_width, wrap);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.vertical_extents += extents_start.elapsed();
    }
    self
      .texts
      .entry(fingerprint, text, style, max_width, wrap)
      .vertical_extents = Some(extents);
    extents
  }

  /// Center alignment needs the first cap height and last nonempty baseline.
  /// Top/bottom alignment and fonts without usable cap metrics still use exact ink bounds.
  pub(crate) fn text_optical_extents(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
  ) -> Option<(f32, f32)> {
    let fingerprint = self.texts.fingerprint(text, style, max_width, wrap);
    if let Some(extents) = self
      .texts
      .get(fingerprint, text, style, max_width, wrap)
      .and_then(|entry| entry.optical_extents)
    {
      self.measure_hits += 1;
      return extents;
    }
    self.measure_misses += 1;
    #[cfg(feature = "perf_profile")]
    let extents_start = Instant::now();
    let extents = self.compute_text_optical_extents(text, style, max_width, wrap);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.vertical_extents += extents_start.elapsed();
    }
    self
      .texts
      .entry(fingerprint, text, style, max_width, wrap)
      .optical_extents = Some(extents);
    extents
  }

  fn compute_text_optical_extents(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
  ) -> Option<(f32, f32)> {
    #[cfg(feature = "perf_profile")]
    let shape_start = Instant::now();
    let buffer = self.full_text_buffer(text, style, max_width, wrap);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.vertical_extents_shape += shape_start.elapsed();
    }
    let mut first = None;
    let mut last_line_y = 0.0;
    let mut needs_ink = false;
    'runs: for run in buffer.layout_runs() {
      #[cfg(feature = "perf_profile")]
      {
        self.profile.vertical_extents_runs += 1;
      }
      for glyph in run.glyphs {
        #[cfg(feature = "perf_profile")]
        {
          self.profile.vertical_extents_glyphs += 1;
        }
        if glyph_cluster_is_whitespace(run.text, glyph) {
          continue;
        }
        let (key, ..) = GlyphCacheKey::new(
          glyph.font_id,
          glyph.glyph_id,
          glyph.font_size,
          (0.0, 0.0),
          glyph.font_weight,
          glyph.cache_key_flags,
        );
        if self.get_or_pack_glyph(key).is_none_or(|packed| packed.height == 0) {
          continue;
        }
        if first.is_none() {
          let cap = self
            .font_cap_height_px(GlyphFace::of(glyph))
            .filter(|&cap| cap > glyph.font_size * 0.4 && cap < glyph.font_size * 0.95);
          let Some(cap) = cap else {
            needs_ink = true;
            break 'runs;
          };
          let baseline = run.line_y + glyph.y - glyph.font_size * glyph.y_offset;
          first = Some((baseline, cap, run.line_y));
        }
        last_line_y = run.line_y;
        break;
      }
    }
    self.retain_plain_buffer(buffer);
    if needs_ink {
      return self
        .compute_text_vertical_extents(text, style, max_width, wrap)
        .map(|extents| (extents.optical_top, extents.optical_bottom));
    }
    first.map(|(baseline, cap, first_line_y)| (baseline - cap, baseline + last_line_y - first_line_y))
  }

  fn compute_text_vertical_extents(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
  ) -> Option<TextVerticalExtents> {
    #[cfg(feature = "perf_profile")]
    let shape_start = Instant::now();
    let buffer = self.full_text_buffer(text, style, max_width, wrap);

    #[cfg(feature = "perf_profile")]
    {
      self.profile.vertical_extents_shape += shape_start.elapsed();
    }

    let mut top = f32::INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    let mut first: Option<(GlyphFace, f32, f32)> = None;
    let mut last_line_y = None;
    for run in buffer.layout_runs() {
      #[cfg(feature = "perf_profile")]
      {
        self.profile.vertical_extents_runs += 1;
        self.profile.vertical_extents_glyphs += run.glyphs.len();
      }
      for glyph in run.glyphs.iter() {
        if glyph_cluster_is_whitespace(run.text, glyph) {
          continue;
        }
        let y_offset = glyph.font_size * glyph.y_offset;
        let (cache_key, ..) = GlyphCacheKey::new(
          glyph.font_id,
          glyph.glyph_id,
          glyph.font_size,
          (0.0, 0.0),
          glyph.font_weight,
          glyph.cache_key_flags,
        );
        let Some(packed) = self.get_or_pack_glyph(cache_key) else {
          continue;
        };
        if packed.height == 0 {
          continue;
        }
        let baseline = run.line_y + glyph.y - y_offset;
        let glyph_top = baseline - packed.top as f32;
        top = top.min(glyph_top);
        bottom = bottom.max(glyph_top + packed.height as f32);
        if first.is_none() {
          first = Some((GlyphFace::of(glyph), baseline, run.line_y));
        }
        last_line_y = Some(run.line_y);
      }
    }

    self.retain_plain_buffer(buffer);
    if bottom < top {
      return None;
    }

    // Optical box = the font's cap height above the first line's baseline
    // through the final line's baseline. Including every line advance is
    // essential: centering only the first line's cap box shifts a wrapped run
    // downward and lets its final line escape a trimmed line box. Icon fonts
    // usually report no usable cap height, so fall back to the ink box for them.
    let (optical_top, optical_bottom) = first
      .and_then(|(glyph, baseline, first_line_y)| {
        let font_size = glyph.font_size;
        let cap_px = self.font_cap_height_px(glyph)?;
        // Only trust a plausible text cap height. Icon/symbol fonts report 0 or
        // a full-em value; those fall back to ink so the glyph shape itself is
        // centered (which keeps icons aligned with adjacent cap-centered text).
        let final_baseline = baseline + last_line_y.unwrap_or(first_line_y) - first_line_y;
        (cap_px > font_size * 0.4 && cap_px < font_size * 0.95).then_some((baseline - cap_px, final_baseline))
      })
      .unwrap_or((top, bottom));

    Some(TextVerticalExtents {
      ink_top: top,
      ink_bottom: bottom,
      optical_top,
      optical_bottom,
    })
  }

  /// Where the laid-out lines of a plain text lie: one box per line that
  /// holds words, laid out as measuring and painting lay it out, from the
  /// plain text cache they share.
  pub(crate) fn text_line_boxes(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
  ) -> std::sync::Arc<[SpanBox]> {
    let buffer = self.full_text_buffer(text, style, max_width, wrap);
    let boxes = span_boxes::line_boxes(&buffer, &mut self.font_system, &mut self.underline_metrics);
    self.retain_plain_buffer(buffer);
    boxes
  }

  /// Where the glyphs of a line in `style` sit in its line box by the font's
  /// metrics, as `(top, height)`: its ascent plus descent, centered in the
  /// line height as cosmic-text places them. Unlike ink bounds it does not
  /// depend on which glyphs the line holds.
  pub(crate) fn line_content_band(&mut self, style: &TextStyle) -> Option<(f32, f32)> {
    let buffer = self.full_text_buffer("H", style, f32::MAX, false);
    let band = buffer.layout_runs().next().and_then(|run| {
      let line = buffer.lines.get(run.line_i)?.layout_opt()?.first()?;
      let height = line.max_ascent + line.max_descent;
      (height > 0.0).then_some(((run.line_height - height) * 0.5, height))
    });
    self.retain_plain_buffer(buffer);
    band
  }

  fn font_cap_height_px(&mut self, glyph: GlyphFace) -> Option<f32> {
    let font = self.font_system.get_font(glyph.font_id, glyph.weight)?;
    cap_height::cap_height_px(self.font_system.db(), &font, glyph.weight, glyph.font_size)
  }

  /// The size of `spans` with their pixel sizes scaled by `scale`, laid out
  /// at `max_width`.
  #[cfg_attr(not(feature = "markdown"), allow(dead_code))]
  pub(crate) fn measure_rich_text(&mut self, spans: &[RichTextSpan], scale: f32, max_width: f32) -> Size {
    if spans.is_empty() {
      return Size::default();
    }
    if let [span] = spans {
      let mut style = span.style.clone();
      style.scale_pixels(scale);
      return self.measure_text(&span.text, &style, max_width);
    }
    let wrap = is_bounded_text_width(max_width);
    let fingerprint = rich_text_shape_fingerprint(spans, scale, max_width, wrap);
    if let Some(cached) = self.find_rich_shaped_layout(fingerprint, spans, scale, max_width, wrap) {
      self.measure_hits += 1;
      return cached.size;
    }
    self.measure_misses += 1;
    self.rich_shaped_layout(spans, scale, max_width, wrap, false).size
  }

  /// The caret positions of `spans` with their pixel sizes scaled by
  /// `scale`, laid out at `max_width` (wrapping when `wrap` is set and the
  /// width is bounded, as painting does): one before and one after every
  /// glyph painting draws, in the layout's pixels, kept with the shaped
  /// layout that measuring and painting share, so a selection indexes the
  /// glyphs on screen in every span's own font. Built in the shaping that
  /// makes the layout; a layout shaped without them is shaped once more the
  /// first time they are asked for.
  #[cfg(feature = "markdown")]
  pub(crate) fn rich_text_caret_positions(
    &mut self,
    spans: &[RichTextSpan],
    scale: f32,
    max_width: f32,
    wrap: bool,
  ) -> CaretPositions {
    match spans {
      [] => carets::buffer_caret_positions(&Buffer::new_empty(Metrics::new(1.0, 1.0)), 0),
      [span] => {
        let mut style = span.style.clone();
        style.scale_pixels(scale);
        self.caret_positions(&span.text, &style, max_width, wrap)
      }
      _ => {
        let layout = self.rich_shaped_layout(spans, scale, max_width, wrap, true);
        layout
          .carets
          .get()
          .cloned()
          .unwrap_or_else(|| carets::buffer_caret_positions(&Buffer::new_empty(Metrics::new(1.0, 1.0)), 0))
      }
    }
  }

  /// The shaped layout of `spans` with their pixel sizes scaled by `scale`,
  /// from the cache that measuring and painting share, shaped and cached
  /// when it is not there yet; with its carets when `with_carets` asks for
  /// them.
  fn rich_shaped_layout(
    &mut self,
    spans: &[RichTextSpan],
    scale: f32,
    max_width: f32,
    wrap: bool,
    with_carets: bool,
  ) -> CachedRichShapedLayout {
    let wrap = effective_text_wrap(max_width, wrap);
    let fingerprint = rich_text_shape_fingerprint(spans, scale, max_width, wrap);
    if let Some(cached) = self.find_rich_shaped_layout(fingerprint, spans, scale, max_width, wrap) {
      if with_carets && cached.carets.get().is_none() {
        let shaped = self.shape_rich_text_layout(&scaled_spans(spans, scale), max_width, wrap, true);
        if let Some(carets) = shaped.carets.get() {
          // The cache entry shares the cell: it has the carets from now on.
          cached.carets.get_or_init(|| carets.clone());
        }
      }
      return cached;
    }
    let scaled = scaled_spans(spans, scale);
    let layout = self.shape_rich_text_layout(&scaled, max_width, wrap, with_carets);
    if self.rich_shaped_layout_cache.len() >= GLYPH_LAYOUT_CACHE_LIMIT {
      self.rich_shaped_layout_cache.clear();
    }
    self
      .rich_shaped_layout_cache
      .entry(fingerprint)
      .or_default()
      .push((RichTextShapeKey::new(&scaled, max_width, wrap), layout.clone()));
    layout
  }

  /// The size of `spans` laid out at `max_width` (wrapping when `wrap` is
  /// set and the width is bounded, as painting does) and where each span lies
  /// in it: one box per span and laid-out line. Shares the shaped layout
  /// cache with measuring and painting.
  #[cfg(feature = "markdown")]
  pub(crate) fn rich_text_span_boxes(
    &mut self,
    spans: &[RichTextSpan],
    max_width: f32,
    wrap: bool,
  ) -> (Size, std::sync::Arc<[SpanBox]>) {
    if spans.is_empty() {
      return (Size::default(), Vec::<SpanBox>::new().into());
    }
    let layout = self.rich_shaped_layout(spans, 1.0, max_width, wrap, false);
    (layout.size, layout.span_boxes)
  }

  pub(crate) fn caret_positions(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
  ) -> CaretPositions {
    // Selectable text recomputes caret positions on every layout; cache them
    // like measurements — a full per-character shaping walk per node per pass
    // makes lists of selectable text crawl.
    #[cfg(feature = "perf_profile")]
    let start = Instant::now();
    #[cfg(feature = "perf_profile")]
    {
      self.profile.caret_requests += 1;
    }
    let fingerprint = self.texts.fingerprint(text, style, max_width, wrap);
    if let Some(cached) = self
      .texts
      .get(fingerprint, text, style, max_width, wrap)
      .and_then(|entry| entry.carets.clone())
    {
      #[cfg(feature = "perf_profile")]
      {
        self.profile.caret_hits += 1;
        self.profile.caret_returned_positions += cached.len();
        self.profile.caret_total += start.elapsed();
      }
      return cached;
    }
    #[cfg(feature = "perf_profile")]
    {
      self.profile.caret_misses += 1;
    }
    let positions = self.compute_caret_positions(text, style, max_width, wrap);
    self.texts.entry(fingerprint, text, style, max_width, wrap).carets = Some(positions.clone());
    #[cfg(feature = "perf_profile")]
    {
      self.profile.caret_returned_positions += positions.len();
      self.profile.caret_total += start.elapsed();
    }
    positions
  }

  fn compute_caret_positions(&mut self, text: &str, style: &TextStyle, max_width: f32, wrap: bool) -> CaretPositions {
    #[cfg(feature = "perf_profile")]
    let buffer_start = Instant::now();
    #[cfg(feature = "perf_profile")]
    let layout_hits = self.profile.plain_buffer_hits;
    // Carets may run before measurement. Retain their full layout so either
    // consumer can build it first, including paragraph reuse after an edit.
    let mut buffer = self.full_text_buffer(text, style, max_width, effective_text_wrap(max_width, wrap));

    #[cfg(feature = "perf_profile")]
    {
      self.profile.caret_buffer += buffer_start.elapsed();
      self.profile.caret_layout_reuses += self.profile.plain_buffer_hits - layout_hits;
    }
    #[cfg(feature = "perf_profile")]
    let extract_start = Instant::now();
    if text.len() >= 1024 && text.contains(['\r', '\n']) {
      let positions = self.paragraph_caret_positions(&mut buffer, text.len());
      #[cfg(feature = "perf_profile")]
      {
        self.profile.caret_extract += extract_start.elapsed();
      }
      self.retain_plain_buffer(buffer);
      return positions;
    }
    let positions = carets::buffer_caret_positions(&buffer, text.len());
    #[cfg(feature = "perf_profile")]
    {
      self.profile.caret_built_paragraphs += buffer.lines.len();
      self.profile.caret_built_positions += positions.len();
    }
    #[cfg(feature = "perf_profile")]
    {
      self.profile.caret_extract += extract_start.elapsed();
    }
    self.retain_plain_buffer(buffer);
    positions
  }

  fn paragraph_caret_positions(&mut self, entry: &mut CachedPlainBuffer, text_len: usize) -> CaretPositions {
    let mut offsets = Vec::with_capacity(entry.buffer.lines.len());
    let mut offset = 0;
    let mut runs = 0;
    for (line, metadata) in entry.buffer.lines.iter().zip(&mut entry.paragraphs) {
      offsets.push(offset);
      offset += line.text().len() + line.ending().as_str().len();
      if let Some(layouts) = line.layout_opt() {
        runs += layouts.len();
        if metadata.carets.is_none() {
          let carets = ParagraphCarets::new(line.text(), layouts);
          #[cfg(feature = "perf_profile")]
          {
            self.profile.caret_built_paragraphs += 1;
            self.profile.caret_built_positions += carets.len();
          }
          entry.bytes += carets.bytes();
          metadata.carets = Some(std::sync::Arc::new(carets));
        } else {
          #[cfg(feature = "perf_profile")]
          {
            self.profile.caret_reused_paragraphs += 1;
          }
        }
      }
    }
    let mut positions = CaretPositionsBuilder::new(text_len, runs);
    let mut previous_line = usize::MAX;
    let mut layout_index = 0;
    for run in entry.buffer.layout_runs() {
      if run.line_i != previous_line {
        layout_index = 0;
        previous_line = run.line_i;
      }
      let layouts = entry.buffer.lines[run.line_i].layout_opt().unwrap();
      // Cosmic may skip a row with a negative baseline. Match the returned
      // glyph slice to its layout row without recomputing Cosmic's y arithmetic.
      while layouts[layout_index].glyphs.as_ptr() != run.glyphs.as_ptr()
        || layouts[layout_index].glyphs.len() != run.glyphs.len()
      {
        layout_index += 1;
      }
      positions.push_run(
        entry.paragraphs[run.line_i].carets.as_ref().unwrap().clone(),
        layout_index,
        offsets[run.line_i],
        run.text.len(),
        run.line_top.max(0.0),
      );
      layout_index += 1;
    }
    positions.finish()
  }

  pub(crate) fn rasterize_text(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    origin_x: f32,
    origin_y: f32,
  ) -> Vec<GlyphCmd> {
    let mut glyphs = Vec::new();
    self.rasterize_text_with_snap_into(
      text,
      style,
      max_width,
      max_width.is_finite(),
      origin_x,
      origin_y,
      true,
      None,
      &mut glyphs,
    );
    glyphs
  }

  #[allow(dead_code)]
  pub(crate) fn rasterize_text_with_wrap_into(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
    origin_x: f32,
    origin_y: f32,
    out: &mut Vec<GlyphCmd>,
  ) {
    self.rasterize_text_with_snap_into(text, style, max_width, wrap, origin_x, origin_y, true, None, out);
  }

  #[allow(clippy::too_many_arguments)]
  pub(crate) fn rasterize_text_with_wrap_clipped_into(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
    origin_x: f32,
    origin_y: f32,
    clip: ClipRect,
    out: &mut Vec<GlyphCmd>,
  ) {
    self.rasterize_text_with_snap_into(text, style, max_width, wrap, origin_x, origin_y, true, Some(clip), out);
  }

  #[allow(dead_code)]
  pub(crate) fn rasterize_rich_text_with_wrap_into(
    &mut self,
    spans: &[RichTextSpan],
    max_width: f32,
    wrap: bool,
    origin_x: f32,
    origin_y: f32,
    out: &mut Vec<GlyphCmd>,
  ) {
    self.rasterize_rich_text_with_snap_into(spans, max_width, wrap, origin_x, origin_y, true, out);
  }

  #[allow(clippy::too_many_arguments)]
  pub(crate) fn rasterize_rich_text_with_wrap_clipped_into(
    &mut self,
    spans: &[RichTextSpan],
    max_width: f32,
    wrap: bool,
    origin_x: f32,
    origin_y: f32,
    clip: ClipRect,
    out: &mut Vec<GlyphCmd>,
  ) {
    if let [span] = spans {
      self.rasterize_text_with_wrap_clipped_into(
        &span.text,
        &span.style,
        max_width,
        wrap,
        origin_x,
        origin_y,
        clip,
        out,
      );
      return;
    }
    self.rasterize_rich_text_with_snap_into(spans, max_width, wrap, origin_x, origin_y, true, out);
  }

  /// Paints `text`, untransformed, in a box at (`origin_x`, `box_y`) that
  /// is `box_height` tall: its glyphs at whole pixels, where `align` puts
  /// its first line, cut to `clip`, each with `order` and `clip` set. What
  /// `text_vertical_align_offset` and `rasterize_text_with_wrap_clipped_into`
  /// append, in one lookup when the text was painted before: a frame that
  /// changes no text spends one hash and one comparison on each.
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn paint_text_into(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
    align: VerticalAlign,
    box_height: f32,
    origin_x: f32,
    box_y: f32,
    clip: ClipRect,
    order: usize,
    out: &mut Vec<GlyphCmd>,
  ) {
    // The offset and the glyphs are one entry when both are keyed alike; a
    // traced run goes the long way, which prints it.
    if effective_text_wrap(max_width, wrap) == wrap && glyph_debug_marker().is_none() {
      let fingerprint = self.texts.fingerprint(text, style, max_width, wrap);
      if let Some(entry) = self.texts.get(fingerprint, text, style, max_width, wrap) {
        let offset = if box_height <= 0.0 || align == VerticalAlign::LineBox {
          Some(0.0)
        } else if align == VerticalAlign::Center {
          entry
            .optical_extents
            .map(|extents| extents.map_or(0.0, |(top, bottom)| (box_height - (bottom - top)) * 0.5 - top))
        } else {
          entry.vertical_extents.map(|extents| {
            extents.map_or(0.0, |extents| {
              vertical_align::offset_for_extents(align, extents, box_height)
            })
          })
        };
        if let (Some(offset), Some(cached)) = (offset, entry.glyphs(GlyphPlacement::Snapped)) {
          #[cfg(feature = "perf_profile")]
          let append_start = Instant::now();
          append_painted_glyph_cmds(cached, origin_x, box_y + offset, style, clip, order, out);
          let hits = cached.len();
          #[cfg(feature = "perf_profile")]
          {
            self.profile.append_cached += append_start.elapsed();
          }
          self.glyph_hits += hits;
          // The offset's lookup, where one was needed.
          self.measure_hits += usize::from(box_height > 0.0 && align != VerticalAlign::LineBox);
          return;
        }
      }
    }
    let origin_y = box_y + self.text_vertical_align_offset(text, style, max_width, wrap, align, box_height);
    let start = out.len();
    self.rasterize_text_with_wrap_clipped_into(text, style, max_width, wrap, origin_x, origin_y, clip, out);
    for glyph in &mut out[start..] {
      glyph.order = order;
      glyph.clip = clip;
    }
  }

  #[cfg(test)]
  pub(crate) fn rasterize_text_unsnapped(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    origin_x: f32,
    origin_y: f32,
  ) -> Vec<GlyphCmd> {
    let mut glyphs = Vec::new();
    self.rasterize_text_with_snap_into(
      text,
      style,
      max_width,
      max_width.is_finite(),
      origin_x,
      origin_y,
      false,
      None,
      &mut glyphs,
    );
    glyphs
  }

  pub(crate) fn rasterize_text_unsnapped_with_wrap_into(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
    origin_x: f32,
    origin_y: f32,
    out: &mut Vec<GlyphCmd>,
  ) {
    self.rasterize_text_with_snap_into(text, style, max_width, wrap, origin_x, origin_y, false, None, out);
  }

  pub(crate) fn rasterize_rich_text_unsnapped_with_wrap_into(
    &mut self,
    spans: &[RichTextSpan],
    max_width: f32,
    wrap: bool,
    origin_x: f32,
    origin_y: f32,
    out: &mut Vec<GlyphCmd>,
  ) {
    self.rasterize_rich_text_with_snap_into(spans, max_width, wrap, origin_x, origin_y, false, out);
  }

  #[allow(clippy::too_many_arguments)]
  pub(crate) fn rasterize_text_with_baked_transform_into(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
    origin_x: f32,
    origin_y: f32,
    transform: Transform2D,
    transform_origin: [f32; 2],
    out: &mut Vec<GlyphCmd>,
  ) {
    let wrap = effective_text_wrap(max_width, wrap);
    let key = CacheKey::new_for_baked_transform(text, style, max_width, wrap);
    let swash_transform = swash_transform_from_screen(transform);
    if let Some(cached) = self.transformed_glyph_layout_cache.get(&key).cloned() {
      self.append_baked_transformed_glyph_cmds_from_cached(
        &cached,
        origin_x,
        origin_y,
        style,
        transform,
        transform_origin,
        swash_transform,
        out,
      );
      return;
    }

    let mut buffer = self.acquire_buffer(style, max_width, wrap);
    let resolved = self.resolve_family(style);
    let attrs = self.style_attrs(style, &resolved);
    set_buffer_text(&mut buffer, text, attrs, style.text_align);
    buffer.shape_until_scroll(&mut self.font_system, false);

    let mut cached = Vec::new();
    for run in buffer.layout_runs() {
      for glyph in run.glyphs.iter() {
        if glyph_cluster_is_whitespace(run.text, glyph) {
          continue;
        }
        let x_offset = glyph.font_size * glyph.x_offset;
        let y_offset = glyph.font_size * glyph.y_offset;
        let (cache_key, ..) = GlyphCacheKey::new(
          glyph.font_id,
          glyph.glyph_id,
          glyph.font_size,
          (0.0, 0.0),
          glyph.font_weight,
          glyph.cache_key_flags,
        );
        cached.push(CachedTransformedGlyph {
          x: glyph.x + x_offset,
          y: run.line_y + glyph.y - y_offset,
          cache_key,
        });
      }
    }

    self.buffer_pool.push(buffer);
    if self.transformed_glyph_layout_cache.len() >= GLYPH_LAYOUT_CACHE_LIMIT {
      self.transformed_glyph_layout_cache.clear();
    }
    self.append_baked_transformed_glyph_cmds_from_cached(
      &cached,
      origin_x,
      origin_y,
      style,
      transform,
      transform_origin,
      swash_transform,
      out,
    );
    self.transformed_glyph_layout_cache.insert(key, cached);
  }

  #[allow(clippy::too_many_arguments)]
  pub(crate) fn rasterize_rich_text_with_baked_transform_into(
    &mut self,
    spans: &[RichTextSpan],
    max_width: f32,
    wrap: bool,
    origin_x: f32,
    origin_y: f32,
    transform: Transform2D,
    transform_origin: [f32; 2],
    out: &mut Vec<GlyphCmd>,
  ) {
    let Some(first) = spans.first() else {
      return;
    };
    if spans.len() == 1 {
      self.rasterize_text_with_baked_transform_into(
        &first.text,
        &first.style,
        max_width,
        wrap,
        origin_x,
        origin_y,
        transform,
        transform_origin,
        out,
      );
      return;
    }
    let wrap = effective_text_wrap(max_width, wrap);
    let swash_transform = swash_transform_from_screen(transform);
    let mut buffer = self.acquire_buffer(&first.style, max_width, wrap);
    self.set_rich_buffer_text(&mut buffer, spans);
    buffer.shape_until_scroll(&mut self.font_system, false);

    for run in buffer.layout_runs() {
      for glyph in run.glyphs.iter() {
        if glyph_cluster_is_whitespace(run.text, glyph) {
          continue;
        }
        let x_offset = glyph.font_size * glyph.x_offset;
        let y_offset = glyph.font_size * glyph.y_offset;
        let (cache_key, ..) = GlyphCacheKey::new(
          glyph.font_id,
          glyph.glyph_id,
          glyph.font_size,
          (0.0, 0.0),
          glyph.font_weight,
          glyph.cache_key_flags,
        );
        let Some(packed) = self.get_or_pack_transformed_glyph(cache_key, swash_transform) else {
          continue;
        };
        let (transformed_origin_x, transformed_origin_y) = transformed_glyph_origin(
          origin_x,
          origin_y,
          glyph.x + x_offset,
          run.line_y + glyph.y - y_offset,
          transform,
          transform_origin,
        );
        self.push_glyph_cmd(
          out,
          transformed_origin_x + packed.left as f32,
          transformed_origin_y - packed.top as f32,
          packed,
          glyph_color(glyph.color_opt, first.style.color),
          false,
        );
      }
    }
    self.buffer_pool.push(buffer);
  }

  #[allow(clippy::too_many_arguments)]
  fn rasterize_text_with_snap_into(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
    origin_x: f32,
    origin_y: f32,
    snap_to_pixel: bool,
    clip: Option<ClipRect>,
    out: &mut Vec<GlyphCmd>,
  ) {
    let wrap = effective_text_wrap(max_width, wrap);
    let placement = if snap_to_pixel {
      GlyphPlacement::Snapped
    } else {
      GlyphPlacement::Unsnapped
    };
    let fingerprint = self.texts.fingerprint(text, style, max_width, wrap);
    let atlas_w = self.atlas_packer.width as f32;
    let atlas_h = self.atlas_packer.height as f32;
    // `LURQ_GLYPH_DEBUG=<substring>` dumps every emission of a matching text
    // run: cache path, geometry, atlas coords, and the resulting UVs — the
    // ground truth for "this text is invisible on screen" hunts.
    let debug_marker = glyph_debug_marker().filter(|marker| text.contains(marker.as_str()));
    if let Some(marker) = &debug_marker {
      eprintln!(
        "[glyph-debug:{marker}] run text={text:?} size={} family={:?} origin=({origin_x}, {origin_y}) \
         max_width={max_width} snap={snap_to_pixel} clip={} atlas={atlas_w}x{atlas_h}",
        style.font_size,
        style.font_family,
        clip_debug_text(clip),
      );
    }
    if let Some(cached) = self
      .texts
      .get(fingerprint, text, style, max_width, wrap)
      .and_then(|entry| entry.glyphs(placement))
    {
      if let Some(marker) = &debug_marker {
        for glyph in cached.iter() {
          eprintln!(
            "[glyph-debug:{marker}] HIT glyph at ({}, {}) {}x{} atlas=({}, {})",
            origin_x + glyph.x,
            origin_y + glyph.y,
            glyph.width,
            glyph.height,
            glyph.atlas_x,
            glyph.atlas_y,
          );
        }
      }
      self.glyph_hits += cached.len();
      #[cfg(feature = "perf_profile")]
      let append_start = Instant::now();
      append_glyph_cmds_from_cached(
        cached,
        origin_x,
        origin_y,
        style,
        atlas_w,
        atlas_h,
        snap_to_pixel,
        clip,
        out,
      );
      #[cfg(feature = "perf_profile")]
      {
        self.profile.append_cached += append_start.elapsed();
      }
      return;
    }
    // A short text is always laid out whole, and cached whole: the clip then
    // picks its glyphs wherever the text is scrolled to. Only a document is
    // laid out down to the clip, and cached by where the clip cuts it.
    let whole = text.len() < SHORT_TEXT_BYTES;
    let run_clip = if whole { None } else { clip };
    let clipped_key = run_clip.and_then(|clip| {
      let key = CacheKey::new_for_raster(text, style, max_width, wrap, snap_to_pixel);
      ClippedCacheKey::new(key, origin_x, origin_y, clip)
    });
    if let Some(clipped_key) = clipped_key.as_ref()
      && let Some(cached) = self.clipped_glyph_layout_cache.get(clipped_key)
    {
      self.glyph_hits += cached.len();
      #[cfg(feature = "perf_profile")]
      let append_start = Instant::now();
      append_glyph_cmds_from_cached(
        cached,
        origin_x,
        origin_y,
        style,
        atlas_w,
        atlas_h,
        snap_to_pixel,
        clip,
        out,
      );
      #[cfg(feature = "perf_profile")]
      {
        self.profile.append_cached += append_start.elapsed();
      }
      return;
    }

    #[cfg(feature = "perf_profile")]
    let acquire_start = Instant::now();
    let shaped = if whole {
      Some(self.full_text_buffer(text, style, max_width, wrap))
    } else {
      self.take_plain_buffer(text, style, max_width, wrap)
    };
    let mut partial = None;
    if shaped.is_none() {
      let mut buffer = self.acquire_buffer(style, max_width, wrap);
      if let Some(height) = clipped_raster_shape_height(origin_y, style, clip) {
        buffer.set_size(text_buffer_width(max_width), Some(height));
      }
      partial = Some(buffer);
    }
    #[cfg(feature = "perf_profile")]
    {
      self.profile.raster_acquire_buffer += acquire_start.elapsed();
    }
    #[cfg(feature = "perf_profile")]
    let text_start = Instant::now();
    if let Some(buffer) = &mut partial {
      let resolved = self.resolve_family(style);
      let attrs = self.style_attrs(style, &resolved);
      set_buffer_text(buffer, text, attrs, style.text_align);
      buffer.shape_until_scroll(&mut self.font_system, false);
    }

    let buffer = shaped
      .as_ref()
      .map(|entry| &entry.buffer)
      .or(partial.as_ref())
      .expect("raster buffer");

    #[cfg(feature = "perf_profile")]
    {
      self.profile.raster_set_text += text_start.elapsed();
    }
    let mut cached = Vec::new();
    let mut skipped_run_for_clip = false;
    let mut visited_runs = 0;
    for run in buffer.layout_runs() {
      visited_runs += 1;
      if !text_run_intersects_clip(origin_y, run.line_top, run.line_height, run_clip) {
        if let Some(marker) = &debug_marker {
          eprintln!(
            "[glyph-debug:{marker}] MISS line SKIPPED by clip: line_top={} line_height={} origin_y={origin_y} clip={}",
            run.line_top,
            run.line_height,
            clip_debug_text(clip),
          );
        }
        skipped_run_for_clip = true;
        continue;
      }
      for glyph in run.glyphs.iter() {
        if glyph_cluster_is_whitespace(run.text, glyph) {
          continue;
        }
        let cached_glyph = if snap_to_pixel {
          let physical = self.hinted_fonts.physical(glyph, run.line_y);
          let Some(packed) = self.get_or_pack_glyph(physical.cache_key) else {
            if let Some(marker) = &debug_marker {
              eprintln!(
                "[glyph-debug:{marker}] MISS glyph {} FAILED to rasterize/pack (font_id={:?})",
                glyph.glyph_id, physical.cache_key.font_id,
              );
            }
            continue;
          };

          CachedGlyph {
            x: (physical.x + packed.left) as f32,
            y: (physical.y - packed.top) as f32,
            atlas_x: packed.x,
            atlas_y: packed.y,
            width: packed.width,
            height: packed.height,
            is_color: packed.is_color,
          }
        } else {
          let x_offset = glyph.font_size * glyph.x_offset;
          let y_offset = glyph.font_size * glyph.y_offset;
          let (cache_key, ..) = GlyphCacheKey::new(
            glyph.font_id,
            glyph.glyph_id,
            glyph.font_size,
            (0.0, 0.0),
            glyph.font_weight,
            glyph.cache_key_flags,
          );
          let Some(packed) = self.get_or_pack_glyph(cache_key) else {
            if let Some(marker) = &debug_marker {
              eprintln!(
                "[glyph-debug:{marker}] MISS glyph {} FAILED to rasterize/pack (font_id={:?})",
                glyph.glyph_id, glyph.font_id,
              );
            }
            continue;
          };

          CachedGlyph {
            x: glyph.x + x_offset + packed.left as f32,
            y: run.line_y + glyph.y - y_offset - packed.top as f32,
            atlas_x: packed.x,
            atlas_y: packed.y,
            width: packed.width,
            height: packed.height,
            is_color: packed.is_color,
          }
        };

        cached.push(cached_glyph);
      }
    }
    // A buffer shaped down to the clip ends its runs there, often without
    // skipping one; the rows it never reached are cut by the clip all the same.
    let clipped = skipped_run_for_clip || !runs_cover_buffer(buffer, visited_runs);

    if let Some(entry) = shaped {
      self.retain_plain_buffer(entry);
    } else if let Some(buffer) = partial {
      self.buffer_pool.push(buffer);
    }
    let atlas_w = self.atlas_packer.width as f32;
    let atlas_h = self.atlas_packer.height as f32;
    if let Some(marker) = &debug_marker {
      eprintln!(
        "[glyph-debug:{marker}] MISS rasterized {} glyphs, skipped_for_clip={skipped_run_for_clip}, atlas now {atlas_w}x{atlas_h}",
        cached.len(),
      );
      for glyph in &cached {
        eprintln!(
          "[glyph-debug:{marker}] MISS glyph at ({}, {}) {}x{} atlas=({}, {})",
          origin_x + glyph.x,
          origin_y + glyph.y,
          glyph.width,
          glyph.height,
          glyph.atlas_x,
          glyph.atlas_y,
        );
      }
    }
    #[cfg(feature = "perf_profile")]
    let append_start = Instant::now();
    append_glyph_cmds_from_cached(
      &cached,
      origin_x,
      origin_y,
      style,
      atlas_w,
      atlas_h,
      snap_to_pixel,
      clip,
      out,
    );
    #[cfg(feature = "perf_profile")]
    {
      self.profile.append_cached += append_start.elapsed();
    }
    if !clipped {
      self
        .texts
        .entry(fingerprint, text, style, max_width, wrap)
        .set_glyphs(placement, cached);
    } else if let Some(clipped_key) = clipped_key {
      if self.clipped_glyph_layout_cache.len() >= GLYPH_LAYOUT_CACHE_LIMIT {
        self.clipped_glyph_layout_cache.clear();
      }
      self.clipped_glyph_layout_cache.insert(clipped_key, cached);
    }
  }

  #[allow(clippy::too_many_arguments)]
  fn rasterize_rich_text_with_snap_into(
    &mut self,
    spans: &[RichTextSpan],
    max_width: f32,
    wrap: bool,
    origin_x: f32,
    origin_y: f32,
    snap_to_pixel: bool,
    out: &mut Vec<GlyphCmd>,
  ) {
    let Some(first) = spans.first() else {
      return;
    };
    if spans.len() == 1 {
      self.rasterize_text_with_snap_into(
        &first.text,
        &first.style,
        max_width,
        wrap,
        origin_x,
        origin_y,
        snap_to_pixel,
        None,
        out,
      );
      return;
    }
    let wrap = effective_text_wrap(max_width, wrap);
    let fingerprint = rich_text_raster_fingerprint(spans, max_width, wrap, snap_to_pixel);
    let atlas_w = self.atlas_packer.width as f32;
    let atlas_h = self.atlas_packer.height as f32;
    if let Some(cached) = self.find_rich_glyph_layout(fingerprint, spans, max_width, wrap, snap_to_pixel) {
      let hit_count = cached.len();
      #[cfg(feature = "perf_profile")]
      let append_start = Instant::now();
      append_rich_glyph_cmds_from_cached(cached, origin_x, origin_y, atlas_w, atlas_h, snap_to_pixel, out);
      #[cfg(feature = "perf_profile")]
      {
        self.profile.append_cached += append_start.elapsed();
      }
      self.glyph_hits += hit_count;
      return;
    }

    if snap_to_pixel {
      let shape_fingerprint = rich_text_shape_fingerprint(spans, 1.0, max_width, wrap);
      if let Some(shaped) = self.find_rich_shaped_layout(shape_fingerprint, spans, 1.0, max_width, wrap) {
        let cached = self.pack_rich_shaped_layout(&shaped);
        if self.rich_glyph_layout_cache.len() >= GLYPH_LAYOUT_CACHE_LIMIT {
          self.rich_glyph_layout_cache.clear();
        }
        let atlas_w = self.atlas_packer.width as f32;
        let atlas_h = self.atlas_packer.height as f32;
        #[cfg(feature = "perf_profile")]
        let append_start = Instant::now();
        append_rich_glyph_cmds_from_cached(&cached, origin_x, origin_y, atlas_w, atlas_h, snap_to_pixel, out);
        #[cfg(feature = "perf_profile")]
        {
          self.profile.append_cached += append_start.elapsed();
        }
        self.rich_glyph_layout_cache.entry(fingerprint).or_default().push((
          RichTextCacheKey::new_for_raster(spans, max_width, wrap, snap_to_pixel),
          cached,
        ));
        return;
      }

      let shaped = self.shape_rich_text_layout(spans, max_width, wrap, false);
      let cached = self.pack_rich_shaped_layout(&shaped);
      if self.rich_shaped_layout_cache.len() >= GLYPH_LAYOUT_CACHE_LIMIT {
        self.rich_shaped_layout_cache.clear();
      }
      self
        .rich_shaped_layout_cache
        .entry(shape_fingerprint)
        .or_default()
        .push((RichTextShapeKey::new(spans, max_width, wrap), shaped));
      if self.rich_glyph_layout_cache.len() >= GLYPH_LAYOUT_CACHE_LIMIT {
        self.rich_glyph_layout_cache.clear();
      }
      let atlas_w = self.atlas_packer.width as f32;
      let atlas_h = self.atlas_packer.height as f32;
      #[cfg(feature = "perf_profile")]
      let append_start = Instant::now();
      append_rich_glyph_cmds_from_cached(&cached, origin_x, origin_y, atlas_w, atlas_h, snap_to_pixel, out);
      #[cfg(feature = "perf_profile")]
      {
        self.profile.append_cached += append_start.elapsed();
      }
      self.rich_glyph_layout_cache.entry(fingerprint).or_default().push((
        RichTextCacheKey::new_for_raster(spans, max_width, wrap, snap_to_pixel),
        cached,
      ));
      return;
    }

    let mut buffer = self.acquire_buffer(&first.style, max_width, wrap);
    self.set_rich_buffer_text(&mut buffer, spans);
    buffer.shape_until_scroll(&mut self.font_system, false);

    let mut cached = Vec::new();
    for run in buffer.layout_runs() {
      for glyph in run.glyphs.iter() {
        if glyph_cluster_is_whitespace(run.text, glyph) {
          continue;
        }
        let default_color = first.style.color;
        let color = glyph_color(glyph.color_opt, default_color);
        let cached_glyph = if snap_to_pixel {
          let physical = self.hinted_fonts.physical(glyph, run.line_y);
          let Some(packed) = self.get_or_pack_glyph(physical.cache_key) else {
            continue;
          };

          CachedGlyph {
            x: (physical.x + packed.left) as f32,
            y: (physical.y - packed.top) as f32,
            atlas_x: packed.x,
            atlas_y: packed.y,
            width: packed.width,
            height: packed.height,
            is_color: packed.is_color,
          }
        } else {
          let x_offset = glyph.font_size * glyph.x_offset;
          let y_offset = glyph.font_size * glyph.y_offset;
          let (cache_key, ..) = GlyphCacheKey::new(
            glyph.font_id,
            glyph.glyph_id,
            glyph.font_size,
            (0.0, 0.0),
            glyph.font_weight,
            glyph.cache_key_flags,
          );
          let Some(packed) = self.get_or_pack_glyph(cache_key) else {
            continue;
          };

          CachedGlyph {
            x: glyph.x + x_offset + packed.left as f32,
            y: run.line_y + glyph.y - y_offset - packed.top as f32,
            atlas_x: packed.x,
            atlas_y: packed.y,
            width: packed.width,
            height: packed.height,
            is_color: packed.is_color,
          }
        };

        cached.push(CachedRichGlyph {
          glyph: cached_glyph,
          color,
        });
      }
    }
    self.buffer_pool.push(buffer);
    if self.rich_glyph_layout_cache.len() >= GLYPH_LAYOUT_CACHE_LIMIT {
      self.rich_glyph_layout_cache.clear();
    }
    let atlas_w = self.atlas_packer.width as f32;
    let atlas_h = self.atlas_packer.height as f32;
    #[cfg(feature = "perf_profile")]
    let append_start = Instant::now();
    append_rich_glyph_cmds_from_cached(&cached, origin_x, origin_y, atlas_w, atlas_h, snap_to_pixel, out);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.append_cached += append_start.elapsed();
    }
    self.rich_glyph_layout_cache.entry(fingerprint).or_default().push((
      RichTextCacheKey::new_for_raster(spans, max_width, wrap, snap_to_pixel),
      cached,
    ));
  }

  #[allow(clippy::too_many_arguments)]
  fn append_baked_transformed_glyph_cmds_from_cached(
    &mut self,
    cached: &[CachedTransformedGlyph],
    origin_x: f32,
    origin_y: f32,
    style: &TextStyle,
    transform: Transform2D,
    transform_origin: [f32; 2],
    swash_transform: SwashTransform,
    out: &mut Vec<GlyphCmd>,
  ) {
    let color = style.color.to_linear_f32_array();
    out.reserve(cached.len());

    for glyph in cached {
      let Some(packed) = self.get_or_pack_transformed_glyph(glyph.cache_key, swash_transform) else {
        continue;
      };
      let (transformed_origin_x, transformed_origin_y) =
        transformed_glyph_origin(origin_x, origin_y, glyph.x, glyph.y, transform, transform_origin);

      out.push(GlyphCmd {
        order: 0,
        x: transformed_origin_x + packed.left as f32,
        y: transformed_origin_y - packed.top as f32,
        width: packed.width as f32,
        height: packed.height as f32,
        color,
        atlas_min: [packed.x as f32, packed.y as f32],
        atlas_max: [(packed.x + packed.width) as f32, (packed.y + packed.height) as f32],
        transform: [1.0, 0.0, 0.0, 1.0],
        transform_origin: [0.0, 0.0],
        sharpness: 1.0,
        color_glyph: packed.is_color,
        shadow_sigma: 0.0,
        clip: crate::layout::quad::ClipRect::default(),
      });
    }
  }

  pub(crate) fn atlas(&mut self) -> GlyphAtlas {
    self.atlas_packer.to_atlas()
  }

  /// RGBA bytes of the glyph atlas's pixels.
  #[cfg(feature = "perf_profile")]
  pub(crate) fn atlas_bytes(&self) -> usize {
    self.atlas_packer.data.len()
  }

  /// Snapshots of the atlas (`atlas()`) still held somewhere; while there is
  /// one, the next new glyph is packed into a copy of the atlas's pixels.
  #[cfg(all(test, feature = "screenshot", feature = "raster"))]
  pub(crate) fn atlas_snapshots_held(&self) -> usize {
    std::sync::Arc::strong_count(&self.atlas_packer.data) - 1
  }

  fn get_or_pack_glyph(&mut self, cache_key: GlyphCacheKey) -> Option<PackedGlyph> {
    if let Some(&packed) = self.atlas_entries.get(&cache_key) {
      self.glyph_hits += 1;
      return Some(packed);
    }

    let font = self.font_system.get_font(cache_key.font_id, cache_key.font_weight)?;
    #[cfg(feature = "perf_profile")]
    {
      self.profile.swash_requests += 1;
    }
    #[cfg(feature = "perf_profile")]
    let swash_start = Instant::now();
    let image = if cache_key.flags.contains(hinted::HINTED) {
      let face = self
        .font_system
        .db()
        .face(cache_key.font_id)
        .map_or(0, |face| face.index);
      self.hinted_fonts.render(&font, face, cache_key)
    } else {
      render_glyph_image(&mut self.swash_context, &font, cache_key)
    };
    #[cfg(feature = "perf_profile")]
    {
      self.profile.swash_lookup += swash_start.elapsed();
    }
    let Some(image) = image else {
      self.glyph_misses += 1;
      return None;
    };
    if image.placement.width == 0 || image.placement.height == 0 {
      return None;
    }

    #[cfg(feature = "perf_profile")]
    let atlas_pack_start = Instant::now();
    let (x, y, width, height, is_color) = self.atlas_packer.pack_image(&image);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.atlas_pack += atlas_pack_start.elapsed();
      self.profile.atlas_packs += 1;
    }
    let packed = PackedGlyph {
      x,
      y,
      width,
      height,
      left: image.placement.left - GLYPH_ATLAS_PADDING as i32,
      top: image.placement.top + GLYPH_ATLAS_PADDING as i32,
      is_color,
    };
    self.atlas_entries.insert(cache_key, packed);
    self.glyph_misses += 1;
    Some(packed)
  }

  fn get_or_pack_transformed_glyph(
    &mut self,
    cache_key: GlyphCacheKey,
    transform: SwashTransform,
  ) -> Option<PackedGlyph> {
    let key = TransformedGlyphKey::new(cache_key, transform);
    if let Some(&packed) = self.transformed_atlas_entries.get(&key) {
      self.glyph_hits += 1;
      return Some(packed);
    }

    let font = self.font_system.get_font(cache_key.font_id, cache_key.font_weight)?;
    let mut scaler = scaler::unhinted_scaler(&mut self.transformed_scale_context, &font, cache_key).build();
    let offset = Vector::new(cache_key.x_bin.as_float(), cache_key.y_bin.as_float());
    let transform = if cache_key.flags.contains(cosmic_text::CacheKeyFlags::FAKE_ITALIC) {
      SwashTransform::skew(Angle::from_degrees(14.0), Angle::from_degrees(0.0)).then(&transform)
    } else {
      transform
    };
    #[cfg(feature = "perf_profile")]
    {
      self.profile.swash_requests += 1;
    }
    #[cfg(feature = "perf_profile")]
    let swash_start = Instant::now();
    let image = Render::new(&[
      Source::ColorOutline(0),
      Source::ColorBitmap(StrikeWith::BestFit),
      Source::Outline,
    ])
    .format(Format::Alpha)
    .offset(offset)
    .transform(Some(transform))
    .render(&mut scaler, cache_key.glyph_id);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.swash_lookup += swash_start.elapsed();
    }
    let image = image?;
    if image.placement.width == 0 || image.placement.height == 0 {
      return None;
    }

    #[cfg(feature = "perf_profile")]
    let atlas_pack_start = Instant::now();
    let (x, y, width, height, is_color) = self.atlas_packer.pack_image(&image);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.atlas_pack += atlas_pack_start.elapsed();
      self.profile.atlas_packs += 1;
    }
    let packed = PackedGlyph {
      x,
      y,
      width,
      height,
      left: image.placement.left - GLYPH_ATLAS_PADDING as i32,
      top: image.placement.top + GLYPH_ATLAS_PADDING as i32,
      is_color,
    };
    self.transformed_atlas_entries.insert(key, packed);
    self.glyph_misses += 1;
    Some(packed)
  }

  fn shape_and_measure(&mut self, text: &str, style: &TextStyle, max_width: f32, wrap: bool) -> Size {
    #[cfg(feature = "perf_profile")]
    let profile_start = Instant::now();
    let metrics = Metrics::new(style.font_size, style.font_size * style.line_height);
    let buffer = self.full_text_buffer(text, style, max_width, wrap);

    let mut width = 0.0_f32;
    let mut first_line_y = 0.0_f32;
    let mut last_line_y = 0.0_f32;
    let mut has_runs = false;
    for run in buffer.layout_runs() {
      width = width.max(run.line_w);
      if !has_runs {
        first_line_y = run.line_y;
        has_runs = true;
      }
      last_line_y = run.line_y;
    }
    let height = if has_runs {
      last_line_y - first_line_y + metrics.line_height - line_box_trim(style)
    } else {
      0.0
    };

    self.retain_plain_buffer(buffer);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.shape_text += profile_start.elapsed();
    }
    Size::new(width, height)
  }

  fn take_plain_buffer(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
  ) -> Option<CachedPlainBuffer> {
    if text.len() < SHORT_TEXT_BYTES {
      return self.take_shaped_text(text, style, max_width, wrap);
    }
    let index = self
      .plain_buffers
      .iter()
      .position(|entry| entry.key.matches_measure(text, style, max_width, wrap));
    #[cfg(feature = "perf_profile")]
    {
      if index.is_some() {
        self.profile.plain_buffer_hits += 1;
      } else {
        self.profile.plain_buffer_misses += 1;
      }
    }
    let mut entry = self.plain_buffers.remove(index?);
    self.plain_buffer_bytes -= entry.bytes;
    if !entry.layout_ready {
      #[cfg(feature = "perf_profile")]
      let start = Instant::now();
      for index in 0..entry.buffer.lines.len() {
        self.layout_plain_paragraph(&mut entry.buffer, index);
        self.update_plain_layout_metadata(&mut entry, index);
      }
      entry.buffer.shape_until_scroll(&mut self.font_system, false);
      entry.layout_ready = true;
      entry.refresh_bytes();
      #[cfg(feature = "perf_profile")]
      {
        self.profile.plain_buffer_build += start.elapsed();
      }
    }
    Some(entry)
  }

  /// A short text kept shaped, laid out as asked. Shaped for the same text
  /// in the same face, it may have been laid out for another size, width,
  /// wrapping or alignment: it is then laid out again, not shaped again.
  fn take_shaped_text(
    &mut self,
    text: &str,
    style: &TextStyle,
    max_width: f32,
    wrap: bool,
  ) -> Option<CachedPlainBuffer> {
    let entry = self.shaped_texts.take(text, style);
    #[cfg(feature = "perf_profile")]
    {
      if entry.is_some() {
        self.profile.plain_buffer_hits += 1;
      } else {
        self.profile.plain_buffer_misses += 1;
      }
    }
    let mut entry = entry?;
    if entry.key.matches_measure(text, style, max_width, wrap) {
      return Some(entry);
    }
    #[cfg(feature = "perf_profile")]
    let start = Instant::now();
    // The settings `acquire_buffer` and `set_buffer_text` give a new buffer.
    let wraps = effective_text_wrap(max_width, wrap);
    entry
      .buffer
      .set_metrics(Metrics::new(style.font_size, style.font_size * style.line_height));
    entry.buffer.set_size(text_buffer_width(max_width), None);
    entry
      .buffer
      .set_wrap(if wraps { Wrap::WordOrGlyph } else { Wrap::None });
    let align = Some(style.text_align.to_cosmic());
    for line in &mut entry.buffer.lines {
      line.set_align(align);
    }
    entry.buffer.shape_until_scroll(&mut self.font_system, false);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.plain_paragraph_layout += start.elapsed();
      self.profile.plain_laid_out_paragraphs += entry.buffer.lines.len();
    }
    for (line, paragraph) in entry.buffer.lines.iter().zip(&mut entry.paragraphs) {
      paragraph.carets = None;
      paragraph.layout_bytes = plain_line_layout_bytes(line);
    }
    entry.key.lay_out_as(style, max_width, wrap);
    entry.refresh_bytes();
    Some(entry)
  }

  fn full_text_buffer(&mut self, text: &str, style: &TextStyle, max_width: f32, wrap: bool) -> CachedPlainBuffer {
    if let Some(buffer) = self.take_plain_buffer(text, style, max_width, wrap) {
      return buffer;
    }
    #[cfg(feature = "perf_profile")]
    let build_start = Instant::now();
    let multiline = text.len() >= 1024 && text.contains(['\r', '\n']);
    let previous = multiline
      .then(|| self.take_related_plain_buffer(text, style, wrap))
      .flatten();
    let width_changed = previous
      .as_ref()
      .is_some_and(|entry| entry.key.max_width_bits != max_width.to_bits());
    let mut entry = previous.unwrap_or_else(|| CachedPlainBuffer {
      key: CacheKey::new(text, style, max_width, wrap),
      buffer: self.acquire_buffer(style, max_width, wrap),
      paragraphs: Vec::new(),
      bytes: 0,
      layout_ready: true,
    });
    let resolved = self.resolve_family(style);
    let attrs = self.style_attrs(style, &resolved);
    if multiline {
      let ranges = cosmic_text::LineIter::new(text).collect::<Vec<_>>();
      let matches = |old: &cosmic_text::BufferLine, new: &(std::ops::Range<usize>, cosmic_text::LineEnding)| {
        old.text() == &text[new.0.clone()] && old.ending() == new.1
      };
      let prefix = entry
        .buffer
        .lines
        .iter()
        .zip(&ranges)
        .take_while(|(old, new)| matches(old, new))
        .count();
      let suffix = entry.buffer.lines[prefix..]
        .iter()
        .rev()
        .zip(ranges[prefix..].iter().rev())
        .take_while(|(old, new)| matches(old, new))
        .count();
      let old_end = entry.buffer.lines.len() - suffix;
      let new_end = ranges.len() - suffix;
      // Most edits change only a small middle. Matching boundaries retain allocations
      // directly; moved/reordered middle paragraphs still require exact text matches.
      let mut middle: HashMap<u64, Vec<(cosmic_text::BufferLine, PlainParagraph)>> = HashMap::new();
      for (line, paragraph) in entry
        .buffer
        .lines
        .drain(prefix..old_end)
        .zip(entry.paragraphs.drain(prefix..old_end))
      {
        middle.entry(paragraph.fingerprint).or_default().push((line, paragraph));
        #[cfg(feature = "perf_profile")]
        {
          self.profile.plain_reindexed_paragraphs += 1;
        }
      }
      #[cfg(feature = "perf_profile")]
      {
        self.profile.plain_retained_paragraphs += prefix + suffix;
      }
      let mut new_lines = Vec::with_capacity(new_end - prefix);
      let mut new_paragraphs = Vec::with_capacity(new_end - prefix);
      for (range, ending) in &ranges[prefix..new_end] {
        let paragraph = &text[range.clone()];
        let fingerprint = paragraph_fingerprint(paragraph);
        let retained = middle.get_mut(&fingerprint).and_then(|candidates| {
          let index = candidates
            .iter()
            .rposition(|(line, _)| line.text() == paragraph && line.ending() == *ending)?;
          Some(candidates.swap_remove(index))
        });
        let (line, metadata) = if let Some(retained) = retained {
          #[cfg(feature = "perf_profile")]
          {
            self.profile.plain_retained_paragraphs += 1;
          }
          retained
        } else {
          let mut line = cosmic_text::BufferLine::new(
            paragraph,
            *ending,
            cosmic_text::AttrsList::new(&attrs),
            Shaping::Advanced,
          );
          line.set_align(Some(style.text_align.to_cosmic()));
          (
            line,
            PlainParagraph {
              fingerprint,
              ..PlainParagraph::default()
            },
          )
        };
        new_lines.push(line);
        new_paragraphs.push(metadata);
      }
      entry.buffer.lines.splice(prefix..prefix, new_lines);
      entry.paragraphs.splice(prefix..prefix, new_paragraphs);
      if width_changed {
        // The setter marks every laid-out line for relayout on the next shape
        // pass. Set the width while empty and settle that flag there, then
        // explicitly rewrap retained shapes so reflow reuse survives and
        // profiling covers that work.
        let lines = std::mem::take(&mut entry.buffer.lines);
        entry.buffer.set_size(text_buffer_width(max_width), None);
        entry.buffer.shape_until_scroll(&mut self.font_system, false);
        entry.buffer.lines = lines;
        let new_width = entry.buffer.size().0;
        for (line, metadata) in entry.buffer.lines.iter_mut().zip(&mut entry.paragraphs) {
          if line.layout_opt().is_some() && metadata.reflow.is_some_and(|range| range.contains(new_width)) {
            #[cfg(feature = "perf_profile")]
            {
              self.profile.plain_reflow_reuses += 1;
            }
          } else {
            line.reset_layout();
            metadata.carets = None;
            metadata.reflow = None;
          }
        }
      }
      let work = if width_changed || !entry.layout_ready {
        0..ranges.len()
      } else {
        prefix..new_end
      };
      let mut duplicates: HashMap<u64, usize> = HashMap::new();
      for index in 0..work.start.min(256) {
        duplicates.insert(entry.paragraphs[index].fingerprint, index);
      }
      for index in work {
        let fingerprint = entry.paragraphs[index].fingerprint;
        let needs_shape = entry.buffer.lines[index].shape_opt().is_none();
        if needs_shape
          && let Some(&prior) = duplicates.get(&fingerprint).filter(|&&prior| {
            let old = &entry.buffer.lines[prior];
            let new = &entry.buffer.lines[index];
            old.text() == new.text() && old.ending() == new.ending()
          })
        {
          entry.buffer.lines[index] = entry.buffer.lines[prior].clone();
          #[cfg(feature = "perf_profile")]
          {
            self.profile.plain_reused_lines += 1;
          }
        }
        let needs_layout = entry.buffer.lines[index].layout_opt().is_none();
        if needs_layout {
          self.layout_plain_paragraph(&mut entry.buffer, index);
        }
        if needs_shape {
          // Cloning a Cosmic line can change Vec capacities, so measure the clone.
          entry.paragraphs[index].shape_bytes = plain_line_shape_bytes(&entry.buffer.lines[index]);
          #[cfg(feature = "perf_profile")]
          {
            self.profile.plain_accounted_paragraphs += 1;
          }
        }
        if needs_shape || needs_layout {
          entry.paragraphs[index].carets = None;
          self.update_plain_layout_metadata(&mut entry, index);
        }
        if duplicates.len() < 256 {
          duplicates.insert(fingerprint, index);
        }
      }
      entry.buffer.set_scroll(cosmic_text::Scroll::default());
    } else {
      set_buffer_text(&mut entry.buffer, text, attrs, style.text_align);
      // Shape and wrap each paragraph here, as the pass below would, so a
      // profile tells shaping from wrapping for short text too.
      #[cfg(feature = "perf_profile")]
      for index in 0..entry.buffer.lines.len() {
        self.layout_plain_paragraph(&mut entry.buffer, index);
      }
    }
    #[cfg(feature = "perf_profile")]
    let finalize_start = Instant::now();
    entry.buffer.shape_until_scroll(&mut self.font_system, false);
    if !multiline {
      entry.paragraphs = entry
        .buffer
        .lines
        .iter()
        .map(|line| PlainParagraph {
          carets: None,
          reflow: None,
          fingerprint: paragraph_fingerprint(line.text()),
          shape_bytes: plain_line_shape_bytes(line),
          layout_bytes: plain_line_layout_bytes(line),
        })
        .collect();
    }
    if entry.key.text != text || width_changed {
      entry.key = CacheKey::new(text, style, max_width, wrap);
    }
    entry.layout_ready = true;
    entry.refresh_bytes();
    #[cfg(feature = "perf_profile")]
    {
      self.profile.plain_buffer_finalize += finalize_start.elapsed();
      self.profile.plain_buffer_build += build_start.elapsed();
    }
    entry
  }

  /// Consume a recent compatible document version to reuse its independent paragraphs.
  /// A shared first/last paragraph is a cheap candidate heuristic, never a correctness key.
  fn take_related_plain_buffer(&mut self, text: &str, style: &TextStyle, wrap: bool) -> Option<CachedPlainBuffer> {
    let first = text.lines().next();
    let last = text.lines().next_back();
    let index = self.plain_buffers.iter().rposition(|entry| {
      entry.buffer.lines.len() > 1
        && entry
          .key
          .matches_measure(&entry.key.text, style, f32::from_bits(entry.key.max_width_bits), wrap)
        && (entry.key.text.lines().next() == first || entry.key.text.lines().next_back() == last)
    })?;
    let entry = self.plain_buffers.remove(index);
    self.plain_buffer_bytes -= entry.bytes;
    Some(entry)
  }

  fn layout_plain_paragraph(&mut self, buffer: &mut Buffer, index: usize) {
    #[cfg(feature = "perf_profile")]
    {
      let start = Instant::now();
      let needs_shape = buffer.lines[index].shape_opt().is_none();
      buffer.line_shape(&mut self.font_system, index);
      self.profile.plain_paragraph_shape += start.elapsed();
      self.profile.plain_shaped_paragraphs += usize::from(needs_shape);
    }
    #[cfg(feature = "perf_profile")]
    let start = Instant::now();
    #[cfg(feature = "perf_profile")]
    let needs_layout = buffer.lines[index].layout_opt().is_none();
    buffer.line_layout(&mut self.font_system, index);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.plain_paragraph_layout += start.elapsed();
      self.profile.plain_laid_out_paragraphs += usize::from(needs_layout);
    }
  }

  fn update_plain_layout_metadata(&mut self, entry: &mut CachedPlainBuffer, index: usize) {
    let line = &entry.buffer.lines[index];
    entry.paragraphs[index].layout_bytes = plain_line_layout_bytes(line);
    #[cfg(feature = "perf_profile")]
    let start = Instant::now();
    entry.paragraphs[index].reflow = ReflowRange::new(
      line,
      entry.buffer.metrics().font_size,
      entry.buffer.size().0,
      entry.buffer.wrap() != Wrap::None,
    );
    #[cfg(feature = "perf_profile")]
    {
      self.profile.plain_reflow_analysis += start.elapsed();
    }
  }

  fn retain_plain_buffer(&mut self, entry: CachedPlainBuffer) {
    #[cfg(feature = "perf_profile")]
    let start = Instant::now();
    if entry.key.text.len() < SHORT_TEXT_BYTES {
      self.shaped_texts.put(entry);
      #[cfg(feature = "perf_profile")]
      {
        self.profile.plain_buffer_retain += start.elapsed();
      }
      return;
    }
    let bytes = entry.bytes;
    // Oversized documents still render correctly, but cannot displace the entire bounded cache.
    if bytes > self.plain_buffer_budget {
      #[cfg(feature = "perf_profile")]
      {
        self.profile.plain_buffer_retain += start.elapsed();
        self.profile.plain_cache_bypasses += 1;
      }
      return;
    }
    // Retain expensive shaping before evicting a document. At fractional DPI the
    // logical measurement and physical paint layouts may not both fit in full.
    // Wrapping is rebuilt lazily if a compacted entry is used again.
    if self.plain_buffers.len() < PLAIN_BUFFER_CACHE_LIMIT {
      for entry in &mut self.plain_buffers {
        if self.plain_buffer_bytes + bytes <= self.plain_buffer_budget {
          break;
        }
        if entry.layout_ready {
          for (line, paragraph) in entry.buffer.lines.iter_mut().zip(&mut entry.paragraphs) {
            line.reset_layout();
            paragraph.layout_bytes = 0;
            paragraph.carets = None;
            paragraph.reflow = None;
          }
          entry.layout_ready = false;
          let old_bytes = entry.bytes;
          entry.refresh_bytes();
          self.plain_buffer_bytes -= old_bytes - entry.bytes;
          #[cfg(feature = "perf_profile")]
          {
            self.profile.plain_cache_compactions += 1;
          }
        }
      }
    }
    while self.plain_buffers.len() >= PLAIN_BUFFER_CACHE_LIMIT
      || self.plain_buffer_bytes + bytes > self.plain_buffer_budget
    {
      self.plain_buffer_bytes -= self.plain_buffers.remove(0).bytes;
      #[cfg(feature = "perf_profile")]
      {
        self.profile.plain_cache_evictions += 1;
      }
    }
    self.plain_buffers.push(entry);
    self.plain_buffer_bytes += bytes;
    #[cfg(feature = "perf_profile")]
    {
      self.profile.plain_buffer_retain += start.elapsed();
    }
  }

  fn acquire_buffer(&mut self, style: &TextStyle, max_width: f32, wrap: bool) -> Buffer {
    let metrics = Metrics::new(style.font_size, style.font_size * style.line_height);
    let wrap = effective_text_wrap(max_width, wrap);
    let mut buffer = self
      .buffer_pool
      .pop()
      .unwrap_or_else(|| Buffer::new(&mut self.font_system, metrics));
    // This buffer is about to receive new text; drop the old lines so they are never shaped.
    buffer.lines.clear();
    buffer.set_metrics(metrics);
    buffer.set_size(text_buffer_width(max_width), None);
    buffer.set_wrap(if wrap { Wrap::WordOrGlyph } else { Wrap::None });
    buffer
  }

  /// Shapes `spans` at `max_width`: their size, glyphs and span boxes, and
  /// their carets when `with_carets` asks for them, all from one shaping.
  fn shape_rich_text_layout(
    &mut self,
    spans: &[RichTextSpan],
    max_width: f32,
    wrap: bool,
    with_carets: bool,
  ) -> CachedRichShapedLayout {
    let Some(first) = spans.first() else {
      return CachedRichShapedLayout {
        size: Size::default(),
        glyphs: Vec::<CachedRichShapedGlyph>::new().into(),
        span_boxes: Vec::<SpanBox>::new().into(),
        carets: std::sync::Arc::default(),
      };
    };
    #[cfg(feature = "perf_profile")]
    let profile_start = Instant::now();

    #[cfg(feature = "perf_profile")]
    let phase_start = Instant::now();
    let mut buffer = self.acquire_buffer(&first.style, max_width, wrap);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.rich_acquire_buffer += phase_start.elapsed();
    }

    #[cfg(feature = "perf_profile")]
    let phase_start = Instant::now();
    self.set_rich_buffer_text(&mut buffer, spans);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.rich_set_text += phase_start.elapsed();
    }

    #[cfg(feature = "perf_profile")]
    let phase_start = Instant::now();
    buffer.shape_until_scroll(&mut self.font_system, false);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.rich_cosmic_shape += phase_start.elapsed();
    }

    #[cfg(feature = "perf_profile")]
    let phase_start = Instant::now();
    let size = measure_buffer(&buffer, first.style.font_size * first.style.line_height);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.rich_measure += phase_start.elapsed();
    }

    #[cfg(feature = "perf_profile")]
    let phase_start = Instant::now();
    let mut glyphs = Vec::new();
    for run in buffer.layout_runs() {
      for glyph in run.glyphs.iter() {
        if glyph_cluster_is_whitespace(run.text, glyph) {
          continue;
        }
        let physical = self.hinted_fonts.physical(glyph, run.line_y);
        glyphs.push(CachedRichShapedGlyph {
          x: physical.x as f32,
          y: physical.y as f32,
          cache_key: physical.cache_key,
          color: glyph_color(glyph.color_opt, first.style.color),
        });
      }
    }
    #[cfg(feature = "perf_profile")]
    {
      self.profile.rich_extract += phase_start.elapsed();
    }

    let span_boxes = span_boxes::span_boxes(&buffer, &mut self.font_system, &mut self.underline_metrics);
    let carets = std::sync::OnceLock::new();
    if with_carets {
      let text_len = spans.iter().map(|span| span.text.len()).sum();
      carets.get_or_init(|| carets::buffer_caret_positions(&buffer, text_len));
    }
    self.buffer_pool.push(buffer);
    #[cfg(feature = "perf_profile")]
    {
      self.profile.shape_rich_text += profile_start.elapsed();
    }
    CachedRichShapedLayout {
      size,
      glyphs: glyphs.into(),
      span_boxes,
      carets: std::sync::Arc::new(carets),
    }
  }

  fn pack_rich_shaped_layout(&mut self, layout: &CachedRichShapedLayout) -> Vec<CachedRichGlyph> {
    #[cfg(feature = "perf_profile")]
    let profile_start = Instant::now();
    let mut cached = Vec::with_capacity(layout.glyphs.len());
    for glyph in layout.glyphs.iter() {
      let Some(packed) = self.get_or_pack_glyph(glyph.cache_key) else {
        continue;
      };
      cached.push(CachedRichGlyph {
        glyph: CachedGlyph {
          x: glyph.x + packed.left as f32,
          y: glyph.y - packed.top as f32,
          atlas_x: packed.x,
          atlas_y: packed.y,
          width: packed.width,
          height: packed.height,
          is_color: packed.is_color,
        },
        color: glyph.color,
      });
    }
    #[cfg(feature = "perf_profile")]
    {
      self.profile.pack_rich_shaped += profile_start.elapsed();
    }
    cached
  }

  /// The cached layout of `spans` with their pixel sizes scaled by `scale`.
  fn find_rich_shaped_layout(
    &self,
    fingerprint: u64,
    spans: &[RichTextSpan],
    scale: f32,
    max_width: f32,
    wrap: bool,
  ) -> Option<CachedRichShapedLayout> {
    self
      .rich_shaped_layout_cache
      .get(&fingerprint)?
      .iter()
      .find(|(key, _)| key.matches(spans, scale, max_width, wrap))
      .map(|(_, layout)| layout.clone())
  }

  fn find_rich_glyph_layout(
    &self,
    fingerprint: u64,
    spans: &[RichTextSpan],
    max_width: f32,
    wrap: bool,
    snap_to_pixel: bool,
  ) -> Option<&[CachedRichGlyph]> {
    self
      .rich_glyph_layout_cache
      .get(&fingerprint)?
      .iter()
      .find(|(key, _)| key.matches(spans, max_width, wrap, snap_to_pixel))
      .map(|(_, glyphs)| glyphs.as_slice())
  }

  fn set_rich_buffer_text(&mut self, buffer: &mut Buffer, spans: &[RichTextSpan]) {
    let Some(first) = spans.first() else {
      return;
    };
    #[cfg(feature = "perf_profile")]
    {
      self.profile.rich_text_loads += 1;
      self.profile.rich_loaded_spans += spans.len();
      self.profile.rich_loaded_bytes += spans.iter().map(|span| span.text.len()).sum::<usize>();
      if spans.len() == 1 {
        self.profile.rich_single_span_loads += 1;
      } else {
        self.profile.rich_multi_span_loads += 1;
      }
    }

    if spans.len() == 1 {
      #[cfg(feature = "perf_profile")]
      let phase_start = Instant::now();
      if self.font_aliases.is_empty() {
        let weight = self.face_weight(first.style.font_family.as_ref(), &first.style);
        let attrs = attrs_for_style(
          &first.style,
          first.style.font_family.as_ref(),
          weight,
          first.style.font_size,
        );
        #[cfg(feature = "perf_profile")]
        {
          self.profile.rich_prepare_spans += phase_start.elapsed();
        }

        #[cfg(feature = "perf_profile")]
        let phase_start = Instant::now();
        set_buffer_text(buffer, &first.text, attrs, first.style.text_align);
        #[cfg(feature = "perf_profile")]
        {
          self.profile.rich_buffer_set_text += phase_start.elapsed();
        }
      } else {
        let family = self.resolve_family(&first.style);
        let weight = self.face_weight(&family, &first.style);
        let attrs = attrs_for_style(&first.style, &family, weight, first.style.font_size);
        #[cfg(feature = "perf_profile")]
        {
          self.profile.rich_prepare_spans += phase_start.elapsed();
        }

        #[cfg(feature = "perf_profile")]
        let phase_start = Instant::now();
        set_buffer_text(buffer, &first.text, attrs, first.style.text_align);
        #[cfg(feature = "perf_profile")]
        {
          self.profile.rich_buffer_set_text += phase_start.elapsed();
        }
      }
      return;
    }

    if self.font_aliases.is_empty() {
      #[cfg(feature = "perf_profile")]
      let phase_start = Instant::now();
      let rich_spans: Vec<_> = spans
        .iter()
        .enumerate()
        .map(|(index, span)| {
          (
            span.text.as_str(),
            attrs_for_style(
              &span.style,
              span.style.font_family.as_ref(),
              self.face_weight(span.style.font_family.as_ref(), &span.style),
              first.style.font_size,
            )
            .metadata(index),
          )
        })
        .collect();
      let default_weight = self.face_weight(first.style.font_family.as_ref(), &first.style);
      let default_attrs = attrs_for_style(
        &first.style,
        first.style.font_family.as_ref(),
        default_weight,
        first.style.font_size,
      );
      #[cfg(feature = "perf_profile")]
      {
        self.profile.rich_prepare_spans += phase_start.elapsed();
      }

      #[cfg(feature = "perf_profile")]
      let phase_start = Instant::now();
      buffer.set_rich_text(rich_spans, &default_attrs, Shaping::Advanced, None);
      #[cfg(feature = "perf_profile")]
      {
        self.profile.rich_buffer_set_text += phase_start.elapsed();
      }
    } else {
      #[cfg(feature = "perf_profile")]
      let phase_start = Instant::now();
      let families: Vec<_> = spans.iter().map(|span| self.resolve_family(&span.style)).collect();
      let rich_spans: Vec<_> = spans
        .iter()
        .zip(families.iter())
        .enumerate()
        .map(|(index, (span, family))| {
          (
            span.text.as_str(),
            attrs_for_style(
              &span.style,
              family,
              self.face_weight(family, &span.style),
              first.style.font_size,
            )
            .metadata(index),
          )
        })
        .collect();
      let default_family = self.resolve_family(&first.style);
      let default_weight = self.face_weight(&default_family, &first.style);
      let default_attrs = attrs_for_style(&first.style, &default_family, default_weight, first.style.font_size);
      #[cfg(feature = "perf_profile")]
      {
        self.profile.rich_prepare_spans += phase_start.elapsed();
      }

      #[cfg(feature = "perf_profile")]
      let phase_start = Instant::now();
      buffer.set_rich_text(rich_spans, &default_attrs, Shaping::Advanced, None);
      #[cfg(feature = "perf_profile")]
      {
        self.profile.rich_buffer_set_text += phase_start.elapsed();
      }
    }
    #[cfg(feature = "perf_profile")]
    let phase_start = Instant::now();
    for line in &mut buffer.lines {
      line.set_align(Some(first.style.text_align.to_cosmic()));
    }
    #[cfg(feature = "perf_profile")]
    {
      self.profile.rich_align_lines += phase_start.elapsed();
    }
  }

  fn push_glyph_cmd(
    &self,
    out: &mut Vec<GlyphCmd>,
    x: f32,
    y: f32,
    packed: PackedGlyph,
    color: [f32; 4],
    snap_to_pixel: bool,
  ) {
    out.push(GlyphCmd {
      order: 0,
      x: if snap_to_pixel { x.round() } else { x },
      y: if snap_to_pixel { y.round() } else { y },
      width: packed.width as f32,
      height: packed.height as f32,
      color,
      atlas_min: [packed.x as f32, packed.y as f32],
      atlas_max: [(packed.x + packed.width) as f32, (packed.y + packed.height) as f32],
      transform: [1.0, 0.0, 0.0, 1.0],
      transform_origin: [0.0, 0.0],
      sharpness: 1.0,
      color_glyph: packed.is_color,
      shadow_sigma: 0.0,
      clip: crate::layout::quad::ClipRect::default(),
    });
  }

  /// Weight of the loaded face nearest to `style.weight` in `family`; see [`FaceWeights`].
  fn face_weight(&mut self, family: &str, style: &TextStyle) -> cosmic_text::Weight {
    // A person who asked the system for bold text has every text two weights heavier, whatever set its
    // weight.
    let weight = if self.bold_text {
      crate::layout::text_style::FontWeight::Numeric((style.weight.value() + 200).min(900))
    } else {
      style.weight
    };
    self
      .face_weights
      .resolve(self.font_system.db(), family, weight, style.style)
  }

  /// Draws every text heavier, or as it is set: the theme's bold text. The text laid out before is laid
  /// out anew.
  pub(crate) fn set_bold_text(&mut self, bold: bool) {
    if self.bold_text != bold {
      self.bold_text = bold;
      self.clear_text_caches();
    }
  }

  fn style_attrs<'a>(&mut self, style: &TextStyle, resolved_family: &'a str) -> Attrs<'a> {
    let weight = self.face_weight(resolved_family, style);
    plain_attrs(style, resolved_family, weight, style.font_size)
  }

  fn resolve_family(&self, style: &TextStyle) -> std::sync::Arc<str> {
    self
      .font_aliases
      .get(alias_name(&style.font_family))
      .map(|s| std::sync::Arc::from(s.as_str()))
      .unwrap_or_else(|| style.font_family.clone())
  }

  #[cfg_attr(not(feature = "perf_profile"), allow(dead_code))]
  pub(crate) fn estimated_memory_bytes(&self) -> usize {
    let alias_heap = self
      .font_aliases
      .iter()
      .map(|(name, family)| name.capacity() + family.capacity())
      .sum::<usize>();
    let rich_shaped_key_heap = self
      .rich_shaped_layout_cache
      .values()
      .flat_map(|bucket| bucket.iter().flat_map(|(key, _)| key.spans.iter()))
      .map(|span| span.text.capacity() + span.font_family.len())
      .sum::<usize>();
    let rich_shaped_layout_cache_bytes = self
      .rich_shaped_layout_cache
      .values()
      .flat_map(|bucket| bucket.iter().map(|(_, layout)| layout))
      .map(|layout| {
        layout.glyphs.len() * std::mem::size_of::<CachedRichShapedGlyph>()
          + layout.span_boxes.len() * std::mem::size_of::<SpanBox>()
      })
      .sum::<usize>();
    let clipped_key_heap = self
      .clipped_glyph_layout_cache
      .keys()
      .map(|key| key.base.text.capacity() + key.base.font_family.len())
      .sum::<usize>();
    let clipped_glyph_layout_cache_bytes = self
      .clipped_glyph_layout_cache
      .values()
      .map(|glyphs| glyphs.capacity() * std::mem::size_of::<CachedGlyph>())
      .sum::<usize>();
    let rich_key_heap = self
      .rich_glyph_layout_cache
      .values()
      .flat_map(|bucket| bucket.iter().flat_map(|(key, _)| key.spans.iter()))
      .map(|span| span.text.capacity() + span.font_family.len())
      .sum::<usize>();
    let rich_glyph_layout_cache_bytes = self
      .rich_glyph_layout_cache
      .values()
      .flat_map(|bucket| bucket.iter().map(|(_, glyphs)| glyphs))
      .map(|glyphs| glyphs.capacity() * std::mem::size_of::<CachedRichGlyph>())
      .sum::<usize>();
    let transformed_glyph_layout_cache_bytes = self
      .transformed_glyph_layout_cache
      .values()
      .map(|glyphs| glyphs.capacity() * std::mem::size_of::<CachedTransformedGlyph>())
      .sum::<usize>();
    std::mem::size_of::<Self>()
      + self.plain_buffer_bytes
      + self.shaped_texts.bytes()
      + self.shaped_texts.estimated_overhead_bytes()
      + self.input_cache.estimated_memory_bytes()
      + self.texts.estimated_memory_bytes()
      + self.font_aliases.capacity() * std::mem::size_of::<(String, String)>()
      + alias_heap
      + self.rich_shaped_layout_cache.capacity()
        * std::mem::size_of::<(u64, Vec<(RichTextShapeKey, CachedRichShapedLayout)>)>()
      + self
        .rich_shaped_layout_cache
        .values()
        .map(|bucket| bucket.capacity() * std::mem::size_of::<(RichTextShapeKey, CachedRichShapedLayout)>())
        .sum::<usize>()
      + rich_shaped_key_heap
      + rich_shaped_layout_cache_bytes
      + self.underline_metrics.estimated_memory_bytes()
      + self.clipped_glyph_layout_cache.capacity() * std::mem::size_of::<(ClippedCacheKey, Vec<CachedGlyph>)>()
      + clipped_key_heap
      + clipped_glyph_layout_cache_bytes
      + self.rich_glyph_layout_cache.capacity()
        * std::mem::size_of::<(u64, Vec<(RichTextCacheKey, Vec<CachedRichGlyph>)>)>()
      + self
        .rich_glyph_layout_cache
        .values()
        .map(|bucket| bucket.capacity() * std::mem::size_of::<(RichTextCacheKey, Vec<CachedRichGlyph>)>())
        .sum::<usize>()
      + rich_key_heap
      + rich_glyph_layout_cache_bytes
      + self.transformed_glyph_layout_cache.capacity() * std::mem::size_of::<(CacheKey, Vec<CachedTransformedGlyph>)>()
      + transformed_glyph_layout_cache_bytes
      + self.atlas_entries.capacity() * std::mem::size_of::<(GlyphCacheKey, PackedGlyph)>()
      + self.transformed_atlas_entries.capacity() * std::mem::size_of::<(TransformedGlyphKey, PackedGlyph)>()
      + self.buffer_pool.capacity() * std::mem::size_of::<Buffer>()
  }
}

// Hashing narrows paragraph candidates; callers still compare their complete text.
fn paragraph_fingerprint(text: &str) -> u64 {
  let mut hasher = DefaultHasher::new();
  text.hash(&mut hasher);
  hasher.finish()
}

fn plain_buffer_cache_budget() -> usize {
  // Diagnostic override for comparing budgets in the same dev profiling binary.
  // Ordinary builds, release builds, and unit tests always use the default.
  #[cfg(all(feature = "perf_profile", debug_assertions, not(test)))]
  if let Ok(value) = std::env::var("LURQ_TEXT_CACHE_MIB")
    && let Ok(mib @ 1..=1024) = value.parse::<usize>()
  {
    return mib * 1024 * 1024;
  }
  PLAIN_BUFFER_CACHE_BYTES
}

// Public Cosmic Text storage only; its private scratch/font allocations are not exposed.
fn plain_line_shape_bytes(line: &cosmic_text::BufferLine) -> usize {
  let mut bytes = line.text().len();
  if let Some(shape) = line.shape_opt() {
    bytes += shape.spans.capacity() * std::mem::size_of::<cosmic_text::ShapeSpan>();
    for span in &shape.spans {
      bytes += span.words.capacity() * std::mem::size_of::<cosmic_text::ShapeWord>();
      for word in &span.words {
        bytes += word.glyphs.capacity() * std::mem::size_of::<cosmic_text::ShapeGlyph>();
      }
    }
  }
  bytes
}

fn plain_line_layout_bytes(line: &cosmic_text::BufferLine) -> usize {
  let mut bytes = 0;
  if let Some(layout) = line.layout_opt() {
    bytes += layout.capacity() * std::mem::size_of::<cosmic_text::LayoutLine>();
    for run in layout {
      bytes += run.glyphs.capacity() * std::mem::size_of::<LayoutGlyph>();
    }
  }
  bytes
}

#[cfg(test)]
fn plain_buffer_memory_bytes(buffer: &Buffer) -> usize {
  buffer.lines.capacity() * std::mem::size_of::<cosmic_text::BufferLine>()
    + buffer
      .lines
      .iter()
      .map(|line| plain_line_shape_bytes(line) + plain_line_layout_bytes(line))
      .sum::<usize>()
}

#[derive(Clone, Copy)]
struct PackedGlyph {
  x: u32,
  y: u32,
  width: u32,
  height: u32,
  left: i32,
  top: i32,
  is_color: bool,
}

#[derive(Clone, Copy)]
struct CachedGlyph {
  x: f32,
  y: f32,
  atlas_x: u32,
  atlas_y: u32,
  width: u32,
  height: u32,
  is_color: bool,
}

#[derive(Clone)]
struct CachedRichShapedLayout {
  size: Size,
  glyphs: std::sync::Arc<[CachedRichShapedGlyph]>,
  span_boxes: std::sync::Arc<[SpanBox]>,
  /// The carets of the drawn glyphs, what a selection of the text indexes;
  /// built when a selection first needs them, since most rich text is never
  /// selected.
  carets: std::sync::Arc<std::sync::OnceLock<CaretPositions>>,
}

#[derive(Clone, Copy)]
struct CachedRichShapedGlyph {
  x: f32,
  y: f32,
  cache_key: GlyphCacheKey,
  color: [f32; 4],
}

#[derive(Clone, Copy)]
struct CachedRichGlyph {
  glyph: CachedGlyph,
  color: [f32; 4],
}

#[derive(Clone, Copy)]
struct CachedTransformedGlyph {
  x: f32,
  y: f32,
  cache_key: GlyphCacheKey,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TransformedGlyphKey {
  cache_key: GlyphCacheKey,
  transform: [u32; 4],
}

impl TransformedGlyphKey {
  fn new(cache_key: GlyphCacheKey, transform: SwashTransform) -> Self {
    Self {
      cache_key,
      transform: [
        transform.xx.to_bits(),
        transform.xy.to_bits(),
        transform.yx.to_bits(),
        transform.yy.to_bits(),
      ],
    }
  }
}

fn swash_transform_from_screen(transform: Transform2D) -> SwashTransform {
  SwashTransform::new(transform.a, -transform.b, -transform.c, transform.d, 0.0, 0.0)
}

/// The `LURQ_GLYPH_DEBUG` env marker, read once — a substring of the text runs
/// to trace through emission (see `rasterize_text_with_snap_into`).
/// Render an optional clip rect for the glyph-debug lines.
fn clip_debug_text(clip: Option<ClipRect>) -> String {
  match clip {
    Some(clip) => format!(
      "({}, {}, {}x{}, active={})",
      clip.x, clip.y, clip.width, clip.height, clip.active
    ),
    None => "none".to_owned(),
  }
}

pub(crate) fn glyph_debug_marker() -> Option<String> {
  static MARKER: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
  MARKER
    .get_or_init(|| {
      std::env::var("LURQ_GLYPH_DEBUG")
        .ok()
        .filter(|marker| !marker.is_empty())
    })
    .clone()
}

/// `LURQ_GLYPH_DEBUG_FORCE` — draw-command overrides applied to the marked
/// text runs, to bisect why a well-formed run is invisible: `noclip` clears
/// their clip, `order` lifts them above everything, `both` does both.
pub(crate) fn glyph_debug_force() -> Option<String> {
  static FORCE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
  FORCE
    .get_or_init(|| {
      std::env::var("LURQ_GLYPH_DEBUG_FORCE")
        .ok()
        .filter(|force| !force.is_empty())
    })
    .clone()
}

fn render_glyph_image(context: &mut ScaleContext, font: &Font, cache_key: GlyphCacheKey) -> Option<SwashImage> {
  // Hinting must stay OFF here: cosmic quantizes glyph x positions into
  // quarter-pixel bins, so the same glyph rasterizes at four subpixel phases.
  // TrueType hinting snaps stems to the pixel grid FIRST, and shifting that
  // grid-fit outline by a fraction re-distributes its coverage differently per
  // bin — adjacent instances of one letter come out with visibly different
  // weight and geometry, which reads as jumpy, uneven text at UI sizes.
  // Unhinted outlines sample the same smooth shape at every phase, so all
  // instances match (the transformed-glyph path below renders unhinted for the
  // same reason, as do DirectWrite's "natural" modes, which pair subpixel
  // positioning with unhinted horizontal metrics). A font loaded as hinted does
  // not come here from a pixel-snapped run: `hinted` places and draws its glyphs.
  let mut scaler = scaler::unhinted_scaler(context, font, cache_key).build();
  let offset = Vector::new(cache_key.x_bin.as_float(), cache_key.y_bin.as_float());
  let transform = cache_key
    .flags
    .contains(cosmic_text::CacheKeyFlags::FAKE_ITALIC)
    .then(|| SwashTransform::skew(Angle::from_degrees(14.0), Angle::from_degrees(0.0)));

  let image = Render::new(&[
    Source::ColorOutline(0),
    Source::ColorBitmap(StrikeWith::BestFit),
    Source::Outline,
  ])
  .format(Format::Alpha)
  .offset(offset)
  .transform(transform)
  .render(&mut scaler, cache_key.glyph_id);
  // An emoji's picture on iOS is in a format swash does not read: the system draws it; see `ios_emoji`.
  #[cfg(target_os = "ios")]
  let image = if ios_emoji::has_pictures(font) && image.as_ref().is_none_or(ios_emoji::is_blank) {
    ios_emoji::draw(cache_key.glyph_id, f32::from_bits(cache_key.font_size_bits)).or(image)
  } else {
    image
  };
  image
}

fn is_bounded_text_width(max_width: f32) -> bool {
  max_width.is_finite() && max_width < f32::MAX
}

fn effective_text_wrap(max_width: f32, wrap: bool) -> bool {
  wrap && is_bounded_text_width(max_width)
}

fn text_buffer_width(max_width: f32) -> Option<f32> {
  is_bounded_text_width(max_width).then_some(max_width)
}

/// `em_px` is the buffer's font size; see [`with_letter_spacing`].
fn attrs_for_style<'a>(
  style: &TextStyle,
  resolved_family: &'a str,
  weight: cosmic_text::Weight,
  em_px: f32,
) -> Attrs<'a> {
  plain_attrs(style, resolved_family, weight, em_px).color(CosmicColor::rgba(
    style.color.r(),
    style.color.g(),
    style.color.b(),
    style.color.a(),
  ))
}

/// `weight` is the matched face weight from [`FaceWeights`], not the requested
/// one; `em_px` is the buffer's font size.
fn plain_attrs<'a>(style: &TextStyle, resolved_family: &'a str, weight: cosmic_text::Weight, em_px: f32) -> Attrs<'a> {
  let mut attrs = Attrs::new()
    .family(cosmic_family(resolved_family))
    .weight(weight)
    .style(style.style.to_cosmic());
  if !style.font_features.is_empty() {
    attrs = attrs.font_features(style.font_features.to_cosmic());
  }
  with_letter_spacing(attrs, style.letter_spacing, em_px)
}

fn glyph_color(color: Option<CosmicColor>, default: Color) -> [f32; 4] {
  color
    .map(|color| Color::new(color.r(), color.g(), color.b(), color.a()).to_linear_f32_array())
    .unwrap_or_else(|| default.to_linear_f32_array())
}

fn glyph_cluster_is_whitespace(line_text: &str, glyph: &LayoutGlyph) -> bool {
  cluster_is_whitespace(line_text, glyph.start, glyph.end)
}

/// Whether the source cluster `[start, end)` is non-empty and entirely
/// whitespace. An empty range slices to "", and `"".chars().all(..)` is
/// vacuously true — which would misclassify a glyph with no source bytes as
/// whitespace and skip drawing it (its advance stays, leaving a gap mid-word),
/// so an empty cluster must not count as whitespace.
fn cluster_is_whitespace(line_text: &str, start: usize, end: usize) -> bool {
  match line_text.get(start..end) {
    Some(cluster) => !cluster.is_empty() && cluster.chars().all(char::is_whitespace),
    None => false,
  }
}

#[cfg_attr(not(feature = "markdown"), allow(dead_code))]
fn measure_buffer(buffer: &Buffer, fallback_line_height: f32) -> Size {
  let mut width = 0.0_f32;
  let mut first_line_y = 0.0_f32;
  let mut last_line_y = 0.0_f32;
  let mut last_line_height = fallback_line_height;
  let mut has_runs = false;
  for run in buffer.layout_runs() {
    width = width.max(run.line_w);
    if !has_runs {
      first_line_y = run.line_y;
      has_runs = true;
    }
    last_line_y = run.line_y;
    last_line_height = run.line_height;
  }
  let height = if has_runs {
    last_line_y - first_line_y + last_line_height
  } else {
    0.0
  };
  Size::new(width, height)
}

/// The glyph commands of a text painted at whole pixels from (`origin_x`,
/// `origin_y`): those of `cached` that `clip` does not cut away, each with
/// `order` and `clip` set. What `append_glyph_cmds_from_cached` appends for
/// snapped glyphs, written once.
fn append_painted_glyph_cmds(
  cached: &[CachedGlyph],
  origin_x: f32,
  origin_y: f32,
  style: &TextStyle,
  clip: ClipRect,
  order: usize,
  out: &mut Vec<GlyphCmd>,
) {
  let color = style.color.to_linear_f32_array();
  out.reserve(cached.len());
  for glyph in cached {
    let x = origin_x + glyph.x;
    let y = origin_y + glyph.y;
    if !glyph_rect_intersects_clip(x, y, glyph.width as f32, glyph.height as f32, Some(clip)) {
      continue;
    }
    out.push(GlyphCmd {
      order,
      x: x.round(),
      y: y.round(),
      width: glyph.width as f32,
      height: glyph.height as f32,
      color,
      atlas_min: [glyph.atlas_x as f32, glyph.atlas_y as f32],
      atlas_max: [
        (glyph.atlas_x + glyph.width) as f32,
        (glyph.atlas_y + glyph.height) as f32,
      ],
      transform: [1.0, 0.0, 0.0, 1.0],
      transform_origin: [0.0, 0.0],
      sharpness: 1.0,
      color_glyph: glyph.is_color,
      shadow_sigma: 0.0,
      clip,
    });
  }
}

fn append_glyph_cmds_from_cached(
  cached: &[CachedGlyph],
  origin_x: f32,
  origin_y: f32,
  style: &TextStyle,
  _atlas_w: f32,
  _atlas_h: f32,
  snap_to_pixel: bool,
  clip: Option<ClipRect>,
  out: &mut Vec<GlyphCmd>,
) {
  let color = style.color.to_linear_f32_array();
  out.reserve(cached.len());
  for glyph in cached {
    let x = origin_x + glyph.x;
    let y = origin_y + glyph.y;
    if !glyph_rect_intersects_clip(x, y, glyph.width as f32, glyph.height as f32, clip) {
      continue;
    }
    out.push(GlyphCmd {
      order: 0,
      x: if snap_to_pixel { x.round() } else { x },
      y: if snap_to_pixel { y.round() } else { y },
      width: glyph.width as f32,
      height: glyph.height as f32,
      color,
      atlas_min: [glyph.atlas_x as f32, glyph.atlas_y as f32],
      atlas_max: [
        (glyph.atlas_x + glyph.width) as f32,
        (glyph.atlas_y + glyph.height) as f32,
      ],
      transform: [1.0, 0.0, 0.0, 1.0],
      transform_origin: [0.0, 0.0],
      sharpness: 1.0,
      color_glyph: glyph.is_color,
      shadow_sigma: 0.0,
      clip: crate::layout::quad::ClipRect::default(),
    });
  }
}

/// Height to subtract from a measured text box for `text-box-trim` (see
/// [`TextStyle::trim_line_box`]). `line_height * font_size` puts half the extra
/// leading above the run and half below (cosmic centers each line); trimming
/// both collapses a single line to the em box (`font_size`) while leaving the
/// leading BETWEEN wrapped lines intact. Zero when trim is off or `line_height`
/// is already at/under 1.0 (no extra leading to reclaim).
fn line_box_trim(style: &TextStyle) -> f32 {
  if !style.trim_line_box {
    return 0.0;
  }
  (style.font_size * (style.line_height - 1.0)).max(0.0)
}

fn text_run_intersects_clip(origin_y: f32, line_top: f32, line_height: f32, clip: Option<ClipRect>) -> bool {
  let Some(clip) = clip.filter(|clip| clip.active) else {
    return true;
  };
  let y = origin_y + line_top;
  y < clip.y + clip.height && y + line_height > clip.y
}

fn glyph_rect_intersects_clip(x: f32, y: f32, width: f32, height: f32, clip: Option<ClipRect>) -> bool {
  let Some(clip) = clip.filter(|clip| clip.active) else {
    return true;
  };
  x < clip.x + clip.width && x + width > clip.x && y < clip.y + clip.height && y + height > clip.y
}

/// Whether `visited` layout runs were every row of `buffer`. Its scroll is at
/// the top, so the runs end early only at a height limit or an unshaped line.
fn runs_cover_buffer(buffer: &Buffer, visited: usize) -> bool {
  let rows = buffer
    .lines
    .iter()
    .try_fold(0, |rows, line| line.layout_opt().map(|layout| rows + layout.len()));
  rows == Some(visited)
}

fn clipped_raster_shape_height(origin_y: f32, style: &TextStyle, clip: Option<ClipRect>) -> Option<f32> {
  let clip = clip.filter(|clip| clip.active)?;
  let local_bottom = clip.y + clip.height - origin_y;
  let line_slop = style.font_size * style.line_height;
  (local_bottom > 0.0).then_some(local_bottom + line_slop)
}

fn append_rich_glyph_cmds_from_cached(
  cached: &[CachedRichGlyph],
  origin_x: f32,
  origin_y: f32,
  _atlas_w: f32,
  _atlas_h: f32,
  snap_to_pixel: bool,
  out: &mut Vec<GlyphCmd>,
) {
  out.reserve(cached.len());
  for rich_glyph in cached {
    let glyph = rich_glyph.glyph;
    let x = origin_x + glyph.x;
    let y = origin_y + glyph.y;
    out.push(GlyphCmd {
      order: 0,
      x: if snap_to_pixel { x.round() } else { x },
      y: if snap_to_pixel { y.round() } else { y },
      width: glyph.width as f32,
      height: glyph.height as f32,
      color: rich_glyph.color,
      atlas_min: [glyph.atlas_x as f32, glyph.atlas_y as f32],
      atlas_max: [
        (glyph.atlas_x + glyph.width) as f32,
        (glyph.atlas_y + glyph.height) as f32,
      ],
      transform: [1.0, 0.0, 0.0, 1.0],
      transform_origin: [0.0, 0.0],
      sharpness: 1.0,
      color_glyph: glyph.is_color,
      shadow_sigma: 0.0,
      clip: crate::layout::quad::ClipRect::default(),
    });
  }
}

#[allow(clippy::too_many_arguments)]
fn transformed_glyph_origin(
  origin_x: f32,
  origin_y: f32,
  glyph_x: f32,
  glyph_y: f32,
  transform: Transform2D,
  transform_origin: [f32; 2],
) -> (f32, f32) {
  let glyph_origin_x = origin_x + glyph_x;
  let glyph_origin_y = origin_y + glyph_y;
  (
    transform_origin[0]
      + transform.a * (glyph_origin_x - transform_origin[0])
      + transform.c * (glyph_origin_y - transform_origin[1]),
    transform_origin[1]
      + transform.b * (glyph_origin_x - transform_origin[0])
      + transform.d * (glyph_origin_y - transform_origin[1]),
  )
}

#[cfg(test)]
fn glyph_coverage_mask(image: &SwashImage) -> Cow<'_, [u8]> {
  match image.content {
    SwashContent::Mask => Cow::Borrowed(&image.data),
    SwashContent::Color => Cow::Owned(image.data.chunks_exact(4).map(|rgba| rgba[3]).collect::<Vec<_>>()),
    SwashContent::SubpixelMask => Cow::Owned(
      image
        .data
        .chunks_exact(4)
        .map(|rgba| {
          let coverage = rgba[0] as u16 + rgba[1] as u16 + rgba[2] as u16;
          (coverage / 3) as u8
        })
        .collect::<Vec<_>>(),
    ),
  }
}

#[cfg(test)]
fn glyph_atlas_pixels(image: &SwashImage) -> (Cow<'_, [u8]>, bool) {
  match image.content {
    SwashContent::Color => (Cow::Borrowed(&image.data), true),
    SwashContent::Mask => (Cow::Owned(alpha_to_rgba(&image.data)), false),
    SwashContent::SubpixelMask => {
      let alpha = image
        .data
        .chunks_exact(3)
        .map(|rgb| {
          let coverage = rgb[0] as u16 + rgb[1] as u16 + rgb[2] as u16;
          (coverage / 3) as u8
        })
        .collect::<Vec<_>>();
      (Cow::Owned(alpha_to_rgba(&alpha)), false)
    }
  }
}

#[cfg(test)]
fn alpha_to_rgba(alpha: &[u8]) -> Vec<u8> {
  let mut rgba = Vec::with_capacity(alpha.len() * 4);
  for coverage in alpha {
    rgba.extend_from_slice(&[255, 255, 255, *coverage]);
  }
  rgba
}

fn write_alpha_row_as_rgba(data: &mut [u8], dst_start: usize, alpha: &[u8]) {
  for (index, coverage) in alpha.iter().enumerate() {
    let dst = dst_start + index * GLYPH_ATLAS_BYTES_PER_PIXEL;
    if dst + 4 > data.len() {
      break;
    }
    data[dst..dst + 4].copy_from_slice(&[255, 255, 255, *coverage]);
  }
}

fn write_subpixel_row_as_rgba(data: &mut [u8], dst_start: usize, subpixel: &[u8]) {
  for (index, rgb) in subpixel.chunks_exact(3).enumerate() {
    let dst = dst_start + index * GLYPH_ATLAS_BYTES_PER_PIXEL;
    if dst + 4 > data.len() {
      break;
    }
    let coverage = (rgb[0] as u16 + rgb[1] as u16 + rgb[2] as u16) / 3;
    data[dst..dst + 4].copy_from_slice(&[255, 255, 255, coverage as u8]);
  }
}

/// A font system over `db` with the platform's fallback fonts.
fn platform_font_system(locale: String, db: cosmic_text::fontdb::Database) -> FontSystem {
  #[cfg(target_os = "ios")]
  return FontSystem::new_with_locale_and_db_and_fallback(locale, db, ios_fallback::IosFallback);
  #[cfg(not(target_os = "ios"))]
  FontSystem::new_with_locale_and_db(locale, db)
}

pub(crate) fn load_platform_fonts(font_system: &mut FontSystem) {
  #[cfg(target_os = "windows")]
  {
    for file in [
      "C:\\Windows\\Fonts\\segoeui.ttf",
      "C:\\Windows\\Fonts\\segoeuib.ttf",
      "C:\\Windows\\Fonts\\arial.ttf",
      "C:\\Windows\\Fonts\\arialbd.ttf",
    ] {
      let _ = font_system.db_mut().load_font_file(file);
    }
  }

  // fontdb knows the font directories of macOS, not of iOS. A simulator runs on the host, where the runtime's
  // files are under `IPHONE_SIMULATOR_ROOT`.
  #[cfg(target_os = "ios")]
  {
    let root = std::env::var_os("IPHONE_SIMULATOR_ROOT").map_or_else(|| "/".into(), std::path::PathBuf::from);
    font_system.db_mut().load_fonts_dir(root.join("System/Library/Fonts"));
  }

  #[cfg(not(any(target_os = "windows", target_os = "ios")))]
  {
    let _ = font_system;
  }
}

/// Upper bound on retained dirty-log entries; consumers further behind than
/// the pruned tail fall back to a full atlas upload.
const GLYPH_ATLAS_DIRTY_LOG_LIMIT: usize = 512;

pub(crate) struct AtlasPacker {
  /// The atlas's pixels, shared with the snapshots `to_atlas` hands out. A
  /// glyph is packed in place while no snapshot is held, and into a copy
  /// otherwise (`pixels_mut`), so a snapshot never changes under its holder.
  pub data: std::sync::Arc<[u8]>,
  pub width: u32,
  pub height: u32,
  cursor_x: u32,
  cursor_y: u32,
  row_height: u32,
  version: u64,
  /// Version-tagged log of packed rects (see [`GlyphAtlas::dirty_rects`]).
  /// Retained — not drained per snapshot — because multiple windows consume
  /// the same atlas from different uploaded versions.
  dirty_rects: std::collections::VecDeque<GlyphAtlasDirtyRect>,
  /// Versions at or before this are no longer covered by the log.
  dirty_from_version: u64,
  /// The dirty log as of `snapshot_version`, shared by the snapshots.
  snapshot_rects: std::sync::Arc<[GlyphAtlasDirtyRect]>,
  snapshot_version: u64,
}

impl AtlasPacker {
  pub(crate) fn new() -> Self {
    let width = 1024;
    let height = 1024;
    Self {
      data: vec![0u8; (width * height) as usize * GLYPH_ATLAS_BYTES_PER_PIXEL].into(),
      width,
      height,
      cursor_x: 0,
      cursor_y: 0,
      row_height: 0,
      version: 0,
      dirty_rects: std::collections::VecDeque::new(),
      dirty_from_version: 0,
      snapshot_rects: Vec::new().into(),
      snapshot_version: u64::MAX,
    }
  }

  /// The pixels to pack a glyph into. They are the atlas's own while no
  /// snapshot of it is held; a held snapshot keeps the pixels it was taken
  /// with, and packing goes on in a copy of them.
  fn pixels_mut(&mut self) -> &mut [u8] {
    if std::sync::Arc::get_mut(&mut self.data).is_none() {
      self.data = std::sync::Arc::from(&self.data[..]);
    }
    std::sync::Arc::get_mut(&mut self.data).expect("the atlas's pixels were just made its own")
  }

  #[cfg(test)]
  pub(crate) fn pack(&mut self, glyph_data: &[u8], gw: u32, gh: u32) -> (f32, f32, f32, f32) {
    let (x0, y0, gw, gh) = self.pack_pixels(glyph_data, gw, gh);
    let u0 = x0 as f32 / self.width as f32;
    let v0 = y0 as f32 / self.height as f32;
    let u1 = (x0 + gw) as f32 / self.width as f32;
    let v1 = (y0 + gh) as f32 / self.height as f32;
    (u0, v0, u1, v1)
  }

  #[cfg(test)]
  fn pack_pixels(&mut self, glyph_data: &[u8], gw: u32, gh: u32) -> (u32, u32, u32, u32) {
    let (x0, y0, reserved_width, reserved_height) = self.reserve_pixels(gw, gh);
    let padding = GLYPH_ATLAS_PADDING;
    let width = self.width;
    let data = self.pixels_mut();

    for row in 0..gh {
      let src_start = (row * gw) as usize * GLYPH_ATLAS_BYTES_PER_PIXEL;
      let src_end = src_start + gw as usize * GLYPH_ATLAS_BYTES_PER_PIXEL;
      let dst_start = ((y0 + padding + row) * width + x0 + padding) as usize * GLYPH_ATLAS_BYTES_PER_PIXEL;
      let dst_end = dst_start + gw as usize * GLYPH_ATLAS_BYTES_PER_PIXEL;
      if src_end <= glyph_data.len() && dst_end <= data.len() {
        data[dst_start..dst_end].copy_from_slice(&glyph_data[src_start..src_end]);
      }
    }

    self.record_packed_rect(x0, y0, reserved_width, reserved_height);
    (x0, y0, reserved_width, reserved_height)
  }

  fn pack_image(&mut self, image: &SwashImage) -> (u32, u32, u32, u32, bool) {
    let gw = image.placement.width;
    let gh = image.placement.height;
    let (x0, y0, reserved_width, reserved_height) = self.reserve_pixels(gw, gh);
    let padding = GLYPH_ATLAS_PADDING;
    let is_color = matches!(image.content, SwashContent::Color);
    let width = self.width;
    let data = self.pixels_mut();

    for row in 0..gh {
      let dst_start = ((y0 + padding + row) * width + x0 + padding) as usize * GLYPH_ATLAS_BYTES_PER_PIXEL;
      match image.content {
        SwashContent::Color => {
          let src_start = (row * gw) as usize * GLYPH_ATLAS_BYTES_PER_PIXEL;
          let src_end = src_start + gw as usize * GLYPH_ATLAS_BYTES_PER_PIXEL;
          let dst_end = dst_start + gw as usize * GLYPH_ATLAS_BYTES_PER_PIXEL;
          if src_end <= image.data.len() && dst_end <= data.len() {
            data[dst_start..dst_end].copy_from_slice(&image.data[src_start..src_end]);
          }
        }
        SwashContent::Mask => {
          let src_start = (row * gw) as usize;
          let src_end = src_start + gw as usize;
          if src_end <= image.data.len() {
            write_alpha_row_as_rgba(data, dst_start, &image.data[src_start..src_end]);
          }
        }
        SwashContent::SubpixelMask => {
          let src_start = (row * gw) as usize * 3;
          let src_end = src_start + gw as usize * 3;
          if src_end <= image.data.len() {
            write_subpixel_row_as_rgba(data, dst_start, &image.data[src_start..src_end]);
          }
        }
      }
    }

    self.record_packed_rect(x0, y0, reserved_width, reserved_height);
    (x0, y0, reserved_width, reserved_height, is_color)
  }

  fn reserve_pixels(&mut self, gw: u32, gh: u32) -> (u32, u32, u32, u32) {
    let padding = GLYPH_ATLAS_PADDING;
    let reserved_width = gw + padding * 2;
    let reserved_height = gh + padding * 2;

    if self.cursor_x + reserved_width > self.width {
      self.cursor_x = 0;
      self.cursor_y += self.row_height;
      self.row_height = 0;
    }

    if self.cursor_y + reserved_height > self.height {
      let new_height = self.height * 2;
      // The taller atlas: the rows there are, over transparent ones.
      let grown_len = (self.width * new_height) as usize * GLYPH_ATLAS_BYTES_PER_PIXEL;
      let mut grown = Vec::with_capacity(grown_len);
      grown.extend_from_slice(&self.data);
      grown.resize(grown_len, 0);
      self.data = grown.into();
      self.height = new_height;
    }

    let padded_x = self.cursor_x;
    let padded_y = self.cursor_y;
    let x0 = padded_x;
    let y0 = padded_y;

    self.cursor_x += reserved_width;
    self.row_height = self.row_height.max(reserved_height);
    (x0, y0, reserved_width, reserved_height)
  }

  fn record_packed_rect(&mut self, x0: u32, y0: u32, reserved_width: u32, reserved_height: u32) {
    self.version += 1;
    let rect = GlyphAtlasDirtyRect {
      x: x0,
      y: y0,
      width: reserved_width,
      height: reserved_height,
      version: self.version,
    };
    // Merge adjacent same-row packs as they land so glyph bursts don't blow
    // up the log (the merged rect carries the newest version).
    match self.dirty_rects.back_mut() {
      Some(last) if should_merge_dirty_rects(*last, rect) => *last = union_dirty_rect(*last, rect),
      _ => self.dirty_rects.push_back(rect),
    }
    while self.dirty_rects.len() > GLYPH_ATLAS_DIRTY_LOG_LIMIT {
      if let Some(removed) = self.dirty_rects.pop_front() {
        self.dirty_from_version = removed.version;
      }
    }
  }

  /// A snapshot of the atlas: its pixels as they are now, shared, not
  /// copied. Glyphs packed while a snapshot is held go into a copy.
  // It refreshes the snapshot it hands out, so it needs `&mut self`.
  #[allow(clippy::wrong_self_convention)]
  pub(crate) fn to_atlas(&mut self) -> GlyphAtlas {
    if self.snapshot_version != self.version {
      self.snapshot_rects = self.dirty_rects.iter().copied().collect();
      self.snapshot_version = self.version;
    }

    GlyphAtlas {
      data: self.data.clone(),
      width: self.width,
      height: self.height,
      version: self.version,
      dirty_rects: self.snapshot_rects.clone(),
      dirty_from_version: self.dirty_from_version,
    }
  }
}

/// The dirty rects a consumer whose GPU texture is at `uploaded_version` must
/// apply to reach `atlas.version` — `None` when the log no longer covers the
/// gap and the full atlas has to be re-uploaded instead.
pub(crate) fn atlas_rects_to_apply(atlas: &GlyphAtlas, uploaded_version: u64) -> Option<Vec<GlyphAtlasDirtyRect>> {
  if uploaded_version < atlas.dirty_from_version {
    return None;
  }
  let pending: Vec<GlyphAtlasDirtyRect> = atlas
    .dirty_rects
    .iter()
    .filter(|rect| rect.version > uploaded_version)
    .copied()
    .collect();
  if pending.is_empty() && uploaded_version < atlas.version {
    // The version moved but the log doesn't show how — stay safe.
    return None;
  }
  Some(coalesce_dirty_rects(pending))
}

fn coalesce_dirty_rects(mut rects: Vec<GlyphAtlasDirtyRect>) -> Vec<GlyphAtlasDirtyRect> {
  if rects.len() <= 1 {
    return rects;
  }

  rects.sort_by_key(|rect| (rect.y, rect.x));
  let mut merged: Vec<GlyphAtlasDirtyRect> = Vec::with_capacity(rects.len());
  for rect in rects {
    if rect.width == 0 || rect.height == 0 {
      continue;
    }

    if let Some(last) = merged.last_mut()
      && should_merge_dirty_rects(*last, rect)
    {
      *last = union_dirty_rect(*last, rect);
      continue;
    }

    merged.push(rect);
  }

  merged
}

fn should_merge_dirty_rects(a: GlyphAtlasDirtyRect, b: GlyphAtlasDirtyRect) -> bool {
  if a.y != b.y {
    return false;
  }

  let gap = horizontal_gap(a, b);
  if gap > DIRTY_RECT_MAX_HORIZONTAL_GAP {
    return false;
  }

  let area_sum = dirty_rect_area(a) + dirty_rect_area(b);
  let merged_area = dirty_rect_area(union_dirty_rect(a, b));
  merged_area * DIRTY_RECT_MERGE_WASTE_DENOMINATOR <= area_sum * DIRTY_RECT_MERGE_WASTE_NUMERATOR
}

fn horizontal_gap(a: GlyphAtlasDirtyRect, b: GlyphAtlasDirtyRect) -> u32 {
  let a_right = a.x.saturating_add(a.width);
  let b_right = b.x.saturating_add(b.width);
  if a_right < b.x {
    b.x - a_right
  } else {
    a.x.saturating_sub(b_right)
  }
}

fn union_dirty_rect(a: GlyphAtlasDirtyRect, b: GlyphAtlasDirtyRect) -> GlyphAtlasDirtyRect {
  let x0 = a.x.min(b.x);
  let y0 = a.y.min(b.y);
  let x1 = a.x.saturating_add(a.width).max(b.x.saturating_add(b.width));
  let y1 = a.y.saturating_add(a.height).max(b.y.saturating_add(b.height));
  GlyphAtlasDirtyRect {
    x: x0,
    y: y0,
    width: x1.saturating_sub(x0),
    height: y1.saturating_sub(y0),
    version: a.version.max(b.version),
  }
}

fn dirty_rect_area(rect: GlyphAtlasDirtyRect) -> u64 {
  u64::from(rect.width) * u64::from(rect.height)
}

#[cfg(test)]
mod tests {
  use cosmic_text::{Attrs, Buffer, Family, Placement, Shaping, SwashContent, SwashImage};
  use swash::scale::ScaleContext;

  use super::{
    AtlasPacker, GLYPH_ATLAS_BYTES_PER_PIXEL, GLYPH_ATLAS_PADDING, GlyphAtlasDirtyRect, GlyphEngine,
    PLAIN_BUFFER_CACHE_LIMIT, SHORT_TEXT_BYTES, cluster_is_whitespace, coalesce_dirty_rects, glyph_atlas_pixels,
    glyph_coverage_mask, is_bounded_text_width, render_glyph_image, set_buffer_text, swash_transform_from_screen,
  };
  use crate::{
    layout::{
      quad::ClipRect,
      render_list::GlyphCmd,
      text_style::{TextAlign, TextStyle},
    },
    node::transform::Transform2D,
  };

  #[test]
  fn empty_cluster_is_not_treated_as_whitespace() {
    // A glyph whose source-cluster range is empty (start == end) must not be
    // skipped as whitespace, otherwise it drops out of the word while keeping
    // its advance, leaving a visible gap mid-word.
    assert!(!cluster_is_whitespace("items", 3, 3));
    assert!(!cluster_is_whitespace("items", 3, 4)); // "m" — a real glyph
    assert!(cluster_is_whitespace("a b", 1, 2)); // the space
    assert!(!cluster_is_whitespace("items", 10, 12)); // out of range
  }

  #[test]
  fn atlas_packer_leaves_padding_between_glyph_regions() {
    let mut packer = AtlasPacker::new();
    let (_, _, first_u1, _) = packer.pack(&[255; 16], 2, 2);
    let (second_u0, ..) = packer.pack(&[255; 16], 2, 2);

    let first_x1 = (first_u1 * packer.width as f32).round() as u32;
    let second_x0 = (second_u0 * packer.width as f32).round() as u32;

    assert!(second_x0 >= first_x1);
  }

  #[test]
  fn atlas_packer_leaves_transparent_glyph_padding() {
    let mut packer = AtlasPacker::new();
    let pixels = [
      10, 11, 12, 13, //
      20, 21, 22, 23, //
      30, 31, 32, 33, //
      40, 41, 42, 43,
    ];
    let (x, y, width, height) = packer.pack_pixels(&pixels, 2, 2);
    let p = GLYPH_ATLAS_PADDING as usize;
    let stride = packer.width as usize;
    let pixel = |x: usize, y: usize| (y * stride + x) * GLYPH_ATLAS_BYTES_PER_PIXEL;

    assert_eq!(
      (x, y, width, height),
      (0, 0, 2 + GLYPH_ATLAS_PADDING * 2, 2 + GLYPH_ATLAS_PADDING * 2)
    );
    assert_eq!(packer.data[0], 0);
    assert_eq!(&packer.data[pixel(p, p)..pixel(p, p) + 4], &[10, 11, 12, 13]);
    assert_eq!(&packer.data[pixel(p + 1, p)..pixel(p + 1, p) + 4], &[20, 21, 22, 23]);
    assert_eq!(&packer.data[pixel(p, p + 1)..pixel(p, p + 1) + 4], &[30, 31, 32, 33]);
    assert_eq!(
      &packer.data[pixel(p + 1, p + 1)..pixel(p + 1, p + 1) + 4],
      &[40, 41, 42, 43]
    );
    assert_eq!(
      &packer.data[pixel(p + 2, p + 2)..pixel(p + 2, p + 2) + 4],
      &[0, 0, 0, 0]
    );
  }

  #[test]
  fn color_glyph_atlas_pixels_preserve_rgba() {
    let image = SwashImage {
      content: SwashContent::Color,
      placement: Placement {
        left: 0,
        top: 0,
        width: 1,
        height: 1,
      },
      data: vec![12, 34, 56, 78],
      ..SwashImage::default()
    };

    let (pixels, is_color) = glyph_atlas_pixels(&image);

    assert!(is_color);
    assert_eq!(pixels.as_ref(), &[12, 34, 56, 78]);
  }

  #[test]
  fn mask_glyph_atlas_pixels_store_alpha_coverage() {
    let image = SwashImage {
      content: SwashContent::Mask,
      placement: Placement {
        left: 0,
        top: 0,
        width: 2,
        height: 1,
      },
      data: vec![10, 240],
      ..SwashImage::default()
    };

    let (pixels, is_color) = glyph_atlas_pixels(&image);

    assert!(!is_color);
    assert_eq!(pixels.as_ref(), &[255, 255, 255, 10, 255, 255, 255, 240]);
  }

  #[test]
  fn atlas_snapshot_reuses_data_when_version_is_unchanged() {
    let mut packer = AtlasPacker::new();
    let first = packer.to_atlas();
    let second = packer.to_atlas();

    assert!(std::sync::Arc::ptr_eq(&first.data, &second.data));

    packer.pack_pixels(&[255; 4], 2, 2);
    let third = packer.to_atlas();

    assert!(!std::sync::Arc::ptr_eq(&second.data, &third.data));
    assert_eq!(third.dirty_rects.len(), 1);

    // The dirty log is retained (not drained) so every window's render
    // engine can catch up from its own uploaded version.
    let fourth = packer.to_atlas();
    assert_eq!(fourth.dirty_rects.len(), 1);
    assert!(std::sync::Arc::ptr_eq(&third.dirty_rects, &fourth.dirty_rects));
    assert!(std::sync::Arc::ptr_eq(&third.data, &fourth.data));
  }

  #[test]
  fn atlas_packs_in_place_while_no_snapshot_is_held() {
    let mut packer = AtlasPacker::new();
    packer.pack_pixels(&[1; 4], 1, 1);
    // A snapshot that was taken and let go of leaves the pixels the atlas's own.
    drop(packer.to_atlas());
    let pixels = packer.data.as_ptr();

    let (x, y, ..) = packer.pack_pixels(&[2; 4], 1, 1);

    assert_eq!(packer.data.as_ptr(), pixels, "packed where the pixels are");
    let snapshot = packer.to_atlas();
    assert_eq!(snapshot.data.as_ptr(), pixels, "a snapshot shares them");
    let packed = ((y + GLYPH_ATLAS_PADDING) * packer.width + x + GLYPH_ATLAS_PADDING) as usize * 4;
    assert_eq!(&snapshot.data[packed..packed + 4], &[2; 4]);
  }

  #[test]
  fn a_held_atlas_snapshot_keeps_the_pixels_it_was_taken_with() {
    let mut packer = AtlasPacker::new();
    let (x, y, ..) = packer.pack_pixels(&[1; 4], 1, 1);
    let first = ((y + GLYPH_ATLAS_PADDING) * packer.width + x + GLYPH_ATLAS_PADDING) as usize * 4;
    let held = packer.to_atlas();

    let (x, y, ..) = packer.pack_pixels(&[2; 4], 1, 1);
    let second = ((y + GLYPH_ATLAS_PADDING) * packer.width + x + GLYPH_ATLAS_PADDING) as usize * 4;

    // The held snapshot has the first glyph and not the second.
    assert_eq!(&held.data[first..first + 4], &[1; 4]);
    assert_eq!(&held.data[second..second + 4], &[0; 4]);
    assert_eq!(held.version + 1, packer.version);
    // The atlas went on in a copy that has both, and packs in place again
    // once that copy is its own.
    let pixels = packer.data.as_ptr();
    assert_ne!(pixels, held.data.as_ptr());
    let next = packer.to_atlas();
    assert_eq!(&next.data[first..first + 4], &[1; 4]);
    assert_eq!(&next.data[second..second + 4], &[2; 4]);
    drop(next);
    packer.pack_pixels(&[3; 4], 1, 1);
    assert_eq!(packer.data.as_ptr(), pixels);
  }

  #[test]
  fn a_growing_atlas_keeps_its_pixels_and_those_of_held_snapshots() {
    let mut packer = AtlasPacker::new();
    let (x, y, ..) = packer.pack_pixels(&[7; 4], 1, 1);
    let first = ((y + GLYPH_ATLAS_PADDING) * packer.width + x + GLYPH_ATLAS_PADDING) as usize * 4;
    let held = packer.to_atlas();
    let (width, height) = (packer.width, packer.height);

    // Glyphs as wide as the atlas allows, one per row, until it grows.
    let side = width - GLYPH_ATLAS_PADDING * 2;
    let glyph = vec![9_u8; (side * 60) as usize * 4];
    while packer.height == height {
      packer.pack_pixels(&glyph, side, 60);
    }

    assert_eq!(packer.height, height * 2);
    assert_eq!(packer.data.len(), (width * height * 2) as usize * 4);
    assert_eq!(&packer.data[first..first + 4], &[7; 4]);
    assert_eq!(
      packer.data[packer.data.len() - 1],
      0,
      "rows not packed yet are transparent"
    );
    assert_eq!(held.data.len(), (width * height) as usize * 4);
    assert_eq!(&held.data[first..first + 4], &[7; 4]);
    let snapshot = packer.to_atlas();
    assert_eq!((snapshot.width, snapshot.height), (width, height * 2));
    assert_eq!(snapshot.data.len(), packer.data.len());
  }

  #[test]
  fn atlas_dirty_log_serves_consumers_at_different_versions() {
    let mut packer = AtlasPacker::new();
    packer.pack_pixels(&[255; 4], 2, 2);
    let snap1 = packer.to_atlas();
    // Consumer A uploads snap1 fully; its texture is now at snap1.version.
    let a_version = snap1.version;

    packer.pack_pixels(&[255; 4], 2, 2);
    packer.pack_pixels(&[255; 4], 2, 2);
    let snap2 = packer.to_atlas();

    // Consumer A only needs the packs after its version.
    let a_rects = super::atlas_rects_to_apply(&snap2, a_version).expect("log covers consumer A");
    assert!(a_rects.iter().all(|rect| rect.version > a_version));
    assert!(!a_rects.is_empty());

    // Consumer B never uploaded any rects (texture at version 0) — the log
    // must still cover it, including the packs consumer A already applied.
    let b_rects = super::atlas_rects_to_apply(&snap2, 0).expect("log covers consumer B");
    let b_area: u64 = b_rects.iter().map(|rect| super::dirty_rect_area(*rect)).sum();
    let a_area: u64 = a_rects.iter().map(|rect| super::dirty_rect_area(*rect)).sum();
    assert!(
      b_area >= a_area,
      "stale consumer must receive at least as much as a fresh one"
    );

    // Up to date: nothing to apply.
    let none = super::atlas_rects_to_apply(&snap2, snap2.version).expect("up-to-date consumer");
    assert!(none.is_empty());
  }

  #[test]
  fn atlas_dirty_log_prunes_to_full_upload_for_stale_consumers() {
    let mut packer = AtlasPacker::new();
    packer.pack_pixels(&[255; 4], 2, 2);
    let stale_version = packer.to_atlas().version;

    // Overflow the log with widely-spaced packs (each on its own atlas row so
    // they never merge away).
    for _ in 0..(super::GLYPH_ATLAS_DIRTY_LOG_LIMIT + 8) {
      packer.pack_pixels(&[255; 4096], 1000, 1);
    }
    let snap = packer.to_atlas();

    assert!(snap.dirty_from_version > stale_version);
    // A consumer behind the pruned tail must full-upload...
    assert!(super::atlas_rects_to_apply(&snap, stale_version).is_none());
    // ...while one at the tail can still use the log.
    assert!(super::atlas_rects_to_apply(&snap, snap.dirty_from_version).is_some());
  }

  #[test]
  fn dirty_rect_coalescing_merges_same_atlas_row() {
    let rects = coalesce_dirty_rects(vec![
      GlyphAtlasDirtyRect {
        x: 0,
        y: 12,
        width: 8,
        height: 10,
        version: 1,
      },
      GlyphAtlasDirtyRect {
        x: 8,
        y: 12,
        width: 7,
        height: 10,
        version: 2,
      },
      GlyphAtlasDirtyRect {
        x: 15,
        y: 12,
        width: 9,
        height: 10,
        version: 3,
      },
    ]);

    assert_eq!(rects.len(), 1);
    assert_eq!(
      rects[0],
      GlyphAtlasDirtyRect {
        x: 0,
        y: 12,
        width: 24,
        height: 10,
        version: 3,
      }
    );
  }

  #[test]
  fn dirty_rect_coalescing_keeps_separate_atlas_rows() {
    let rects = coalesce_dirty_rects(vec![
      GlyphAtlasDirtyRect {
        x: 0,
        y: 12,
        width: 20,
        height: 10,
        version: 1,
      },
      GlyphAtlasDirtyRect {
        x: 0,
        y: 22,
        width: 20,
        height: 10,
        version: 2,
      },
    ]);

    assert_eq!(rects.len(), 2);
  }

  #[test]
  fn small_key_label_descender_mask_contains_bottom_coverage() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle {
      font_size: 11.0,
      ..crate::layout::text_style::TextStyle::default()
    };
    let mut buffer = engine.acquire_buffer(&style, 42.0, true);
    let resolved = engine.resolve_family(&style);
    let attrs = engine.style_attrs(&style, &resolved);
    buffer.set_text("key=\"a\"", &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut engine.font_system, false);

    let y_glyph = buffer
      .layout_runs()
      .flat_map(|run| run.glyphs.iter().map(move |glyph| (run.line_y, glyph)))
      .find(|(_, glyph)| glyph.start <= 2 && glyph.end >= 3)
      .expect("the y glyph should be present");
    let physical = y_glyph.1.physical((0.0, y_glyph.0), 1.0);
    let font = engine
      .font_system
      .get_font(physical.cache_key.font_id, physical.cache_key.font_weight)
      .expect("the y glyph font should be available");
    let mut context = ScaleContext::new();
    let image = render_glyph_image(&mut context, &font, physical.cache_key).expect("the y glyph should rasterize");
    let mask = glyph_coverage_mask(&image);
    let width = image.placement.width as usize;
    let height = image.placement.height as usize;
    let bottom_row = &mask[(height - 1) * width..height * width];

    assert!(
      bottom_row.iter().any(|coverage| *coverage > 0),
      "the rasterized y glyph should contain nonzero bottom-row coverage"
    );

    engine.buffer_pool.push(buffer);
  }

  /// Glyphs are positioned in quarter-pixel bins, so the same glyph renders at
  /// four subpixel phases. Rasterization must therefore stay UNHINTED: TrueType
  /// hinting grid-fits the outline first, and shifting a grid-fit outline by a
  /// fraction redistributes its coverage differently per bin — adjacent
  /// instances of one letter get visibly different weight/geometry, which
  /// reads as jumpy text at UI sizes. This pins the main raster path to the
  /// same unhinted output the transformed-glyph path produces; re-enabling
  /// hinting makes the masks diverge and fails this test.
  #[test]
  fn subpixel_binned_glyphs_rasterize_unhinted() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle {
      font_size: 11.0,
      ..crate::layout::text_style::TextStyle::default()
    };
    let mut buffer = engine.acquire_buffer(&style, 100.0, false);
    let resolved = engine.resolve_family(&style);
    let attrs = engine.style_attrs(&style, &resolved);
    buffer.set_text("Hml", &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut engine.font_system, false);

    let mut compared = 0;
    for run in buffer.layout_runs() {
      for glyph in run.glyphs.iter() {
        for bin in [0.0_f32, 0.25, 0.5, 0.75] {
          let (cache_key, ..) = super::GlyphCacheKey::new(
            glyph.font_id,
            glyph.glyph_id,
            glyph.font_size,
            (bin, 0.0),
            glyph.font_weight,
            glyph.cache_key_flags,
          );
          let font = engine
            .font_system
            .get_font(cache_key.font_id, cache_key.font_weight)
            .expect("font");
          let mut context = ScaleContext::new();
          let main = render_glyph_image(&mut context, &font, cache_key).expect("main raster");

          // Reference: an explicitly unhinted render at the same subpixel offset.
          let mut scaler = context
            .builder(font.as_swash())
            .size(f32::from_bits(cache_key.font_size_bits))
            .hint(false)
            .build();
          let offset = super::Vector::new(cache_key.x_bin.as_float(), cache_key.y_bin.as_float());
          let reference = super::Render::new(&[
            super::Source::ColorOutline(0),
            super::Source::ColorBitmap(super::StrikeWith::BestFit),
            super::Source::Outline,
          ])
          .format(super::Format::Alpha)
          .offset(offset)
          .render(&mut scaler, cache_key.glyph_id)
          .expect("reference raster");

          assert_eq!(
            (
              main.placement.left,
              main.placement.top,
              main.placement.width,
              main.placement.height
            ),
            (
              reference.placement.left,
              reference.placement.top,
              reference.placement.width,
              reference.placement.height
            ),
            "glyph {} at bin {bin} must place like the unhinted reference",
            glyph.glyph_id,
          );
          let main_mask = glyph_coverage_mask(&main);
          let reference_mask = glyph_coverage_mask(&reference);
          let diff: u64 = main_mask
            .iter()
            .zip(reference_mask.iter())
            .map(|(a, b)| a.abs_diff(*b) as u64)
            .sum();
          let mean_diff = diff as f64 / main_mask.len().max(1) as f64;
          assert!(
            mean_diff < 1.0,
            "glyph {} at bin {bin} must rasterize unhinted (mean coverage diff {mean_diff:.2})",
            glyph.glyph_id,
          );
          compared += 1;
        }
      }
    }
    assert!(
      compared >= 12,
      "expected all test glyphs to be compared, got {compared}"
    );
    engine.buffer_pool.push(buffer);
  }

  /// Tiny UI text (chips/pills render around 7-8px) must still rasterize
  /// visible coverage — a correctly-sized but all-zero mask packs into the
  /// atlas and draws as invisible text, which no glyph-command-level check
  /// catches.
  #[test]
  fn tiny_font_sizes_rasterize_nonzero_coverage() {
    let mut engine = GlyphEngine::new();
    for weight in [
      crate::layout::text_style::FontWeight::Normal,
      crate::layout::text_style::FontWeight::Bold,
    ] {
      for font_size in [7.7_f32, 8.4, 9.1, 11.0] {
        let style = crate::layout::text_style::TextStyle {
          weight,
          font_size,
          ..crate::layout::text_style::TextStyle::default()
        };
        let mut buffer = engine.acquire_buffer(&style, 100.0, true);
        let resolved = engine.resolve_family(&style);
        let attrs = engine.style_attrs(&style, &resolved);
        buffer.set_text("2363", &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut engine.font_system, false);

        let mut glyph_count = 0;
        for run in buffer.layout_runs() {
          for glyph in run.glyphs.iter() {
            glyph_count += 1;
            // A glyph must rasterize visibly at every subpixel offset it can
            // land on, not just the whole-pixel bin.
            for x_offset in [0.0_f32, 0.25, 0.5, 0.75] {
              for y_offset in [0.0_f32, 0.25, 0.5, 0.75] {
                let physical = glyph.physical((x_offset, run.line_y + y_offset), 1.0);
                let font = engine
                  .font_system
                  .get_font(physical.cache_key.font_id, physical.cache_key.font_weight)
                  .expect("glyph font should be available");
                let mut context = ScaleContext::new();
                let image = render_glyph_image(&mut context, &font, physical.cache_key);
                let visible = image
                  .as_ref()
                  .is_some_and(|image| glyph_coverage_mask(image).iter().any(|coverage| *coverage > 0));
                assert!(
                  visible,
                  "{font_size}px glyph {} at subpixel ({x_offset}, {y_offset}) rasterized invisible",
                  glyph.glyph_id
                );
              }
            }
          }
        }
        engine.buffer_pool.push(buffer);

        assert!(glyph_count > 0, "digits should shape at {font_size}px");
      }
    }
  }

  #[test]
  fn rasterized_glyph_positions_snap_after_fractional_origin() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle {
      font_size: 11.0,
      ..crate::layout::text_style::TextStyle::default()
    };

    let glyphs = engine.rasterize_text("key=\"a\"", &style, 42.0, 0.0, 13.5);

    assert!(!glyphs.is_empty());
    for glyph in glyphs {
      assert!(
        glyph.y.fract().abs() < f32::EPSILON,
        "glyph y should be snapped after adding the final origin: {}",
        glyph.y
      );
    }
  }

  #[test]
  fn unsnapped_rasterized_glyph_positions_preserve_float_layout() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle {
      font_size: 16.0,
      ..crate::layout::text_style::TextStyle::default()
    };

    let glyphs = engine.rasterize_text_unsnapped("This selectable text", &style, 400.0, 0.0, 0.0);

    assert!(!glyphs.is_empty());
    assert!(
      glyphs.iter().any(|glyph| glyph.x.fract().abs() > 0.001),
      "transformed text should keep float layout glyph positions"
    );
  }

  #[test]
  fn whitespace_only_text_does_not_pack_glyphs() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle {
      font_size: 16.0,
      ..crate::layout::text_style::TextStyle::default()
    };

    let glyphs = engine.rasterize_text(" \t  ", &style, 400.0, 0.0, 0.0);

    assert!(glyphs.is_empty());
    assert_eq!(engine.glyph_hits, 0);
    assert_eq!(engine.glyph_misses, 0);
  }

  #[test]
  fn installing_fonts_preserves_existing_atlas_entries() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle::default();
    let glyphs = engine.rasterize_text("Existing atlas text", &style, 400.0, 0.0, 0.0);
    engine.measure_text("Cached measurement", &style, 400.0);
    assert!(!glyphs.is_empty());
    assert!(!engine.texts.is_empty());

    let atlas_version = engine.atlas_packer.version;
    let atlas_entries = engine.atlas_entries.len();
    engine.install_fonts(
      [include_bytes!("../../assets/lucide.ttf").to_vec()],
      [("test-icons", "lucide")],
    );

    assert_eq!(engine.atlas_packer.version, atlas_version);
    assert_eq!(engine.atlas_entries.len(), atlas_entries);
    assert!(engine.texts.is_empty());
    assert_eq!(
      engine.font_aliases.get("test-icons").map(String::as_str),
      Some("lucide")
    );
  }

  #[test]
  fn screen_transform_is_flipped_for_glyph_outline_space() {
    let screen = Transform2D::rotate_deg(-10.0);
    let glyph = swash_transform_from_screen(screen);

    assert_eq!(glyph.xx, screen.a);
    assert_eq!(glyph.xy, -screen.b);
    assert_eq!(glyph.yx, -screen.c);
    assert_eq!(glyph.yy, screen.d);
  }

  #[test]
  #[cfg(target_os = "windows")]
  fn medium_weight_text_does_not_fallback_to_symbol_font() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle {
      font_size: 14.0,
      weight: crate::layout::text_style::FontWeight::Medium,
      ..crate::layout::text_style::TextStyle::default()
    };
    let mut buffer = engine.acquire_buffer(&style, 72.0, true);
    let resolved = engine.resolve_family(&style);
    let attrs = engine.style_attrs(&style, &resolved);
    buffer.set_text("Name", &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut engine.font_system, false);

    for run in buffer.layout_runs() {
      for glyph in run.glyphs {
        let family = engine
          .font_system
          .db()
          .face(glyph.font_id)
          .and_then(|face| face.families.first())
          .map(|family| family.0.as_str())
          .unwrap_or("<unknown>");
        assert_ne!(family, "Marlett");
      }
    }
    engine.buffer_pool.push(buffer);
  }

  #[test]
  fn default_mask_falls_back_when_primary_font_lacks_bullet() {
    let mut engine = GlyphEngine::new();
    engine.load_font(include_bytes!("../../assets/lucide.ttf").to_vec());
    let icon_font_id = engine
      .font_system
      .db()
      .faces()
      .find(|face| face.families.iter().any(|(name, _)| name == "lucide"))
      .expect("bundled icon font should be loaded")
      .id;
    let icon_font = engine
      .font_system
      .get_font(icon_font_id, cosmic_text::Weight::NORMAL)
      .unwrap();
    assert_eq!(icon_font.as_swash().charmap().map('•'), 0);

    let style = crate::layout::text_style::TextStyle {
      font_family: "lucide".into(),
      ..crate::layout::text_style::TextStyle::default()
    };
    let mut buffer = engine.acquire_buffer(&style, 200.0, false);
    super::set_buffer_text(
      &mut buffer,
      "•••",
      Attrs::new().family(Family::Name("lucide")),
      style.text_align,
    );
    buffer.shape_until_scroll(&mut engine.font_system, false);

    let glyphs: Vec<_> = buffer.layout_runs().flat_map(|run| run.glyphs).collect();
    assert_eq!(glyphs.len(), 3);
    for glyph in glyphs {
      assert_ne!(glyph.font_id, icon_font_id, "bullet should use a fallback font");
      assert_ne!(glyph.glyph_id, 0, "bullet should not render the missing-glyph box");
    }
    engine.buffer_pool.push(buffer);
    assert_eq!(engine.rasterize_text("•••", &style, 200.0, 0.0, 0.0).len(), 3);
  }

  #[test]
  fn wrapped_glyph_y_positions_are_pixel_snapped() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle {
      font_size: 13.0,
      line_height: 1.17,
      ..crate::layout::text_style::TextStyle::default()
    };

    let glyphs = engine.rasterize_text("alpha beta gamma delta", &style, 48.0, 0.0, 0.0);

    assert!(
      glyphs.iter().any(|glyph| glyph.y > style.font_size * style.line_height),
      "test text should wrap to at least two lines"
    );
    for glyph in glyphs {
      assert!(
        glyph.y.fract().abs() < f32::EPSILON,
        "glyph y should be pixel-snapped after baseline offset is applied: {}",
        glyph.y
      );
    }
  }

  #[test]
  fn shared_plain_layout_matches_cosmic_for_repeated_and_unique_paragraphs() {
    let mut engine = GlyphEngine::new();
    let repeated = "office cafe\u{301} — Ελληνικά\r\nمرحبا (123)\nשלום 456\r\n漢字 😀\n\t\n".repeat(20);
    let unique = (0..40)
      .map(|i| format!("Row {i}: variable width text ({i:04})\n"))
      .collect::<String>();
    for text in [&repeated, &unique] {
      for align in [TextAlign::Left, TextAlign::Right, TextAlign::Justified] {
        for width in [96.0, 270.5, f32::MAX] {
          let style = TextStyle {
            font_size: 19.5,
            text_align: align,
            ..TextStyle::default()
          };
          let wrap = is_bounded_text_width(width);
          let mut reference = engine.acquire_buffer(&style, width, wrap);
          let family = engine.resolve_family(&style);
          let attrs = Attrs::new().family(Family::Name(&family));
          set_buffer_text(&mut reference, text, attrs, align);
          reference.shape_until_scroll(&mut engine.font_system, false);
          let actual = engine.full_text_buffer(text, &style, width, wrap);
          let signature = |buffer: &Buffer| {
            buffer
              .layout_runs()
              .map(|run| {
                format!(
                  "{} {:?} {} {} {} {} {:?}",
                  run.line_i, run.rtl, run.line_y, run.line_top, run.line_height, run.line_w, run.glyphs
                )
              })
              .collect::<Vec<_>>()
          };
          assert_eq!(
            signature(&actual),
            signature(&reference),
            "shaped runs changed at width {width}"
          );
        }
      }
    }
  }

  #[test]
  fn incremental_paragraph_edits_and_reflow_match_fresh_layout() {
    let mut engine = GlyphEngine::new();
    let mut lines = (0..72)
      .map(|index| format!("Paragraph {index}: office cafe\u{301} مرحبا שלום 漢字 ({index:04})"))
      .collect::<Vec<_>>();
    let mut style = TextStyle::default();
    let mut width = 240.0;
    let mut wrap = true;
    for action in 0..11 {
      match action {
        1 => lines[35].push_str(" edited 😀"),
        2 => lines.insert(14, "Inserted paragraph مرحبا".to_owned()),
        3 => {
          lines.remove(27);
        }
        4 => lines.swap(44, 45),
        5 => lines[30].push_str("\nSplit paragraph שלום"),
        6 => width = 173.5,
        7 => style.font_size *= 1.25,
        8 => style.text_align = TextAlign::Right,
        9 => wrap = false,
        _ => {}
      }
      let text = lines.join(if action == 10 { "\r\n" } else { "\n" });
      engine.reset_stats();
      let actual = engine.full_text_buffer(&text, &style, width, wrap);
      #[cfg(feature = "perf_profile")]
      if action == 1 {
        assert_eq!(engine.profile.plain_shaped_paragraphs, 1);
        assert_eq!(engine.profile.plain_retained_paragraphs, lines.len() - 1);
        assert_eq!(engine.profile.plain_reindexed_paragraphs, 1);
        assert_eq!(engine.profile.plain_accounted_paragraphs, 1);
      }
      #[cfg(feature = "perf_profile")]
      if action == 6 {
        assert_eq!(engine.profile.plain_shaped_paragraphs, 0);
        assert!(engine.profile.plain_laid_out_paragraphs > 0);
        assert_eq!(engine.profile.plain_reindexed_paragraphs, 0);
        assert_eq!(engine.profile.plain_accounted_paragraphs, 0);
      }
      assert_plain_layout_matches_fresh(&mut engine, &actual, &text, &style, width, wrap);
      engine.retain_plain_buffer(actual);
    }
  }

  fn caret_signature(positions: &[crate::node::text_selection::CaretPosition]) -> Vec<(usize, u32, u32)> {
    positions
      .iter()
      .map(|p| (p.index, p.x.to_bits(), p.y.to_bits()))
      .collect()
  }

  // Reference the original fresh-buffer path, independently of retained layouts.
  fn fresh_caret_signature(
    engine: &mut GlyphEngine,
    text: &str,
    style: &TextStyle,
    width: f32,
    wrap: bool,
  ) -> Vec<(usize, u32, u32)> {
    let mut buffer = engine.acquire_buffer(style, width, super::effective_text_wrap(width, wrap));
    let resolved = engine.resolve_family(style);
    let attrs = engine.style_attrs(style, &resolved);
    set_buffer_text(&mut buffer, text, attrs, style.text_align);
    buffer.shape_until_scroll(&mut engine.font_system, false);
    let mut expected = Vec::new();
    for run in buffer.layout_runs() {
      let offset = buffer
        .lines
        .iter()
        .take(run.line_i)
        .map(|line| line.text().len() + line.ending().as_str().len())
        .sum::<usize>();
      let y = run.line_top.max(0.0).to_bits();
      if run.glyphs.is_empty() {
        expected.push((offset, 0.0_f32.to_bits(), y));
      }
      for glyph in run.glyphs {
        for (index, x) in crate::node::text_selection::glyph_carets(glyph, run.text) {
          expected.push((offset + index, x.to_bits(), y));
        }
      }
    }
    if expected.is_empty() {
      expected.push((0, 0.0_f32.to_bits(), 0.0_f32.to_bits()));
    }
    if !expected.iter().any(|position| position.0 == text.len()) {
      let last = *expected.last().unwrap();
      expected.push((text.len(), last.1, last.2));
    }
    expected
  }

  #[test]
  fn retained_carets_match_fresh_positions_and_keep_owned_results() {
    let mut engine = GlyphEngine::new();
    let texts = [
      String::new(),
      "\n".to_owned(),
      "\r\n \t\n".to_owned(),
      "office cafe\u{301} مرحبا שלום 漢字 👨‍👩‍👧‍👦".to_owned(),
      "•••••".to_owned(),
      "office cafe\u{301} مرحبا שלום 漢字 😀\r\n\nLast line\n".repeat(32),
    ];
    for text in &texts {
      for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
        for font_size in [13.0, 19.5] {
          let style = TextStyle {
            font_size,
            line_height: 1.23,
            text_align: align,
            ..TextStyle::default()
          };
          for (width, wrap) in [(64.0, true), (240.0, true), (240.0, false), (f32::MAX, true)] {
            engine.clear_text_caches();
            let expected = fresh_caret_signature(&mut engine, text, &style, width, wrap);
            let mut actual = engine.caret_positions(text, &style, width, wrap);
            assert_eq!(caret_signature(&actual), expected, "width={width} wrap={wrap}");
            assert!(actual.iter().all(|p| text.is_char_boundary(p.index)));
            // Consumers such as masked inputs may remap the returned vector.
            actual.make_mut()[0].index = usize::MAX;
            actual.make_mut()[0].x = -999.0;
            let cached = engine.caret_positions(text, &style, width, wrap);
            assert_eq!(caret_signature(&cached), expected);
            for entry in &engine.plain_buffers {
              assert_plain_buffer_charge(entry);
            }
          }
        }
      }
    }
  }

  #[test]
  fn caret_and_measurement_share_layout_in_both_orders_and_after_edits() {
    let mut engine = GlyphEngine::new();
    let mut lines = (0..72)
      .map(|i| format!("Paragraph {i}: office cafe\u{301} مرحبا 😀"))
      .collect::<Vec<_>>();
    let style = TextStyle::default();
    let mut width = 240.0;
    for action in 0..5 {
      match action {
        1 | 3 => lines[9].push_str(" edited"),
        2 => width = 173.5,
        4 => engine.clear_text_caches(),
        _ => {}
      }
      let text = lines.join("\r\n");
      let expected = fresh_caret_signature(&mut engine, &text, &style, width, true);
      engine.reset_stats();
      if action == 3 {
        engine.measure_text(&text, &style, width);
      }
      let actual = engine.caret_positions(&text, &style, width, true);
      assert_eq!(caret_signature(&actual), expected);
      engine.measure_text(&text, &style, width);
      #[cfg(feature = "perf_profile")]
      {
        assert_eq!(
          engine.profile.plain_shaped_paragraphs,
          match action {
            0 | 4 => 72,
            1 | 3 => 1,
            _ => 0,
          }
        );
        assert_eq!(
          engine.profile.caret_built_paragraphs,
          match action {
            1 | 3 => 1,
            _ => 72,
          }
        );
        assert_eq!(
          engine.profile.caret_reused_paragraphs,
          match action {
            1 | 3 => 71,
            _ => 0,
          }
        );
        assert_eq!(engine.profile.caret_layout_reuses, usize::from(action == 3));
        assert_eq!(engine.profile.caret_misses, 1);
      }
      for entry in &engine.plain_buffers {
        assert_plain_buffer_charge(entry);
      }
    }
  }

  #[test]
  fn paragraph_carets_reuse_geometry_across_edits_and_keep_old_snapshots() {
    let mut engine = GlyphEngine::new();
    let mut lines = (0..96)
      .map(|i| format!("{i}: office cafe\u{301} مرحبا שלום 漢字 👨‍👩‍👧‍👦 long wrapping paragraph"))
      .collect::<Vec<_>>();
    let mut style = TextStyle {
      line_height: 1.23,
      ..TextStyle::default()
    };
    let mut width = 173.5;
    let mut snapshots = Vec::new();
    for action in 0..10 {
      let built = match action {
        0 => 96,
        1 => {
          lines[0].push_str(" prefix becomes much longer and wraps again");
          1
        }
        2 => {
          lines.insert(20, "inserted paragraph 😀".to_owned());
          1
        }
        3 => {
          let tail = lines[40].split_off(6);
          lines.insert(41, tail);
          2
        }
        4 => {
          lines.drain(10..20);
          0
        }
        5 => {
          lines[30..40].reverse();
          0
        }
        6 => {
          lines.last_mut().unwrap().push_str(" end");
          1
        }
        7 => {
          width = 239.75;
          lines.len()
        }
        8 => {
          style.font_size = 19.5;
          lines.len()
        }
        _ => {
          style.text_align = TextAlign::Right;
          lines.len()
        }
      };
      let text = lines.join("\r\n");
      let expected = fresh_caret_signature(&mut engine, &text, &style, width, true);
      engine.reset_stats();
      let positions = engine.caret_positions(&text, &style, width, true);
      assert_eq!(
        positions
          .iter()
          .map(|p| (p.index, p.x.to_bits(), p.y.to_bits()))
          .collect::<Vec<_>>(),
        expected
      );
      #[cfg(feature = "perf_profile")]
      {
        let built = if action == 7 {
          built - engine.profile.plain_reflow_reuses
        } else {
          built
        };
        assert_eq!(engine.profile.caret_built_paragraphs, built, "action {action}");
        assert_eq!(
          engine.profile.caret_reused_paragraphs,
          lines.len() - built,
          "action {action}"
        );
      }
      #[cfg(not(feature = "perf_profile"))]
      let _ = built;
      for entry in &engine.plain_buffers {
        assert_plain_buffer_charge(entry);
      }
      snapshots.push((positions, expected));
      for (old, expected) in &snapshots {
        assert_eq!(
          old
            .iter()
            .map(|p| (p.index, p.x.to_bits(), p.y.to_bits()))
            .collect::<Vec<_>>(),
          *expected,
          "editing must not alter older shared paragraph geometry"
        );
      }
    }
  }

  #[test]
  fn paragraph_caret_snapshots_survive_cache_pressure_and_font_invalidation() {
    let mut engine = GlyphEngine::new();
    let source = (0..96)
      .map(|i| format!("{i}: caret snapshot wrapping text cafe\u{301} مرحبا 😀"))
      .collect::<Vec<_>>()
      .join("\n");
    let style = TextStyle::default();
    let old = engine.caret_positions(&source, &style, 173.5, true);
    let signature = old
      .iter()
      .map(|p| (p.index, p.x.to_bits(), p.y.to_bits()))
      .collect::<Vec<_>>();
    engine.plain_buffer_budget = engine.plain_buffer_bytes * 3 / 2;
    engine.reset_stats();
    let other_source = source
      .lines()
      .map(|s| format!("other {s}"))
      .collect::<Vec<_>>()
      .join("\n");
    engine.caret_positions(&other_source, &style, 173.5, true);
    assert!(engine.plain_buffer_bytes <= engine.plain_buffer_budget);
    #[cfg(feature = "perf_profile")]
    assert!(engine.profile.plain_cache_compactions + engine.profile.plain_cache_evictions > 0);
    for entry in &engine.plain_buffers {
      assert_plain_buffer_charge(entry);
    }
    assert_eq!(
      old
        .iter()
        .map(|p| (p.index, p.x.to_bits(), p.y.to_bits()))
        .collect::<Vec<_>>(),
      signature
    );
    engine.register_font("caret-snapshot-test", "Segoe UI");
    let current = engine.caret_positions(&source, &style, 173.5, true);
    assert_eq!(
      current
        .iter()
        .map(|p| (p.index, p.x.to_bits(), p.y.to_bits()))
        .collect::<Vec<_>>(),
      signature
    );
    assert_eq!(
      old
        .iter()
        .map(|p| (p.index, p.x.to_bits(), p.y.to_bits()))
        .collect::<Vec<_>>(),
      signature
    );
    for entry in &engine.plain_buffers {
      assert_plain_buffer_charge(entry);
    }
  }

  #[test]
  fn width_intervals_preserve_layouts_and_carets_through_reflow_and_edits() {
    let mut engine = GlyphEngine::new();
    let mut rows = (0..72)
      .map(|i| format!("{i}: alpha beta gamma delta epsilon zeta eta theta iota kappa"))
      .collect::<Vec<_>>();
    let style = TextStyle {
      font_size: 19.5,
      line_height: 1.23,
      ..TextStyle::default()
    };
    let mut snapshots = Vec::new();
    for (step, width) in [
      240.0, 240.125, 173.5, 173.625, 600.0, 600.125, 599.875, 0.0, 480.0, 479.875,
    ]
    .into_iter()
    .enumerate()
    {
      if step == 5 {
        rows[17].push_str(" edited");
        rows.swap(22, 33);
      }
      let text = rows.join("\r\n");
      let expected = fresh_caret_signature(&mut engine, &text, &style, width, true);
      engine.reset_stats();
      let current = engine.caret_positions(&text, &style, width, true);
      assert_eq!(caret_signature(&current), expected, "step {step}");
      #[cfg(feature = "perf_profile")]
      {
        let p = engine.profile;
        assert_eq!(p.caret_built_paragraphs + p.caret_reused_paragraphs, rows.len());
        assert_eq!(p.plain_laid_out_paragraphs + p.plain_reflow_reuses, rows.len());
        assert_eq!(p.caret_reused_paragraphs, p.plain_reflow_reuses);
        if step == 1 {
          assert!(
            p.plain_reflow_reuses > 0,
            "a fractional resize should reuse wrapped paragraphs"
          );
        }
        if step == 5 {
          assert_eq!(p.plain_shaped_paragraphs, 1);
          assert!(p.plain_reflow_reuses > 0);
        }
      }
      let actual = engine.full_text_buffer(&text, &style, width, true);
      assert_plain_layout_matches_fresh(&mut engine, &actual, &text, &style, width, true);
      engine.retain_plain_buffer(actual);
      snapshots.push((current, expected));
      for (old, signature) in &snapshots {
        assert_eq!(caret_signature(old), *signature, "resize mutated an older snapshot");
      }
    }
    // Compaction must discard the interval with the layout, then rebuild it.
    let text = rows.join("\r\n");
    let mut entry = engine.full_text_buffer(&text, &style, 479.875, true);
    for (line, metadata) in entry.buffer.lines.iter_mut().zip(&mut entry.paragraphs) {
      line.reset_layout();
      metadata.layout_bytes = 0;
      metadata.carets = None;
      metadata.reflow = None;
    }
    entry.layout_ready = false;
    entry.refresh_bytes();
    engine.retain_plain_buffer(entry);
    let actual = engine.full_text_buffer(&text, &style, 479.875, true);
    assert_plain_layout_matches_fresh(&mut engine, &actual, &text, &style, 479.875, true);
    assert!(actual.paragraphs.iter().any(|p| p.reflow.is_some()));
  }

  fn assert_plain_layout_matches_fresh(
    engine: &mut GlyphEngine,
    actual: &super::CachedPlainBuffer,
    text: &str,
    style: &TextStyle,
    width: f32,
    wrap: bool,
  ) {
    assert_plain_buffer_charge(actual);
    let mut reference = engine.acquire_buffer(style, width, wrap);
    let family = engine.resolve_family(style);
    let attrs = engine.style_attrs(style, &family);
    set_buffer_text(&mut reference, text, attrs, style.text_align);
    reference.shape_until_scroll(&mut engine.font_system, false);
    let signature = |buffer: &Buffer| {
      buffer
        .layout_runs()
        .map(|run| {
          format!(
            "{} {:?} {} {} {} {} {:?}",
            run.line_i, run.rtl, run.line_y, run.line_top, run.line_height, run.line_w, run.glyphs
          )
        })
        .collect::<Vec<_>>()
    };
    assert_eq!(
      signature(actual),
      signature(&reference),
      "layout changed at width {width}"
    );
  }

  /// Charge of `entry` once compaction has dropped its wrapped layouts and carets.
  fn shaping_only_bytes(entry: &super::CachedPlainBuffer) -> usize {
    let layout_bytes = entry
      .paragraphs
      .iter()
      .map(|paragraph| paragraph.layout_bytes + paragraph.carets.as_ref().map_or(0, |carets| carets.bytes()))
      .sum::<usize>();
    entry.bytes - layout_bytes
  }

  fn assert_plain_buffer_charge(entry: &super::CachedPlainBuffer) {
    assert_eq!(entry.paragraphs.len(), entry.buffer.lines.len());
    let expected = super::plain_buffer_memory_bytes(&entry.buffer)
      + entry.key.text.len()
      + std::mem::size_of::<super::CachedPlainBuffer>()
      + entry.paragraphs.capacity() * std::mem::size_of::<super::PlainParagraph>()
      + entry
        .paragraphs
        .iter()
        .filter_map(|p| p.carets.as_ref())
        .map(|p| p.bytes())
        .sum::<usize>();
    assert_eq!(
      entry.bytes, expected,
      "cached charge diverged from actual public allocations"
    );
    for (line, metadata) in entry.buffer.lines.iter().zip(&entry.paragraphs) {
      assert_eq!(metadata.fingerprint, super::paragraph_fingerprint(line.text()));
      assert_eq!(metadata.shape_bytes, super::plain_line_shape_bytes(line));
      assert_eq!(metadata.layout_bytes, super::plain_line_layout_bytes(line));
    }
  }

  #[test]
  fn paragraph_boundaries_duplicates_and_shrinking_match_fresh_layout() {
    let mut engine = GlyphEngine::new();
    let mut lines = (0..72)
      .map(|index| format!("Paragraph {}: cafe\u{301} مرحبا שלום 😀", index % 6))
      .collect::<Vec<_>>();
    let style = TextStyle::default();
    for action in 0..10 {
      match action {
        1 => lines[0].push_str(" first"),
        2 => lines.last_mut().unwrap().push_str(" last"),
        3 => lines.insert(2, lines[1].clone()),
        4 => {
          lines.drain(10..55);
        }
        5 => lines.extend(lines.clone()),
        6 => lines.reverse(),
        7 => {
          lines.drain(..lines.len() - 2);
        }
        8 => lines.clear(),
        9 => lines.push("\r\n\n".to_owned()),
        _ => {}
      }
      let text = lines.join(if action == 6 { "\r\n" } else { "\n" });
      let actual = engine.full_text_buffer(&text, &style, 180.5, true);
      assert_plain_layout_matches_fresh(&mut engine, &actual, &text, &style, 180.5, true);
      engine.retain_plain_buffer(actual);
    }
  }

  #[test]
  fn paragraph_fingerprint_collision_cannot_reuse_different_text() {
    let mut engine = GlyphEngine::new();
    let mut lines = (0..72)
      .map(|index| format!("Paragraph {index}: cafe\u{301} مرحبا 😀"))
      .collect::<Vec<_>>();
    let style = TextStyle::default();
    let original = lines.join("\n");
    let mut cached = engine.full_text_buffer(&original, &style, 240.0, true);
    lines[9].push_str(" changed");
    cached.paragraphs[9].fingerprint = super::paragraph_fingerprint(&lines[9]);
    engine.retain_plain_buffer(cached);
    engine.reset_stats();
    let text = lines.join("\n");
    let actual = engine.full_text_buffer(&text, &style, 240.0, true);
    assert_plain_layout_matches_fresh(&mut engine, &actual, &text, &style, 240.0, true);
    #[cfg(feature = "perf_profile")]
    assert_eq!(engine.profile.plain_shaped_paragraphs, 1);
  }

  #[test]
  fn cache_pressure_preserves_logical_and_scaled_paragraph_shaping() {
    let mut engine = GlyphEngine::new();
    let readme = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"));
    let text = (0..24)
      .flat_map(|copy| {
        readme
          .lines()
          .enumerate()
          .map(move |(index, line)| format!("{copy:02}/{index:03}: {line}\n"))
      })
      .collect::<String>();
    let style = TextStyle::default();
    let scaled = TextStyle {
      font_size: style.font_size * 1.5,
      ..style.clone()
    };
    let buffers = [(&style, 860.0 / 1.5), (&scaled, 860.0)]
      .map(|(style, width)| engine.full_text_buffer(&text, style, width, true));
    // The budget is derived from the actual charges, not a fixed size: shaping sizes depend on the
    // platform's fonts and on cosmic-text's glyph layout. It holds either document fully laid out
    // next to the other one compacted to shaping only, but never both fully laid out.
    let [logical, physical] = buffers.each_ref().map(|entry| (entry.bytes, shaping_only_bytes(entry)));
    engine.plain_buffer_budget = (logical.0 + physical.1).max(logical.1 + physical.0);
    assert!(logical.0 + physical.0 > engine.plain_buffer_budget);
    for buffer in buffers {
      engine.retain_plain_buffer(buffer);
    }
    assert_eq!(
      engine.plain_buffers.len(),
      2,
      "DPI layouts must not evict each other's shaping"
    );
    assert!(
      engine.plain_buffers.iter().any(|entry| !entry.layout_ready),
      "exercise compacted layout recovery"
    );
    assert!(engine.plain_buffer_bytes <= engine.plain_buffer_budget);
    for entry in &engine.plain_buffers {
      assert_plain_buffer_charge(entry);
    }
    engine.reset_stats();
    let recovered = engine
      .take_plain_buffer(&text, &style, 860.0 / 1.5, true)
      .expect("logical shaping retained");
    #[cfg(feature = "perf_profile")]
    {
      assert_eq!(engine.profile.plain_shaped_paragraphs, 0);
      assert!(engine.profile.plain_laid_out_paragraphs > 0);
    }
    assert_plain_layout_matches_fresh(&mut engine, &recovered, &text, &style, 860.0 / 1.5, true);
    engine.retain_plain_buffer(recovered);
    assert_eq!(engine.plain_buffers.len(), 2);
    assert!(engine.plain_buffer_bytes <= engine.plain_buffer_budget);
  }

  #[test]
  fn combined_plain_layouts_fit_without_rewrapping() {
    let mut engine = GlyphEngine::new();
    let text = "office cafe\u{301} مرحبا שלום 漢字 😀\n".repeat(40);
    let logical = TextStyle::default();
    let physical = TextStyle {
      font_size: logical.font_size * 1.5,
      ..logical.clone()
    };
    let logical_buffer = engine.full_text_buffer(&text, &logical, 240.0, true);
    let physical_buffer = engine.full_text_buffer(&text, &physical, 360.0, true);
    // Exercise the exact charge boundary independently of platform font metrics.
    engine.plain_buffer_budget = logical_buffer.bytes + physical_buffer.bytes;
    engine.reset_stats();
    engine.retain_plain_buffer(logical_buffer);
    engine.retain_plain_buffer(physical_buffer);
    assert_eq!(engine.plain_buffers.len(), 2);
    assert_eq!(engine.plain_buffer_bytes, engine.plain_buffer_budget);
    assert!(engine.plain_buffers.iter().all(|entry| entry.layout_ready));
    #[cfg(feature = "perf_profile")]
    {
      assert_eq!(engine.profile().plain_cache_bytes, engine.plain_buffer_budget);
      assert_eq!(engine.profile().plain_cache_compactions, 0);
    }
    for (style, width) in [(&logical, 240.0), (&physical, 360.0)] {
      let recovered = engine.take_plain_buffer(&text, style, width, true).unwrap();
      assert_plain_layout_matches_fresh(&mut engine, &recovered, &text, style, width, true);
    }
    #[cfg(feature = "perf_profile")]
    {
      assert_eq!(engine.profile().plain_shaped_paragraphs, 0);
      assert_eq!(engine.profile().plain_laid_out_paragraphs, 0);
      assert_eq!(engine.profile().plain_cache_bytes, 0);
    }
  }

  #[test]
  fn optical_shortcut_matches_full_ink_scan() {
    let mut engine = GlyphEngine::new();
    for text in [
      "\n \t\n",
      "Hello gjpq",
      "\nHello\n\n  last line\n\t\n",
      "مرحبا (123)\nשלום\nMixed 😀 office",
      "😀\n🙂",
      "\u{200d}\u{200b}",
    ] {
      for font_size in [12.0, 19.5, 32.0] {
        let style = TextStyle {
          font_size,
          ..TextStyle::default()
        };
        for width in [48.0, 240.0] {
          let expected = engine
            .text_vertical_extents(text, &style, width, true)
            .map(|e| (e.optical_top, e.optical_bottom));
          assert_eq!(
            engine.text_optical_extents(text, &style, width, true),
            expected,
            "text={text:?}"
          );
        }
      }
    }
  }

  #[test]
  fn full_layout_reuse_preserves_clipped_glyphs_at_different_origins() {
    let mut engine = GlyphEngine::new();
    let text = "First line\nMixed office cafe\u{301} مرحبا 😀\n".repeat(32);
    let style = TextStyle::default();
    let clip = ClipRect {
      x: 0.0,
      y: 0.0,
      width: 200.0,
      height: 75.0,
      active: true,
      border_radius: None,
    };
    for origin_y in [0.0, -120.5, -480.0] {
      engine.clear_text_caches();
      engine.measure_text(&text, &style, 200.0);
      let mut cached = Vec::new();
      engine.rasterize_text_with_wrap_clipped_into(&text, &style, 200.0, true, 0.0, origin_y, clip, &mut cached);
      engine.clear_text_caches();
      let mut reference = Vec::new();
      engine.rasterize_text_with_wrap_clipped_into(&text, &style, 200.0, true, 0.0, origin_y, clip, &mut reference);
      let signature = |glyphs: &[GlyphCmd]| {
        glyphs
          .iter()
          .map(|g| {
            (
              [g.x, g.y, g.width, g.height],
              g.color,
              g.atlas_min,
              g.atlas_max,
              g.color_glyph,
            )
          })
          .collect::<Vec<_>>()
      };
      assert!(!cached.is_empty());
      assert_eq!(signature(&cached), signature(&reference));
    }
  }

  #[test]
  fn plain_buffer_cache_invalidates_and_evicts_without_stale_layouts() {
    let mut engine = GlyphEngine::new();
    engine.plain_buffer_budget = 32 * 1024 * 1024;
    let style = TextStyle::default();
    // Documents: one paragraph each, too long for the short texts' cache.
    let document = |index: usize| format!("Entry {index} {}", "word ".repeat(SHORT_TEXT_BYTES / 5));
    for index in 0..PLAIN_BUFFER_CACHE_LIMIT + 3 {
      engine.measure_text(&document(index), &style, 200.0);
    }
    assert_eq!(engine.plain_buffers.len(), PLAIN_BUFFER_CACHE_LIMIT);
    assert_eq!(engine.shaped_texts.len(), 0);
    assert!(engine.plain_buffer_bytes <= engine.plain_buffer_budget);
    assert!(engine.take_plain_buffer(&document(0), &style, 200.0, true).is_none());
    assert!(engine.take_plain_buffer(&document(64), &style, 201.0, true).is_none());
    let changed_style = TextStyle {
      font_size: 20.0,
      ..style.clone()
    };
    assert!(
      engine
        .take_plain_buffer(&document(64), &changed_style, 200.0, true)
        .is_none()
    );
    engine.measure_text("A short text", &style, 200.0);
    assert_eq!(engine.shaped_texts.len(), 1);
    engine.register_font("test-alias", "Segoe UI");
    assert!(engine.plain_buffers.is_empty());
    assert_eq!(engine.shaped_texts.len(), 0);
    assert_eq!(engine.shaped_texts.bytes(), 0);
    assert_eq!(engine.plain_buffer_bytes, 0);
    engine.measure_text("after font change", &style, 200.0);
    engine.text_optical_extents("after font change", &style, 200.0, true);
    engine.install_fonts([Vec::new()], std::iter::empty::<(&str, &str)>());
    assert!(engine.plain_buffers.is_empty());
    assert_eq!(engine.shaped_texts.len(), 0);
    assert!(engine.texts.is_empty());
    assert_eq!(engine.plain_buffer_bytes, 0);
    let large = "A bounded cache retains recently shaped document lines.\n".repeat(800);
    for index in 0..8 {
      let style = TextStyle {
        font_size: 16.0 + index as f32,
        ..TextStyle::default()
      };
      engine.measure_text(&large, &style, 400.0);
      assert!(engine.plain_buffer_bytes <= engine.plain_buffer_budget);
    }
    assert!(
      engine.plain_buffers.len() < 8,
      "large layouts should trigger byte-budget eviction"
    );
  }

  /// Short texts whose shaping meets what a width or a size could disturb:
  /// two directions, a combining mark, an emoji from a fallback font, a word
  /// longer than its line, line breaks, tabs, nothing at all.
  const SHAPED_TEXTS: &[&str] = &[
    "Short label",
    "the quick brown fox jumps over the lazy dog and keeps running until the line has wrapped twice",
    "Mixed office cafe\u{301} مرحبا بالعالم 😀 and back",
    "first line\nsecond line\r\nthird",
    "Supercalifragilisticexpialidocious-and-then-some-more-letters",
    "tab\tseparated\tcells",
    "",
  ];

  /// `(font size, line height, width, wrap, alignment)`, in an order that
  /// changes one or several of them each time and comes back to the first.
  const LAYOUTS: &[(f32, f32, f32, bool, TextAlign)] = &[
    (16.0, 1.2, 200.0, true, TextAlign::Left),
    (16.0, 1.2, 120.0, true, TextAlign::Left),
    (24.0, 1.2, 180.0, true, TextAlign::Left),
    (16.0, 1.5, 200.0, true, TextAlign::Center),
    (16.0, 1.2, f32::MAX, false, TextAlign::Left),
    (16.0, 1.2, 90.5, true, TextAlign::Right),
    (11.0, 1.4, 200.0, false, TextAlign::End),
    (16.0, 1.2, 200.0, true, TextAlign::Justified),
    (16.0, 1.2, 200.0, true, TextAlign::Left),
  ];

  fn laid_out(
    base: &TextStyle,
    (font_size, line_height, _, _, text_align): (f32, f32, f32, bool, TextAlign),
  ) -> TextStyle {
    TextStyle {
      font_size,
      line_height,
      text_align,
      ..base.clone()
    }
  }

  #[test]
  fn a_short_text_laid_out_again_matches_a_fresh_layout() {
    let mut engine = GlyphEngine::new();
    let faces = [
      TextStyle::default(),
      TextStyle {
        weight: crate::layout::text_style::FontWeight::Bold,
        ..TextStyle::default()
      },
      TextStyle {
        style: crate::layout::text_style::FontStyle::Italic,
        ..TextStyle::default()
      },
    ];
    for (shaped, text) in SHAPED_TEXTS.iter().enumerate() {
      for (face, base) in faces.iter().enumerate() {
        for layout in LAYOUTS {
          let style = laid_out(base, *layout);
          let (_, _, width, wrap, _) = *layout;
          let entry = engine.full_text_buffer(text, &style, width, wrap);
          assert!(entry.key.matches_measure(text, &style, width, wrap));
          assert_plain_layout_matches_fresh(&mut engine, &entry, text, &style, width, wrap);
          engine.retain_plain_buffer(entry);
          // One shaped buffer per text and face, whatever it is laid out as.
          assert_eq!(engine.shaped_texts.len(), shaped * faces.len() + face + 1);
        }
      }
    }
  }

  #[test]
  fn what_a_short_text_measures_and_paints_does_not_depend_on_what_it_was_laid_out_as_before() {
    let style_of = |layout| laid_out(&TextStyle::default(), layout);
    let glyphs = |engine: &mut GlyphEngine, text: &str, layout: (f32, f32, f32, bool, TextAlign)| {
      let style = style_of(layout);
      let mut glyphs = Vec::new();
      engine.rasterize_text_with_wrap_into(text, &style, layout.2, layout.3, 3.0, 7.0, &mut glyphs);
      glyphs
        .iter()
        .map(|glyph| (glyph.x, glyph.y, glyph.width, glyph.height))
        .collect::<Vec<_>>()
    };
    let mut reused = GlyphEngine::new();
    for text in SHAPED_TEXTS {
      for (index, layout) in LAYOUTS.iter().enumerate() {
        let style = style_of(*layout);
        let mut fresh = GlyphEngine::new();
        let size = reused.measure_text(text, &style, layout.2);
        let expected = fresh.measure_text(text, &style, layout.2);
        assert_eq!(
          (size.width, size.height),
          (expected.width, expected.height),
          "{text:?}, layout {index}"
        );
        assert_eq!(
          reused.text_optical_extents(text, &style, layout.2, layout.3),
          fresh.text_optical_extents(text, &style, layout.2, layout.3),
          "{text:?}, layout {index}"
        );
        assert_eq!(
          glyphs(&mut reused, text, *layout),
          glyphs(&mut fresh, text, *layout),
          "{text:?}, layout {index}"
        );
      }
    }
  }

  #[test]
  fn tracking_shapes_a_short_text_by_its_ems() {
    let mut engine = GlyphEngine::new();
    let text = "tracked letters wrap here";
    let style = |font_size: f32, letter_spacing: f32| TextStyle {
      font_size,
      letter_spacing,
      ..TextStyle::default()
    };
    // 2 px at 16 px and 3 px at 24 px are both an eighth of an em: one
    // shaping, as a scale factor of 1.5 makes of a text. 2 px at 24 px is
    // another tracking, and another shaping.
    for (style, shaped) in [
      (style(16.0, 2.0), 1),
      (style(24.0, 3.0), 1),
      (style(24.0, 2.0), 2),
      (style(16.0, 0.0), 3),
    ] {
      let entry = engine.full_text_buffer(text, &style, 150.0, true);
      assert_plain_layout_matches_fresh(&mut engine, &entry, text, &style, 150.0, true);
      engine.retain_plain_buffer(entry);
      assert_eq!(engine.shaped_texts.len(), shaped);
    }
  }

  #[test]
  #[cfg(feature = "perf_profile")]
  fn a_short_text_is_shaped_once_for_every_width_and_size() {
    let mut engine = GlyphEngine::new();
    let text = "a message long enough to wrap when the window gets narrow";
    let base = TextStyle::default();
    for (index, layout) in LAYOUTS.iter().enumerate() {
      engine.reset_stats();
      let style = laid_out(&base, *layout);
      engine.measure_text(text, &style, layout.2);
      engine.text_optical_extents(text, &style, layout.2, layout.3);
      let mut glyphs = Vec::new();
      engine.rasterize_text_with_wrap_into(text, &style, layout.2, layout.3, 0.0, 0.0, &mut glyphs);
      assert_eq!(
        engine.profile.plain_shaped_paragraphs,
        usize::from(index == 0),
        "layout {index}"
      );
    }
  }

  #[test]
  fn bold_text_set_or_unset_lays_every_kept_text_out_anew() {
    // No cache has the setting in its key: a text measured, wrapped and kept
    // shaped before the setting changed must not answer for it after.
    let style = TextStyle::default();
    let text = "Bold text for every text, whatever set its weight";
    let sizes = |engine: &mut GlyphEngine| {
      let whole = engine.measure_text(text, &style, f32::MAX);
      let wrapped = engine.measure_text(text, &style, 120.0);
      [whole.width, whole.height, wrapped.width, wrapped.height]
    };
    let mut regular_only = GlyphEngine::new();
    let regular = sizes(&mut regular_only);
    let mut bold_only = GlyphEngine::new();
    bold_only.set_bold_text(true);
    let bold = sizes(&mut bold_only);

    let mut engine = GlyphEngine::new();
    assert_eq!(sizes(&mut engine), regular);
    engine.set_bold_text(true);
    let misses = engine.measure_misses;
    assert_eq!(sizes(&mut engine), bold, "as an engine that never drew it lighter");
    assert_eq!(engine.measure_misses, misses + 2, "both measured anew");
    engine.set_bold_text(false);
    assert_eq!(sizes(&mut engine), regular, "and back");
    // The same setting again changes nothing and drops nothing.
    let hits = engine.measure_hits;
    engine.set_bold_text(false);
    assert_eq!(sizes(&mut engine), regular);
    assert_eq!(engine.measure_hits, hits + 2);
  }

  #[test]
  fn long_texts_with_one_fingerprint_are_laid_out_once_each() {
    // Paragraphs of one length that differ only between the first, the middle
    // and the last 128 bytes, which is all the fingerprint of a long text
    // reads: each is measured once and found on every pass after, with the
    // size of an engine that never saw the others.
    let style = TextStyle::default();
    let texts: Vec<String> = ["i", "W", "m", " "]
      .iter()
      .map(|differs| {
        let mut text = "a line of a log that says the same as the others. ".repeat(10);
        text.replace_range(128..186, &differs.repeat(58));
        text
      })
      .collect();
    let mut engine = GlyphEngine::new();
    let fingerprints: std::collections::HashSet<u64> = texts
      .iter()
      .map(|text| engine.texts.fingerprint(text, &style, 300.0, true))
      .collect();
    assert_eq!(fingerprints.len(), 1, "the texts differ where the fingerprint reads");
    let size = |engine: &mut GlyphEngine, text: &str| {
      let size = engine.measure_text(text, &style, 300.0);
      [size.width, size.height]
    };
    let alone: Vec<[f32; 2]> = texts.iter().map(|text| size(&mut GlyphEngine::new(), text)).collect();
    assert_ne!(alone[0], alone[1], "the texts are laid out differently");

    for pass in 0..4 {
      engine.begin_pass();
      for (text, alone) in texts.iter().zip(&alone) {
        assert_eq!(size(&mut engine, text), *alone, "pass {pass}");
      }
      let misses = if pass == 0 { texts.len() } else { 0 };
      assert_eq!(engine.measure_misses, misses, "pass {pass}");
    }
  }

  #[test]
  fn short_texts_kept_shaped_stay_within_their_budget_and_the_oldest_go_first() {
    let mut engine = GlyphEngine::new();
    engine.shaped_texts = super::ShapedTexts::new(512 * 1024);
    let style = TextStyle::default();
    let text = |index: usize| format!("Message {index} with a few more words to shape");
    for index in 0..200 {
      engine.measure_text(&text(index), &style, 200.0);
      assert!(engine.shaped_texts.bytes() <= 512 * 1024);
    }
    let kept = engine.shaped_texts.len();
    assert!(kept > 4 && kept < 200, "{kept} texts kept");
    assert!(engine.shaped_texts.take(&text(0), &style).is_none());
    assert!(engine.shaped_texts.take(&text(199 - kept), &style).is_none());
    let newest = engine.shaped_texts.take(&text(199), &style).expect("the newest text");
    let oldest_kept = engine
      .shaped_texts
      .take(&text(200 - kept), &style)
      .expect("the oldest kept text");
    assert_eq!(engine.shaped_texts.len(), kept - 2);
    engine.shaped_texts.put(newest);
    engine.shaped_texts.put(oldest_kept);

    // A text used again is the newest again: as many more texts as the cache
    // holds push out those put before it, not it.
    for index in 200..200 + kept {
      let used_again = engine.full_text_buffer(&text(200 - kept), &style, 200.0, true);
      engine.retain_plain_buffer(used_again);
      engine.measure_text(&text(index), &style, 200.0);
    }
    assert!(engine.shaped_texts.take(&text(200 - kept), &style).is_some());
    assert!(engine.shaped_texts.take(&text(199), &style).is_none());
    // The places a text taken and put again leaves behind do not pile up.
    assert!(engine.shaped_texts.places() <= engine.shaped_texts.len() * 2 + 64 + 1);
  }

  #[test]
  fn painting_a_text_in_one_lookup_appends_what_the_two_calls_append() {
    use crate::layout::text_style::VerticalAlign;
    let signature = |glyphs: &[GlyphCmd]| {
      glyphs
        .iter()
        .map(|glyph| {
          (
            (glyph.order, glyph.x, glyph.y, glyph.width, glyph.height),
            (glyph.color, glyph.atlas_min, glyph.atlas_max, glyph.color_glyph),
            (
              glyph.transform,
              glyph.transform_origin,
              glyph.sharpness,
              glyph.shadow_sigma,
            ),
            (
              glyph.clip.x,
              glyph.clip.y,
              glyph.clip.width,
              glyph.clip.height,
              glyph.clip.active,
            ),
          )
        })
        .collect::<Vec<_>>()
    };
    let clips = [
      ClipRect::default(),
      // Cuts the text's lower lines and its right edge.
      ClipRect {
        x: 3.0,
        y: 5.0,
        width: 120.0,
        height: 30.0,
        active: true,
        border_radius: None,
      },
    ];
    let aligns = [
      VerticalAlign::Top,
      VerticalAlign::Center,
      VerticalAlign::Bottom,
      VerticalAlign::LineBox,
    ];
    let style = TextStyle {
      color: crate::node::color::Color::new(20, 40, 60, 200),
      ..TextStyle::default()
    };
    let mut painted = GlyphEngine::new();
    let mut reference = GlyphEngine::new();
    for text in SHAPED_TEXTS {
      for (width, wrap) in [(160.0, true), (f32::MAX, false), (f32::MAX, true)] {
        for (order, align) in aligns.into_iter().enumerate() {
          for clip in clips {
            for box_height in [0.0, 19.2, 80.0] {
              let mut expected = Vec::new();
              let offset = reference.text_vertical_align_offset(text, &style, width, wrap, align, box_height);
              reference.rasterize_text_with_wrap_clipped_into(
                text,
                &style,
                width,
                wrap,
                3.0,
                7.0 + offset,
                clip,
                &mut expected,
              );
              for glyph in &mut expected {
                glyph.order = order;
                glyph.clip = clip;
              }
              // The first time through the two calls, then from the entry.
              for pass in 0..2 {
                let mut glyphs = Vec::new();
                painted.paint_text_into(
                  text,
                  &style,
                  width,
                  wrap,
                  align,
                  box_height,
                  3.0,
                  7.0,
                  clip,
                  order,
                  &mut glyphs,
                );
                assert_eq!(
                  signature(&glyphs),
                  signature(&expected),
                  "{text:?} at {width}, wrap {wrap}, align {order}, box {box_height}, pass {pass}"
                );
              }
            }
          }
        }
      }
    }
  }

  #[test]
  fn clipped_wrapped_text_only_appends_intersecting_glyphs() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle {
      font_size: 16.0,
      ..crate::layout::text_style::TextStyle::default()
    };
    let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau";
    let max_width = 96.0;

    let full = engine.rasterize_text(text, &style, max_width, 0.0, 0.0);
    let clip = ClipRect {
      x: 0.0,
      y: 0.0,
      width: max_width,
      height: 36.0,
      active: true,
      border_radius: None,
    };
    let mut clipped = Vec::new();
    engine.rasterize_text_with_wrap_clipped_into(text, &style, max_width, true, 0.0, 0.0, clip, &mut clipped);

    assert!(!clipped.is_empty());
    assert!(clipped.len() < full.len());
    let clipped_len = clipped.len();
    for glyph in clipped {
      assert!(
        glyph.x < clip.x + clip.width
          && glyph.x + glyph.width > clip.x
          && glyph.y < clip.y + clip.height
          && glyph.y + glyph.height > clip.y,
        "clipped rasterization appended a glyph outside the clip: x={} y={} w={} h={}",
        glyph.x,
        glyph.y,
        glyph.width,
        glyph.height
      );
    }

    engine.reset_stats();
    let mut cached = Vec::new();
    engine.rasterize_text_with_wrap_clipped_into(text, &style, max_width, true, 0.0, 0.0, clip, &mut cached);

    assert_eq!(cached.len(), clipped_len);
    assert!(engine.glyph_hits >= cached.len());
    assert_eq!(engine.glyph_misses, 0);
  }

  #[test]
  fn wrapped_text_measurement_contains_painted_glyph_bounds() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle {
      font_size: 16.0,
      ..crate::layout::text_style::TextStyle::default()
    };
    let text = "Wrapping keeps long typography inside its assigned layout width.";
    let max_width = 260.0;
    let measured = engine.measure_text(text, &style, max_width);
    let glyphs = engine.rasterize_text(text, &style, max_width, 0.0, 0.0);
    let glyph_bottom = glyphs
      .iter()
      .map(|glyph| glyph.y + glyph.height)
      .fold(0.0_f32, f32::max);

    let transparent_padding = GLYPH_ATLAS_PADDING as f32;
    assert!(
      glyph_bottom <= measured.height.ceil() + transparent_padding,
      "painted glyph bottom should fit measured height plus transparent atlas padding: bottom={}, measured={}, padding={}",
      glyph_bottom,
      measured.height,
      transparent_padding
    );
  }

  #[test]
  fn wrapped_text_measurement_contains_scaled_painted_glyph_bounds() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle {
      font_size: 16.0,
      ..crate::layout::text_style::TextStyle::default()
    };
    let text = "Wrapping keeps long typography inside its assigned layout width.";
    let max_width = 260.0;
    let measured = engine.measure_text(text, &style, max_width);
    for scale in [1.1, 1.25, 1.5, 1.75, 2.0] {
      let mut scaled_style = style.clone();
      scaled_style.font_size *= scale;
      let glyphs = engine.rasterize_text(text, &scaled_style, max_width * scale, 0.0, 0.0);
      let glyph_bottom = glyphs
        .iter()
        .map(|glyph| glyph.y + glyph.height)
        .fold(0.0_f32, f32::max);

      let clip_bottom = (measured.height * scale).ceil();
      let transparent_padding = GLYPH_ATLAS_PADDING as f32;
      assert!(
        glyph_bottom <= clip_bottom + transparent_padding + 1.0,
        "painted scaled glyph bottom should fit scaled measured height plus rasterization slop and transparent atlas padding: bottom={}, clip={}, measured={}, scale={}, padding={}",
        glyph_bottom,
        clip_bottom,
        measured.height,
        scale,
        transparent_padding
      );
    }
  }

  #[test]
  fn max_float_text_width_is_unbounded() {
    assert!(!is_bounded_text_width(f32::MAX));
    assert!(!is_bounded_text_width(f32::INFINITY));
    assert!(is_bounded_text_width(320.0));
  }

  #[test]
  fn max_float_raster_width_does_not_wrap_text() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle {
      font_size: 13.0,
      line_height: 1.17,
      ..crate::layout::text_style::TextStyle::default()
    };
    let glyphs = engine.rasterize_text("0.1.8 -> 0.10.10", &style, f32::MAX, 0.0, 0.0);
    let max_y = glyphs.iter().map(|glyph| glyph.y).fold(0.0_f32, f32::max);

    assert!(
      max_y < style.font_size * style.line_height,
      "unbounded rasterization should stay on one line, max glyph y was {max_y}"
    );
  }

  #[test]
  fn rich_text_rasterization_reuses_layout_cache() {
    let mut engine = GlyphEngine::new();
    let base = crate::layout::text_style::TextStyle {
      font_size: 16.0,
      ..crate::layout::text_style::TextStyle::default()
    };
    let mut accent = base.clone();
    accent.color = crate::node::color::Color::from_hex("#2563eb");
    let spans = vec![
      crate::layout::quad::RichTextSpan {
        text: "Hello ".to_owned(),
        style: base,
      },
      crate::layout::quad::RichTextSpan {
        text: "rich text cache".to_owned(),
        style: accent,
      },
    ];

    let mut first = Vec::new();
    engine.rasterize_rich_text_with_wrap_into(&spans, 320.0, true, 0.0, 0.0, &mut first);
    assert!(!first.is_empty());

    engine.reset_stats();
    let mut second = Vec::new();
    engine.rasterize_rich_text_with_wrap_into(&spans, 320.0, true, 10.0, 20.0, &mut second);

    assert_eq!(first.len(), second.len());
    assert!(
      engine.glyph_hits >= second.len(),
      "second rich-text rasterization should hit the rich glyph layout cache"
    );
    assert_eq!(first[0].color, second[0].color);
    assert_eq!(first.last().unwrap().color, second.last().unwrap().color);
  }

  #[test]
  fn measured_rich_text_layout_feeds_rasterization() {
    let mut engine = GlyphEngine::new();
    let style = crate::layout::text_style::TextStyle {
      font_size: 16.0,
      ..crate::layout::text_style::TextStyle::default()
    };
    let spans = vec![
      crate::layout::quad::RichTextSpan {
        text: "Measured rich text ".to_owned(),
        style: style.clone(),
      },
      crate::layout::quad::RichTextSpan {
        text: "can be rasterized without shaping it again.".to_owned(),
        style,
      },
    ];

    let measured = engine.measure_rich_text(&spans, 1.0, 260.0);
    assert!(measured.width > 0.0);
    assert_eq!(engine.rich_shaped_layout_cache.len(), 1);

    engine.reset_stats();
    let mut glyphs = Vec::new();
    engine.rasterize_rich_text_with_wrap_into(&spans, 260.0, true, 0.0, 0.0, &mut glyphs);

    assert!(!glyphs.is_empty());
    assert_eq!(engine.rich_shaped_layout_cache.len(), 1);
    assert_eq!(engine.rich_glyph_layout_cache.len(), 1);
  }
}
