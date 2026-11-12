// Flattened group opacity through the real render engines, read back from a
// hidden window of the test's own. A faded subtree must look like CSS
// `opacity` or a design tool: painted as one layer, then composited once.
//
// cargo test -p lurq --features wgpu,dx12,screenshot --lib opacity_layer_readback -- --ignored --test-threads=1

use std::sync::Arc;

use super::readback_window;
use crate::{
  app::render_engine::RenderEngine,
  layout::{
    opacity_layer::{LayerBounds, LayerCmd},
    quad::ClipRect,
    render_list::{GlyphAtlas, GlyphCmd, RectCmd, RenderList},
  },
  node::{border::BorderRadius, color::Color},
};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 96;
const IDENTITY: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

fn rect(order: usize, [x, y, width, height]: [f32; 4], color: Color) -> RectCmd {
  RectCmd {
    order,
    x,
    y,
    width,
    height,
    color,
    radii: [0.0; 4],
    stroke: [0.0; 4],
    stroke_color: Color::new(0, 0, 0, 0),
    stroke_style: crate::layout::render_list::StrokeStyle::default(),
    transform: IDENTITY,
    transform_origin: [width * 0.5, height * 0.5],
    clip: ClipRect::default(),
    gradient: None,
    shadow: None,
  }
}

/// A glyph instance with full coverage from the solid white atlas, so its
/// colour alone decides the result.
fn glyph(order: usize, [x, y, width, height]: [f32; 4], color: Color) -> GlyphCmd {
  GlyphCmd {
    order,
    x,
    y,
    width,
    height,
    color: color.to_linear_f32_array(),
    atlas_min: [1.0, 1.0],
    atlas_max: [3.0, 3.0],
    transform: IDENTITY,
    transform_origin: [0.0; 2],
    sharpness: 1.0,
    color_glyph: false,
    shadow_sigma: 0.0,
    clip: ClipRect::default(),
  }
}

fn layer(start_order: usize, end_order: usize, opacity: f32, [x, y, width, height]: [u32; 4]) -> LayerCmd {
  LayerCmd {
    start_order,
    end_order,
    opacity,
    bounds: LayerBounds { x, y, width, height },
    blur: None,
  }
}

fn list(clear_color: Color, rects: Vec<RectCmd>, glyphs: Vec<GlyphCmd>, layers: Vec<LayerCmd>) -> RenderList {
  RenderList {
    clear_color,
    rects,
    glyphs,
    #[cfg(feature = "raster")]
    images: Vec::new(),
    #[cfg(feature = "svg")]
    svgs: Vec::new(),
    layers,
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

fn render(engine: &mut dyn RenderEngine, list: &RenderList) -> Vec<u8> {
  let frame = readback_window::capture(engine, list, WIDTH, HEIGHT);
  assert_eq!((frame.width, frame.height), (WIDTH, HEIGHT));
  frame.rgba
}

fn pixel(frame: &[u8], x: u32, y: u32) -> [u8; 3] {
  let index = ((y * WIDTH + x) * 4) as usize;
  [frame[index], frame[index + 1], frame[index + 2]]
}

fn assert_pixel(frame: &[u8], (x, y): (u32, u32), expected: [u8; 3], what: &str) {
  let actual = pixel(frame, x, y);
  assert!(
    actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 1),
    "{what} at ({x}, {y}): {actual:?}, expected {expected:?}"
  );
}

/// `opacity` of `front` over `back`, per sRGB channel as CSS blends.
fn mix(front: u8, back: u8, opacity: f32) -> u8 {
  (f32::from(front) * opacity + f32::from(back) * (1.0 - opacity)).round() as u8
}

const BACKGROUND: u8 = 0x1c;
const FILL: u8 = 0xe4;
const LABEL: u8 = 0x17;
/// Button box: x, y, width, height.
const BUTTON: [f32; 4] = [20.0, 20.0, 100.0, 36.0];

/// A disabled primary button: light fill with a 1 px border of the same
/// colour, a dark label, the whole button at opacity 0.4, over a dark
/// background.
fn button_scene() -> RenderList {
  let mut button = rect(0, BUTTON, Color::new(FILL, FILL, FILL, 255));
  button.radii = [6.0; 4];
  button.stroke = [1.0; 4];
  button.stroke_color = Color::new(FILL, FILL, FILL, 255);
  let label = glyph(1, [50.0, 30.0, 40.0, 16.0], Color::new(LABEL, LABEL, LABEL, 255));
  list(
    Color::new(BACKGROUND, BACKGROUND, BACKGROUND, 255),
    vec![button],
    vec![label],
    vec![layer(0, 2, 0.4, [18, 18, 104, 40])],
  )
}

fn assert_flattened_button(frame: &[u8], backend: &str) {
  let fill = mix(FILL, BACKGROUND, 0.4);
  let label = mix(LABEL, BACKGROUND, 0.4);
  assert_eq!((fill, label), (0x6c, 0x1a), "the design's colours");
  assert_pixel(frame, (30, 38), [fill; 3], &format!("{backend}: fill"));
  // Fading each piece gives #4A4A4A: the label blends with the faded fill.
  assert_pixel(frame, (70, 38), [label; 3], &format!("{backend}: label"));
  // The border paints over the fill: flattened it is the fill; faded on its
  // own it would be a lighter ring (#9C9C9C).
  for (x, y) in [(20, 38), (119, 38), (70, 20), (70, 55)] {
    assert_pixel(frame, (x, y), [fill; 3], &format!("{backend}: border"));
  }
  // Nothing in or around the button is lighter than the faded fill: its
  // anti-aliased corners and edges blend between the fill and the background.
  for y in 16..60 {
    for x in 16..124 {
      let value = pixel(frame, x, y)[0];
      assert!(
        (label..=fill).contains(&value),
        "{backend}: ({x}, {y}) is {value:#04x}, outside #{label:02X}..#{fill:02X}"
      );
    }
  }
  assert_pixel(frame, (140, 80), [BACKGROUND; 3], &format!("{backend}: background"));
}

/// Layers inside layers: a red rect (outer layer at 0.5) partly covered by a
/// blue rect and a green rect over it (inner layer at 0.5), over light grey.
fn nested_scene() -> RenderList {
  let rects = vec![
    rect(0, [10.0, 10.0, 60.0, 40.0], Color::new(200, 0, 0, 255)),
    rect(1, [40.0, 20.0, 60.0, 20.0], Color::new(0, 0, 200, 255)),
    rect(2, [60.0, 25.0, 30.0, 10.0], Color::new(0, 160, 0, 255)),
  ];
  let layers = vec![layer(0, 3, 0.5, [8, 8, 94, 44]), layer(1, 3, 0.5, [38, 18, 64, 24])];
  list(Color::new(240, 240, 240, 255), rects, Vec::new(), layers)
}

fn assert_nested(frame: &[u8], backend: &str) {
  let case = |what: &str| format!("{backend}: {what}");
  // Outer layer only: red at 0.5.
  assert_pixel(frame, (20, 30), [220, 120, 120], &case("red"));
  // Inner layer over red: (blue 0.5 over red) at 0.5.
  assert_pixel(frame, (50, 30), [170, 120, 170], &case("blue over red"));
  // Green hides the blue inside the inner layer.
  assert_pixel(frame, (65, 30), [170, 160, 120], &case("green over red"));
  // Past the red: the inner layer's 0.5 and the outer 0.5 compose to 0.25.
  assert_pixel(frame, (80, 30), [180, 220, 180], &case("green alone"));
  assert_pixel(frame, (95, 30), [180, 180, 230], &case("blue alone"));
  assert_pixel(frame, (120, 30), [240, 240, 240], &case("background"));
}

/// Single primitives fade the same way in a layer as with their own alpha,
/// wherever they sit: fractional positions (as 1.25x and 1.5x scales
/// produce), a rounded clip, a rotation and a glyph. A layer is composited
/// texel for texel, so the two frames match.
fn single_primitive_scenes() -> (RenderList, RenderList) {
  let opacity = 0.4;
  let clip = ClipRect {
    x: 72.5,
    y: 8.75,
    width: 30.0,
    height: 30.0,
    active: true,
    border_radius: Some(BorderRadius::all(9.0)),
  };
  let color = Color::new(40, 110, 220, 255);
  let mut clipped = rect(1, [70.0, 5.0, 40.0, 40.0], color);
  clipped.clip = clip;
  let mut turned = rect(2, [15.0, 55.0, 50.0, 18.0], color);
  turned.transform = [0.866, 0.5, -0.5, 0.866];
  turned.radii = [4.0; 4];
  let mut rects = vec![rect(0, [10.4, 12.6, 25.3, 17.1], color), clipped, turned];
  let glyphs = vec![glyph(3, [100.6, 60.3, 21.7, 13.9], Color::new(250, 250, 250, 255))];
  let background = Color::new(30, 34, 40, 255);
  let layers = vec![
    layer(0, 1, opacity, [8, 10, 30, 22]),
    layer(1, 2, opacity, [72, 8, 31, 31]),
    layer(2, 3, opacity, [8, 36, 66, 58]),
    layer(3, 4, opacity, [98, 58, 26, 18]),
  ];
  let layered = list(background, rects.clone(), glyphs.clone(), layers);
  let alpha = (opacity * 255.0).round() as u8;
  for rect in &mut rects {
    rect.color = Color::new(rect.color.r(), rect.color.g(), rect.color.b(), alpha);
  }
  let mut glyphs = glyphs;
  for glyph in &mut glyphs {
    glyph.color[3] = opacity;
  }
  (layered, list(background, rects, glyphs, Vec::new()))
}

fn assert_layer_matches_alpha(layered: &[u8], direct: &[u8], backend: &str) {
  let mut worst = (0, (0, 0));
  for y in 0..HEIGHT {
    for x in 0..WIDTH {
      let (a, b) = (pixel(layered, x, y), pixel(direct, x, y));
      let diff = (0..3).map(|channel| a[channel].abs_diff(b[channel])).max().unwrap_or(0);
      if diff > worst.0 {
        worst = (diff, (x, y));
      }
    }
  }
  println!("{backend}: layered vs own alpha, worst {} at {:?}", worst.0, worst.1);
  // 8-bit layer storage rounds anti-aliased edge texels once more.
  assert!(
    worst.0 <= 2,
    "{backend}: layered differs by {} at {:?}",
    worst.0,
    worst.1
  );
}

/// Checks one engine per scene: the flattened button, nested layers, layers
/// matching single primitives faded with their own alpha, and that a frame
/// without layers creates no layer texture.
fn assert_group_opacity<E: RenderEngine>(new_engine: fn() -> E, backend: &str, texture_count: fn(&E) -> usize) {
  assert_flattened_button(&render(&mut new_engine(), &button_scene()), backend);

  let mut engine = new_engine();
  assert_nested(&render(&mut engine, &nested_scene()), backend);
  assert_eq!(texture_count(&engine), 2, "{backend}: one texture per nested layer");

  let (layered, direct) = single_primitive_scenes();
  let layered = render(&mut new_engine(), &layered);
  let direct = render(&mut new_engine(), &direct);
  assert_layer_matches_alpha(&layered, &direct, backend);

  let mut plain = button_scene();
  plain.layers.clear();
  let mut engine = new_engine();
  render(&mut engine, &plain);
  assert_eq!(
    texture_count(&engine),
    0,
    "{backend}: a frame without layers creates none"
  );
}

#[cfg(feature = "wgpu")]
#[test]
#[ignore = "requires a Windows desktop and a GPU adapter; creates a hidden window"]
fn wgpu_group_opacity_flattens_the_subtree() {
  use crate::app::wgpu_render::WgpuRenderEngine;
  assert_group_opacity(WgpuRenderEngine::new, "wgpu", WgpuRenderEngine::layer_texture_count);
}

/// Consecutive frames of one window reuse the pooled layer textures. Only
/// wgpu: a hidden window's DXGI flip swapchain does not free its frame
/// latency slot for a second frame, so DX12 renders one frame per window.
#[cfg(feature = "wgpu")]
#[test]
#[ignore = "requires a Windows desktop and a GPU adapter; creates a hidden window"]
fn wgpu_layer_textures_are_pooled_across_frames() {
  let mut engine = crate::app::wgpu_render::WgpuRenderEngine::new();
  let (button, nested) = (button_scene(), nested_scene());
  let frames = readback_window::capture_each(&mut engine, &[&nested, &button, &nested, &button], WIDTH, HEIGHT);
  assert_nested(&frames[2].rgba, "wgpu");
  assert_flattened_button(&frames[3].rgba, "wgpu");
  assert_eq!(
    engine.layer_texture_count(),
    2,
    "the button reuses a nested layer's texture"
  );
}

#[cfg(feature = "dx12")]
#[test]
#[ignore = "requires a Windows desktop and a DX12 adapter; creates a hidden window"]
fn dx12_group_opacity_flattens_the_subtree() {
  use crate::app::dx12_render::Dx12RenderEngine;
  assert_group_opacity(Dx12RenderEngine::new, "dx12", Dx12RenderEngine::layer_texture_count);
}

#[cfg(all(feature = "wgpu", feature = "dx12"))]
#[test]
#[ignore = "requires a Windows desktop, a GPU adapter and DX12; creates hidden windows"]
fn wgpu_and_dx12_group_opacity_match_each_other() {
  for (scene, list) in [
    ("button", button_scene()),
    ("nested", nested_scene()),
    ("single", single_primitive_scenes().0),
  ] {
    let a = render(&mut crate::app::wgpu_render::WgpuRenderEngine::new(), &list);
    let b = render(&mut crate::app::dx12_render::Dx12RenderEngine::new(), &list);
    let worst = a
      .chunks_exact(4)
      .zip(b.chunks_exact(4))
      .map(|(a, b)| (0..3).map(|channel| a[channel].abs_diff(b[channel])).max().unwrap_or(0))
      .max()
      .unwrap_or(0);
    assert!(worst <= 2, "{scene}: wgpu and dx12 differ by {worst}");
  }
}
