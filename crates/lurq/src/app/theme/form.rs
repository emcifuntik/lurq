use super::{BorderSize, PaletteColor, RadiusSize, SpacingSize, ThemePalette, ThemeTypography, TypographyStyle};
use crate::{
  layout::text_style::TextStyle,
  node::{CheckboxStyle, SliderPartStyle, dimension::Dimension, padding::Padding, spacing_value::SpacingValue},
};

#[derive(Clone, Default)]
pub struct FormTheme {
  pub field: FormFieldTheme,
  pub input: FormInputTheme,
  pub checkbox: FormCheckboxStyle,
  pub slider: FormSliderStyle,
  pub button: FormButtonTheme,
}

#[derive(Clone, PartialEq, Eq)]
pub struct FormTextRole {
  pub typography: TypographyStyle,
  pub color: PaletteColor,
}

#[derive(Clone, PartialEq)]
pub struct FormFieldTheme {
  pub spacing: SpacingValue,
  pub label: FormTextRole,
  pub hint: FormTextRole,
  pub error: FormTextRole,
}

#[derive(Clone, PartialEq)]
pub struct FormInputTheme {
  pub height: Dimension,
  pub padding: Padding,
  pub radius: RadiusSize,
  pub background: PaletteColor,
  pub border: PaletteColor,
  /// Border while the input has focus, by click, Tab or a request: a text
  /// input shows its focus for any focus, like a browser's text field.
  pub border_focus: PaletteColor,
  pub background_error: PaletteColor,
  pub border_error: PaletteColor,
  pub text: FormTextRole,
  pub placeholder: FormTextRole,
  pub caret: PaletteColor,
}

#[derive(Clone)]
pub struct FormCheckboxStyle {
  pub background: PaletteColor,
  pub border: PaletteColor,
  pub border_hover: PaletteColor,
  pub checked_background: PaletteColor,
  pub checked_border: PaletteColor,
  pub checked_background_hover: PaletteColor,
  /// Box border while the checkbox has keyboard focus (focus-visible: Tab or a
  /// key press, not a click), checked or not.
  pub border_focus: PaletteColor,
  pub radius: RadiusSize,
}

#[derive(Clone)]
pub struct FormSliderStyle {
  pub track: PaletteColor,
  pub track_hover: PaletteColor,
  pub thumb: PaletteColor,
  pub thumb_hover: PaletteColor,
  /// Thumb border while the slider has keyboard focus (focus-visible: Tab or a
  /// key press, not a click or drag).
  pub thumb_border_focus: PaletteColor,
}

#[derive(Clone, PartialEq)]
pub struct FormButtonTheme {
  pub primary: FormButtonRole,
  pub secondary: FormButtonRole,
}

#[derive(Clone, PartialEq)]
pub struct FormButtonRole {
  pub width: Dimension,
  pub height: Dimension,
  pub padding: Padding,
  pub radius: RadiusSize,
  pub background: PaletteColor,
  pub border: PaletteColor,
  pub background_hover: PaletteColor,
  pub border_hover: PaletteColor,
  pub background_active: PaletteColor,
  pub border_active: PaletteColor,
  /// Border while the button has keyboard focus (focus-visible: Tab or a key
  /// press, not a click).
  pub border_focus: PaletteColor,
  pub text: FormTextRole,
}

impl FormTextRole {
  pub fn resolve(&self, typography: &ThemeTypography, palette: &ThemePalette) -> TextStyle {
    let mut style = typography.resolve(self.typography);
    style.color = palette.resolve(&self.color);
    style
  }
}

impl FormCheckboxStyle {
  pub fn box_style(&self, palette: &ThemePalette) -> CheckboxStyle {
    CheckboxStyle::new()
      .size(16.0, 16.0)
      .background(palette.resolve(&self.background))
      .rounded(self.radius)
      .border_inside(BorderSize::Sm, &self.border)
  }

  pub fn checked_box_style(&self, palette: &ThemePalette) -> CheckboxStyle {
    CheckboxStyle::new()
      .background(palette.resolve(&self.checked_background))
      .border_inside(BorderSize::Sm, &self.checked_border)
      .indicator_size(8.0, 8.0)
  }

  pub fn box_hovered_style(&self) -> CheckboxStyle {
    CheckboxStyle::new().border_inside(BorderSize::Sm, &self.border_hover)
  }

  pub fn checked_box_hovered_style(&self, palette: &ThemePalette) -> CheckboxStyle {
    CheckboxStyle::new().background(palette.resolve(&self.checked_background_hover))
  }

  pub fn box_focused_style(&self) -> CheckboxStyle {
    CheckboxStyle::new().border_inside(BorderSize::Sm, &self.border_focus)
  }
}

impl FormSliderStyle {
  pub fn track_style(&self, palette: &ThemePalette) -> SliderPartStyle {
    SliderPartStyle::new()
      .height(4.0)
      .background(palette.resolve(&self.track))
      .rounded(999.0)
  }

  pub fn track_hovered_style(&self, palette: &ThemePalette) -> SliderPartStyle {
    SliderPartStyle::new().background(palette.resolve(&self.track_hover))
  }

  pub fn thumb_style(&self, palette: &ThemePalette) -> SliderPartStyle {
    SliderPartStyle::new()
      .size(16.0, 16.0)
      .background(palette.resolve(&self.thumb))
      .rounded(999.0)
  }

  pub fn thumb_hovered_style(&self, palette: &ThemePalette) -> SliderPartStyle {
    SliderPartStyle::new().background(palette.resolve(&self.thumb_hover))
  }

  pub fn thumb_focused_style(&self) -> SliderPartStyle {
    SliderPartStyle::new().border_inside(BorderSize::Sm, &self.thumb_border_focus)
  }
}

impl Default for FormFieldTheme {
  fn default() -> Self {
    Self {
      spacing: SpacingValue::from(SpacingSize::Xs),
      label: FormTextRole {
        typography: TypographyStyle::FieldLabel,
        color: PaletteColor::TextPrimary,
      },
      hint: FormTextRole {
        typography: TypographyStyle::Caption,
        color: PaletteColor::TextMuted,
      },
      error: FormTextRole {
        typography: TypographyStyle::Caption,
        color: PaletteColor::Danger,
      },
    }
  }
}

impl Default for FormInputTheme {
  fn default() -> Self {
    Self {
      height: Dimension::Px(36.0),
      padding: Padding::symmetric(10.0, 8.0),
      radius: RadiusSize::Md,
      background: PaletteColor::SurfaceInput,
      border: PaletteColor::Border,
      border_focus: PaletteColor::BorderFocus,
      background_error: PaletteColor::DangerMuted,
      border_error: PaletteColor::Danger,
      text: FormTextRole {
        typography: TypographyStyle::Body,
        color: PaletteColor::TextPrimary,
      },
      placeholder: FormTextRole {
        typography: TypographyStyle::Body,
        color: PaletteColor::TextMuted,
      },
      caret: PaletteColor::BorderFocus,
    }
  }
}

impl Default for FormCheckboxStyle {
  fn default() -> Self {
    Self {
      background: PaletteColor::SurfaceInput,
      border: PaletteColor::Border,
      border_hover: PaletteColor::BorderFocus,
      checked_background: PaletteColor::Accent,
      checked_border: PaletteColor::Accent,
      checked_background_hover: PaletteColor::AccentHover,
      border_focus: PaletteColor::BorderFocus,
      radius: RadiusSize::Sm,
    }
  }
}

impl Default for FormSliderStyle {
  fn default() -> Self {
    Self {
      track: PaletteColor::Border,
      track_hover: PaletteColor::BorderStrong,
      thumb: PaletteColor::Accent,
      thumb_hover: PaletteColor::AccentHover,
      thumb_border_focus: PaletteColor::BorderFocus,
    }
  }
}

impl Default for FormButtonTheme {
  fn default() -> Self {
    Self {
      primary: FormButtonRole::primary(),
      secondary: FormButtonRole::secondary(),
    }
  }
}

impl FormButtonRole {
  pub fn primary() -> Self {
    Self {
      width: Dimension::Pct(100.0),
      height: Dimension::Px(36.0),
      padding: Padding::symmetric(14.0, 8.0),
      radius: RadiusSize::Md,
      background: PaletteColor::Accent,
      border: PaletteColor::Accent,
      background_hover: PaletteColor::AccentHover,
      border_hover: PaletteColor::AccentHover,
      background_active: PaletteColor::AccentHover,
      border_active: PaletteColor::AccentHover,
      border_focus: PaletteColor::BorderFocus,
      text: FormTextRole {
        typography: TypographyStyle::Button,
        color: PaletteColor::TextInverse,
      },
    }
  }

  pub fn secondary() -> Self {
    Self {
      width: Dimension::Pct(100.0),
      height: Dimension::Px(36.0),
      padding: Padding::symmetric(14.0, 8.0),
      radius: RadiusSize::Md,
      background: PaletteColor::SurfaceInput,
      border: PaletteColor::BorderStrong,
      background_hover: PaletteColor::SurfacePanel,
      border_hover: PaletteColor::BorderStrong,
      background_active: PaletteColor::Border,
      border_active: PaletteColor::BorderStrong,
      border_focus: PaletteColor::BorderFocus,
      text: FormTextRole {
        typography: TypographyStyle::Button,
        color: PaletteColor::TextPrimary,
      },
    }
  }
}
