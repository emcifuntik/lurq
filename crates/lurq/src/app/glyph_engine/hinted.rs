//! Grid-fitted glyphs: a font loaded as hinted is drawn at whole pixel sizes and
//! positions, with its TrueType hints applied along both axes.
//!
//! Text stays unhinted (see `render_glyph_image`): it sits at fractions of a pixel,
//! and a grid-fit outline shifted by a fraction looks different at every phase. An
//! icon font is the opposite case. Its glyphs are drawn on a pixel grid and stand
//! alone in a box, and a stroke smeared over two pixels is what its hints are there
//! to prevent.
//!
//! The outline is hinted by skrifa directly. swash's own scaler asks for linear
//! metrics, which keeps the interpreter in backward-compatible ClearType: there every
//! move along x is dropped, whatever the font's `prep` asks for.

use std::collections::{HashMap, HashSet, hash_map::Entry};

use cosmic_text::{
  CacheKey, CacheKeyFlags, Font, LayoutGlyph, PhysicalGlyph, SubpixelBin, SwashContent, SwashImage, fontdb,
};
use skrifa::{
  GlyphId, MetadataProvider,
  instance::{LocationRef, Size},
  outline::{DrawSettings, HintingInstance, HintingMode, OutlinePen},
};
use swash::{
  scale::Source,
  zeno::{Command, Format, Mask, Origin, Vector},
};

/// Marks the cache key of a glyph rasterized hinted. It is not one of cosmic-text's
/// flags: the bit keeps such a glyph apart, in the atlas, from the same glyph drawn
/// unhinted under a transform.
pub(super) const HINTED: CacheKeyFlags = CacheKeyFlags::from_bits_retain(1 << 31);

/// Smooth rendering without linear metrics: the mode in which a font's `prep` can
/// ask for native ClearType, and then its hints along x are applied too.
const MODE: HintingMode = HintingMode::Smooth {
  lcd_subpixel: None,
  preserve_linear_metrics: false,
};

/// Hinting state is kept per face and size. An interface draws its icons at a
/// handful of sizes, so the set stays small; past this many it starts over.
const INSTANCE_LIMIT: usize = 16;

/// The faces drawn grid-fitted, and the interpreter state for the sizes in use.
#[derive(Default)]
pub(super) struct HintedFonts {
  faces: HashSet<fontdb::ID>,
  instances: HashMap<(fontdb::ID, u32), HintingInstance>,
}

impl HintedFonts {
  pub(super) fn add(&mut self, faces: impl IntoIterator<Item = fontdb::ID>) {
    self.faces.extend(faces);
  }

  /// Where a glyph of a pixel-snapped run goes, and the atlas entry that draws it.
  ///
  /// A glyph of a hinted face goes to the nearest whole pixel at the nearest whole
  /// size, and its key says so. Any other glyph is placed as cosmic-text places it,
  /// with the fraction of its position kept in the key.
  pub(super) fn physical(&self, glyph: &LayoutGlyph, line_y: f32) -> PhysicalGlyph {
    let mut physical = glyph.physical((0.0, line_y), 1.0);
    if self.faces.contains(&glyph.font_id) {
      physical.x = (glyph.x + glyph.font_size * glyph.x_offset).round() as i32;
      physical.y = (line_y + glyph.y - glyph.font_size * glyph.y_offset).round() as i32;
      let key = &mut physical.cache_key;
      key.font_size_bits = glyph.font_size.round().max(1.0).to_bits();
      key.x_bin = SubpixelBin::Zero;
      key.y_bin = SubpixelBin::Zero;
      key.flags |= HINTED;
    }
    physical
  }

  /// The coverage mask of a glyph whose key came from [`Self::physical`] marked
  /// hinted. `None` when the face has no outline for it.
  pub(super) fn render(&mut self, font: &Font, face_index: u32, key: CacheKey) -> Option<SwashImage> {
    let face = skrifa::FontRef::from_index(font.data(), face_index).ok()?;
    let outlines = face.outline_glyphs();
    let glyph = outlines.get(GlyphId::new(u32::from(key.glyph_id)))?;
    let slot = (key.font_id, key.font_size_bits);
    if self.instances.len() >= INSTANCE_LIMIT && !self.instances.contains_key(&slot) {
      self.instances.clear();
    }
    let instance = match self.instances.entry(slot) {
      Entry::Occupied(entry) => entry.into_mut(),
      Entry::Vacant(entry) => {
        let size = Size::new(f32::from_bits(key.font_size_bits));
        entry.insert(HintingInstance::new(&outlines, size, LocationRef::default(), MODE).ok()?)
      }
    };
    // Not pedantic: a glyph program that fails leaves its glyph unhinted.
    let mut path = Path::default();
    glyph.draw(DrawSettings::hinted(instance, false), &mut path).ok()?;
    // `render_into`, not `render`: only it sizes the mask before working out `top`.
    let mut data = Vec::new();
    let placement = Mask::new(&path.0[..])
      .format(Format::Alpha)
      .origin(Origin::BottomLeft)
      .inspect(|format, width, height| data.resize(format.buffer_size(width, height), 0))
      .render_into(&mut data[..], None);
    Some(SwashImage {
      source: Source::Outline,
      content: SwashContent::Mask,
      placement,
      data,
    })
  }
}

/// An outline as zeno takes it.
#[derive(Default)]
struct Path(Vec<Command>);

impl OutlinePen for Path {
  fn move_to(&mut self, x: f32, y: f32) {
    self.0.push(Command::MoveTo(Vector::new(x, y)));
  }

  fn line_to(&mut self, x: f32, y: f32) {
    self.0.push(Command::LineTo(Vector::new(x, y)));
  }

  fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
    self.0.push(Command::QuadTo(Vector::new(cx, cy), Vector::new(x, y)));
  }

  fn curve_to(&mut self, ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32) {
    self.0.push(Command::CurveTo(
      Vector::new(ax, ay),
      Vector::new(bx, by),
      Vector::new(x, y),
    ));
  }

  fn close(&mut self) {
    self.0.push(Command::Close);
  }
}

#[cfg(test)]
mod tests {
  use super::super::{GLYPH_ATLAS_BYTES_PER_PIXEL, GlyphEngine};
  use crate::layout::{quad::RichTextSpan, render_list::GlyphCmd, text_style::TextStyle};

  const ICONS: &[u8] = include_bytes!("../../../assets/lurq-icons.ttf");
  /// A square outline: two upright strokes and two level ones, a twelfth of the size thick.
  const SQUARE: &str = "\u{E07A}";

  fn style(font_size: f32) -> TextStyle {
    TextStyle {
      font_family: "lurq icons".into(),
      font_size,
      ..TextStyle::default()
    }
  }

  /// An engine with the icon font in it, hinted or loaded as any font is.
  fn engine(hinted: bool) -> GlyphEngine {
    let mut engine = GlyphEngine::new();
    if hinted {
      engine.load_hinted_font(ICONS.to_vec());
    } else {
      engine.load_font(ICONS.to_vec());
    }
    let is_loaded = |face: &cosmic_text::fontdb::FaceInfo| face.families.iter().any(|(name, _)| name == "lurq icons");
    assert!(
      engine.font_system.db().faces().any(is_loaded),
      "the database should take the icon font: without it another font draws the glyph"
    );
    engine
  }

  /// The coverage of a drawn glyph, row by row, as the atlas holds it.
  fn coverage(engine: &mut GlyphEngine, glyph: &GlyphCmd) -> Vec<Vec<u8>> {
    let atlas = engine.atlas();
    let (left, right) = (glyph.atlas_min[0] as usize, glyph.atlas_max[0] as usize);
    (glyph.atlas_min[1] as usize..glyph.atlas_max[1] as usize)
      .map(|row| {
        (left..right)
          .map(|column| atlas.data[(row * atlas.width as usize + column) * GLYPH_ATLAS_BYTES_PER_PIXEL + 3])
          .collect()
      })
      .collect()
  }

  /// The row and the column through the middle of a glyph: each crosses two strokes of the square.
  fn cross_sections(rows: &[Vec<u8>]) -> [Vec<u8>; 2] {
    let level = rows[rows.len() / 2].clone();
    let upright = rows.iter().map(|row| row[row.len() / 2]).collect();
    [level, upright]
  }

  #[test]
  fn a_hinted_font_draws_its_strokes_on_whole_pixels_along_both_axes() {
    let mut engine = engine(true);

    let glyphs = engine.rasterize_text(SQUARE, &style(16.0), f32::MAX, 10.3, 20.6);

    assert_eq!(glyphs.len(), 1);
    for section in cross_sections(&coverage(&mut engine, &glyphs[0])) {
      assert!(
        section.iter().all(|&alpha| alpha == 0 || alpha == 255),
        "a stroke should cover its pixels fully or not at all: {section:?}"
      );
      assert_eq!(
        section.iter().filter(|&&alpha| alpha == 255).count(),
        2,
        "each of the two strokes should be one pixel thick at 16 px: {section:?}"
      );
    }
  }

  #[test]
  fn a_font_loaded_plainly_stays_unhinted() {
    let mut engine = engine(false);

    let glyphs = engine.rasterize_text(SQUARE, &style(16.0), f32::MAX, 10.3, 20.6);

    assert_eq!(glyphs.len(), 1);
    for section in cross_sections(&coverage(&mut engine, &glyphs[0])) {
      assert!(
        section.iter().any(|&alpha| alpha != 0 && alpha != 255),
        "an unhinted stroke a pixel and a third thick should cover some pixel in part: {section:?}"
      );
    }
  }

  #[test]
  fn a_hinted_glyph_is_drawn_once_at_the_nearest_whole_size_and_position() {
    let mut engine = engine(true);

    // At 17.5 px the second glyph starts half a pixel in: a text glyph would be drawn again for it.
    let glyphs = engine.rasterize_text(&SQUARE.repeat(2), &style(17.5), f32::MAX, 3.2, 7.7);

    assert_eq!(glyphs.len(), 2);
    for glyph in &glyphs {
      assert_eq!(glyph.x.fract(), 0.0, "x should be a whole pixel");
      assert_eq!(glyph.y.fract(), 0.0, "y should be a whole pixel");
    }
    assert_eq!(
      glyphs[0].atlas_min, glyphs[1].atlas_min,
      "both should draw the same atlas entry"
    );
    assert_eq!(engine.atlas_entries.len(), 1);
    assert_eq!(
      glyphs[1].x - glyphs[0].x,
      18.0,
      "17.5 px on from the first, at the nearest pixel"
    );
  }

  #[test]
  fn a_hinted_glyph_after_text_of_another_font_is_still_on_whole_pixels() {
    let mut engine = engine(true);
    let spans = [
      RichTextSpan {
        text: "Save ".into(),
        style: TextStyle {
          font_size: 16.0,
          ..TextStyle::default()
        },
      },
      RichTextSpan {
        text: SQUARE.into(),
        style: style(16.0),
      },
    ];
    let mut glyphs = Vec::new();

    engine.rasterize_rich_text_with_wrap_into(&spans, f32::MAX, false, 4.3, 9.6, &mut glyphs);

    let icon = glyphs.last().expect("the icon is the last glyph of the line");
    for section in cross_sections(&coverage(&mut engine, icon)) {
      assert!(
        section.iter().all(|&alpha| alpha == 0 || alpha == 255),
        "the icon should be hinted wherever the text before it ends: {section:?}"
      );
    }
  }

  #[test]
  fn a_hinted_font_in_a_run_that_is_not_snapped_is_drawn_unhinted() {
    let mut engine = engine(true);

    let snapped = engine.rasterize_text(SQUARE, &style(16.0), f32::MAX, 0.0, 0.0);
    let floating = engine.rasterize_text_unsnapped(SQUARE, &style(16.0), f32::MAX, 0.0, 0.0);

    assert_ne!(
      snapped[0].atlas_min, floating[0].atlas_min,
      "the unhinted glyph should be its own atlas entry"
    );
    let [level, _] = cross_sections(&coverage(&mut engine, &floating[0]));
    assert!(level.iter().any(|&alpha| alpha != 0 && alpha != 255), "{level:?}");
  }
}
