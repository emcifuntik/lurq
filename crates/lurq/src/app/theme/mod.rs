use std::{
  ops::Deref,
  sync::{Arc, RwLock, RwLockReadGuard},
};

use crate::{
  core::Signal,
  layout::{scrollbar::ScrollBarStyle, text_style::TextStyle},
  node::{box_shadow::BoxShadow, color::Color, dimension::Dimension},
};

mod border;
mod breakpoints;
mod caret;
#[cfg(feature = "form")]
mod form;
#[cfg(feature = "markdown")]
mod markdown;
mod palette;
mod radius;
mod role_name;
mod shadow;
mod spacing;
#[cfg(feature = "markdown")]
mod text_marks;
mod typography;

pub use border::{BorderSize, ThemeBorderSizes};
pub use breakpoints::{Breakpoint, ThemeBreakpoints};
pub use caret::{CaretMode, ThemeCaret};
#[cfg(feature = "form")]
pub use form::{
  FormButtonRole, FormButtonTheme, FormCheckboxStyle, FormFieldTheme, FormInputTheme, FormSliderStyle, FormTextRole,
  FormTheme,
};
#[cfg(feature = "markdown")]
pub use markdown::{
  MarkdownBlockStyle, MarkdownFocusRing, MarkdownInlineStyle, MarkdownLinkState, MarkdownTextStyle, ThemeMarkdown,
};
pub use palette::{PaletteColor, ThemePalette};
pub use radius::{RadiusSize, ThemeRadii};
pub use role_name::RoleName;
pub use shadow::{ShadowStyle, ThemeShadows};
pub use spacing::{SpacingSize, ThemeSpacing};
#[cfg(feature = "markdown")]
pub use text_marks::{TextMarkStyle, ThemeTextMarks};
pub use typography::{ThemeFonts, ThemeTypography, TypographyStyle};

#[derive(Clone)]
pub struct Theme {
  inner: Arc<RwLock<ThemeInner>>,
  version_signal: Signal<u64>,
}

struct ThemeInner {
  palette: ThemePalette,
  border_sizes: ThemeBorderSizes,
  spacing: ThemeSpacing,
  radii: ThemeRadii,
  shadows: Arc<ThemeShadows>,
  breakpoints: ThemeBreakpoints,
  caret: ThemeCaret,
  scrollbar: ScrollBarStyle,
  typography: ThemeTypography,
  #[cfg(feature = "markdown")]
  text_marks: Arc<ThemeTextMarks>,
  #[cfg(feature = "markdown")]
  markdown: ThemeMarkdown,
  #[cfg(feature = "form")]
  form: FormTheme,
  version: u64,
}

pub struct ThemeRef<'a, T> {
  inner: RwLockReadGuard<'a, ThemeInner>,
  value: fn(&ThemeInner) -> &T,
}

impl<T> Deref for ThemeRef<'_, T> {
  type Target = T;

  fn deref(&self) -> &Self::Target {
    (self.value)(&self.inner)
  }
}

impl Default for Theme {
  fn default() -> Self {
    Self {
      inner: Arc::new(RwLock::new(ThemeInner {
        palette: ThemePalette::default(),
        border_sizes: ThemeBorderSizes::default(),
        spacing: ThemeSpacing::default(),
        radii: ThemeRadii::default(),
        shadows: Arc::new(ThemeShadows::default()),
        breakpoints: ThemeBreakpoints::default(),
        caret: ThemeCaret::default(),
        scrollbar: ScrollBarStyle::default(),
        typography: ThemeTypography::default(),
        #[cfg(feature = "markdown")]
        text_marks: Arc::new(ThemeTextMarks::default()),
        #[cfg(feature = "markdown")]
        markdown: ThemeMarkdown::default(),
        #[cfg(feature = "form")]
        form: FormTheme::default(),
        version: 0,
      })),
      version_signal: Signal::new(0),
    }
  }
}

impl Theme {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn lens<T: Clone + Send + Sync + 'static>(
    &self,
    getter: impl Fn(&Theme) -> T + Send + Sync + 'static,
    setter: impl Fn(&Theme, T) + Send + Sync + 'static,
  ) -> ThemeLens<T> {
    ThemeLens {
      theme: self.clone(),
      getter: Arc::new(getter),
      setter: Arc::new(setter),
    }
  }

  pub fn palette(&self) -> ThemeRef<'_, ThemePalette> {
    ThemeRef {
      inner: self.inner.read().unwrap(),
      value: |inner| &inner.palette,
    }
  }

  pub fn set_palette(&self, palette: ThemePalette) {
    self.mutate_inner(|inner| inner.palette = palette);
  }

  pub fn set_palette_color(&self, palette_color: impl Into<PaletteColor>, color: Color) {
    self.mutate_inner(|inner| inner.palette.set(palette_color, color));
  }

  pub fn palette_color(&self, palette_color: impl Into<PaletteColor>) -> Color {
    self.inner.read().unwrap().palette.get(palette_color)
  }

  pub fn border_sizes(&self) -> ThemeRef<'_, ThemeBorderSizes> {
    ThemeRef {
      inner: self.inner.read().unwrap(),
      value: |inner| &inner.border_sizes,
    }
  }

  pub fn set_border_sizes(&self, border_sizes: ThemeBorderSizes) {
    self.mutate_inner(|inner| inner.border_sizes = border_sizes);
  }

  pub fn set_border_size_value(&self, size: impl Into<BorderSize>, value: f32) {
    self.mutate_inner(|inner| inner.border_sizes.set(size, value));
  }

  pub fn border_size_value(&self, size: impl Into<BorderSize>) -> f32 {
    self.inner.read().unwrap().border_sizes.get(size)
  }

  pub fn spacing(&self) -> ThemeRef<'_, ThemeSpacing> {
    ThemeRef {
      inner: self.inner.read().unwrap(),
      value: |inner| &inner.spacing,
    }
  }

  pub fn set_spacing(&self, spacing: ThemeSpacing) {
    self.mutate_inner(|inner| inner.spacing = spacing);
  }

  pub fn set_spacing_value(&self, size: impl Into<SpacingSize>, value: impl Into<Dimension>) {
    self.mutate_inner(|inner| inner.spacing.set(size, value));
  }

  pub fn spacing_value(&self, size: impl Into<SpacingSize>) -> Dimension {
    self.inner.read().unwrap().spacing.get(size)
  }

  pub fn radii(&self) -> ThemeRef<'_, ThemeRadii> {
    ThemeRef {
      inner: self.inner.read().unwrap(),
      value: |inner| &inner.radii,
    }
  }

  pub fn set_radii(&self, radii: ThemeRadii) {
    self.mutate_inner(|inner| inner.radii = radii);
  }

  pub fn set_radius_value(&self, size: impl Into<RadiusSize>, value: f32) {
    self.mutate_inner(|inner| inner.radii.set(size, value));
  }

  pub fn radius_value(&self, size: impl Into<RadiusSize>) -> f32 {
    self.inner.read().unwrap().radii.get(size)
  }

  pub fn shadows(&self) -> ThemeRef<'_, ThemeShadows> {
    ThemeRef {
      inner: self.inner.read().unwrap(),
      value: |inner| &inner.shadows,
    }
  }

  pub fn set_shadows(&self, shadows: ThemeShadows) {
    self.mutate_inner(|inner| inner.shadows = Arc::new(shadows));
  }

  /// Sets the shadows of one role; an empty list gives the role no shadow.
  pub fn set_shadow_style(&self, style: impl Into<ShadowStyle>, shadows: impl Into<Arc<[BoxShadow]>>) {
    self.mutate_inner(|inner| Arc::make_mut(&mut inner.shadows).set(style, shadows));
  }

  /// Panics when a [`ShadowStyle::Extra`] name is not in the table; see [`ThemeShadows::try_get`].
  pub fn shadow_style(&self, style: impl Into<ShadowStyle>) -> Arc<[BoxShadow]> {
    self.inner.read().unwrap().shadows.get(style)
  }

  /// The shared shadow table, for painting without copying it.
  pub(crate) fn shared_shadows(&self) -> Arc<ThemeShadows> {
    self.inner.read().unwrap().shadows.clone()
  }

  pub fn breakpoints(&self) -> ThemeRef<'_, ThemeBreakpoints> {
    ThemeRef {
      inner: self.inner.read().unwrap(),
      value: |inner| &inner.breakpoints,
    }
  }

  pub fn set_breakpoints(&self, breakpoints: ThemeBreakpoints) {
    self.mutate_inner(|inner| inner.breakpoints = breakpoints);
  }

  pub fn set_breakpoint_value(&self, breakpoint: Breakpoint, value: f32) {
    self.mutate_inner(|inner| inner.breakpoints.set(breakpoint, value));
  }

  pub fn breakpoint_value(&self, breakpoint: Breakpoint) -> f32 {
    self.inner.read().unwrap().breakpoints.get(breakpoint)
  }

  pub fn caret(&self) -> ThemeRef<'_, ThemeCaret> {
    ThemeRef {
      inner: self.inner.read().unwrap(),
      value: |inner| &inner.caret,
    }
  }

  pub fn set_caret(&self, caret: ThemeCaret) {
    self.mutate_inner(|inner| inner.caret = caret);
  }

  pub fn set_caret_mode(&self, mode: CaretMode) {
    self.mutate_inner(|inner| inner.caret.set_mode(mode));
  }

  pub fn caret_mode(&self) -> CaretMode {
    self.inner.read().unwrap().caret.mode()
  }

  pub fn scrollbar(&self) -> ThemeRef<'_, ScrollBarStyle> {
    ThemeRef {
      inner: self.inner.read().unwrap(),
      value: |inner| &inner.scrollbar,
    }
  }

  pub fn set_scrollbar(&self, scrollbar: ScrollBarStyle) {
    self.mutate_inner(|inner| inner.scrollbar = scrollbar);
  }

  pub fn typography(&self) -> ThemeRef<'_, ThemeTypography> {
    ThemeRef {
      inner: self.inner.read().unwrap(),
      value: |inner| &inner.typography,
    }
  }

  pub fn set_typography(&self, typography: ThemeTypography) {
    self.mutate_inner(|inner| inner.typography = typography);
  }

  pub fn set_typography_style(&self, typography_style: impl Into<TypographyStyle>, style: TextStyle) {
    self.mutate_inner(|inner| inner.typography.set(typography_style, style));
  }

  pub fn default_text_style(&self) -> TextStyle {
    self.inner.read().unwrap().typography.default_style().clone()
  }

  pub fn set_default_text_style(&self, style: TextStyle) {
    self.mutate_inner(|inner| inner.typography.set_default_style(style));
  }

  pub fn typography_style(&self, typography_style: impl Into<TypographyStyle>) -> TextStyle {
    self.inner.read().unwrap().typography.get(typography_style)
  }

  #[cfg(feature = "markdown")]
  pub fn markdown(&self) -> ThemeRef<'_, ThemeMarkdown> {
    ThemeRef {
      inner: self.inner.read().unwrap(),
      value: |inner| &inner.markdown,
    }
  }

  #[cfg(feature = "markdown")]
  pub fn set_markdown(&self, markdown: ThemeMarkdown) {
    self.mutate_inner(|inner| inner.markdown = markdown);
  }

  /// How the marks of styled text inputs look.
  #[cfg(feature = "markdown")]
  pub fn text_marks(&self) -> ThemeRef<'_, ThemeTextMarks> {
    ThemeRef {
      inner: self.inner.read().unwrap_or_else(std::sync::PoisonError::into_inner),
      value: |inner| &inner.text_marks,
    }
  }

  #[cfg(feature = "markdown")]
  pub fn set_text_marks(&self, text_marks: ThemeTextMarks) {
    self.mutate_inner(|inner| inner.text_marks = Arc::new(text_marks));
  }

  /// The shared text marks, for layout without copying them.
  #[cfg(feature = "markdown")]
  pub(crate) fn shared_text_marks(&self) -> Arc<ThemeTextMarks> {
    self
      .inner
      .read()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .text_marks
      .clone()
  }

  pub fn fonts(&self) -> ThemeFonts {
    self.inner.read().unwrap().typography.clone().into()
  }

  pub fn set_fonts(&self, fonts: ThemeFonts) {
    self.set_typography(fonts.into());
  }

  #[cfg(feature = "form")]
  pub fn form(&self) -> ThemeRef<'_, FormTheme> {
    ThemeRef {
      inner: self.inner.read().unwrap(),
      value: |inner| &inner.form,
    }
  }

  #[cfg(feature = "form")]
  pub fn set_form(&self, form: FormTheme) {
    self.mutate_inner(|inner| inner.form = form);
  }

  pub(crate) fn version(&self) -> u64 {
    self.inner.read().unwrap().version
  }

  pub(crate) fn track_access(&self) {
    let _ = self.version_signal.get();
  }

  fn mutate_inner(&self, f: impl FnOnce(&mut ThemeInner)) {
    let version = {
      let mut inner = self.inner.write().unwrap();
      f(&mut inner);
      inner.version = inner.version.wrapping_add(1);
      inner.version
    };
    self.version_signal.set(version);
  }
}

pub struct ThemeLens<T> {
  theme: Theme,
  getter: Arc<dyn Fn(&Theme) -> T + Send + Sync>,
  setter: Arc<dyn Fn(&Theme, T) + Send + Sync>,
}

impl<T: Clone + Send + Sync + 'static> ThemeLens<T> {
  pub fn get(&self) -> T {
    (self.getter)(&self.theme)
  }

  pub fn set(&self, value: T) {
    (self.setter)(&self.theme, value);
  }

  pub fn update(&self, f: impl FnOnce(&mut T)) {
    let mut value = self.get();
    f(&mut value);
    self.set(value);
  }
}

impl<T> Clone for ThemeLens<T> {
  fn clone(&self) -> Self {
    Self {
      theme: self.theme.clone(),
      getter: self.getter.clone(),
      setter: self.setter.clone(),
    }
  }
}
