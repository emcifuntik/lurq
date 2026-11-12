use std::sync::Arc;

use crate::{
  layout::Alignment,
  node::{TextColor, color::Color},
};

mod font_features;
mod generic_family;
pub use font_features::{FontFeature, FontFeatures};
pub use generic_family::GenericFamily;
pub(crate) use generic_family::{alias_name, cosmic_family};

const DEFAULT_FONT_SIZE: f32 = 16.0;
const DEFAULT_LINE_HEIGHT: f32 = 1.2;
/// The color of `TextStyle::default()`. A typography style that keeps it names no color: its text is drawn in
/// the theme's foreground (`ThemePalette::foreground`).
pub(crate) const DEFAULT_TEXT_COLOR: Color = Color::new(0, 0, 0, 255);
#[cfg(target_os = "windows")]
const DEFAULT_FONT_FAMILY_WINDOWS: &str = "Segoe UI";
#[cfg(not(target_os = "windows"))]
const DEFAULT_FONT_FAMILY_FALLBACK: &str = "";

#[derive(Clone, PartialEq)]
pub struct TextStyle {
  /// The font family: a family name, an alias registered with
  /// `App::register_font` (resolved first), or a CSS generic family
  /// (`"serif"`, `"sans-serif"`, `"monospace"`, `"cursive"`, `"fantasy"`, in
  /// any ASCII case), drawn with the platform's font for it. See
  /// [`GenericFamily`] for the fonts each platform uses and for drawing one in
  /// a bundled font.
  pub font_family: Arc<str>,
  pub font_size: f32,
  pub line_height: f32,
  pub weight: FontWeight,
  pub style: FontStyle,
  pub text_align: TextAlign,
  /// Vertical placement of the glyph run within its box (the line-height box
  /// for a single line, or the node's box when it is taller). Alignment is
  /// based on the actual glyph ink bounds so text is optically centered rather
  /// than metric-centered (which leaves most fonts sitting visibly high).
  pub vertical_align: VerticalAlign,
  /// CSS `text-box-trim`-style leading trim. When set, `line_height` only adds
  /// leading BETWEEN wrapped lines: the measured box of a single line collapses
  /// to the em box (`font_size`) instead of `font_size * line_height`, and a
  /// multi-line run's box drops the half-leading above the first line and below
  /// the last. Glyph placement is unaffected — the render path already centers
  /// the optical box within the (now trimmed) box — so a single line looks
  /// identical to `line_height: 1.0` while wrapped lines keep their full leading.
  /// Lets a caller pick one readable `line_height` for both single- and
  /// multi-line text without inflating single-line vertical rhythm.
  pub trim_line_box: bool,
  /// Extra space added after every glyph, in logical pixels, like CSS
  /// `letter-spacing` (including after the last glyph of a line). Negative values
  /// tighten text. It scales with the display scale factor like `font_size`, and
  /// measurement, wrapping, painting, carets and hit testing all use the spaced
  /// advances. Non-finite values are treated as `0.0`.
  pub letter_spacing: f32,
  /// OpenType feature settings for shaping, like CSS `font-feature-settings`.
  /// Empty by default, which keeps the shaper's default features — including
  /// ligatures and contextual alternates. Programming ligatures can misrender
  /// commands and code (Geist Mono shapes `--` into one cell-wide glyph drawn
  /// over the preceding space, so ` --flag` reads as `--flag`); turn them off with
  /// `FontFeatures::new([FontFeature::disable(*b"liga"), FontFeature::disable(*b"calt")])`.
  pub font_features: FontFeatures,
  pub color: Color,
  pub caret_color: Option<TextColor>,
  pub shadow: Option<TextShadow>,
  /// A line under the text, like CSS `text-decoration-line: underline`.
  /// `None` by default. It is drawn under every laid-out line the text
  /// covers, wrapped lines included, and changes neither measurement nor
  /// wrapping.
  pub underline: Option<TextUnderline>,
  /// A line through the text, like CSS `text-decoration-line: line-through`.
  /// `None` by default. Drawn as the underline is, over the glyphs.
  pub strikethrough: Option<TextStrikethrough>,
}

impl Default for TextStyle {
  fn default() -> Self {
    Self {
      font_family: default_font_family(),
      font_size: DEFAULT_FONT_SIZE,
      line_height: DEFAULT_LINE_HEIGHT,
      weight: FontWeight::Normal,
      style: FontStyle::Normal,
      text_align: TextAlign::Left,
      vertical_align: VerticalAlign::default(),
      trim_line_box: false,
      letter_spacing: 0.0,
      font_features: FontFeatures::default(),
      color: DEFAULT_TEXT_COLOR,
      caret_color: None,
      shadow: None,
      underline: None,
      strikethrough: None,
    }
  }
}

impl TextStyle {
  /// Scales the pixel metrics (font size and letter spacing) for shaping at
  /// `factor` device pixels per logical pixel.
  pub(crate) fn scale_pixels(&mut self, factor: f32) {
    self.font_size *= factor;
    self.letter_spacing *= factor;
  }
}

/// An underline, like CSS `text-decoration: underline` with
/// `text-decoration-color`, `text-decoration-thickness` and
/// `text-underline-offset`. Each unset value follows the text: the text's
/// color, and the thickness and position the font gives its underline.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextUnderline {
  /// The line's color; `None` is the text's color.
  pub color: Option<Color>,
  /// The line's thickness in logical pixels; `None` is the font's underline
  /// thickness. Non-finite or negative values count as unset.
  pub thickness: Option<f32>,
  /// How far below the baseline the line's top edge sits, in logical pixels;
  /// `None` is the font's underline position. Non-finite values count as
  /// unset.
  pub offset: Option<f32>,
}

impl TextUnderline {
  /// An underline in the text's color, at the font's thickness and position.
  pub fn new() -> Self {
    Self::default()
  }

  pub fn color(mut self, color: Color) -> Self {
    self.color = Some(color);
    self
  }

  pub fn thickness(mut self, thickness: f32) -> Self {
    self.thickness = Some(thickness);
    self
  }

  pub fn offset(mut self, offset: f32) -> Self {
    self.offset = Some(offset);
    self
  }
}

/// A line through the text, like CSS `text-decoration: line-through` with
/// `text-decoration-color` and `text-decoration-thickness`. Each unset value
/// follows the text: the text's color, and the thickness the font gives its
/// strikeout. The line sits where the font puts its strikeout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextStrikethrough {
  /// The line's color; `None` is the text's color.
  pub color: Option<Color>,
  /// The line's thickness in logical pixels; `None` is the font's strikeout
  /// thickness. Non-finite or negative values count as unset.
  pub thickness: Option<f32>,
}

impl TextStrikethrough {
  /// A line through the text in its color, at the font's thickness.
  pub fn new() -> Self {
    Self::default()
  }

  pub fn color(mut self, color: Color) -> Self {
    self.color = Some(color);
    self
  }

  pub fn thickness(mut self, thickness: f32) -> Self {
    self.thickness = Some(thickness);
    self
  }
}

/// CSS-like text shadow: offsets and blur radius are in logical pixels, the
/// blur radius maps to a Gaussian with `sigma = blur_radius / 2`.
#[derive(Clone, Copy, PartialEq)]
pub struct TextShadow {
  pub offset_x: f32,
  pub offset_y: f32,
  pub blur_radius: f32,
  pub color: Color,
}

impl TextShadow {
  pub fn new(offset_x: f32, offset_y: f32, blur_radius: f32, color: Color) -> Self {
    Self {
      offset_x,
      offset_y,
      blur_radius: blur_radius.max(0.0),
      color,
    }
  }

  pub(crate) fn is_visible(&self) -> bool {
    self.color.a() > 0 && (self.offset_x != 0.0 || self.offset_y != 0.0 || self.blur_radius > 0.0)
  }
}

fn default_font_family() -> Arc<str> {
  #[cfg(target_os = "windows")]
  {
    Arc::from(DEFAULT_FONT_FAMILY_WINDOWS)
  }
  #[cfg(not(target_os = "windows"))]
  {
    Arc::from(DEFAULT_FONT_FAMILY_FALLBACK)
  }
}

/// CSS font weight. The named variants carry the CSS keyword values (`Thin` =
/// 100 … `Black` = 900); `Numeric` accepts any other weight and is clamped to
/// CSS's `1..=1000` range. Weights compare, hash and cache by numeric value, so
/// `FontWeight::Numeric(600) == FontWeight::SemiBold`.
///
/// Text is drawn with the loaded face of the requested family chosen by CSS
/// font matching (as implemented by fontdb): an exact weight first; for a
/// request of 400–449, then 500; for 450–500, then 400; otherwise the nearest
/// lighter face for requests up to 500 and the nearest heavier face above 500,
/// then the nearest face on the other side. Faces are never synthesized, so
/// `SemiBold` renders with the Bold face when the family has no 600 face.
#[derive(Clone, Copy, Debug, Default)]
pub enum FontWeight {
  Thin,
  ExtraLight,
  Light,
  #[default]
  Normal,
  Medium,
  SemiBold,
  Bold,
  ExtraBold,
  Black,
  Numeric(u16),
}

impl FontWeight {
  pub const MIN_VALUE: u16 = 1;
  pub const MAX_VALUE: u16 = 1000;

  /// The CSS numeric weight, clamped to `1..=1000`.
  pub const fn value(self) -> u16 {
    match self {
      Self::Thin => 100,
      Self::ExtraLight => 200,
      Self::Light => 300,
      Self::Normal => 400,
      Self::Medium => 500,
      Self::SemiBold => 600,
      Self::Bold => 700,
      Self::ExtraBold => 800,
      Self::Black => 900,
      Self::Numeric(value) => {
        if value < Self::MIN_VALUE {
          Self::MIN_VALUE
        } else if value > Self::MAX_VALUE {
          Self::MAX_VALUE
        } else {
          value
        }
      }
    }
  }

  /// The requested weight. Layout passes the weight of the nearest loaded face
  /// instead, because cosmic-text only selects faces of exactly this weight.
  pub fn to_cosmic(&self) -> cosmic_text::Weight {
    cosmic_text::Weight(self.value())
  }
}

impl PartialEq for FontWeight {
  fn eq(&self, other: &Self) -> bool {
    self.value() == other.value()
  }
}

impl Eq for FontWeight {}

impl std::hash::Hash for FontWeight {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.value().hash(state);
  }
}

impl From<u16> for FontWeight {
  fn from(value: u16) -> Self {
    Self::Numeric(value)
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontStyle {
  #[default]
  Normal,
  Italic,
}

impl FontStyle {
  pub fn to_cosmic(&self) -> cosmic_text::Style {
    match self {
      Self::Normal => cosmic_text::Style::Normal,
      Self::Italic => cosmic_text::Style::Italic,
    }
  }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum TextAlign {
  #[default]
  Left,
  Center,
  Right,
  Justified,
  End,
}

impl TextAlign {
  pub(crate) fn to_cosmic(self) -> cosmic_text::Align {
    match self {
      Self::Left => cosmic_text::Align::Left,
      Self::Center => cosmic_text::Align::Center,
      Self::Right => cosmic_text::Align::Right,
      Self::Justified => cosmic_text::Align::Justified,
      Self::End => cosmic_text::Align::End,
    }
  }
}

/// Vertical alignment of a text run's glyph ink within its box, mirroring
/// browser `vertical-align` semantics against the line-height box:
/// - `Top`: the top of the glyph ink meets the top of the box.
/// - `Bottom`: the bottom of the glyph ink meets the bottom of the box.
/// - `Center`: the ink is centered — `offset = (box_height - ink_height) / 2`.
/// - `LineBox`: no ink at all: the first line box starts at the top of the box, and each line's glyphs sit on the
///   baseline the font's ascent, descent and the line height give it, like CSS text. The run keeps its position when
///   its glyphs change, and carets and selections line up with it. Text inputs place their lines this way.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum VerticalAlign {
  Top,
  #[default]
  Center,
  Bottom,
  LineBox,
}

impl From<Alignment> for TextAlign {
  fn from(alignment: Alignment) -> Self {
    match alignment {
      Alignment::Start | Alignment::Stretch => Self::Left,
      Alignment::Center => Self::Center,
      Alignment::End => Self::Right,
    }
  }
}
