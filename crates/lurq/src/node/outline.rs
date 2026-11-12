use crate::{
  app::theme::{ThemeBorderSizes, ThemePalette},
  node::{BackgroundColor, border_size_value::BorderSizeValue, color::Color},
};

/// A line drawn around an element, outside its box, like CSS `outline` with
/// `outline-offset`: a focus ring. It takes no room in layout and never moves
/// the element or what is around it; it follows the element's corner radii.
/// It is painted over the element's content and inside the clips the element
/// is under, as its outer shadows are.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
  pub width: BorderSizeValue,
  pub color: BackgroundColor,
  /// The gap between the element's box and the line, in logical pixels.
  /// Negative values draw the line inside the box.
  pub offset: f32,
}

/// An outline with its width and color resolved against the theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedOutline {
  pub(crate) width: f32,
  pub(crate) color: Color,
  pub(crate) offset: f32,
}

impl Outline {
  /// A line of `width` (logical pixels or a `BorderSize` role) in `color` (a
  /// concrete color or a `PaletteColor`), touching the element's box.
  pub fn new(width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) -> Self {
    Self {
      width: width.into(),
      color: color.into(),
      offset: 0.0,
    }
  }

  /// No outline. A state style uses it to remove the element's own.
  pub fn none() -> Self {
    Self::new(0.0, Color::new(0, 0, 0, 0))
  }

  /// Moves the line `offset` logical pixels away from the element's box.
  pub fn offset(mut self, offset: f32) -> Self {
    self.offset = offset;
    self
  }

  /// `None` for an outline that paints nothing: no width, or a color the
  /// palette does not have.
  pub(crate) fn resolve(&self, palette: &ThemePalette, border_sizes: &ThemeBorderSizes) -> Option<ResolvedOutline> {
    let width = self.width.resolve(border_sizes);
    if width.is_nan() || width <= 0.0 || !self.offset.is_finite() {
      return None;
    }
    Some(ResolvedOutline {
      width,
      color: self.color.resolve(palette)?,
      offset: self.offset,
    })
  }
}

impl ResolvedOutline {
  /// How far past the element's box the line reaches on each side.
  pub(crate) fn outset(&self) -> f32 {
    self.offset + self.width
  }
}
