use std::{collections::HashMap, sync::Arc};

use crate::node::color::Color;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PaletteColor {
  Accent,
  AccentHover,
  AccentMuted,
  SurfaceBase,
  SurfacePanel,
  SurfaceRaised,
  SurfaceInput,
  Border,
  BorderStrong,
  BorderFocus,
  TextPrimary,
  TextSecondary,
  TextMuted,
  TextInverse,
  /// The color of text that names none: a `Text` with no color of its own
  /// and no `foreground` above it, whose typography style keeps the default
  /// text color. See [`ThemePalette::foreground`].
  Foreground,
  Success,
  SuccessMuted,
  Warning,
  WarningMuted,
  Danger,
  DangerMuted,
  Info,
  InfoMuted,
  Extra(Arc<str>),
}

impl PaletteColor {
  pub fn extra(name: impl Into<Arc<str>>) -> Self {
    Self::Extra(name.into())
  }

  pub fn as_str(&self) -> &str {
    match self {
      Self::Accent => "accent",
      Self::AccentHover => "accent_hover",
      Self::AccentMuted => "accent_muted",
      Self::SurfaceBase => "surface_base",
      Self::SurfacePanel => "surface_panel",
      Self::SurfaceRaised => "surface_raised",
      Self::SurfaceInput => "surface_input",
      Self::Border => "border",
      Self::BorderStrong => "border_strong",
      Self::BorderFocus => "border_focus",
      Self::TextPrimary => "text_primary",
      Self::TextSecondary => "text_secondary",
      Self::TextMuted => "text_muted",
      Self::TextInverse => "text_inverse",
      Self::Foreground => "foreground",
      Self::Success => "success",
      Self::SuccessMuted => "success_muted",
      Self::Warning => "warning",
      Self::WarningMuted => "warning_muted",
      Self::Danger => "danger",
      Self::DangerMuted => "danger_muted",
      Self::Info => "info",
      Self::InfoMuted => "info_muted",
      Self::Extra(name) => name,
    }
  }
}

impl From<&PaletteColor> for PaletteColor {
  fn from(color: &PaletteColor) -> Self {
    color.clone()
  }
}

impl From<Arc<str>> for PaletteColor {
  fn from(name: Arc<str>) -> Self {
    Self::Extra(name)
  }
}

impl From<&str> for PaletteColor {
  fn from(name: &str) -> Self {
    Self::Extra(Arc::from(name))
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemePalette {
  pub accent: Color,
  pub accent_hover: Color,
  pub accent_muted: Color,
  pub surface_base: Color,
  pub surface_panel: Color,
  pub surface_raised: Color,
  pub surface_input: Color,
  pub border: Color,
  pub border_strong: Color,
  pub border_focus: Color,
  pub text_primary: Color,
  pub text_secondary: Color,
  pub text_muted: Color,
  pub text_inverse: Color,
  /// The color text is drawn in when nothing names one: the text sets no
  /// color, no element above it sets a `foreground`, and its typography style
  /// has the color of `TextStyle::default()`. Black by default, which is that
  /// color, so a theme that never sets it draws text as it always did.
  pub foreground: Color,
  pub success: Color,
  pub success_muted: Color,
  pub warning: Color,
  pub warning_muted: Color,
  pub danger: Color,
  pub danger_muted: Color,
  pub info: Color,
  pub info_muted: Color,
  pub extra: HashMap<Arc<str>, Color>,
}

impl ThemePalette {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn get(&self, color: impl Into<PaletteColor>) -> Color {
    let color = color.into();
    self
      .try_get(&color)
      .unwrap_or_else(|| panic!("palette color not found: {}", color.as_str()))
  }

  pub fn try_get(&self, color: impl Into<PaletteColor>) -> Option<Color> {
    match color.into() {
      PaletteColor::Accent => Some(self.accent),
      PaletteColor::AccentHover => Some(self.accent_hover),
      PaletteColor::AccentMuted => Some(self.accent_muted),
      PaletteColor::SurfaceBase => Some(self.surface_base),
      PaletteColor::SurfacePanel => Some(self.surface_panel),
      PaletteColor::SurfaceRaised => Some(self.surface_raised),
      PaletteColor::SurfaceInput => Some(self.surface_input),
      PaletteColor::Border => Some(self.border),
      PaletteColor::BorderStrong => Some(self.border_strong),
      PaletteColor::BorderFocus => Some(self.border_focus),
      PaletteColor::TextPrimary => Some(self.text_primary),
      PaletteColor::TextSecondary => Some(self.text_secondary),
      PaletteColor::TextMuted => Some(self.text_muted),
      PaletteColor::TextInverse => Some(self.text_inverse),
      PaletteColor::Foreground => Some(self.foreground),
      PaletteColor::Success => Some(self.success),
      PaletteColor::SuccessMuted => Some(self.success_muted),
      PaletteColor::Warning => Some(self.warning),
      PaletteColor::WarningMuted => Some(self.warning_muted),
      PaletteColor::Danger => Some(self.danger),
      PaletteColor::DangerMuted => Some(self.danger_muted),
      PaletteColor::Info => Some(self.info),
      PaletteColor::InfoMuted => Some(self.info_muted),
      PaletteColor::Extra(name) => self.extra.get(&name).copied(),
    }
  }

  pub fn set(&mut self, color: impl Into<PaletteColor>, value: Color) {
    match color.into() {
      PaletteColor::Accent => self.accent = value,
      PaletteColor::AccentHover => self.accent_hover = value,
      PaletteColor::AccentMuted => self.accent_muted = value,
      PaletteColor::SurfaceBase => self.surface_base = value,
      PaletteColor::SurfacePanel => self.surface_panel = value,
      PaletteColor::SurfaceRaised => self.surface_raised = value,
      PaletteColor::SurfaceInput => self.surface_input = value,
      PaletteColor::Border => self.border = value,
      PaletteColor::BorderStrong => self.border_strong = value,
      PaletteColor::BorderFocus => self.border_focus = value,
      PaletteColor::TextPrimary => self.text_primary = value,
      PaletteColor::TextSecondary => self.text_secondary = value,
      PaletteColor::TextMuted => self.text_muted = value,
      PaletteColor::TextInverse => self.text_inverse = value,
      PaletteColor::Foreground => self.foreground = value,
      PaletteColor::Success => self.success = value,
      PaletteColor::SuccessMuted => self.success_muted = value,
      PaletteColor::Warning => self.warning = value,
      PaletteColor::WarningMuted => self.warning_muted = value,
      PaletteColor::Danger => self.danger = value,
      PaletteColor::DangerMuted => self.danger_muted = value,
      PaletteColor::Info => self.info = value,
      PaletteColor::InfoMuted => self.info_muted = value,
      PaletteColor::Extra(name) => {
        self.extra.insert(name, value);
      }
    }
  }

  pub fn resolve(&self, color: impl Into<PaletteColor>) -> Color {
    self.get(color)
  }

  pub fn try_resolve(&self, color: impl Into<PaletteColor>) -> Option<Color> {
    self.try_get(color)
  }

  /// The color a text of `style` is drawn in when the text names none: the
  /// style's own, or the [`foreground`](Self::foreground) for a style that
  /// keeps the default text color and so names none either.
  pub(crate) fn text_color(&self, style: &crate::layout::text_style::TextStyle) -> Color {
    if style.color == crate::layout::text_style::DEFAULT_TEXT_COLOR {
      self.foreground
    } else {
      style.color
    }
  }
}

impl Default for ThemePalette {
  fn default() -> Self {
    Self {
      accent: Color::from_hex("#2563eb"),
      accent_hover: Color::from_hex("#1d4ed8"),
      accent_muted: Color::from_hex("#dbeafe"),
      surface_base: Color::from_hex("#ffffff"),
      surface_panel: Color::from_hex("#f8fafc"),
      surface_raised: Color::from_hex("#ffffff"),
      surface_input: Color::from_hex("#ffffff"),
      border: Color::from_hex("#e2e8f0"),
      border_strong: Color::from_hex("#94a3b8"),
      border_focus: Color::from_hex("#2563eb"),
      text_primary: Color::from_hex("#0f172a"),
      text_secondary: Color::from_hex("#334155"),
      text_muted: Color::from_hex("#64748b"),
      text_inverse: Color::from_hex("#ffffff"),
      foreground: crate::layout::text_style::DEFAULT_TEXT_COLOR,
      success: Color::from_hex("#16a34a"),
      success_muted: Color::from_hex("#dcfce7"),
      warning: Color::from_hex("#d97706"),
      warning_muted: Color::from_hex("#fef3c7"),
      danger: Color::from_hex("#dc2626"),
      danger_muted: Color::from_hex("#fee2e2"),
      info: Color::from_hex("#0284c7"),
      info_muted: Color::from_hex("#e0f2fe"),
      extra: HashMap::new(),
    }
  }
}
