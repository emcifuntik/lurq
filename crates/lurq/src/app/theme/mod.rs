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
mod motion;
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
#[cfg_attr(not(feature = "winit"), allow(unused_imports))]
pub(crate) use motion::system_reduced_motion;
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
  /// The styles as they are used: `set_typography`'s, at `text_scale`.
  typography: ThemeTypography,
  base_typography: ThemeTypography,
  text_scale: f32,
  bold_text: bool,
  /// The palettes of a theme that has one for each appearance, and the appearance it is in.
  palettes: Option<(ThemePalette, ThemePalette)>,
  appearance: crate::app::window::Appearance,
  follows_system: bool,
  #[cfg(feature = "markdown")]
  text_marks: Arc<ThemeTextMarks>,
  #[cfg(feature = "markdown")]
  markdown: ThemeMarkdown,
  #[cfg(feature = "form")]
  form: FormTheme,
  /// What the app decided about reduced motion; `None` follows the system.
  reduced_motion: Option<bool>,
  /// What the system asks for, as the shell last read it.
  system_reduced_motion: bool,
  version: u64,
}

impl ThemeInner {
  fn adjust_typography(&mut self) {
    self.typography = self.base_typography.scaled(self.text_scale);
  }
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
        base_typography: ThemeTypography::default(),
        text_scale: 1.0,
        bold_text: false,
        palettes: None,
        appearance: crate::app::window::Appearance::Light,
        follows_system: true,
        #[cfg(feature = "markdown")]
        text_marks: Arc::new(ThemeTextMarks::default()),
        #[cfg(feature = "markdown")]
        markdown: ThemeMarkdown::default(),
        #[cfg(feature = "form")]
        form: FormTheme::default(),
        reduced_motion: None,
        system_reduced_motion: false,
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
    self.mutate_inner(|inner| {
      inner.base_typography = typography;
      inner.adjust_typography();
    });
  }

  pub fn set_typography_style(&self, typography_style: impl Into<TypographyStyle>, style: TextStyle) {
    self.mutate_inner(|inner| {
      inner.base_typography.set(typography_style, style);
      inner.adjust_typography();
    });
  }

  /// How much larger than set the typography's styles are: what a person asked the system for (a phone's
  /// Dynamic Type), 1 where they asked for nothing. A size that is not one of the typography's is scaled
  /// with [`Theme::scaled`].
  pub fn text_scale(&self) -> f32 {
    self.inner.read().unwrap().text_scale
  }

  pub fn set_text_scale(&self, scale: f32) {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    if self.text_scale() == scale {
      return;
    }
    self.mutate_inner(|inner| {
      inner.text_scale = scale;
      inner.adjust_typography();
    });
  }

  /// A size in logical pixels at the theme's text scale.
  pub fn scaled(&self, size: f32) -> f32 {
    size * self.text_scale()
  }

  /// Every text is drawn two weights heavier, whatever set its weight: a person asked the system for bold
  /// text.
  pub fn bold_text(&self) -> bool {
    self.inner.read().unwrap().bold_text
  }

  pub fn set_bold_text(&self, bold: bool) {
    if self.bold_text() == bold {
      return;
    }
    self.mutate_inner(|inner| inner.bold_text = bold);
  }

  /// Gives the theme a palette for each appearance: the one for the appearance it is in is its palette, now
  /// and when the appearance changes.
  pub fn set_palettes(&self, light: ThemePalette, dark: ThemePalette) {
    self.mutate_inner(|inner| {
      inner.palette = match inner.appearance {
        crate::app::window::Appearance::Light => light.clone(),
        crate::app::window::Appearance::Dark => dark.clone(),
      };
      inner.palettes = Some((light, dark));
    });
  }

  /// Light or dark: the system's where the theme follows it, and what [`Theme::set_appearance`] said.
  pub fn appearance(&self) -> crate::app::window::Appearance {
    self.inner.read().unwrap().appearance
  }

  pub fn set_appearance(&self, appearance: crate::app::window::Appearance) {
    if self.appearance() == appearance {
      return;
    }
    self.mutate_inner(|inner| {
      inner.appearance = appearance;
      if let Some((light, dark)) = &inner.palettes {
        inner.palette = match appearance {
          crate::app::window::Appearance::Light => light.clone(),
          crate::app::window::Appearance::Dark => dark.clone(),
        };
      }
    });
  }

  /// Whether the theme takes what a person set for the whole system: its appearance, its text size and bold
  /// text. It does unless told not to; an app with a look of its own, or its own setting for these, says no
  /// and sets them itself.
  pub fn follows_system(&self) -> bool {
    self.inner.read().unwrap().follows_system
  }

  pub fn set_follows_system(&self, follows: bool) {
    self.inner.write().unwrap().follows_system = follows;
  }

  /// Takes the system's settings, if the theme follows them. A shell calls it when they change.
  pub fn apply_system(&self, system: &crate::app::window::SystemSettings) {
    if !self.follows_system() {
      return;
    }
    self.set_appearance(system.appearance);
    self.set_text_scale(system.text_scale);
    self.set_bold_text(system.bold_text);
  }

  pub fn default_text_style(&self) -> TextStyle {
    self.inner.read().unwrap().typography.default_style().clone()
  }

  pub fn set_default_text_style(&self, style: TextStyle) {
    self.mutate_inner(|inner| {
      inner.base_typography.set_default_style(style);
      inner.adjust_typography();
    });
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

  /// Whether motion is to be reduced: what [`Self::set_reduced_motion`] set,
  /// or else what the user asked of every app in the system's accessibility
  /// settings, on the platforms lurq reads it on (Windows, macOS, iOS).
  ///
  /// While it is, a transition of where an element is or how large it is
  /// (its transform, offset, width and height) ends at once; a fade or a
  /// change of color still runs. Keyframe animations are the app's: read
  /// this, or `Ctx::reduced_motion`, to hold back one that moves.
  pub fn reduced_motion(&self) -> bool {
    let inner = self.inner.read().unwrap();
    inner.reduced_motion.unwrap_or(inner.system_reduced_motion)
  }

  /// Decides whether motion is reduced, for the app: `Some(true)` or
  /// `Some(false)`. `None`, the default, follows the system's setting.
  pub fn set_reduced_motion(&self, reduced: Option<bool>) {
    if self.inner.read().unwrap().reduced_motion != reduced {
      self.mutate_inner(|inner| inner.reduced_motion = reduced);
    }
  }

  /// The system's setting, from the shell that read it.
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn set_system_reduced_motion(&self, reduced: bool) {
    if self.inner.read().unwrap().system_reduced_motion != reduced {
      self.mutate_inner(|inner| inner.system_reduced_motion = reduced);
    }
  }

  pub(crate) fn version(&self) -> u64 {
    self.inner.read().unwrap().version
  }

  /// Whether `other` is this theme or a clone of it: the same values, which
  /// two separate themes at one version need not be.
  pub(crate) fn same_theme(&self, other: &Theme) -> bool {
    Arc::ptr_eq(&self.inner, &other.inner)
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
  // Reads as the pair of the getter above it.
  #[allow(clippy::type_complexity)]
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
