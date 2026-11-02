use std::path::Path;

use parking_lot::Mutex;

use super::{shape_cache::ShapeCache, *};
use crate::{app::glyph_engine::GlyphEngine, canvas::frame_cache::Limits};

const WEIGHT_PROBE: &[u8] = include_bytes!("../../../tests/assets/weight_probe/LurqWeightProbe-Regular.ttf");
const LIGATURE_PROBE: &[u8] = include_bytes!("../../../tests/assets/ligature_probe/LurqLigatureProbe-Regular.ttf");
const NO_ALIASES: [(&str, &str); 0] = [];

fn engine() -> CanvasTextEngine {
  let mut fonts = FontSystem::new_with_locale_and_db("en-US".into(), Default::default());
  for data in [WEIGHT_PROBE, LIGATURE_PROBE] {
    fonts.db_mut().load_font_data(data.to_vec());
  }
  CanvasTextEngine::new(fonts, Default::default())
}

const RED: Color = Color::new(255, 0, 0, 255);
const BLUE: Color = Color::new(0, 0, 255, 128);

#[test]
fn metrics_preserve_real_glyph_bounds_without_rgba_and_fill_after_measure_is_visible() {
  let mut engine = engine();
  let font = CanvasFont::new("Lurq Weight Probe", 10.);
  let measured = engine.measure("aaaa", &font, RED).unwrap();
  assert_eq!(measured.width, 20.);
  assert!(measured.pixels.is_none());
  assert!(measured.data.is_empty());
  assert_eq!(measured.asset_id, 0);
  let rendered = engine.shape("aaaa", &font, 1., RED).unwrap();
  assert!(rendered.pixels.is_some());
  assert!(rendered.data.chunks_exact(4).any(|p| p[3] > 0));
  for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
    for baseline in [
      TextBaseline::Alphabetic,
      TextBaseline::Top,
      TextBaseline::Middle,
      TextBaseline::Bottom,
    ] {
      assert_eq!(measured.metrics(align, baseline), rendered.metrics(align, baseline));
    }
  }
  assert!(!Arc::ptr_eq(&measured, &rendered));
  assert!(Arc::ptr_eq(&measured, &engine.measure("aaaa", &font, RED).unwrap()));
  assert!(Arc::ptr_eq(&rendered, &engine.shape("aaaa", &font, 1., RED).unwrap()));
}

#[test]
fn rendered_cache_preserves_color_scale_and_previous_drawing_owners() {
  let mut engine = engine();
  let font = CanvasFont::new("Lurq Weight Probe", 20.);
  let red = engine.shape("aaaa", &font, 1., RED).unwrap();
  let original = red.data.as_ref().clone();
  engine.measure("aaaa", &font, BLUE).unwrap();
  let blue = engine.shape("aaaa", &font, 1., BLUE).unwrap();
  assert!(red.data.chunks_exact(4).all(|p| p[1] == 0 && p[2] == 0 && p[0] == p[3]));
  assert!(
    blue
      .data
      .chunks_exact(4)
      .all(|p| p[0] == 0 && p[1] == 0 && p[2] == p[3] && p[3] <= 128)
  );
  assert!(blue.data.chunks_exact(4).any(|p| p[3] > 0));
  let scaled = engine.shape("aaaa", &font, 2., RED).unwrap();
  assert_eq!((red.width, scaled.width), (40., 40.));
  assert!(scaled.data.len() > red.data.len());
  assert_ne!(red.asset_id, blue.asset_id);
  assert_ne!(red.asset_id, scaled.asset_id);
  // Without frames the cache is least recently used: large drawings past its
  // budget evict the first one, while its previous owners keep their pixels.
  let large = CanvasFont::new("Lurq Weight Probe", 200.);
  let mut drawn = 0;
  for index in 0.. {
    drawn += engine
      .shape("aaaa", &large, 1., Color::new(0, index, 0, 255))
      .unwrap()
      .data
      .len();
    if drawn > 2 * shape_cache::LIMITS.budget {
      break;
    }
  }
  assert_eq!(red.data.as_ref(), &original);
  let revisited = engine.shape("aaaa", &font, 1., RED).unwrap();
  assert_eq!(revisited.data.as_ref(), &original);
  assert_ne!(revisited.asset_id, red.asset_id);
  assert!(engine.shaped.bytes() <= shape_cache::LIMITS.budget);
}

#[test]
fn metrics_keep_spacing_features_and_refusals() {
  let mut engine = engine();
  let mut font = CanvasFont::new("Lurq Weight Probe", 10.);
  for (spacing, expected) in [(0., 20.), (1.5, 26.), (-1., 16.)] {
    font.letter_spacing = spacing;
    assert_eq!(engine.measure("aaaa", &font, RED).unwrap().width, expected);
  }
  let mut font = CanvasFont::new("Lurq Ligature Probe", 10.);
  assert_eq!(engine.measure("a --a", &font, RED).unwrap().width, 20.);
  font.font_features = FontFeatures::from([crate::layout::text_style::FontFeature::disable(*b"liga")]);
  assert_eq!(engine.measure("a --a", &font, RED).unwrap().width, 25.);
  let oversized = "a".repeat(65_537);
  assert!(matches!(
    engine.measure(&oversized, &font, RED),
    Err(CanvasError::TextTooLarge)
  ));
  assert!(matches!(
    engine.shape(&oversized, &font, 1., RED),
    Err(CanvasError::TextTooLarge)
  ));
  let oversized_font = CanvasFont::new("Lurq Weight Probe", 4097.);
  assert!(matches!(
    engine.measure("a", &oversized_font, RED),
    Err(CanvasError::TextTooLarge)
  ));
  assert!(matches!(
    engine.shape("a", &oversized_font, 1., RED),
    Err(CanvasError::TextTooLarge)
  ));
}

/// One frame of a dense page, in the order it is drawn every frame: a measure
/// and a fill of each label. 700 distinct keys, like the ~684 of a real page.
fn page(engine: &mut CanvasTextEngine, font: &CanvasFont) -> Vec<Arc<ShapedText>> {
  (0..350)
    .flat_map(|index: usize| {
      let text = "a".repeat(1 + index % 4);
      let color = Color::new(index as u8, (index / 256) as u8, 96, 255);
      [
        engine.measure(&text, font, color).unwrap(),
        engine.shape(&text, font, 1., color).unwrap(),
      ]
    })
    .collect()
}

#[test]
fn a_page_with_more_keys_than_the_old_entry_limit_is_shaped_once() {
  let mut engine = engine();
  let font = CanvasFont::new("Lurq Weight Probe", 10.);
  let first = page(&mut engine, &font);
  assert!(first.iter().skip(1).step_by(2).all(|shaped| shaped.pixels.is_some()));
  for frame in 2..=4 {
    let next = page(&mut engine, &font);
    let reshaped = first.iter().zip(&next).filter(|(a, b)| !Arc::ptr_eq(a, b)).count();
    assert_eq!(
      reshaped,
      0,
      "frame {frame} shaped {reshaped} of {} texts again",
      first.len()
    );
  }
}

/// Indices of the texts `next` shaped again instead of returning from cache.
fn reshaped(first: &[Arc<ShapedText>], next: &[Arc<ShapedText>]) -> Vec<usize> {
  (0..first.len())
    .filter(|&index| !Arc::ptr_eq(&first[index], &next[index]))
    .collect()
}

/// Caches sized against the charge of one real page, so that the page itself
/// is larger than the budget.
fn page_limits(engine: &mut CanvasTextEngine, font: &CanvasFont, budget: f32, ceiling: f32) -> Limits {
  page(engine, font);
  let bytes = engine.shaped.bytes() as f32;
  Limits {
    budget: (bytes * budget) as usize,
    ceiling: (bytes * ceiling) as usize,
  }
}

#[test]
fn a_framed_page_over_the_budget_is_kept_whole() {
  let mut engine = engine();
  let font = CanvasFont::new("Lurq Weight Probe", 10.);
  let limits = page_limits(&mut engine, &font, 0.5, 1.5);
  // Without frames the same limits are least recently used and miss on every text.
  engine.shaped = ShapeCache::with_limits(limits);
  let first = page(&mut engine, &font);
  assert_eq!(reshaped(&first, &page(&mut engine, &font)).len(), first.len());
  engine.shaped = ShapeCache::with_limits(limits);
  engine.finish_frame();
  let first = page(&mut engine, &font);
  for frame in 2..=4 {
    engine.finish_frame();
    assert_eq!(
      reshaped(&first, &page(&mut engine, &font)),
      Vec::<usize>::new(),
      "frame {frame}"
    );
  }
  assert!(engine.shaped.bytes() > limits.budget && engine.shaped.bytes() <= limits.ceiling);
}

#[test]
fn a_framed_page_beyond_the_ceiling_keeps_the_same_texts_cached() {
  let mut engine = engine();
  let font = CanvasFont::new("Lurq Weight Probe", 10.);
  let limits = page_limits(&mut engine, &font, 0.25, 0.5);
  engine.shaped = ShapeCache::with_limits(limits);
  engine.finish_frame();
  let first = page(&mut engine, &font);
  engine.finish_frame();
  let overflow = reshaped(&first, &page(&mut engine, &font));
  assert!(
    !overflow.is_empty() && overflow.len() < first.len() * 3 / 4,
    "{}",
    overflow.len()
  );
  for frame in 3..=5 {
    engine.finish_frame();
    assert_eq!(reshaped(&first, &page(&mut engine, &font)), overflow, "frame {frame}");
  }
  assert!(engine.shaped.bytes() <= limits.ceiling);
}

#[test]
fn closing_a_frame_without_text_does_not_age_the_cache() {
  let mut engine = engine();
  let font = CanvasFont::new("Lurq Weight Probe", 10.);
  let limits = page_limits(&mut engine, &font, 0.5, 1.5);
  engine.shaped = ShapeCache::with_limits(limits);
  engine.finish_frame();
  let first = page(&mut engine, &font);
  for _ in 0..4 {
    engine.finish_frame();
  }
  assert_eq!(reshaped(&first, &page(&mut engine, &font)), Vec::<usize>::new());
}

/// The app's text engines, which a window's layout binds to its canvases, with
/// the probe face installed.
fn app_fonts() -> GlyphEngine {
  let mut glyphs = GlyphEngine::new();
  glyphs.install_fonts([WEIGHT_PROBE.to_vec()], NO_ALIASES);
  glyphs
}

/// One frame of a page charged more than the shape budget and less than its
/// ceiling, like the 10.3 MiB of text of a real design at Fit: a measure and a
/// fill of each of 112 large labels.
fn large_page(engine: &Mutex<CanvasTextEngine>) -> Vec<Arc<ShapedText>> {
  let font = CanvasFont::new("Lurq Weight Probe", 96.);
  let mut engine = engine.lock();
  (0..112)
    .flat_map(|index: usize| {
      let text = "a".repeat(4 + index % 4);
      let color = Color::new(index as u8, 0, 160, 255);
      [
        engine.measure(&text, &font, color).unwrap(),
        engine.shape(&text, &font, 1., color).unwrap(),
      ]
    })
    .collect()
}

#[test]
fn a_text_engine_replaced_after_its_frames_began_keeps_its_first_page_over_the_budget() {
  let limits = shape_cache::LIMITS;
  let mut glyphs = app_fonts();
  let engine = glyphs.canvas_text_engine();
  // A GPU renderer ends the engine's frames, and its page stretches the cache.
  engine.lock().finish_frame();
  large_page(&engine);
  engine.lock().finish_frame();
  assert!(engine.lock().shaped.bytes() > limits.budget);
  // Installing a page's face replaces the engine, and the next layout binds
  // the replacement to the same canvases.
  glyphs.install_fonts([LIGATURE_PROBE.to_vec()], NO_ALIASES);
  let replacement = glyphs.canvas_text_engine();
  assert!(!Arc::ptr_eq(&engine, &replacement));
  let first = large_page(&replacement);
  replacement.lock().finish_frame();
  let again = reshaped(&first, &large_page(&replacement)).len();
  assert_eq!(
    again,
    0,
    "the second frame shaped {again} of {} texts again",
    first.len()
  );
  let bytes = replacement.lock().shaped.bytes();
  assert!(bytes > limits.budget && bytes <= limits.ceiling, "{bytes}");
}

#[test]
fn a_text_engine_no_renderer_framed_is_replaced_by_a_least_recently_used_one() {
  let mut glyphs = app_fonts();
  let engine = glyphs.canvas_text_engine();
  glyphs.install_fonts([LIGATURE_PROBE.to_vec()], NO_ALIASES);
  let replacement = glyphs.canvas_text_engine();
  assert!(!Arc::ptr_eq(&engine, &replacement));
  assert!(!replacement.lock().is_framed());
  // Without frames the page over the budget evicts its own first texts.
  large_page(&replacement);
  assert!(replacement.lock().shaped.bytes() <= shape_cache::LIMITS.budget);
}

/// Applies `change` to an app whose text engine a GPU renderer has framed, and
/// checks that the next layout binds a framed replacement.
fn assert_replaced_framed(change: &str, apply: impl FnOnce(&mut GlyphEngine)) {
  let mut glyphs = app_fonts();
  let engine = glyphs.canvas_text_engine();
  engine.lock().finish_frame();
  apply(&mut glyphs);
  let replacement = glyphs.canvas_text_engine();
  assert!(!Arc::ptr_eq(&engine, &replacement), "{change}");
  assert!(replacement.lock().is_framed(), "{change}");
}

#[test]
fn every_font_change_replaces_a_framed_text_engine_with_a_framed_one() {
  let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets/ligature_probe");
  assert_replaced_framed("load_font", |glyphs| glyphs.load_font(LIGATURE_PROBE.to_vec()));
  assert_replaced_framed("load_font_file", |glyphs| {
    glyphs.load_font_file(&assets.join("LurqLigatureProbe-Regular.ttf"))
  });
  assert_replaced_framed("load_fonts_dir", |glyphs| glyphs.load_fonts_dir(&assets));
  assert_replaced_framed("register_font", |glyphs| {
    glyphs.register_font("Probe", "Lurq Weight Probe")
  });
  assert_replaced_framed("install_fonts", |glyphs| {
    glyphs.install_fonts([LIGATURE_PROBE.to_vec()], NO_ALIASES)
  });
  assert_replaced_framed("clear_cache", GlyphEngine::clear_cache);
  assert_replaced_framed("two changes before a layout binds the replacement", |glyphs| {
    glyphs.register_font("Probe", "Lurq Weight Probe");
    glyphs.clear_cache();
  });
}
