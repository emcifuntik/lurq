//! The fonts text falls back to on iOS, for what the font it is set in has no glyph for.
//!
//! cosmic-text chooses its fallback lists by platform and has none for iOS, which gets those of other Unixes:
//! Noto and DejaVu, which an iPhone does not ship. So an emoji, or any script the system font lacks, drew
//! nothing. iOS ships the fonts of macOS, and these are cosmic-text's lists for macOS with the system font's
//! iOS name.

use cosmic_text::Fallback;
use unicode_script::Script;

pub(super) struct IosFallback;

impl Fallback for IosFallback {
  fn common_fallback(&self) -> &[&'static str] {
    &[
      ".SF UI",
      ".SF UI Text",
      "Helvetica Neue",
      "Apple Color Emoji",
      "Apple Symbols",
      "Arial Unicode MS",
    ]
  }

  fn forbidden_fallback(&self) -> &[&'static str] {
    &[".LastResort"]
  }

  fn script_fallback(&self, script: Script, locale: &str) -> &[&'static str] {
    match script {
      Script::Arabic => &["Geeza Pro"],
      Script::Armenian => &["Helvetica Neue"],
      Script::Bengali => &["Bangla Sangam MN", "Kohinoor Bangla"],
      Script::Canadian_Aboriginal => &["Euphemia UCAS"],
      Script::Devanagari => &["Devanagari Sangam MN", "Kohinoor Devanagari"],
      Script::Ethiopic => &["Kefa"],
      Script::Gujarati => &["Gujarati Sangam MN", "Kohinoor Gujarati"],
      Script::Gurmukhi => &["Gurmukhi MN", "Gurmukhi Sangam MN"],
      Script::Han => han(locale),
      Script::Hangul => han("ko"),
      Script::Hebrew => &["Arial Hebrew", "Arial"],
      Script::Hiragana | Script::Katakana => han("ja"),
      Script::Kannada => &["Kannada Sangam MN", "Noto Sans Kannada"],
      Script::Khmer => &["Khmer Sangam MN"],
      Script::Lao => &["Lao Sangam MN"],
      Script::Malayalam => &["Malayalam Sangam MN"],
      Script::Myanmar => &["Noto Sans Myanmar"],
      Script::Oriya => &["Noto Sans Oriya"],
      Script::Sinhala => &["Sinhala Sangam MN"],
      Script::Tamil => &["Tamil Sangam MN"],
      Script::Telugu => &["Telugu Sangam MN", "Kohinoor Telugu"],
      Script::Thai => &["Thonburi"],
      Script::Tibetan => &["Kailasa"],
      Script::Yi => &["PingFang SC"],
      _ => &[],
    }
  }
}

/// The font for Han characters, which differ by region: the locale's, and Simplified Chinese without one.
fn han(locale: &str) -> &'static [&'static str] {
  match locale {
    "ja" => &["Hiragino Sans"],
    "ko" => &["Apple SD Gothic Neo"],
    "zh-HK" => &["PingFang HK"],
    "zh-TW" => &["PingFang TC"],
    _ => &["PingFang SC"],
  }
}
