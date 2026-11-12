//! The fonts the generic `monospace` and `serif` families draw with; the
//! lists are documented on [`GenericFamily`].
//!
//! cosmic-text names Noto Sans Mono and DejaVu Serif for them on every
//! platform, which Windows and macOS do not ship: text in those families fell
//! through to the fallback fonts (Segoe UI on Windows). Each platform's own
//! fonts are named here instead, the first one installed, as browsers do.
//! Without any of them the names stay cosmic-text's. `sans-serif` keeps
//! cosmic-text's name, so it draws as the default family (the empty name)
//! does, and `cursive` and `fantasy` keep fontdb's.
//!
//! iOS names `sans-serif` too: cosmic-text has no fallback fonts for it, so
//! without a name the default family would draw with whichever font comes
//! first.
//!
//! [`GenericFamily`]: crate::layout::text_style::GenericFamily

use cosmic_text::fontdb::Database;

#[cfg(target_os = "windows")]
mod platform {
  pub(super) const MONOSPACE: &[&str] = &["Consolas", "Cascadia Mono", "Courier New"];
  pub(super) const SERIF: &[&str] = &["Times New Roman"];
}

#[cfg(target_os = "macos")]
mod platform {
  pub(super) const MONOSPACE: &[&str] = &["Menlo", "Monaco", "Courier New"];
  pub(super) const SERIF: &[&str] = &["Times", "Times New Roman"];
}

#[cfg(target_os = "ios")]
mod platform {
  pub(super) const SANS_SERIF: &[&str] = &[
    ".SF UI",
    ".SF UI Text",
    ".SF NS",
    "Helvetica Neue",
    "Helvetica",
    "Arial",
  ];
  pub(super) const MONOSPACE: &[&str] = &["Menlo", "Courier New", "Courier"];
  pub(super) const SERIF: &[&str] = &["Times New Roman", "Georgia"];
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "ios")))]
mod platform {
  pub(super) const MONOSPACE: &[&str] = &["DejaVu Sans Mono", "Noto Sans Mono", "Liberation Mono"];
  pub(super) const SERIF: &[&str] = &["DejaVu Serif", "Noto Serif", "Liberation Serif"];
}

/// Names the platform's installed fonts for the generic `monospace` and
/// `serif` families in `db`. Call it after the system fonts are loaded.
pub(super) fn set_generic_families(db: &mut Database) {
  #[cfg(target_os = "ios")]
  if let Some(family) = first_installed(db, platform::SANS_SERIF) {
    db.set_sans_serif_family(family);
  }
  if let Some(family) = first_installed(db, platform::MONOSPACE) {
    db.set_monospace_family(family);
  }
  if let Some(family) = first_installed(db, platform::SERIF) {
    db.set_serif_family(family);
  }
}

fn first_installed(db: &Database, families: &[&'static str]) -> Option<&'static str> {
  families.iter().copied().find(|family| {
    db.faces()
      .any(|face| face.families.iter().any(|(name, _)| name == family))
  })
}
