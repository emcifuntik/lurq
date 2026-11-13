use crate::{
  app::theme::{ThemeBorderSizes, ThemePalette, ThemeRadii},
  node::{BackgroundColor, border_size_value::BorderSizeValue, color::Color, radius_value::RadiusValue},
};

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct BorderRadius {
  pub top_left: f32,
  pub top_right: f32,
  pub bottom_right: f32,
  pub bottom_left: f32,
}

impl BorderRadius {
  pub fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
    Self {
      top_left,
      top_right,
      bottom_right,
      bottom_left,
    }
  }

  pub fn all(r: f32) -> Self {
    Self {
      top_left: r,
      top_right: r,
      bottom_right: r,
      bottom_left: r,
    }
  }

  pub fn to_array(&self) -> [f32; 4] {
    [self.top_left, self.top_right, self.bottom_right, self.bottom_left]
  }

  /// CSS-style overlap normalization: scale all corner radii down so adjacent
  /// radii never exceed the box side they share. Pill shapes commonly use an
  /// oversized radius (e.g. 999) — unclamped, a rounded CLIP with such a
  /// radius has no interior at all, and everything inside it is discarded by
  /// the shader-side clip (invisible children).
  pub fn clamped_to_rect(self, width: f32, height: f32) -> Self {
    let overlap = [
      width / (self.top_left + self.top_right),
      width / (self.bottom_left + self.bottom_right),
      height / (self.top_left + self.bottom_left),
      height / (self.top_right + self.bottom_right),
    ]
    .into_iter()
    .filter(|factor| factor.is_finite())
    .fold(1.0_f32, f32::min);
    if overlap >= 1.0 {
      return self;
    }
    let overlap = overlap.max(0.0);
    Self {
      top_left: self.top_left * overlap,
      top_right: self.top_right * overlap,
      bottom_right: self.bottom_right * overlap,
      bottom_left: self.bottom_left * overlap,
    }
  }
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct ThemedBorderRadius {
  pub top_left: RadiusValue,
  pub top_right: RadiusValue,
  pub bottom_right: RadiusValue,
  pub bottom_left: RadiusValue,
}

impl ThemedBorderRadius {
  pub fn new(
    top_left: impl Into<RadiusValue>,
    top_right: impl Into<RadiusValue>,
    bottom_right: impl Into<RadiusValue>,
    bottom_left: impl Into<RadiusValue>,
  ) -> Self {
    Self {
      top_left: top_left.into(),
      top_right: top_right.into(),
      bottom_right: bottom_right.into(),
      bottom_left: bottom_left.into(),
    }
  }

  pub fn all(radius: impl Into<RadiusValue>) -> Self {
    let radius = radius.into();
    Self {
      top_left: radius,
      top_right: radius,
      bottom_right: radius,
      bottom_left: radius,
    }
  }

  pub fn resolve(&self, radii: &ThemeRadii) -> BorderRadius {
    BorderRadius {
      top_left: self.top_left.resolve(radii),
      top_right: self.top_right.resolve(radii),
      bottom_right: self.bottom_right.resolve(radii),
      bottom_left: self.bottom_left.resolve(radii),
    }
  }

  pub fn as_border_radius(&self) -> Option<BorderRadius> {
    Some(BorderRadius {
      top_left: self.top_left.as_px()?,
      top_right: self.top_right.as_px()?,
      bottom_right: self.bottom_right.as_px()?,
      bottom_left: self.bottom_left.as_px()?,
    })
  }
}

impl From<f32> for ThemedBorderRadius {
  fn from(value: f32) -> Self {
    Self::all(value)
  }
}

impl From<crate::app::theme::RadiusSize> for ThemedBorderRadius {
  fn from(value: crate::app::theme::RadiusSize) -> Self {
    Self::all(value)
  }
}

impl From<BorderRadius> for ThemedBorderRadius {
  fn from(value: BorderRadius) -> Self {
    Self::new(value.top_left, value.top_right, value.bottom_right, value.bottom_left)
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Border {
  pub width: BorderSizeValue,
  pub color: BackgroundColor,
  pub placement: BorderPlacement,
  /// The dashes of a dashed border; `None` is a solid line.
  pub dash: Option<BorderDash>,
}

/// The dashes of a dashed border, cut along the element's outline clockwise from the start of its top edge.
/// The period is fitted to the outline's length, so the last dash meets the first with a whole gap between
/// them; a border on some sides only shows the dashes that fall on them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderDash {
  /// Dashes three times the border's width long, with gaps as long.
  Auto,
  /// A dash and the gap after it, in logical pixels. A border with no dash or no gap is a solid line.
  Lengths { dash: f32, gap: f32 },
}

impl BorderDash {
  /// The dash and the gap of a border `width` wide, both zero for a solid line.
  fn resolve(self, width: f32) -> [f32; 2] {
    let [dash, gap] = match self {
      Self::Auto => [width * 3.0; 2],
      Self::Lengths { dash, gap } => [dash, gap],
    };
    if dash.is_finite() && gap.is_finite() && dash > 0.0 && gap > 0.0 {
      [dash, gap]
    } else {
      [0.0; 2]
    }
  }
}

impl Border {
  pub fn new(width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>, placement: BorderPlacement) -> Self {
    Self {
      width: width.into(),
      color: color.into(),
      placement,
      dash: None,
    }
  }

  /// A dashed line: dashes three times the border's width long, with gaps as long.
  pub fn dashed(mut self) -> Self {
    self.dash = Some(BorderDash::Auto);
    self
  }

  /// A dashed line of dashes `dash` logical pixels long with gaps of `gap`.
  pub fn dash(mut self, dash: f32, gap: f32) -> Self {
    self.dash = Some(BorderDash::Lengths { dash, gap });
    self
  }

  pub fn inside(width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) -> Self {
    Self::new(width, color, BorderPlacement::Inside)
  }

  pub fn outside(width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) -> Self {
    Self::new(width, color, BorderPlacement::Outside)
  }

  pub fn center(width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) -> Self {
    Self::new(width, color, BorderPlacement::Center)
  }

  pub(crate) fn resolve(&self, palette: &ThemePalette, border_sizes: &ThemeBorderSizes) -> Option<ResolvedBorder> {
    let width = self.width.resolve(border_sizes);
    Some(ResolvedBorder {
      width,
      color: self.color.resolve(palette)?,
      placement: self.placement,
      dash: self.dash.map_or([0.0; 2], |dash| dash.resolve(width)),
    })
  }
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Borders {
  pub top: Option<Border>,
  pub right: Option<Border>,
  pub bottom: Option<Border>,
  pub left: Option<Border>,
}

impl Borders {
  pub fn all(border: Border) -> Self {
    Self {
      top: Some(border.clone()),
      right: Some(border.clone()),
      bottom: Some(border.clone()),
      left: Some(border),
    }
  }

  pub fn any(&self) -> bool {
    self.top.is_some() || self.right.is_some() || self.bottom.is_some() || self.left.is_some()
  }

  pub fn color(&self) -> Option<BackgroundColor> {
    self
      .top
      .as_ref()
      .or(self.right.as_ref())
      .or(self.bottom.as_ref())
      .or(self.left.as_ref())
      .map(|border| border.color.clone())
  }

  pub fn set_color(&mut self, color: impl Into<BackgroundColor>) {
    let color = color.into();
    if let Some(border) = &mut self.top {
      border.color = color.clone();
    }
    if let Some(border) = &mut self.right {
      border.color = color.clone();
    }
    if let Some(border) = &mut self.bottom {
      border.color = color.clone();
    }
    if let Some(border) = &mut self.left {
      border.color = color;
    }
  }

  pub(crate) fn resolve_with_sizes(
    &self,
    palette: &ThemePalette,
    border_sizes: &ThemeBorderSizes,
  ) -> Option<ResolvedBorders> {
    let borders = ResolvedBorders {
      top: self
        .top
        .as_ref()
        .and_then(|border| border.resolve(palette, border_sizes)),
      right: self
        .right
        .as_ref()
        .and_then(|border| border.resolve(palette, border_sizes)),
      bottom: self
        .bottom
        .as_ref()
        .and_then(|border| border.resolve(palette, border_sizes)),
      left: self
        .left
        .as_ref()
        .and_then(|border| border.resolve(palette, border_sizes)),
    };
    borders.any().then_some(borders)
  }

  pub fn top_width(&self) -> Option<BorderSizeValue> {
    self.top.as_ref().map(|border| border.width)
  }

  pub fn right_width(&self) -> Option<BorderSizeValue> {
    self.right.as_ref().map(|border| border.width)
  }

  pub fn bottom_width(&self) -> Option<BorderSizeValue> {
    self.bottom.as_ref().map(|border| border.width)
  }

  pub fn left_width(&self) -> Option<BorderSizeValue> {
    self.left.as_ref().map(|border| border.width)
  }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum BorderPlacement {
  #[default]
  Inside,
  Outside,
  Center,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedBorder {
  pub width: f32,
  pub color: Color,
  pub placement: BorderPlacement,
  /// The length of a dash and of the gap after it, in logical pixels; both zero for a solid line.
  pub dash: [f32; 2],
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct ResolvedBorders {
  pub top: Option<ResolvedBorder>,
  pub right: Option<ResolvedBorder>,
  pub bottom: Option<ResolvedBorder>,
  pub left: Option<ResolvedBorder>,
}

impl ResolvedBorders {
  pub(crate) fn any(&self) -> bool {
    self.top.is_some() || self.right.is_some() || self.bottom.is_some() || self.left.is_some()
  }
}

#[cfg(test)]
mod tests {
  use super::BorderRadius;

  #[test]
  fn oversized_radius_clamps_to_pill_shape() {
    // The pill idiom: radius 999 on a small box must clamp to half the short
    // side, keeping a non-empty interior for rounded clips.
    let clamped = BorderRadius::all(999.0).clamped_to_rect(110.0, 25.0);
    assert_eq!(clamped.top_left, 12.5);
    assert_eq!(clamped.bottom_right, 12.5);

    // Radii that already fit stay untouched.
    let fitting = BorderRadius::all(6.0).clamped_to_rect(110.0, 25.0);
    assert_eq!(fitting.top_left, 6.0);

    // Zero radii divide the overlap factors by zero — must pass through.
    let zero = BorderRadius::all(0.0).clamped_to_rect(110.0, 25.0);
    assert_eq!(zero.top_left, 0.0);

    // Asymmetric radii scale uniformly (CSS overlap rule).
    let uneven = BorderRadius::new(40.0, 10.0, 40.0, 10.0).clamped_to_rect(100.0, 25.0);
    assert!(uneven.top_left < 40.0);
    assert!((uneven.top_left / uneven.top_right - 4.0).abs() < 0.001);
  }
}
