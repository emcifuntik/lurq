use std::{collections::HashMap, sync::Arc};

use super::role_name::RoleName;
use crate::layout::text_style::{FontWeight, TextStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypographyStyle {
  Heading,
  Title,
  Body,
  Description,
  Caption,
  Label,
  FieldLabel,
  Button,
  Link,
  Mono,
  /// An application-defined style stored in [`ThemeTypography::extra`].
  Extra(RoleName),
}

impl TypographyStyle {
  /// An application-defined style. The name is interned (see [`TypographyStyle::Extra`]).
  pub fn extra(name: impl AsRef<str>) -> Self {
    Self::Extra(RoleName::new(name))
  }

  pub fn as_str(self) -> &'static str {
    match self {
      Self::Heading => "heading",
      Self::Title => "title",
      Self::Body => "body",
      Self::Description => "description",
      Self::Caption => "caption",
      Self::Label => "label",
      Self::FieldLabel => "field_label",
      Self::Button => "button",
      Self::Link => "link",
      Self::Mono => "mono",
      Self::Extra(name) => name.as_str(),
    }
  }
}

impl From<&str> for TypographyStyle {
  fn from(name: &str) -> Self {
    Self::extra(name)
  }
}

impl From<Arc<str>> for TypographyStyle {
  fn from(name: Arc<str>) -> Self {
    Self::extra(name)
  }
}

#[derive(Clone, PartialEq)]
pub struct ThemeTypography {
  pub heading: TextStyle,
  pub title: TextStyle,
  pub body: TextStyle,
  pub description: TextStyle,
  pub caption: TextStyle,
  pub label: TextStyle,
  pub field_label: TextStyle,
  pub button: TextStyle,
  pub link: TextStyle,
  pub mono: TextStyle,
  pub extra: HashMap<Arc<str>, TextStyle>,
}

impl ThemeTypography {
  pub fn new() -> Self {
    Self::default()
  }

  /// Panics when a [`TypographyStyle::Extra`] name is not in [`Self::extra`]; see [`Self::try_get`].
  pub fn get(&self, style: impl Into<TypographyStyle>) -> TextStyle {
    let style = style.into();
    self
      .try_get(style)
      .unwrap_or_else(|| panic!("typography style not found: {}", style.as_str()))
  }

  pub fn try_get(&self, style: impl Into<TypographyStyle>) -> Option<TextStyle> {
    let style = match style.into() {
      TypographyStyle::Heading => &self.heading,
      TypographyStyle::Title => &self.title,
      TypographyStyle::Body => &self.body,
      TypographyStyle::Description => &self.description,
      TypographyStyle::Caption => &self.caption,
      TypographyStyle::Label => &self.label,
      TypographyStyle::FieldLabel => &self.field_label,
      TypographyStyle::Button => &self.button,
      TypographyStyle::Link => &self.link,
      TypographyStyle::Mono => &self.mono,
      TypographyStyle::Extra(name) => self.extra.get(name.as_str())?,
    };
    Some(style.clone())
  }

  pub fn set(&mut self, style: impl Into<TypographyStyle>, value: TextStyle) {
    match style.into() {
      TypographyStyle::Heading => self.heading = value,
      TypographyStyle::Title => self.title = value,
      TypographyStyle::Body => self.body = value,
      TypographyStyle::Description => self.description = value,
      TypographyStyle::Caption => self.caption = value,
      TypographyStyle::Label => self.label = value,
      TypographyStyle::FieldLabel => self.field_label = value,
      TypographyStyle::Button => self.button = value,
      TypographyStyle::Link => self.link = value,
      TypographyStyle::Mono => self.mono = value,
      TypographyStyle::Extra(name) => {
        self.extra.insert(Arc::from(name.as_str()), value);
      }
    }
  }

  pub fn default_style(&self) -> &TextStyle {
    &self.body
  }

  pub fn set_default_style(&mut self, style: TextStyle) {
    self.body = style;
  }

  pub fn resolve(&self, style: impl Into<TypographyStyle>) -> TextStyle {
    self.get(style)
  }

  pub fn try_resolve(&self, style: impl Into<TypographyStyle>) -> Option<TextStyle> {
    self.try_get(style)
  }

  /// Node resolution: a missing extra style falls back to the default style, as
  /// an unresolved palette color leaves a node's own color in place.
  pub(crate) fn resolve_or_default(&self, style: TypographyStyle) -> TextStyle {
    self.try_get(style).unwrap_or_else(|| self.default_style().clone())
  }
}

impl Default for ThemeTypography {
  fn default() -> Self {
    let body = TextStyle::default();
    Self {
      heading: TextStyle {
        font_size: 24.0,
        weight: FontWeight::Bold,
        ..body.clone()
      },
      title: TextStyle {
        font_size: 20.0,
        weight: FontWeight::Bold,
        ..body.clone()
      },
      body: body.clone(),
      description: TextStyle {
        font_size: 14.0,
        ..body.clone()
      },
      caption: TextStyle {
        font_size: 12.0,
        ..body.clone()
      },
      label: TextStyle {
        font_size: 13.0,
        weight: FontWeight::Medium,
        ..body.clone()
      },
      field_label: TextStyle {
        font_size: 13.0,
        weight: FontWeight::Medium,
        ..body.clone()
      },
      button: TextStyle {
        font_size: 13.0,
        weight: FontWeight::Medium,
        ..body.clone()
      },
      link: body.clone(),
      mono: TextStyle {
        font_family: Arc::from("monospace"),
        ..body
      },
      extra: HashMap::new(),
    }
  }
}

#[derive(Clone)]
pub struct ThemeFonts {
  pub body: TextStyle,
  pub heading: TextStyle,
  pub mono: TextStyle,
}

impl From<ThemeTypography> for ThemeFonts {
  fn from(typography: ThemeTypography) -> Self {
    Self {
      body: typography.body,
      heading: typography.heading,
      mono: typography.mono,
    }
  }
}

impl From<ThemeFonts> for ThemeTypography {
  fn from(fonts: ThemeFonts) -> Self {
    Self {
      body: fonts.body,
      heading: fonts.heading,
      mono: fonts.mono,
      ..Self::default()
    }
  }
}

impl Default for ThemeFonts {
  fn default() -> Self {
    ThemeTypography::default().into()
  }
}
