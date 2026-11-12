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
fn bold_text_is_the_themes_to_tell_and_leaves_the_styles_as_set() {
  let theme = Theme::new();
  theme.apply_system(&asked(|system| system.bold_text = true));
  assert!(theme.bold_text());
  // The weight is added where text is drawn, for every text: a style's own weight is not also raised.
  assert_eq!(theme.typography_style(TypographyStyle::Body).weight.value(), 400);
  assert_eq!(
    theme.typography_style(TypographyStyle::Heading).weight.value(),
    FontWeight::Bold.value()
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

/// How wide a word is laid out in a tree of its own.
fn laid_out(app: &mut lurq::app::App, tree: &mut lurq::app::Tree) -> f32 {
  tree.pass_headless(app);
  tree
    .find_element(|element| element.text_content() == Some("Heavier words"))
    .expect("the text")
    .rect()
    .2
}

#[test]
fn bold_text_makes_text_with_a_weight_of_its_own_heavier_too() {
  use lurq::{components::Text, layout::text_style::TextStyle};
  let styled = |weight: FontWeight| {
    Text::styled(
      "Heavier words",
      TextStyle {
        weight,
        ..TextStyle::default()
      },
    )
  };
  let mut app = lurq::app::App::new();
  let mut tree = lurq::app::Tree::new();
  tree.resize(600, 200);
  tree.set_root(styled(FontWeight::Normal));
  let normal = laid_out(&mut app, &mut tree);
  tree.set_root(styled(FontWeight::SemiBold));
  let heavy = laid_out(&mut app, &mut tree);
  if (heavy - normal).abs() < 0.01 {
    // The platform's font has one weight here: nothing to see.
    return;
  }

  tree.set_root(styled(FontWeight::Normal));
  app.theme().set_bold_text(true);
  let bold = laid_out(&mut app, &mut tree);
  assert!(
    (bold - heavy).abs() < 0.01,
    "two weights up: {normal} to {bold}, not {heavy}"
  );

  app.theme().set_bold_text(false);
  assert!((laid_out(&mut app, &mut tree) - normal).abs() < 0.01, "and back");
}
