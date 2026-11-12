//! Nearest-face weight resolution.
//!
//! cosmic-text (checked through 0.19) only takes a face from the requested
//! family when its weight equals the requested weight exactly, or when it is a
//! variable face whose `wght` axis covers it; any other request falls through
//! to the script and common fallback families, which on Windows can land on a
//! symbol font. Resolving the weight first with fontdb's CSS font-matching query and
//! shaping with the matched face's own weight keeps text in its family and
//! selects the nearest loaded face — Medium and SemiBold faces included.

use std::collections::HashMap;

use cosmic_text::{
  Family, Weight,
  fontdb::{Database, ID, Query, Stretch},
  skrifa::{FontRef, MetadataProvider, raw::types::Tag},
};

use crate::layout::text_style::{FontStyle, FontWeight, cosmic_family};

/// Weight of the face CSS font matching selects for `family`. A family with no
/// loaded faces is matched against the generic sans-serif family instead, since
/// that is where its text falls back to; without either, the requested weight is
/// returned unchanged.
pub(crate) fn match_face_weight(db: &Database, family: &str, weight: FontWeight, style: FontStyle) -> Weight {
  let requested = weight.to_cosmic();
  [cosmic_family(family), Family::SansSerif]
    .iter()
    .find_map(|family| {
      let id = db.query(&Query {
        families: std::slice::from_ref(family),
        weight: requested,
        stretch: Stretch::Normal,
        style: style.to_cosmic(),
      })?;
      // A variable face is every weight its axis covers, whatever one weight its tables name: the
      // platform's own font is one such face on a Mac and a phone, and named Regular.
      if let Some((lightest, heaviest)) = weight_axis(db, id) {
        return Some(Weight((f32::from(requested.0)).clamp(lightest, heaviest).round() as u16));
      }
      db.face(id).map(|face| face.weight)
    })
    .unwrap_or(requested)
}

/// The ends of a face's `wght` axis, for a variable face that has one.
fn weight_axis(db: &Database, id: ID) -> Option<(f32, f32)> {
  db.with_face_data(id, |data, index| {
    let face = FontRef::from_index(data, index).ok()?;
    let axis = face.axes().iter().find(|axis| axis.tag() == Tag::new(b"wght"))?;
    Some((axis.min_value(), axis.max_value()))
  })?
}

/// Memoized [`match_face_weight`]. Must be cleared whenever fonts are loaded.
#[derive(Default)]
pub(crate) struct FaceWeights {
  resolved: HashMap<Box<str>, HashMap<(u16, FontStyle), Weight>>,
}

impl FaceWeights {
  pub(crate) fn resolve(&mut self, db: &Database, family: &str, weight: FontWeight, style: FontStyle) -> Weight {
    let key = (weight.value(), style);
    if let Some(resolved) = self.resolved.get(family).and_then(|weights| weights.get(&key)) {
      return *resolved;
    }
    let resolved = match_face_weight(db, family, weight, style);
    self.resolved.entry(family.into()).or_default().insert(key, resolved);
    resolved
  }

  pub(crate) fn clear(&mut self) {
    self.resolved.clear();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The system's own font on a Mac: one variable face, named Regular.
  const SYSTEM_FONT: &str = "/System/Library/Fonts/SFNS.ttf";

  #[test]
  fn a_variable_face_is_matched_at_the_weight_asked_for() {
    if !std::path::Path::new(SYSTEM_FONT).exists() {
      return;
    }
    let mut db = Database::new();
    db.load_font_file(SYSTEM_FONT).expect("the system font loads");
    let family = db.faces().next().expect("a face").families[0].0.clone();
    assert_eq!(
      match_face_weight(&db, &family, FontWeight::Bold, FontStyle::Normal),
      Weight(700),
      "not the one weight the face names"
    );
    assert_eq!(
      match_face_weight(&db, &family, FontWeight::Normal, FontStyle::Normal),
      Weight(400)
    );
  }
}
