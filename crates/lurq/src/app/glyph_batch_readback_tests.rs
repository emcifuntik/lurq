// Glyph runs through the real render engines, read back from a hidden window
// of the test's own. The engines draw several runs with one instanced draw
// (`render::glyph_batch`); the pixels must be those of a draw per run with
// each run under its own clip.
//
// The scene is drawn twice. Once as texts come: runs one after another in
// paint order, which the engines batch. Once with a transparent rect between
// every two runs, which keeps each run in a draw of its own, and with a glyph
// outside every clip, which makes each clipped run keep its scissor. Both
// frames must be the same, pixel for pixel.
//
// cargo test -p lurq --features wgpu,dx12,screenshot --lib glyph_batch_readback -- --ignored --test-threads=1

use std::sync::Arc;

use super::readback_window;
use crate::{
  app::render_engine::{CapturedFrame, RenderEngine},
  layout::{
    quad::ClipRect,
    render_list::{GlyphAtlas, GlyphCmd, RectCmd, RenderList},
  },
  node::{border::BorderRadius, color::Color},
};

const WIDTH: u32 = 96;
const HEIGHT: u32 = 56;
const CLEAR: Color = Color::new(24, 24, 27, 255);
/// Side of the one glyph in the atlas: an 8 px opaque square inside the two
/// transparent texels the glyph atlas keeps around every glyph.
const GLYPH: f32 = 12.0;
const ATLAS: u32 = 16;

fn atlas() -> GlyphAtlas {
  let mut data = vec![0_u8; (ATLAS * ATLAS * 4) as usize];
  for y in 2..10 {
    for x in 2..10 {
      let index = ((y * ATLAS + x) * 4) as usize;
      data[index..index + 4].copy_from_slice(&[255; 4]);
    }
  }
  GlyphAtlas {
    data: Arc::from(data),
    width: ATLAS,
    height: ATLAS,
    version: 1,
    dirty_rects: Arc::from(Vec::new()),
    dirty_from_version: 0,
  }
}

fn clip(x: f32, y: f32, width: f32, height: f32) -> ClipRect {
  ClipRect {
    x,
    y,
    width,
    height,
    active: true,
    border_radius: None,
  }
}

fn glyph(order: usize, x: f32, y: f32, color: Color, clip: ClipRect) -> GlyphCmd {
  GlyphCmd {
    order,
    x,
    y,
    width: GLYPH,
    height: GLYPH,
    color: color.to_linear_f32_array(),
    atlas_min: [0.0, 0.0],
    atlas_max: [GLYPH, GLYPH],
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [0.0; 2],
    sharpness: 1.0,
    color_glyph: false,
    shadow_sigma: 0.0,
    clip,
  }
}

/// The texts of the scene, in paint order: where each one's glyphs are, its
/// colour and its clip.
fn runs() -> Vec<(Vec<(f32, f32)>, Color, ClipRect)> {
  let row = |y: f32, xs: &[f32]| xs.iter().map(|x| (*x, y)).collect::<Vec<_>>();
  let labels = clip(2.0, 2.0, 44.0, 16.0);
  let rounded = ClipRect {
    border_radius: Some(BorderRadius {
      top_left: 6.0,
      top_right: 6.0,
      bottom_right: 6.0,
      bottom_left: 6.0,
    }),
    ..clip(2.0, 38.0, 44.0, 16.0)
  };
  vec![
    // Inside its clip.
    (row(4.0, &[4.0, 16.0, 28.0]), Color::from_hex("#ef4444"), labels),
    // Its clip cuts only the transparent edge of its glyphs.
    (
      row(20.0, &[50.0, 62.0, 74.0]),
      Color::from_hex("#eab308"),
      clip(51.0, 21.0, 36.0, 10.0),
    ),
    // No clip.
    (row(21.0, &[4.0, 16.0]), Color::from_hex("#22c55e"), ClipRect::default()),
    // Translucent over the first text: paint order shows.
    (
      row(8.0, &[10.0, 22.0]),
      Color::from_hex("#ffffff80"),
      ClipRect::default(),
    ),
    // Translucent over both, under the first text's clip again.
    (row(2.0, &[14.0]), Color::from_hex("#3b82f680"), labels),
    // Its clip cuts through its last glyph.
    (
      row(4.0, &[52.0, 64.0, 76.0]),
      Color::from_hex("#a855f7"),
      clip(50.0, 2.0, 30.0, 16.0),
    ),
    // Under a rounded clip, which cuts the corner of its first glyph's ink.
    (row(36.0, &[0.0, 14.0, 28.0]), Color::from_hex("#06b6d4"), rounded),
    // A second text under the same rounded clip, over the end of the first.
    (row(42.0, &[34.0]), Color::from_hex("#f97316"), rounded),
  ]
}

fn list(rects: Vec<RectCmd>, glyphs: Vec<GlyphCmd>) -> RenderList {
  RenderList {
    clear_color: CLEAR,
    rects,
    glyphs,
    #[cfg(feature = "raster")]
    images: Vec::new(),
    #[cfg(feature = "svg")]
    svgs: Vec::new(),
    layers: Vec::new(),
    backdrops: Vec::new(),
    atlas: atlas(),
  }
}

/// The texts one after another, as a tree paints them.
fn batched() -> RenderList {
  let glyphs = runs()
    .into_iter()
    .enumerate()
    .flat_map(|(order, (places, color, clip))| places.into_iter().map(move |(x, y)| glyph(order, x, y, color, clip)))
    .collect();
  list(Vec::new(), glyphs)
}

/// The same texts, each kept in a draw of its own under its own clip.
fn one_by_one() -> RenderList {
  let mut rects = Vec::new();
  let mut glyphs = Vec::new();
  for (index, (places, color, clip)) in runs().into_iter().enumerate() {
    let order = index * 2;
    glyphs.extend(places.into_iter().map(|(x, y)| glyph(order, x, y, color, clip)));
    if clip.active {
      // Wholly outside the clip: it paints nothing, and the run is cut.
      glyphs.push(glyph(order, clip.x + clip.width + 4.0, clip.y, color, clip));
    }
    // Paints nothing, and comes between this text and the next.
    rects.push(RectCmd {
      order: order + 1,
      x: 0.0,
      y: 0.0,
      width: WIDTH as f32,
      height: HEIGHT as f32,
      color: Color::new(0, 0, 0, 0),
      radii: [0.0; 4],
      stroke: [0.0; 4],
      stroke_color: Color::new(0, 0, 0, 0),
      transform: [1.0, 0.0, 0.0, 1.0],
      transform_origin: [0.0; 2],
      clip: ClipRect::default(),
      gradient: None,
      shadow: None,
    });
  }
  list(rects, glyphs)
}

fn pixel(frame: &CapturedFrame, x: u32, y: u32) -> [u8; 3] {
  let index = ((y * frame.width + x) * 4) as usize;
  [frame.rgba[index], frame.rgba[index + 1], frame.rgba[index + 2]]
}

/// The batched frame, after checking it against the frame drawn run by run.
fn assert_batches_paint_like_single_runs(engine: &mut dyn RenderEngine, backend: &str) -> CapturedFrame {
  let frames = readback_window::capture_each(engine, &[&batched(), &one_by_one()], WIDTH, HEIGHT);
  let (batched, single) = (&frames[0], &frames[1]);
  let differing = batched
    .rgba
    .chunks_exact(4)
    .zip(single.rgba.chunks_exact(4))
    .position(|(a, b)| a != b);
  assert_eq!(
    differing.map(|index| (index as u32 % WIDTH, index as u32 / WIDTH)),
    None,
    "{backend}: the batched frame differs from the frame drawn run by run at this pixel"
  );

  let clear = [CLEAR.r(), CLEAR.g(), CLEAR.b()];
  let close = |actual: [u8; 3], expected: [u8; 3]| actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 1);
  // A glyph's ink, and the transparent edge around it.
  assert!(close(pixel(batched, 8, 8), [0xef, 0x44, 0x44]), "{backend}: ink");
  assert_eq!(pixel(batched, 4, 8), clear, "{backend}: a glyph's transparent edge");
  // The clip that cuts a glyph cuts it in a batch too: ink up to x 80.
  assert!(
    close(pixel(batched, 79, 8), [0xa8, 0x55, 0xf7]),
    "{backend}: inside the cutting clip"
  );
  assert_eq!(pixel(batched, 81, 8), clear, "{backend}: past the cutting clip");
  // The rounded clip cuts the corner of its first glyph's ink.
  assert_eq!(pixel(batched, 2, 38), clear, "{backend}: the rounded clip's corner");
  assert!(
    close(pixel(batched, 6, 42), [0x06, 0xb6, 0xd4]),
    "{backend}: inside the rounded clip"
  );
  // Later texts paint over earlier ones: white at half alpha over red.
  let over = pixel(batched, 13, 12);
  assert!(
    over[0] > 0xef && over[1] > 0x90 && over[1] < 0xb0,
    "{backend}: translucent text over the first text gave {over:?}"
  );
  frames.into_iter().next().expect("the batched frame")
}

#[cfg(feature = "wgpu")]
#[test]
#[ignore = "requires a Windows desktop and a GPU adapter; creates a hidden window"]
fn wgpu_glyph_batches_paint_like_single_runs() {
  assert_batches_paint_like_single_runs(&mut crate::app::wgpu_render::WgpuRenderEngine::new(), "wgpu");
}

#[cfg(feature = "dx12")]
#[test]
#[ignore = "requires a Windows desktop and a DX12 adapter; creates a hidden window"]
fn dx12_glyph_batches_paint_like_single_runs() {
  assert_batches_paint_like_single_runs(&mut crate::app::dx12_render::Dx12RenderEngine::new(), "dx12");
}

#[cfg(all(feature = "wgpu", feature = "dx12"))]
#[test]
#[ignore = "requires a Windows desktop, a GPU adapter and DX12; creates hidden windows"]
fn wgpu_and_dx12_glyph_batches_match_each_other() {
  let wgpu = assert_batches_paint_like_single_runs(&mut crate::app::wgpu_render::WgpuRenderEngine::new(), "wgpu");
  let dx12 = assert_batches_paint_like_single_runs(&mut crate::app::dx12_render::Dx12RenderEngine::new(), "dx12");
  let worst = wgpu
    .rgba
    .iter()
    .zip(&dx12.rgba)
    .map(|(a, b)| a.abs_diff(*b))
    .max()
    .unwrap_or(0);
  assert!(worst <= 2, "the backends differ by {worst}");
}

/// The batched scene is fewer draws than the scene drawn run by run.
#[cfg(all(feature = "wgpu", feature = "perf_profile"))]
#[test]
#[ignore = "requires a Windows desktop and a GPU adapter; creates a hidden window"]
fn wgpu_glyph_runs_share_draws() {
  let mut engine = crate::app::wgpu_render::WgpuRenderEngine::new();
  assert_draw_calls(&mut engine);
}

#[cfg(all(feature = "dx12", feature = "perf_profile"))]
#[test]
#[ignore = "requires a Windows desktop and a DX12 adapter; creates a hidden window"]
fn dx12_glyph_runs_share_draws() {
  let mut engine = crate::app::dx12_render::Dx12RenderEngine::new();
  assert_draw_calls(&mut engine);
}

#[cfg(feature = "perf_profile")]
fn assert_draw_calls(engine: &mut dyn RenderEngine) {
  readback_window::capture(engine, &batched(), WIDTH, HEIGHT);
  let batched = engine.last_profile().expect("a render profile").draw_calls;
  readback_window::capture(engine, &one_by_one(), WIDTH, HEIGHT);
  let single = engine.last_profile().expect("a render profile").draw_calls;
  // Five spared texts in one draw, the cut one alone, two under the rounded
  // clip together; drawn run by run, eight texts and eight rects.
  assert_eq!(batched, 3);
  assert_eq!(single, 16);
}
