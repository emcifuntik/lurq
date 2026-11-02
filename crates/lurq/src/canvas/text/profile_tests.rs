//! Real Canvas text/cache work with an embedded font, without a GPU/window.
use std::time::Duration;

use parking_lot::Mutex;

use super::*;
use crate::{
  app::{
    PassReport,
    profiler::{CanvasTextProfile, ProfileReport, SampleData, producer::WindowProfiler},
  },
  canvas::{CanvasHandle, Context2D},
};

fn context() -> Context2D {
  let mut fonts = FontSystem::new_with_locale_and_db("en-US".into(), Default::default());
  fonts
    .db_mut()
    .load_font_data(include_bytes!("../../../tests/assets/weight_probe/LurqWeightProbe-Regular.ttf").to_vec());
  let canvas = CanvasHandle::new();
  {
    let mut surface = canvas.inner.lock();
    surface.text = Some(Arc::new(Mutex::new(CanvasTextEngine::new(fonts, Default::default()))));
    surface.state.font = CanvasFont::new("Lurq Weight Probe", 10.);
    surface.metrics.scale_factor = 2.;
  }
  canvas.context_2d()
}

fn complete(producer: &mut WindowProfiler, started: std::time::Instant) {
  producer.finish_pass(
    started,
    &PassReport {
      required: true,
      ..Default::default()
    },
    "none",
    None,
  );
}

fn text(report: &ProfileReport, window: &str) -> CanvasTextProfile {
  report
    .samples
    .iter()
    .find_map(|sample| match &sample.data {
      SampleData::Pass(pass) if sample.window == window => pass.canvas_text,
      _ => None,
    })
    .expect("captured Canvas text profile")
}

#[test]
fn canvas_text_profiles_real_measure_fill_cache_and_cpu_stages() {
  let ctx = context();
  let mut producer = WindowProfiler::new();
  let handle = producer.handle();
  let id = handle.start(Default::default()).unwrap().id;
  let (started, phase) = producer.begin_pass(1);
  let scope = producer.context.canvas_text_scope();
  // measure uses scale 1 and fill uses scale 2: distinct real cache entries.
  assert_eq!(ctx.measure_text("aaaa").unwrap().width, 20.);
  ctx.fill_text("aaaa", 0., 0.).unwrap();
  ctx.measure_text("aaaa").unwrap();
  ctx.fill_text("aaaa", 0., 0.).unwrap();
  complete(&mut producer, started);
  drop(scope);
  drop(phase);
  let report = handle.end(id).unwrap();
  let profile = text(&report, "main");
  assert_eq!(
    (profile.measure_calls, profile.fill_calls, profile.shape_calls),
    (2, 2, 4)
  );
  assert_eq!(
    (
      profile.shape_cache_hits,
      profile.shape_cache_misses,
      profile.shape_cache_evictions
    ),
    (2, 2, 0)
  );
  assert!(profile.buffer_font_shape > Duration::ZERO);
  assert!(profile.glyph_prepare > Duration::ZERO);
  assert!(profile.bitmap_composition > Duration::ZERO);
  assert!(profile.buffer_font_shape + profile.glyph_prepare + profile.bitmap_composition <= profile.total);
  let surface = ctx.canvas.inner.lock();
  let engine = surface.text.as_ref().unwrap().lock();
  let produced: usize = engine.shaped.results().map(|result| result.data.len()).sum();
  assert_eq!(profile.produced_bitmap_bytes, produced as u64);
  assert!(produced > 0);
  #[cfg(any(feature = "serde", feature = "mcp"))]
  {
    let export = report.to_json();
    let value = &export["samples"][0]["data"]["canvas_text"];
    assert_eq!(value["counts"]["shape_cache_misses"], 2);
    assert_eq!(value["counts"]["produced_bitmap_bytes"], produced as u64);
    assert!(value["cpu_timings_ms"]["glyph_prepare"].as_f64().unwrap() > 0.);
  }
}

#[test]
fn canvas_text_nested_windows_restore_parent_and_sessions_do_not_mix() {
  let ctx = context();
  let mut parent = WindowProfiler::new();
  let handle = parent.handle();
  let mut child = WindowProfiler::new();
  child.attach(&parent, "w1".into(), false);
  let first = handle.start(Default::default()).unwrap().id;
  let (parent_start, parent_phase) = parent.begin_pass(1);
  let parent_scope = parent.context.canvas_text_scope();
  ctx.measure_text("aaaa").unwrap();
  let second = handle.start(Default::default()).unwrap().id;
  let (child_start, child_phase) = child.begin_pass(1);
  let child_scope = child.context.canvas_text_scope();
  ctx.measure_text("aaaa").unwrap();
  complete(&mut child, child_start);
  drop(child_scope);
  drop(child_phase);
  let ended_second = handle.end(second).unwrap();
  assert_eq!(ended_second.samples.len(), 1);
  assert_eq!(ended_second.samples[0].window, "w1");
  assert_eq!(text(&ended_second, "w1").shape_cache_hits, 1);
  ctx.measure_text("aaaa").unwrap();
  complete(&mut parent, parent_start);
  drop(parent_scope);
  drop(parent_phase);
  let ended_first = handle.end(first).unwrap();
  assert_eq!(ended_first.samples.len(), 2);
  let outer = text(&ended_first, "main");
  assert_eq!(
    (outer.measure_calls, outer.shape_cache_hits, outer.shape_cache_misses),
    (2, 1, 1)
  );
  let inner = text(&ended_first, "w1");
  assert_eq!(
    (inner.measure_calls, inner.shape_cache_hits, inner.shape_cache_misses),
    (1, 1, 0)
  );
  assert_eq!(inner.produced_bitmap_bytes, 0);
  assert_eq!(
    inner.buffer_font_shape + inner.glyph_prepare + inner.bitmap_composition,
    Duration::ZERO
  );
  assert_eq!(ended_second.samples.len(), 1);
  // Text outside a pass warms the same cache, without contaminating the next pass.
  ctx.measure_text("aa").unwrap();
  let third = handle.start(Default::default()).unwrap().id;
  let (started, phase) = parent.begin_pass(2);
  let scope = parent.context.canvas_text_scope();
  ctx.measure_text("aa").unwrap();
  complete(&mut parent, started);
  drop(scope);
  drop(phase);
  let next = text(&handle.end(third).unwrap(), "main");
  assert_eq!(
    (next.shape_calls, next.shape_cache_hits, next.produced_bitmap_bytes),
    (1, 1, 0)
  );
}

#[test]
fn canvas_text_idle_boundary_and_real_cache_eviction_are_bounded() {
  let ctx = context();
  let mut producer = WindowProfiler::new();
  let handle = producer.handle();
  let (started, phase) = producer.begin_pass(1);
  let scope = producer.context.canvas_text_scope();
  ctx.measure_text("aaaa").unwrap();
  let id = handle.start(Default::default()).unwrap().id;
  ctx.measure_text("aaaa").unwrap();
  complete(&mut producer, started);
  drop(scope);
  drop(phase);
  let crossing = handle.read(id).unwrap();
  assert!(crossing.samples.is_empty());
  assert_eq!(crossing.boundary_excluded_samples, 1);
  let (started, phase) = producer.begin_pass(2);
  let scope = producer.context.canvas_text_scope();
  ctx.measure_text("aaaa").unwrap();
  // More distinct font sizes than the former 256 entries: measurements are
  // charged bytes, not entries, so they all stay, with no bitmap cost.
  for size in 1..=257 {
    ctx.set_font(CanvasFont::new("Lurq Weight Probe", size as f32));
    ctx.measure_text("").unwrap();
  }
  complete(&mut producer, started);
  drop(scope);
  drop(phase);
  let report = handle.end(id).unwrap();
  let profile = text(&report, "main");
  assert_eq!(
    (
      profile.shape_calls,
      profile.shape_cache_hits,
      profile.shape_cache_misses
    ),
    (258, 1, 257)
  );
  assert_eq!(profile.shape_cache_evictions, 0);
  assert_eq!(profile.produced_bitmap_bytes, 0);
  assert_eq!(profile.bitmap_composition, Duration::ZERO);
  let surface = ctx.canvas.inner.lock();
  assert_eq!(surface.text.as_ref().unwrap().lock().shaped.len(), 258);
}

#[test]
fn canvas_text_profiles_a_frame_that_stretches_or_overflows_the_shape_cache() {
  let ctx = context();
  let engine = ctx.canvas.inner.lock().text.clone().unwrap();
  // Measurements are charged their text plus 256 bytes: one fits the budget,
  // two fit the ceiling.
  engine.lock().shaped = super::shape_cache::ShapeCache::with_limits(crate::canvas::frame_cache::Limits {
    budget: 300,
    ceiling: 600,
  });
  ctx.canvas.finish_text_frame();
  let mut producer = WindowProfiler::new();
  let handle = producer.handle();
  let id = handle.start(Default::default()).unwrap().id;
  let (started, phase) = producer.begin_pass(1);
  let scope = producer.context.canvas_text_scope();
  for text in ["aa", "aaa", "aaaa"] {
    ctx.measure_text(text).unwrap();
  }
  complete(&mut producer, started);
  drop(scope);
  drop(phase);
  let report = handle.end(id).unwrap();
  let profile = text(&report, "main");
  assert_eq!(
    (
      profile.shape_cache_misses,
      profile.shape_cache_evictions,
      profile.shape_cache_uncached,
      profile.shape_cache_stretch_bytes
    ),
    (3, 0, 1, 258 + 259 - 300)
  );
  #[cfg(any(feature = "serde", feature = "mcp"))]
  {
    let export = report.to_json();
    let counts = &export["samples"][0]["data"]["canvas_text"]["counts"];
    assert_eq!(counts["shape_cache_uncached"], 1);
    assert_eq!(counts["shape_cache_stretch_bytes"], 217);
  }
}

#[test]
fn canvas_text_metrics_do_not_compose_or_produce_bitmap_bytes() {
  let ctx = context();
  let mut producer = WindowProfiler::new();
  let handle = producer.handle();
  let id = handle.start(Default::default()).unwrap().id;
  let (started, phase) = producer.begin_pass(1);
  let scope = producer.context.canvas_text_scope();
  assert_eq!(ctx.measure_text("aaaa").unwrap().width, 20.);
  complete(&mut producer, started);
  drop(scope);
  drop(phase);
  let report = handle.end(id).unwrap();
  let profile = text(&report, "main");
  assert_eq!(
    (profile.measure_calls, profile.fill_calls, profile.shape_cache_misses),
    (1, 0, 1)
  );
  assert!(profile.glyph_prepare > Duration::ZERO);
  assert_eq!(profile.bitmap_composition, Duration::ZERO);
  assert_eq!(profile.produced_bitmap_bytes, 0);
}
