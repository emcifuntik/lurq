use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    theme::{
      BorderSize, Breakpoint, PaletteColor, RadiusSize, SpacingSize, Theme, ThemeBorderSizes, ThemePalette, ThemeRadii,
      ThemeSpacing, ThemeTypography, TypographyStyle,
    },
  },
  layout::text_style::{FontWeight, TextStyle},
  node::{Element, color::Color, dimension::Dimension},
};

use crate::support::run_pass;

#[derive(lurq::DevtoolsInspectable)]
struct Shared<T>(Arc<T>);

impl<T> Clone for Shared<T> {
  fn clone(&self) -> Self {
    Self(self.0.clone())
  }
}

impl<T> PartialEq for Shared<T> {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

impl<T> std::fmt::Debug for Shared<T> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_tuple("Shared").field(&(Arc::as_ptr(&self.0) as usize)).finish()
  }
}

#[test]
fn default_theme_has_palette() {
  let t = Theme::new();
  assert_eq!(t.palette_color(PaletteColor::Accent).to_hex(), "#2563eb");
  assert_eq!(t.palette().accent.to_hex(), "#2563eb");
}

#[test]
fn named_theme_values_can_be_set_and_resolved() {
  let mut spacing = ThemeSpacing::default();
  spacing.set(SpacingSize::Section, Dimension::Pct(5.0));
  assert_eq!(spacing.get(SpacingSize::Section), Dimension::Pct(5.0));

  let mut typography = ThemeTypography::default();
  typography.set(
    TypographyStyle::Title,
    TextStyle {
      font_size: 24.0,
      ..TextStyle::default()
    },
  );
  assert_eq!(typography.get(TypographyStyle::Title).font_size, 24.0);
  assert_eq!(typography.resolve(TypographyStyle::Title).font_size, 24.0);
}

#[test]
fn set_palette_color_updates_variant() {
  let t = Theme::new();
  t.set_palette_color(PaletteColor::Accent, Color::from_hex("#123456"));

  assert_eq!(t.palette_color(PaletteColor::Accent).to_hex(), "#123456");
  assert_eq!(t.palette().accent.to_hex(), "#123456");
}

#[test]
fn set_palette_replaces_named_palette() {
  let t = Theme::new();
  let palette = ThemePalette {
    accent: Color::from_hex("#abcdef"),
    surface_base: Color::from_hex("#101010"),
    ..ThemePalette::default()
  };

  t.set_palette(palette);

  assert_eq!(t.palette_color(PaletteColor::Accent).to_hex(), "#abcdef");
  assert_eq!(t.palette_color(PaletteColor::SurfaceBase).to_hex(), "#101010");
  let mut radii = ThemeRadii::default();
  radii.set(RadiusSize::Lg, 8.0);
  assert_eq!(radii.get(RadiusSize::Lg), 8.0);
}

#[test]
fn theme_border_sizes_have_strict_defaults() {
  let t = Theme::new();
  assert_eq!(t.border_size_value(BorderSize::Sm), 1.0);
  assert_eq!(t.border_size_value(BorderSize::Md), 2.0);
  assert_eq!(t.border_size_value(BorderSize::Lg), 3.0);
  assert_eq!(t.border_sizes().sm, 1.0);
  assert_eq!(t.border_sizes().lg, 3.0);
}

#[test]
fn theme_border_size_setter_updates_one_size() {
  let t = Theme::new();
  t.set_border_size_value(BorderSize::Md, 4.0);
  assert_eq!(t.border_size_value(BorderSize::Md), 4.0);
  assert_eq!(t.border_sizes().md, 4.0);
}

#[test]
fn theme_set_border_sizes_replaces_table() {
  let t = Theme::new();
  t.set_border_sizes(ThemeBorderSizes {
    sm: 0.5,
    md: 1.5,
    lg: 2.5,
    ..ThemeBorderSizes::default()
  });
  assert_eq!(t.border_size_value(BorderSize::Sm), 0.5);
  assert_eq!(t.border_size_value(BorderSize::Lg), 2.5);
}

#[test]
fn default_theme_has_spacing() {
  let t = Theme::new();
  assert_eq!(t.spacing_value(SpacingSize::Xs), Dimension::Px(4.0));
  assert_eq!(t.spacing_value(SpacingSize::Sm), Dimension::Px(8.0));
  assert_eq!(t.spacing_value(SpacingSize::Md), Dimension::Px(12.0));
  assert_eq!(t.spacing_value(SpacingSize::Lg), Dimension::Px(16.0));
  assert_eq!(t.spacing_value(SpacingSize::Xl), Dimension::Px(24.0));
  assert_eq!(t.spacing_value(SpacingSize::Section), Dimension::Px(32.0));
  assert_eq!(t.spacing().xs, Dimension::Px(4.0));
  assert_eq!(t.spacing().section, Dimension::Px(32.0));
}

#[test]
fn set_spacing_value_updates_named_spacing() {
  let t = Theme::new();
  t.set_spacing_value(SpacingSize::Md, 14.0);

  assert_eq!(t.spacing_value(SpacingSize::Md), Dimension::Px(14.0));
  assert_eq!(t.spacing().md, Dimension::Px(14.0));
}

#[test]
fn set_spacing_value_accepts_dimension() {
  let t = Theme::new();
  t.set_spacing_value(SpacingSize::Section, Dimension::Pct(5.0));

  assert_eq!(t.spacing_value(SpacingSize::Section), Dimension::Pct(5.0));
}

#[test]
fn set_spacing_replaces_named_spacing() {
  let t = Theme::new();
  let spacing = ThemeSpacing {
    xs: Dimension::Px(1.0),
    sm: Dimension::Px(2.0),
    md: Dimension::Px(3.0),
    lg: Dimension::Px(4.0),
    xl: Dimension::Px(5.0),
    section: Dimension::Px(6.0),
    ..ThemeSpacing::default()
  };

  t.set_spacing(spacing);

  assert_eq!(t.spacing_value(SpacingSize::Xs), Dimension::Px(1.0));
  assert_eq!(t.spacing_value(SpacingSize::Section), Dimension::Px(6.0));
}

#[test]
fn default_theme_has_radii() {
  let t = Theme::new();
  assert_eq!(t.radius_value(RadiusSize::Sm), 3.0);
  assert_eq!(t.radius_value(RadiusSize::Md), 5.0);
  assert_eq!(t.radius_value(RadiusSize::Lg), 6.0);
  assert_eq!(t.radii().sm, 3.0);
  assert_eq!(t.radii().md, 5.0);
  assert_eq!(t.radii().lg, 6.0);
}

#[test]
fn set_radius_value_updates_named_radius() {
  let t = Theme::new();
  t.set_radius_value(RadiusSize::Md, 8.0);

  assert_eq!(t.radius_value(RadiusSize::Md), 8.0);
  assert_eq!(t.radii().md, 8.0);
}

#[test]
fn set_radii_replaces_named_radii() {
  let t = Theme::new();
  let radii = ThemeRadii {
    sm: 2.0,
    md: 4.0,
    lg: 8.0,
    ..ThemeRadii::default()
  };

  t.set_radii(radii);

  assert_eq!(t.radius_value(RadiusSize::Sm), 2.0);
  assert_eq!(t.radius_value(RadiusSize::Md), 4.0);
  assert_eq!(t.radius_value(RadiusSize::Lg), 8.0);
}

#[test]
fn default_theme_has_fonts() {
  let t = Theme::new();
  let fonts = t.fonts();
  assert_eq!(fonts.body.font_size, 16.0);
  assert_eq!(fonts.heading.font_size, 24.0);
}

#[test]
fn set_fonts_updates() {
  let t = Theme::new();
  let mut fonts = t.fonts();
  fonts.body.font_size = 18.0;
  t.set_fonts(fonts);
  assert_eq!(t.fonts().body.font_size, 18.0);
}

#[test]
fn default_theme_has_typography() {
  let t = Theme::new();
  assert_eq!(t.default_text_style().font_size, 16.0);
  assert_eq!(t.typography().body.font_size, 16.0);
  assert_eq!(t.typography_style(TypographyStyle::Heading).font_size, 24.0);
}

#[test]
fn set_typography_style_updates_variant() {
  let t = Theme::new();
  t.set_typography_style(
    TypographyStyle::Title,
    TextStyle {
      font_size: 32.0,
      weight: FontWeight::Bold,
      ..TextStyle::default()
    },
  );

  let style = t.typography_style(TypographyStyle::Title);
  assert_eq!(style.font_size, 32.0);
  assert!(style.weight == FontWeight::Bold);
}

#[test]
fn set_typography_replaces_named_typography() {
  let t = Theme::new();
  let typography = ThemeTypography {
    caption: TextStyle {
      font_size: 11.0,
      ..TextStyle::default()
    },
    title: TextStyle {
      font_size: 30.0,
      ..TextStyle::default()
    },
    ..ThemeTypography::default()
  };

  t.set_typography(typography);

  assert_eq!(t.typography_style(TypographyStyle::Caption).font_size, 11.0);
  assert_eq!(t.typography_style(TypographyStyle::Title).font_size, 30.0);
}

#[test]
fn theme_lens_reads_sets_and_updates_value() {
  let theme = Theme::new();
  let brand = theme.lens(
    |theme| theme.palette_color(PaletteColor::Accent),
    |theme, color| theme.set_palette_color(PaletteColor::Accent, color),
  );

  assert_eq!(brand.get().to_hex(), "#2563eb");

  brand.set(Color::from_hex("#123456"));
  assert_eq!(theme.palette_color(PaletteColor::Accent).to_hex(), "#123456");

  brand.update(|color| *color = Color::from_hex("#abcdef"));
  assert_eq!(brand.get().to_hex(), "#abcdef");
}

struct ThemeSubscriberRoot;

struct ThemeSubscriberChild {
  renders: Arc<AtomicUsize>,
}

struct BreakpointSubscriber {
  renders: Arc<AtomicUsize>,
}

impl Component for ThemeSubscriberRoot {
  type Props = Shared<AtomicUsize>;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    ctx.mount::<ThemeSubscriberChild>(ctx.props::<Self::Props>().clone())
  }
}

impl Component for ThemeSubscriberChild {
  type Props = Shared<AtomicUsize>;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      renders: ctx.props::<Self::Props>().0.clone(),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    self.renders.fetch_add(1, Ordering::Relaxed);
    let font_size = ctx.theme().default_text_style().font_size;
    lurq::components::Text::new(&format!("font={font_size}"))
  }
}

impl Component for BreakpointSubscriber {
  type Props = Shared<AtomicUsize>;

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      renders: ctx.props::<Self::Props>().0.clone(),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    self.renders.fetch_add(1, Ordering::Relaxed);
    let breakpoint = ctx.breakpoint().map(|breakpoint| breakpoint.as_str()).unwrap_or("base");
    lurq::components::Text::new(breakpoint)
  }
}

#[test]
fn theme_change_rerenders_subscriber_components() {
  let renders = Arc::new(AtomicUsize::new(0));
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<ThemeSubscriberRoot>(&mut app, Shared(renders.clone()));

  run_pass(&mut tree);
  assert_eq!(renders.load(Ordering::Relaxed), 1);

  app.theme().set_default_text_style(TextStyle {
    font_size: 22.0,
    ..TextStyle::default()
  });
  run_pass(&mut tree);

  assert_eq!(renders.load(Ordering::Relaxed), 2);
  assert!(tree.find_element(|el| el.text_content() == Some("font=22")).is_some());
}

#[test]
fn theme_breakpoint_change_recomputes_without_reentering_theme_lock() {
  let renders = Arc::new(AtomicUsize::new(0));
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<BreakpointSubscriber>(&mut app, Shared(renders.clone()));

  run_pass(&mut tree);
  assert_eq!(renders.load(Ordering::Relaxed), 1);
  assert!(tree.find_element(|el| el.text_content() == Some("md")).is_some());

  app.theme().set_breakpoint_value(Breakpoint::Md, 900.0);
  run_pass(&mut tree);

  assert_eq!(renders.load(Ordering::Relaxed), 2);
  assert!(tree.find_element(|el| el.text_content() == Some("sm")).is_some());
}

#[test]
fn window_resize_recomputes_breakpoint_without_reentering_window_lock() {
  let renders = Arc::new(AtomicUsize::new(0));
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<BreakpointSubscriber>(&mut app, Shared(renders.clone()));

  run_pass(&mut tree);
  assert_eq!(renders.load(Ordering::Relaxed), 1);
  assert!(tree.find_element(|el| el.text_content() == Some("md")).is_some());

  tree.resize(1200, 600);
  run_pass(&mut tree);

  assert_eq!(renders.load(Ordering::Relaxed), 2);
  assert!(tree.find_element(|el| el.text_content() == Some("lg")).is_some());
}

#[test]
fn theme_clone_shares_state() {
  let t1 = Theme::new();
  let t2 = t1.clone();
  t1.set_palette_color(PaletteColor::Accent, Color::from_hex("#123456"));
  assert_eq!(t2.palette_color(PaletteColor::Accent).to_hex(), "#123456");
}
