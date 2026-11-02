use std::sync::Arc;

use super::{PaletteColor, ThemePalette};
use crate::{
  components::SpanStyle,
  layout::text_style::{FontFeatures, FontStyle, FontWeight, TextStyle},
  node::{TextColor, color::Color},
};

/// How one mark of a styled text input changes the text it covers. Every
/// unset value keeps the text's own; colors are palette roles or colors.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextMarkStyle {
  pub font_family: Option<Arc<str>>,
  /// Multiplies the font size.
  pub font_size_scale: Option<f32>,
  pub weight: Option<FontWeight>,
  pub style: Option<FontStyle>,
  /// OpenType feature settings; replaces the text's settings.
  pub font_features: Option<FontFeatures>,
  pub color: Option<TextColor>,
  /// A fill behind the marked text, one per laid-out line.
  pub background: Option<TextColor>,
  /// Corner radius of the fill, in logical pixels.
  pub background_radius: f32,
  /// A line under the text, in its color.
  pub underline: bool,
  /// A line through the text, in its color.
  pub strikethrough: bool,
}

impl TextMarkStyle {
  pub fn new() -> Self {
    Self::default()
  }
}

/// The look of each mark of styled text inputs
/// ([`TextInput::spans`](crate::components::TextInput::spans)). A run with
/// several marks takes them in the order code, bold, italic, strikethrough,
/// link: a later mark's value wins where two set the same one.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeTextMarks {
  pub bold: TextMarkStyle,
  pub italic: TextMarkStyle,
  pub code: TextMarkStyle,
  pub strikethrough: TextMarkStyle,
  pub link: TextMarkStyle,
}

impl ThemeTextMarks {
  pub fn new() -> Self {
    Self::default()
  }

  /// The text style, fill and lines of a run with `style` over `base`.
  pub(crate) fn resolve(&self, style: &SpanStyle, base: &TextStyle, palette: &ThemePalette) -> ResolvedMarks {
    let mut resolved = ResolvedMarks {
      text: base.clone(),
      fill: None,
      underline: false,
      strikethrough: false,
    };
    let marks = [
      (style.code, &self.code),
      (style.bold, &self.bold),
      (style.italic, &self.italic),
      (style.strikethrough, &self.strikethrough),
      (style.link.is_some(), &self.link),
    ];
    for (_, mark) in marks.into_iter().filter(|(on, _)| *on) {
      resolved.apply(mark, palette);
    }
    resolved
  }
}

impl Default for ThemeTextMarks {
  fn default() -> Self {
    Self {
      bold: TextMarkStyle {
        weight: Some(FontWeight::Bold),
        ..TextMarkStyle::default()
      },
      italic: TextMarkStyle {
        style: Some(FontStyle::Italic),
        ..TextMarkStyle::default()
      },
      code: TextMarkStyle {
        font_family: Some(Arc::from("monospace")),
        font_size_scale: Some(0.92),
        background: Some(TextColor::Palette(PaletteColor::SurfaceRaised)),
        background_radius: 3.0,
        ..TextMarkStyle::default()
      },
      strikethrough: TextMarkStyle {
        strikethrough: true,
        ..TextMarkStyle::default()
      },
      link: TextMarkStyle {
        color: Some(TextColor::Palette(PaletteColor::Accent)),
        underline: true,
        ..TextMarkStyle::default()
      },
    }
  }
}

/// A run's look once its marks are applied.
#[derive(Clone, PartialEq)]
pub(crate) struct ResolvedMarks {
  pub(crate) text: TextStyle,
  pub(crate) fill: Option<(Color, f32)>,
  pub(crate) underline: bool,
  pub(crate) strikethrough: bool,
}

impl ResolvedMarks {
  fn apply(&mut self, mark: &TextMarkStyle, palette: &ThemePalette) {
    let text = &mut self.text;
    if let Some(family) = &mark.font_family {
      text.font_family = family.clone();
    }
    if let Some(scale) = mark.font_size_scale.filter(|scale| scale.is_finite() && *scale > 0.0) {
      text.font_size *= scale;
    }
    if let Some(weight) = mark.weight {
      text.weight = weight;
    }
    if let Some(style) = mark.style {
      text.style = style;
    }
    if let Some(features) = &mark.font_features {
      text.font_features = features.clone();
    }
    if let Some(color) = mark.color.as_ref().and_then(|color| color.resolve(palette)) {
      text.color = color;
    }
    if let Some(color) = mark.background.as_ref().and_then(|color| color.resolve(palette)) {
      let radius = if mark.background_radius.is_finite() {
        mark.background_radius.max(0.0)
      } else {
        0.0
      };
      self.fill = Some((color, radius));
    }
    self.underline |= mark.underline;
    self.strikethrough |= mark.strikethrough;
  }
}
