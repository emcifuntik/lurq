use std::sync::{
  Arc, Mutex,
  atomic::{AtomicBool, Ordering},
};

pub(crate) use crate::node::select_state::{SelectChangeCallback, SelectState};
use crate::{
  app::{
    events::{KeyboardEvent, TextInputEvent},
    theme::{ThemePalette, ThemeTypography, TypographyStyle},
  },
  core::Signal,
  layout::{
    layout_kind::ScrollState,
    text_style::{TextAlign, TextStyle},
  },
  node::{
    CheckboxStyle, EventHandler, IntoTextInputEventHandler, SliderPartStyle, TextColor, TextTransformMode,
    text_selection::{
      CaretPosition, CaretPositions, TextSelectionRange, caret_x_for_index, caret_y_for_index, clamp_to_char_boundary,
      closest_caret_on_adjacent_row, closest_caret_to_point, line_bounds, next_char_boundary, next_delete_boundary,
      next_word_boundary, previous_char_boundary, previous_delete_boundary, previous_word_boundary,
      selection_range_indices, selection_ranges_for_positions, trailing_break_len, trailing_break_row_y,
      word_selection_bounds,
    },
  },
};

mod checkbox;
mod slider;
mod text_input;

pub(crate) use checkbox::CheckboxState;
pub(crate) use slider::{SliderPartRect, SliderState};
pub(crate) use text_input::{TextInputState, WeakTextInputState};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextInputOverflow {
  Multiline,
  #[default]
  Scroll,
}

/// The on-screen keyboard a text input asks for, on a platform that has kinds of them: a phone shows digits
/// for a number and an `@` key for an email address. Elsewhere it changes nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyboardKind {
  /// Letters, with the rest a switch away.
  #[default]
  Text,
  Email,
  Url,
  /// Digits only: a PIN, a code.
  Number,
  /// Digits and a decimal point.
  Decimal,
  Phone,
  /// Letters with a key that says Search.
  Search,
}

/// What a text input holds, for a platform that fills inputs in from what it knows of the person: a phone
/// offers their email addresses above the keyboard, a saved password, the code from a message that just came.
/// Elsewhere it changes nothing.
///
/// Without one, an input that asks for an email, a phone or a URL keyboard says it holds that, and a masked
/// input that it holds a password.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContentKind {
  /// Whatever the keyboard and the mask say.
  #[default]
  Auto,
  /// Nothing the platform should offer to fill in.
  None,
  Name,
  Email,
  Username,
  Password,
  /// A password being chosen: the platform may suggest a strong one.
  NewPassword,
  /// A code sent to the person to prove who they are.
  OneTimeCode,
  Phone,
  Url,
}

/// What the on-screen keyboard's Return key says, on a platform that labels it. The key sends Enter whatever
/// it says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReturnKey {
  #[default]
  Default,
  Done,
  Go,
  Next,
  Search,
  Send,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextInputOverflowAnchor {
  #[default]
  Start,
  End,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextOverflow {
  #[default]
  Clip,
  Elipsis,
}

/// An image a node waits for from the decoded-image cache.
#[cfg(feature = "image")]
pub(crate) struct LoadingImage {
  pub(crate) key: crate::images::ImageKey,
  pub(crate) cap: Option<crate::images::ImageSizeCap>,
  pub(crate) source: crate::images::ImageSource,
  pub(crate) fit: crate::node::BackgroundSize,
  pub(crate) error_view: Option<crate::node::node_kind::ErrorViewCallback>,
  /// The error view was built: the decode failed and the children show it.
  pub(crate) failed: bool,
}

/// A clone is a new node that waits for the cache (a retained child put
/// back into a re-rendered parent), so it counts as built for the walk that
/// resolves loading images.
#[cfg(feature = "image")]
impl Clone for LoadingImage {
  fn clone(&self) -> Self {
    crate::app::runtime::note_loading_image_built();
    Self {
      key: self.key,
      cap: self.cap,
      source: self.source.clone(),
      fit: self.fit,
      error_view: self.error_view.clone(),
      failed: self.failed,
    }
  }
}

/// Builds what an image node shows once its decode failed.
#[cfg(feature = "image")]
pub(crate) type ErrorViewCallback = Arc<dyn Fn(&crate::images::ImageDecodeError) -> crate::node::Element + Send + Sync>;

#[derive(Clone)]
pub(crate) enum NodeKind {
  Empty,
  #[cfg(feature = "canvas")]
  Canvas {
    canvas: crate::canvas::CanvasHandle,
  },
  Text {
    state: TextState,
    style: TextStyleSource,
    transform_mode: TextTransformMode,
  },
  #[cfg(feature = "markdown")]
  RichText {
    state: TextState,
    spans: Vec<crate::layout::quad::RichTextSpan>,
    transform_mode: TextTransformMode,
    /// Spans drawn as presentation, not part of the text (a link's isolate
    /// marks and trailing runs): left out of copied and inspected text and
    /// of selection highlights. Sorted span index ranges.
    generated: std::sync::Arc<[std::ops::Range<usize>]>,
  },
  /// A link inside its parent `RichText` paragraph; see
  /// [`crate::node::inline_link`].
  #[cfg(feature = "markdown")]
  InlineLink {
    link: crate::node::inline_link::InlineLink,
  },
  TextInput {
    state: TextInputState,
    style: TextStyle,
    /// Boxed: most inputs have none, and every node is as large as its
    /// largest kind (see `size_budget`).
    placeholder_style: Option<Box<TextStyle>>,
  },
  Checkbox {
    state: CheckboxState,
  },
  Slider {
    state: SliderState,
  },
  Select {
    state: SelectState,
  },
  #[cfg(feature = "raster")]
  Image {
    data: crate::images::ImageData,
    /// How the pixels fill the node's box; `Stretch` fills it as the box
    /// is sized.
    fit: crate::node::BackgroundSize,
  },
  /// An image decoded at runtime that has not been resolved yet: the
  /// runtime asks the decoded-image cache each pass and turns the node into
  /// an `Image` once the pixels are there. Its children are the placeholder
  /// while loading, or the error view once failed.
  #[cfg(feature = "image")]
  LoadingImage {
    request: Box<LoadingImage>,
  },
  #[cfg(feature = "raster")]
  Video {
    data: crate::images::ImageData,
    fit: crate::node::BackgroundSize,
  },
  #[cfg(feature = "image")]
  ResourceImage {
    path: Arc<str>,
  },
  #[cfg(feature = "svg")]
  Svg {
    data: crate::svg::SvgData,
  },
  #[cfg(all(feature = "svg", feature = "resources"))]
  ResourceSvg {
    path: Arc<str>,
  },
}

#[derive(Clone, PartialEq)]
pub(crate) struct TextStyleSource {
  base: TextStyleBase,
  color: Option<TextColor>,
  text_align: Option<TextAlign>,
  letter_spacing: Option<f32>,
  font_features: Option<crate::layout::text_style::FontFeatures>,
  shadow: Option<crate::layout::text_style::TextShadow>,
  /// What few texts set. Boxed: a text's style is part of every node.
  rare: Option<Box<RareTextStyle>>,
}

#[derive(Clone, Default, PartialEq)]
struct RareTextStyle {
  underline: Option<crate::layout::text_style::TextUnderline>,
  strikethrough: Option<crate::layout::text_style::TextStrikethrough>,
  /// The most lines the text is laid out in; see `Text::max_lines`.
  max_lines: Option<u32>,
}

#[derive(Clone, PartialEq)]
enum TextStyleBase {
  Default,
  Typography(TypographyStyle),
  Explicit(TextStyle),
}

impl TextStyleSource {
  pub(crate) fn default_style() -> Self {
    Self {
      base: TextStyleBase::Default,
      color: None,
      text_align: None,
      letter_spacing: None,
      font_features: None,
      shadow: None,
      rare: None,
    }
  }

  pub(crate) fn explicit(style: TextStyle) -> Self {
    Self {
      base: TextStyleBase::Explicit(style),
      color: None,
      text_align: None,
      letter_spacing: None,
      font_features: None,
      shadow: None,
      rare: None,
    }
  }

  pub(crate) fn set_variant(&mut self, style: impl Into<TypographyStyle>) {
    self.base = TextStyleBase::Typography(style.into());
  }

  pub(crate) fn set_color(&mut self, color: impl Into<TextColor>) {
    self.color = Some(color.into());
  }

  /// Whether the text names its color: one set on it, or the color of the
  /// style it was built from. Otherwise its color is its typography style's,
  /// which a foreground above it replaces.
  pub(crate) fn names_color(&self) -> bool {
    self.color.is_some() || matches!(self.base, TextStyleBase::Explicit(_))
  }

  pub(crate) fn set_text_align(&mut self, align: impl Into<TextAlign>) {
    self.text_align = Some(align.into());
  }

  pub(crate) fn set_letter_spacing(&mut self, letter_spacing: f32) {
    self.letter_spacing = Some(letter_spacing);
  }

  pub(crate) fn set_font_features(&mut self, font_features: crate::layout::text_style::FontFeatures) {
    self.font_features = Some(font_features);
  }

  pub(crate) fn set_shadow(&mut self, shadow: crate::layout::text_style::TextShadow) {
    self.shadow = Some(shadow);
  }

  fn rare_mut(&mut self) -> &mut RareTextStyle {
    self.rare.get_or_insert_with(Box::default)
  }

  pub(crate) fn set_underline(&mut self, underline: crate::layout::text_style::TextUnderline) {
    self.rare_mut().underline = Some(underline);
  }

  pub(crate) fn set_strikethrough(&mut self, strikethrough: crate::layout::text_style::TextStrikethrough) {
    self.rare_mut().strikethrough = Some(strikethrough);
  }

  pub(crate) fn set_max_lines(&mut self, max_lines: u32) {
    self.rare_mut().max_lines = Some(max_lines);
  }

  pub(crate) fn max_lines(&self) -> Option<u32> {
    self.rare.as_ref().and_then(|rare| rare.max_lines)
  }

  pub(crate) fn resolve(&self, typography: &ThemeTypography, palette: &ThemePalette) -> TextStyle {
    let mut style = match &self.base {
      TextStyleBase::Default => typography.default_style().clone(),
      TextStyleBase::Typography(style) => typography.resolve_or_default(*style),
      TextStyleBase::Explicit(style) => style.clone(),
    };
    // A style that keeps the default text color names none: the theme's foreground is its color.
    style.color = palette.text_color(&style);
    if let Some(color) = self.color.as_ref().and_then(|color| color.resolve(palette)) {
      style.color = color;
    }
    if let Some(text_align) = self.text_align {
      style.text_align = text_align;
    }
    if let Some(letter_spacing) = self.letter_spacing {
      style.letter_spacing = letter_spacing;
    }
    if let Some(font_features) = &self.font_features {
      style.font_features = font_features.clone();
    }
    if let Some(shadow) = self.shadow {
      style.shadow = Some(shadow);
    }
    if let Some(rare) = &self.rare {
      if let Some(underline) = rare.underline {
        style.underline = Some(underline);
      }
      if let Some(strikethrough) = rare.strikethrough {
        style.strikethrough = Some(strikethrough);
      }
    }
    style
  }
}

#[derive(Clone)]
pub(crate) struct TextState {
  inner: Arc<Mutex<TextInner>>,
}

/// The carets of a text's painted layout: in device pixels, `scale` of
/// them per logical pixel. The paint shapes a text at the display scale,
/// and the carets stay in its pixels; a point is scaled into them and a
/// highlight out of them when asked, so a relayout of a large text at a
/// fractional scale copies nothing.
#[derive(Clone)]
pub(crate) struct PaintedCarets {
  pub(crate) positions: CaretPositions,
  pub(crate) scale: f32,
}

impl PaintedCarets {
  /// The carets of a text that was not laid out yet: one at its start.
  fn unmeasured() -> Self {
    Self {
      positions: vec![CaretPosition {
        index: 0,
        x: 0.0,
        y: 0.0,
      }]
      .into(),
      scale: 1.0,
    }
  }
}

struct TextInner {
  selectable: bool,
  /// Inspectors show `REDACTED` instead of the content (`Text::sensitive`).
  sensitive: bool,
  caret: usize,
  selection_anchor: Option<usize>,
  carets: PaintedCarets,
  /// Where the text's first line sits below the top of its box, as painted,
  /// in logical pixels.
  text_top: f32,
  display_text: Option<Arc<str>>,
  render_wrap: bool,
  /// Fills behind ranges of the text (`Text::highlights`).
  highlights: Arc<[crate::node::text_highlight::TextHighlight]>,
}

impl TextInner {
  /// The parts of the lines that bytes `start..end` of `value` cover, in
  /// logical pixels in the text's box.
  fn line_ranges(&self, value: &str, start: usize, end: usize) -> Vec<TextSelectionRange> {
    let scale = self.carets.scale;
    selection_ranges_for_positions(&self.carets.positions, Some(value), start, end, 0.0, 0.0)
      .into_iter()
      .map(|range| TextSelectionRange {
        x: range.x / scale,
        y: range.y / scale + self.text_top,
        width: range.width / scale,
      })
      .collect()
  }
}

impl TextState {
  pub(crate) fn new() -> Self {
    Self {
      inner: Arc::new(Mutex::new(TextInner {
        selectable: false,
        sensitive: false,
        caret: 0,
        selection_anchor: None,
        carets: PaintedCarets::unmeasured(),
        text_top: 0.0,
        display_text: None,
        render_wrap: false,
        highlights: Arc::from([]),
      })),
    }
  }

  pub(crate) fn set_selectable(&self, selectable: bool) {
    let mut inner = self.inner.lock().unwrap();
    inner.selectable = selectable;
    if !selectable {
      inner.selection_anchor = None;
      inner.caret = 0;
    }
  }

  pub(crate) fn selectable(&self) -> bool {
    self.inner.lock().unwrap().selectable
  }

  pub(crate) fn set_highlights(&self, highlights: Arc<[crate::node::text_highlight::TextHighlight]>) {
    self.inner.lock().unwrap().highlights = highlights;
  }

  /// Whether layout measures the carets of the painted text: a selection
  /// and highlights are drawn from them.
  pub(crate) fn measures_carets(&self) -> bool {
    let inner = self.inner.lock().unwrap();
    inner.selectable || !inner.highlights.is_empty()
  }

  pub(crate) fn set_sensitive(&self) {
    self.inner.lock().unwrap().sensitive = true;
  }

  pub(crate) fn is_sensitive(&self) -> bool {
    self.inner.lock().unwrap().sensitive
  }

  pub(crate) fn update_selection_to_point(&self, value: &str, x: f32, y: f32) {
    if !self.selectable() {
      return;
    }
    let caret = self.caret_index_at_point(value, x, y);
    self.inner.lock().unwrap().caret = caret;
  }

  pub(crate) fn clear_selection_at_point(&self, value: &str, x: f32, y: f32) {
    let caret = self.caret_index_at_point(value, x, y);
    let mut inner = self.inner.lock().unwrap();
    inner.caret = caret;
    inner.selection_anchor = None;
  }

  pub(crate) fn clear_selection(&self) {
    self.inner.lock().unwrap().selection_anchor = None;
  }

  pub(crate) fn set_selection_indices(&self, value: &str, anchor: usize, caret: usize) {
    if !self.selectable() {
      return;
    }
    let mut inner = self.inner.lock().unwrap();
    inner.selection_anchor = Some(clamp_to_char_boundary(value, anchor));
    inner.caret = clamp_to_char_boundary(value, caret);
  }

  pub(crate) fn select_word_at_point(&self, value: &str, x: f32, y: f32) {
    if !self.selectable() {
      return;
    }
    let caret = self.caret_index_at_point(value, x, y);
    let (start, end) = word_selection_bounds(value, caret);
    let mut inner = self.inner.lock().unwrap();
    inner.selection_anchor = Some(start);
    inner.caret = end;
  }

  pub(crate) fn select_line_at_point(&self, value: &str, x: f32, y: f32) {
    if !self.selectable() {
      return;
    }
    let caret = self.caret_index_at_point(value, x, y);
    let (start, end) = line_bounds(value, caret);
    let mut inner = self.inner.lock().unwrap();
    inner.selection_anchor = Some(start);
    inner.caret = end;
  }

  pub(crate) fn has_selection(&self, value: &str) -> bool {
    let inner = self.inner.lock().unwrap();
    selection_range_indices(value, inner.selection_anchor, inner.caret).is_some()
  }

  /// Where the selection started, if one is in progress or made.
  pub(crate) fn selection_anchor(&self) -> Option<usize> {
    self.inner.lock().unwrap().selection_anchor
  }

  pub(crate) fn selected_text(&self, value: &str) -> Option<String> {
    self.selected_range(value).map(|range| value[range].to_owned())
  }

  /// The selected bytes of `value`.
  pub(crate) fn selected_range(&self, value: &str) -> Option<std::ops::Range<usize>> {
    let inner = self.inner.lock().unwrap();
    let (start, end) = selection_range_indices(value, inner.selection_anchor, inner.caret)?;
    Some(start..end)
  }

  pub(crate) fn selection_ranges(&self, value: &str) -> Vec<TextSelectionRange> {
    if !self.selectable() {
      return Vec::new();
    }
    let inner = self.inner.lock().unwrap();
    let Some((start, end)) = selection_range_indices(value, inner.selection_anchor, inner.caret) else {
      return Vec::new();
    };
    inner.line_ranges(value, start, end)
  }

  /// Each highlight with the parts of the lines it covers, in logical pixels
  /// in the text's box, in the order given.
  pub(crate) fn highlight_ranges(
    &self,
    value: &str,
  ) -> Vec<(crate::node::text_highlight::TextHighlight, Vec<TextSelectionRange>)> {
    let inner = self.inner.lock().unwrap();
    inner
      .highlights
      .iter()
      .filter_map(|highlight| {
        let start = clamp_to_char_boundary(value, highlight.range.start);
        let end = clamp_to_char_boundary(value, highlight.range.end);
        (start < end).then(|| (highlight.clone(), inner.line_ranges(value, start, end)))
      })
      .collect()
  }

  /// Writes what laying the text out under one set of constraints produced:
  /// the string to draw instead of the content, whether the renderer wraps,
  /// and, for selectable text, the carets of the painted layout (`None`
  /// keeps the current ones) and where its first line sits in the box.
  pub(crate) fn set_layout_output(
    &self,
    display_text: Option<&Arc<str>>,
    render_wrap: bool,
    carets: Option<&PaintedCarets>,
    text_top: f32,
  ) {
    let mut inner = self.inner.lock().unwrap();
    // Serving a cached layout usually writes back what the state already
    // holds; skip the reference-count traffic then.
    if inner.display_text.as_ref().map(Arc::as_ptr) != display_text.map(Arc::as_ptr) {
      inner.display_text = display_text.cloned();
    }
    inner.render_wrap = render_wrap;
    inner.text_top = text_top;
    if let Some(carets) = carets {
      inner.carets = carets.clone();
    }
  }

  pub(crate) fn display_text(&self) -> Option<Arc<str>> {
    self.inner.lock().unwrap().display_text.clone()
  }

  pub(crate) fn render_wrap(&self) -> bool {
    self.inner.lock().unwrap().render_wrap
  }

  pub(crate) fn copy_runtime_state_from(&self, old: &Self, value: &str, preserve_display_text: bool) {
    if Arc::ptr_eq(&self.inner, &old.inner) {
      return;
    }
    let old_inner = old.inner.lock().unwrap();
    let selectable = self.selectable();
    let len = value.len();
    let mut inner = self.inner.lock().unwrap();
    inner.selectable = selectable;
    inner.display_text = if preserve_display_text {
      old_inner.display_text.clone()
    } else {
      None
    };
    inner.render_wrap = old_inner.render_wrap;
    inner.text_top = old_inner.text_top;
    if selectable {
      inner.caret = old_inner.caret.min(len);
      inner.selection_anchor = old_inner.selection_anchor.map(|anchor| anchor.min(len));
    } else {
      inner.caret = 0;
      inner.selection_anchor = None;
    }
    if selectable || !inner.highlights.is_empty() {
      inner.carets = old_inner.carets.clone();
    }
  }

  /// The caret nearest to `(x, y)` in the text's box, in logical pixels.
  pub(crate) fn caret_index_at_point(&self, value: &str, x: f32, y: f32) -> usize {
    let inner = self.inner.lock().unwrap();
    let scale = inner.carets.scale;
    clamp_to_char_boundary(
      value,
      closest_caret_to_point(&inner.carets.positions, x * scale, (y - inner.text_top) * scale),
    )
  }
}
