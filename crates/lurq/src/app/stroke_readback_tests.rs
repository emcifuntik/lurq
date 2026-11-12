// Strokes through the real render engines, read back from a hidden window of
// the test's own and compared, pixel by pixel, with the formulas of
// `layout/stroke.rs`: rings of one width and of several, a stroke on one side
// of a rounded box, radii at the hole and at the middle of the stroke, and
// dashes.
//
// cargo test -p lurq --features wgpu,dx12,screenshot --lib stroke_readback -- --ignored --test-threads=1

use std::sync::Arc;

use super::readback_window;
use crate::{
  app::render_engine::RenderEngine,
  layout::{
    quad::ClipRect,
    render_list::{GlyphAtlas, RectCmd, RenderList, StrokeStyle},
    stroke::{StrokeShape, stroke_coverage},
  },
  node::color::Color,
};

const WIDTH: u32 = 200;
const HEIGHT: u32 = 120;
/// How far a channel may be from the reference away from an edge: the shaders work in 32-bit floats.
const TOLERANCE: u8 = 6;
/// And on a pixel at an edge. The ramp there is as wide as the distance changes over a pixel, which a GPU
/// takes from derivatives over a block of two by two pixels and the reference from the pixel's own.
const EDGE_TOLERANCE: u8 = 72;

/// A white stroke with no fill.
fn stroke(
  [x, y, width, height]: [f32; 4],
  radii: [f32; 4],
  widths: [f32; 4],
  dash: [f32; 2],
  radii_at: f32,
) -> RectCmd {
  RectCmd {
    order: 0,
    x,
    y,
    width,
    height,
    color: Color::new(0, 0, 0, 0),
    radii,
    stroke: widths,
    stroke_color: Color::new(255, 255, 255, 255),
    stroke_style: StrokeStyle { dash, radii_at },
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [width * 0.5, height * 0.5],
    clip: ClipRect::default(),
    gradient: None,
    shadow: None,
  }
}

fn scenes() -> Vec<(&'static str, RectCmd)> {
  const SOLID: [f32; 2] = [0.0; 2];
  vec![
    (
      "a ring of one width",
      stroke([20.0, 20.0, 120.0, 60.0], [10.0; 4], [2.0; 4], SOLID, 0.0),
    ),
    (
      "one side of a rounded box",
      stroke([20.0, 20.0, 120.0, 60.0], [12.0; 4], [0.0, 0.0, 0.0, 4.0], SOLID, 0.0),
    ),
    (
      "one side wider than the corner",
      stroke([20.0, 20.0, 120.0, 60.0], [6.0; 4], [10.0, 0.0, 0.0, 0.0], SOLID, 0.0),
    ),
    (
      "a width per side and a radius per corner",
      stroke(
        [20.0, 20.0, 140.0, 70.0],
        [12.0, 0.0, 20.0, 6.0],
        [1.0, 3.0, 5.0, 2.0],
        SOLID,
        0.0,
      ),
    ),
    (
      "one side at a fraction of a pixel",
      stroke([20.4, 20.7, 120.3, 60.6], [9.0; 4], [0.0, 0.0, 0.0, 3.0], SOLID, 0.0),
    ),
    (
      "three sides at a fraction of a pixel",
      stroke([20.4, 20.7, 120.3, 60.6], [9.0; 4], [2.0, 2.0, 0.0, 2.0], SOLID, 0.0),
    ),
    (
      "radii at the hole",
      stroke([27.0, 27.0, 106.0, 46.0], [10.0; 4], [3.0; 4], SOLID, 1.0),
    ),
    (
      "radii at the hole, one side",
      stroke([26.0, 30.0, 104.0, 40.0], [10.0; 4], [0.0, 0.0, 0.0, 4.0], SOLID, 1.0),
    ),
    (
      "radii at the middle of the stroke",
      stroke([28.0, 28.0, 104.0, 44.0], [10.0; 4], [4.0; 4], SOLID, 0.5),
    ),
    (
      "a square box with radii at the hole",
      stroke([27.0, 27.0, 106.0, 46.0], [0.0; 4], [3.0; 4], SOLID, 1.0),
    ),
    (
      "a dashed ring",
      stroke([20.0, 20.0, 120.0, 60.0], [10.0; 4], [2.0; 4], [6.0, 4.0], 0.0),
    ),
    (
      "a dashed ring with a radius per corner",
      stroke(
        [20.0, 20.0, 140.0, 70.0],
        [12.0, 0.0, 20.0, 6.0],
        [2.0; 4],
        [7.0, 5.0],
        0.0,
      ),
    ),
    (
      "a dashed bottom side",
      stroke(
        [20.0, 20.0, 150.0, 40.0],
        [0.0; 4],
        [0.0, 0.0, 2.0, 0.0],
        [5.0, 3.0],
        0.0,
      ),
    ),
  ]
}

fn list(rect: RectCmd) -> RenderList {
  RenderList {
    clear_color: Color::new(0, 0, 0, 255),
    rects: vec![rect],
    glyphs: Vec::new(),
    #[cfg(feature = "raster")]
    images: Vec::new(),
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

/// What the reference says each pixel of a white stroke over black is.
fn reference(rect: &RectCmd) -> Vec<u8> {
  let shape = StrokeShape {
    half: [rect.width * 0.5, rect.height * 0.5],
    radii: rect.radii,
    stroke: rect.stroke,
    style: rect.stroke_style,
  };
  let centre = (rect.x + shape.half[0], rect.y + shape.half[1]);
  let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
  for y in 0..HEIGHT {
    for x in 0..WIDTH {
      let coverage = stroke_coverage(x as f32 + 0.5 - centre.0, y as f32 + 0.5 - centre.1, &shape);
      pixels.push((coverage * 255.0).round() as u8);
    }
  }
  pixels
}

/// Whether the reference has `value` at `(x, y)` and at every pixel around it.
fn all_around(reference: &[u8], x: u32, y: u32, value: u8) -> bool {
  (-1_i32..=1).all(|dy| {
    (-1_i32..=1).all(|dx| {
      let (nx, ny) = (x as i32 + dx, y as i32 + dy);
      let inside = nx >= 0 && ny >= 0 && nx < WIDTH as i32 && ny < HEIGHT as i32;
      !inside || reference[(ny as u32 * WIDTH + nx as u32) as usize] == value
    })
  })
}

/// The stroke is where the reference has it: a pixel the reference paints whole, with whole pixels all
/// around, is painted whole, and one it leaves empty among empty ones stays empty. On the pixels at an
/// edge, where a renderer takes its ramp from its own derivatives, the two are close, and over the whole
/// frame they cover the same area.
fn assert_strokes_match_the_reference<E: RenderEngine>(mut engine: impl FnMut() -> E, backend: &str) {
  for (scene, rect) in scenes() {
    let expected = reference(&rect);
    let lit = expected.iter().filter(|value| **value > 200).count();
    assert!(lit > 40, "{scene}: the reference paints the stroke ({lit} pixels)");
    let frame = readback_window::capture(&mut engine(), &list(rect), WIDTH, HEIGHT);
    assert_eq!((frame.width, frame.height), (WIDTH, HEIGHT));
    let (mut painted_area, mut expected_area) = (0_u64, 0_u64);
    for (index, (painted, expected_value)) in frame.rgba.chunks_exact(4).zip(&expected).enumerate() {
      let (x, y) = (index as u32 % WIDTH, index as u32 / WIDTH);
      let painted = painted[0];
      painted_area += u64::from(painted);
      expected_area += u64::from(*expected_value);
      let limit = if all_around(&expected, x, y, 255) || all_around(&expected, x, y, 0) {
        TOLERANCE
      } else {
        EDGE_TOLERANCE
      };
      assert!(
        painted.abs_diff(*expected_value) <= limit,
        "{backend}, {scene}: ({x}, {y}) is {painted}, the reference says {expected_value}"
      );
    }
    let off = painted_area.abs_diff(expected_area) as f64 / expected_area as f64;
    assert!(
      off <= 0.03,
      "{backend}, {scene}: the stroke covers {painted_area}, the reference {expected_area}"
    );
  }
}

#[cfg(feature = "wgpu")]
#[test]
#[ignore = "requires a Windows desktop and a GPU adapter; creates a hidden window"]
fn wgpu_strokes_match_the_reference() {
  assert_strokes_match_the_reference(crate::app::wgpu_render::WgpuRenderEngine::new, "wgpu");
}

#[cfg(feature = "dx12")]
#[test]
#[ignore = "requires a Windows desktop and a DX12 adapter; creates a hidden window"]
fn dx12_strokes_match_the_reference() {
  assert_strokes_match_the_reference(crate::app::dx12_render::Dx12RenderEngine::new, "dx12");
}
