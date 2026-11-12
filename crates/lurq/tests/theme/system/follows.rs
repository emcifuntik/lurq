//! What a person set for the whole system, and what the theme takes of it.

use lurq::{
  app::{
    theme::{PaletteColor, Theme, ThemePalette, TypographyStyle},
    window::{Appearance, SystemSettings},
  },
  layout::text_style::FontWeight,
  node::color::Color,
};

fn asked(change: impl FnOnce(&mut SystemSettings)) -> SystemSettings {
  let mut system = SystemSettings::default();
  change(&mut system);
  system
}

#[test]
fn the_typographys_styles_are_as_large_as_the_person_asked() {
  let theme = Theme::new();
  let body = theme.typography_style(TypographyStyle::Body).font_size;
  let heading = theme.typography_style(TypographyStyle::Heading).font_size;

  theme.apply_system(&asked(|system| system.text_scale = 1.5));
  assert_eq!(theme.typography_style(TypographyStyle::Body).font_size, body * 1.5);
  assert_eq!(
    theme.typography_style(TypographyStyle::Heading).font_size,
    heading * 1.5
  );
  assert_eq!(
    theme.scaled(10.0),
    15.0,
    "a size of the app's own is scaled when the app asks"
  );

  theme.apply_system(&SystemSettings::default());
  assert_eq!(
    theme.typography_style(TypographyStyle::Body).font_size,
    body,
    "back to the size that was set, not scaled twice"
  );
}

#[test]
fn a_style_set_while_text_is_scaled_is_set_at_its_own_size() {
  let theme = Theme::new();
  theme.set_text_scale(2.0);
  let mut caption = theme.typography_style(TypographyStyle::Caption);
  caption.font_size = 10.0;
  theme.set_typography_style(TypographyStyle::Caption, caption);
  assert_eq!(theme.typography_style(TypographyStyle::Caption).font_size, 20.0);
  theme.set_text_scale(1.0);
  assert_eq!(theme.typography_style(TypographyStyle::Caption).font_size, 10.0);
}

#[test]
fn bold_text_makes_every_style_heavier() {
  let theme = Theme::new();
  assert_eq!(theme.typography_style(TypographyStyle::Body).weight.value(), 400);
  theme.apply_system(&asked(|system| system.bold_text = true));
  assert_eq!(theme.typography_style(TypographyStyle::Body).weight.value(), 600);
  assert_eq!(
    theme.typography_style(TypographyStyle::Heading).weight.value(),
    FontWeight::Black.value(),
    "bold is as heavy as there is"
  );
}

#[test]
fn a_theme_with_a_palette_for_each_appearance_is_in_the_systems() {
  let theme = Theme::new();
  let (mut light, mut dark) = (ThemePalette::default(), ThemePalette::default());
  light.set(PaletteColor::SurfaceBase, Color::from_hex("#ffffff"));
  dark.set(PaletteColor::SurfaceBase, Color::from_hex("#000000"));
  theme.set_palettes(light, dark);
  assert_eq!(
    theme.palette_color(PaletteColor::SurfaceBase),
    Color::from_hex("#ffffff")
  );

  theme.apply_system(&asked(|system| system.appearance = Appearance::Dark));
  assert_eq!(theme.appearance(), Appearance::Dark);
  assert_eq!(
    theme.palette_color(PaletteColor::SurfaceBase),
    Color::from_hex("#000000")
  );
}

#[test]
fn a_theme_that_does_not_follow_the_system_keeps_its_own() {
  let theme = Theme::new();
  theme.set_follows_system(false);
  let body = theme.typography_style(TypographyStyle::Body).font_size;
  theme.apply_system(&asked(|system| {
    system.text_scale = 2.0;
    system.appearance = Appearance::Dark;
  }));
  assert_eq!(theme.typography_style(TypographyStyle::Body).font_size, body);
  assert_eq!(theme.appearance(), Appearance::Light);
}
