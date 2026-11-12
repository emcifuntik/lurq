//! CSS generic font families. A `font_family` of `"monospace"`, `"serif"`,
//! `"sans-serif"`, `"cursive"` or `"fantasy"` asks for the platform's font
//! for that family, not for a font of that name; any other name is a font's.
//!
//! A monospaced face gives "iiii" and "MMMM" the same width; a proportional
//! one gives "MMMM" about three times the width of "iiii".

use lurq::{
  app::{App, Tree},
  components::Text,
  layout::{
    Constraints, Size,
    text_style::{GenericFamily, TextStyle},
  },
};

use crate::support::TestSurface;

/// Draws "a" half an em wide (see `tests/layout/font_weight.rs`), and
/// "i" and "M" not at all.
const PROBE_FAMILY: &str = "Lurq Weight Probe";
const PROBE: &[u8] = include_bytes!("../assets/weight_probe/LurqWeightProbe-Regular.ttf");

fn width(app: &mut App, text: &str, family: &str) -> f32 {
  let style = TextStyle {
    font_family: family.into(),
    font_size: 20.0,
    ..TextStyle::default()
  };
  let mut tree = Tree::new();
  tree.set_root(Text::styled(text, style).nowrap());
  tree.set_layout_constraints_override(Some(Constraints::loose(Size::new(1000.0, 100.0))));
  tree.pass(app, &TestSurface);
  tree.last_layout().expect("layout").size.width
}

#[test]
fn generic_family_names_are_the_css_keywords_in_any_case() {
  for (name, generic) in [
    ("serif", GenericFamily::Serif),
    ("sans-serif", GenericFamily::SansSerif),
    ("monospace", GenericFamily::Monospace),
    ("cursive", GenericFamily::Cursive),
    ("fantasy", GenericFamily::Fantasy),
    ("Monospace", GenericFamily::Monospace),
    ("MONOSPACE", GenericFamily::Monospace),
    ("Sans-Serif", GenericFamily::SansSerif),
  ] {
    assert_eq!(GenericFamily::from_name(name), Some(generic), "{name:?}");
    assert_eq!(generic.keyword(), name.to_ascii_lowercase());
  }
  for name in [
    "",
    "mono",
    "sans serif",
    "sans",
    " monospace",
    "monospace ",
    "Noto Sans Mono",
    "system-ui",
    PROBE_FAMILY,
  ] {
    assert_eq!(GenericFamily::from_name(name), None, "{name:?}");
  }
}

#[test]
fn generic_family_monospace_draws_every_glyph_with_one_advance() {
  let mut app = App::new();
  for family in ["monospace", "Monospace"] {
    let narrow = width(&mut app, "iiii", family);
    let wide = width(&mut app, "MMMM", family);
    assert!(narrow > 0.0, "{family:?} draws \"iiii\"");
    assert!(
      (narrow - wide).abs() < 0.01,
      "{family:?} draws \"iiii\" {narrow} and \"MMMM\" {wide} wide"
    );
  }
  let body = TextStyle::default().font_family;
  let narrow = width(&mut app, "iiii", &body);
  let wide = width(&mut app, "MMMM", &body);
  assert!(
    wide > 2.0 * narrow,
    "the default family {body:?} is proportional: \"iiii\" {narrow}, \"MMMM\" {wide}"
  );
}

#[test]
fn generic_family_sans_serif_draws_as_the_empty_family() {
  let mut app = App::new();
  for text in ["iiii", "MMMM", "Generic families"] {
    assert_eq!(
      width(&mut app, text, "sans-serif"),
      width(&mut app, text, ""),
      "{text:?}"
    );
  }
}

#[test]
fn generic_family_leaves_other_names_to_the_font_of_that_name() {
  let mut app = App::new();
  app.install_fonts([PROBE.to_vec()], std::iter::empty::<(&str, &str)>());
  assert!((width(&mut app, "aaaa", PROBE_FAMILY) - 40.0).abs() < 0.01);
  // A name that is not a keyword is looked up as a font; with no font of that
  // name, it draws in the fallback fonts, which are proportional.
  let missing = "Lurq Missing Monospace";
  let narrow = width(&mut app, "iiii", missing);
  let wide = width(&mut app, "MMMM", missing);
  assert!(wide > 2.0 * narrow, "{missing:?}: \"iiii\" {narrow}, \"MMMM\" {wide}");
}

#[test]
fn generic_family_alias_of_a_keyword_draws_in_the_aliased_font() {
  let mut app = App::new();
  app.install_fonts([PROBE.to_vec()], [("monospace", PROBE_FAMILY)]);
  assert!((width(&mut app, "aaaa", "monospace") - 40.0).abs() < 0.01);
}

#[test]
fn generic_family_alias_to_a_keyword_draws_in_the_generic_family() {
  let mut app = App::new();
  app.register_font("code", "monospace");
  let narrow = width(&mut app, "iiii", "code");
  let wide = width(&mut app, "MMMM", "code");
  assert!((narrow - wide).abs() < 0.01, "\"iiii\" {narrow}, \"MMMM\" {wide}");
}

#[test]
fn generic_family_alias_of_a_keyword_matches_the_keyword_in_any_case() {
  let mut app = App::new();
  app.install_fonts([PROBE.to_vec()], [("Monospace", PROBE_FAMILY)]);
  for family in ["monospace", "MONOSPACE", "Monospace"] {
    assert!((width(&mut app, "aaaa", family) - 40.0).abs() < 0.01, "{family:?}");
  }
}

/// The font a generic family is expected to draw with on this platform (the
/// first of its list), and a file that holds it where the platform ships it.
struct PlatformFont {
  family: &'static str,
  file: &'static str,
}

#[cfg(target_os = "windows")]
const MONOSPACE: PlatformFont = PlatformFont {
  family: "Consolas",
  file: r"C:\Windows\Fonts\consola.ttf",
};
#[cfg(target_os = "windows")]
const SERIF: PlatformFont = PlatformFont {
  family: "Times New Roman",
  file: r"C:\Windows\Fonts\times.ttf",
};
#[cfg(target_os = "macos")]
const MONOSPACE: PlatformFont = PlatformFont {
  family: "Menlo",
  file: "/System/Library/Fonts/Menlo.ttc",
};
#[cfg(target_os = "macos")]
const SERIF: PlatformFont = PlatformFont {
  family: "Times",
  file: "/System/Library/Fonts/Times.ttc",
};
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const MONOSPACE: PlatformFont = PlatformFont {
  family: "DejaVu Sans Mono",
  file: "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
};
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const SERIF: PlatformFont = PlatformFont {
  family: "DejaVu Serif",
  file: "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
};

const SAMPLE: &str = "Hamburgefonstiv 0123 MW il";

/// Whether `font` is installed; says why the test is skipped when it is not.
fn installed(font: &PlatformFont) -> bool {
  let installed = std::path::Path::new(font.file).exists();
  if !installed {
    eprintln!(
      "skipped: {} is not installed here ({} is missing), so the generic family falls back to another font",
      font.family, font.file
    );
  }
  installed
}

#[test]
fn generic_family_monospace_is_the_platform_monospace_font() {
  if !installed(&MONOSPACE) {
    return;
  }
  let mut app = App::new();
  assert_eq!(
    width(&mut app, SAMPLE, "monospace"),
    width(&mut app, SAMPLE, MONOSPACE.family),
    "monospace draws in {}",
    MONOSPACE.family
  );
}

#[test]
fn generic_family_serif_is_the_platform_serif_font() {
  if !installed(&SERIF) {
    return;
  }
  let mut app = App::new();
  let serif = width(&mut app, SAMPLE, "serif");
  assert_eq!(
    serif,
    width(&mut app, SAMPLE, SERIF.family),
    "serif draws in {}",
    SERIF.family
  );
  assert_eq!(serif, width(&mut app, SAMPLE, "Serif"));
  let sans = width(&mut app, SAMPLE, "sans-serif");
  // Two fonts set a sentence at two widths, however near: the platform's serif and sans-serif can be within
  // a pixel of each other over a short sample, and one font is at the same width to the last bit.
  assert!(
    (serif - sans).abs() > 0.01,
    "serif ({serif}) and sans-serif ({sans}) draw in different fonts"
  );
}
