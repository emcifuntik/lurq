// Translucent colour through the real render engines, read back from a hidden
// window of the test's own. Every pipeline that paints UI content (quads,
// glyphs, images) must blend like CSS: on the sRGB-encoded channels, so
// #000000A6 over #EEEEEE is #535353 as in a browser or design tool, not the
// #959595 that blending in linear light gives.
//
// cargo test -p lurq --features wgpu,dx12,raster,screenshot --lib blend_readback -- --ignored --test-threads=1

use std::sync::Arc;

use super::readback_window;
use crate::{
  app::render_engine::{CapturedFrame, RenderEngine},
  layout::{
    quad::ClipRect,
    render_list::{GlyphAtlas, GlyphCmd, RectCmd, RenderList},
  },
  node::color::Color,
};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 32;
/// Each pipeline paints one 32 px column (quad, glyph, image); the last
/// column shows the clear colour.
const COLUMN: f32 = 32.0;

fn rect(x: f32, color: Color) -> RectCmd {
  RectCmd {
    order: 0,
    x,
    y: 0.0,
    width: COLUMN,
    height: HEIGHT as f32,
    color,
    radii: [0.0; 4],
    stroke: [0.0; 4],
    stroke_color: Color::new(0, 0, 0, 0),
    stroke_style: crate::layout::render_list::StrokeStyle::default(),
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [0.0; 2],
    clip: ClipRect::default(),
    gradient: None,
    shadow: None,
  }
}

/// A glyph instance covering its column with full coverage from a solid white
/// 4x4 atlas, so its colour alone decides the result.
fn glyph(x: f32, color: Color) -> GlyphCmd {
  GlyphCmd {
    order: 1,
    x,
    y: 0.0,
    width: COLUMN,
    height: HEIGHT as f32,
    color: color.to_linear_f32_array(),
    atlas_min: [1.0, 1.0],
    atlas_max: [3.0, 3.0],
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [0.0; 2],
    sharpness: 1.0,
    color_glyph: false,
    shadow_sigma: 0.0,
    clip: ClipRect::default(),
  }
}

/// `id` must change with `color`: engines cache image textures by id and version.
fn image(x: f32, color: Color, id: u64) -> crate::images::ImageCmd {
  crate::images::ImageCmd {
    order: 2,
    x,
    y: 0.0,
    width: COLUMN,
    height: HEIGHT as f32,
    image_id: id,
    frame_index: 0,
    version: 1,
    data: Arc::new([color.r(), color.g(), color.b(), color.a()].repeat(4)),
    animation_frames: None,
    native: None,
    image_width: 2,
    image_height: 2,
    image_format: crate::images::ImagePixelFormat::Rgba8,
    uv_min: [0.0, 0.0],
    uv_max: [1.0, 1.0],
    radii: [0.0; 4],
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [0.0; 2],
    clip: ClipRect::default(),
    opacity: 1.0,
  }
}

fn scene(backdrop: Color, scrim: Color, image_id: u64) -> RenderList {
  RenderList {
    clear_color: backdrop,
    rects: vec![rect(0.0, scrim)],
    glyphs: vec![glyph(COLUMN, scrim)],
    images: vec![image(COLUMN * 2.0, scrim, image_id)],
    #[cfg(feature = "svg")]
    svgs: Vec::new(),
    layers: Vec::new(),
    backdrops: Vec::new(),
    atlas: GlyphAtlas {
      data: Arc::from([255_u8; 4 * 4 * 4].as_slice()),
      width: 4,
      height: 4,
      version: 1,
      dirty_rects: Arc::from(Vec::new()),
      dirty_from_version: 0,
    },
  }
}

/// CSS source-over of a straight-alpha `source` onto an opaque `backdrop`.
fn css_source_over(backdrop: Color, source: Color) -> [u8; 3] {
  let alpha = f32::from(source.a()) / 255.0;
  let channel = |back: u8, front: u8| (f32::from(front) * alpha + f32::from(back) * (1.0 - alpha)).round() as u8;
  [
    channel(backdrop.r(), source.r()),
    channel(backdrop.g(), source.g()),
    channel(backdrop.b(), source.b()),
  ]
}

fn capture(engine: &mut dyn RenderEngine, list: &RenderList) -> CapturedFrame {
  readback_window::capture(engine, list, WIDTH, HEIGHT)
}

fn assert_blends_like_css(engine: &mut dyn RenderEngine, backend: &str) {
  let pipelines = ["quad", "glyph", "image"];
  let cases = [
    (Color::from_hex("#eeeeee"), Color::from_hex("#000000a6")),
    (Color::from_hex("#202830"), Color::from_hex("#f0a0404d")),
  ];
  for (image_id, (backdrop, scrim)) in (1..).zip(cases) {
    let frame = capture(engine, &scene(backdrop, scrim, image_id));
    let expected = css_source_over(backdrop, scrim);
    for (column, pipeline) in pipelines.iter().enumerate() {
      let x = column as u32 * COLUMN as u32 + COLUMN as u32 / 2;
      let index = ((HEIGHT / 2 * frame.width + x) * 4) as usize;
      let actual = [frame.rgba[index], frame.rgba[index + 1], frame.rgba[index + 2]];
      let close = actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 1);
      assert!(
        close,
        "{backend} {pipeline}: {scrim:?} over {backdrop:?} gave {actual:?}, CSS gives {expected:?}"
      );
    }
    let clear_index = ((HEIGHT / 2 * frame.width + WIDTH - 1) * 4) as usize;
    let clear = [
      frame.rgba[clear_index],
      frame.rgba[clear_index + 1],
      frame.rgba[clear_index + 2],
    ];
    assert_eq!(
      clear,
      [backdrop.r(), backdrop.g(), backdrop.b()],
      "{backend} clear colour"
    );
  }
  assert_eq!(
    css_source_over(Color::from_hex("#eeeeee"), Color::from_hex("#000000a6")),
    [0x53; 3]
  );
}

#[cfg(feature = "wgpu")]
#[test]
#[ignore = "requires a Windows desktop and a GPU adapter; creates a hidden window"]
fn wgpu_translucent_content_blends_like_css() {
  let mut engine = crate::app::wgpu_render::WgpuRenderEngine::new();
  assert_blends_like_css(&mut engine, "wgpu");
}

#[cfg(feature = "dx12")]
#[test]
#[ignore = "requires a Windows desktop and a DX12 adapter; creates a hidden window"]
fn dx12_translucent_content_blends_like_css() {
  let mut engine = crate::app::dx12_render::Dx12RenderEngine::new();
  assert_blends_like_css(&mut engine, "dx12");
}
