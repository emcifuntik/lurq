// Layer blur through the real render engines, read back from a hidden window
// of the test's own. A blurred subtree must look like CSS `filter: blur()`:
// painted as one layer, then convolved with a Gaussian, including by what
// lies just outside the window.
//
// cargo test -p lurq --features wgpu,dx12,screenshot --lib layer_blur_readback -- --ignored --test-threads=1

use std::sync::Arc;

use super::readback_window;
use crate::{
  app::render_engine::RenderEngine,
  layout::{
    opacity_layer::{LayerCmd, OpacityGroup, resolve_layers},
    quad::ClipRect,
    render_list::{GlyphAtlas, RectCmd, RenderList},
  },
  node::color::Color,
};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 96;
const BACKGROUND: u8 = 30;
const FILL: u8 = 250;
/// The blurred box: x, y, width, height.
const BOX: [f32; 4] = [60.0, 28.0, 40.0, 40.0];

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
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [width * 0.5, height * 0.5],
    clip: ClipRect::default(),
    gradient: None,
    shadow: None,
  }
}

fn group(start: usize, end: usize, opacity: f32, blur: f32) -> OpacityGroup {
  OpacityGroup {
    start,
    end,
    opacity,
    blur,
    clip: Some([0.0, 0.0, WIDTH as f32, HEIGHT as f32]),
    clip_radii: [0.0; 4],
  }
}

/// A frame of `rects` (one per order) grouped by `groups`, with the layers
/// the runtime resolves for them.
fn list(rects: Vec<RectCmd>, groups: &[OpacityGroup]) -> RenderList {
  let mut layers = Vec::new();
  resolve_layers(
    groups,
    rects.len(),
    [WIDTH as f32, HEIGHT as f32],
    1.0,
    &rects,
    &[],
    #[cfg(feature = "raster")]
    &[],
    #[cfg(feature = "svg")]
    &[],
    &mut layers,
  );
  RenderList {
    clear_color: Color::new(BACKGROUND, BACKGROUND, BACKGROUND, 255),
    rects,
    glyphs: Vec::new(),
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

fn boxed(opacity: f32, blur: f32) -> RenderList {
  list(
    vec![rect(0, BOX, Color::new(FILL, FILL, FILL, 255))],
    &[group(0, 1, opacity, blur)],
  )
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

/// How much of a Gaussian of `sigma` centred on pixel `at` falls on the
/// pixels `start..end`: the box's coverage along one axis after the blur.
fn coverage(at: i32, start: i32, end: i32, sigma: f32) -> f32 {
  let reach = (sigma * 3.0).ceil() as i32 + 1;
  let weight = |offset: i32| (-(offset * offset) as f32 / (2.0 * sigma * sigma)).exp();
  let total: f32 = (-reach..=reach).map(weight).sum();
  let inside: f32 = (-reach..=reach)
    .filter(|offset| (start..end).contains(&(at + offset)))
    .map(weight)
    .sum();
  inside / total
}

/// The frame `boxed(opacity, 2 * sigma)` should give, one channel per pixel.
fn expected_box(sigma: f32, opacity: f32) -> Vec<f32> {
  let [x, y, width, height] = BOX.map(|value| value as i32);
  let mut frame = Vec::with_capacity((WIDTH * HEIGHT) as usize);
  for py in 0..HEIGHT as i32 {
    for px in 0..WIDTH as i32 {
      let alpha = coverage(px, x, x + width, sigma) * coverage(py, y, y + height, sigma) * opacity;
      frame.push(f32::from(FILL) * alpha + f32::from(BACKGROUND) * (1.0 - alpha));
    }
  }
  frame
}

/// Thin stripes across the box: 3 pixel rows of fill every 7, so the rows
/// of a reduced copy of the layer differ sharply from one to the next.
const STRIPES: i32 = 6;

fn striped(blur: f32) -> RenderList {
  let [x, y, width, _] = BOX;
  let rects = (0..STRIPES)
    .map(|stripe| {
      rect(
        stripe as usize,
        [x, y + 7.0 * stripe as f32, width, 3.0],
        Color::new(FILL, FILL, FILL, 255),
      )
    })
    .collect();
  list(rects, &[group(0, STRIPES as usize, 1.0, blur)])
}

/// The frame `striped(2 * sigma)` should give, one channel per pixel.
fn expected_stripes(sigma: f32) -> Vec<f32> {
  let [x, y, width, _] = BOX.map(|value| value as i32);
  let mut frame = Vec::with_capacity((WIDTH * HEIGHT) as usize);
  for py in 0..HEIGHT as i32 {
    let rows: f32 = (0..STRIPES)
      .map(|stripe| coverage(py, y + 7 * stripe, y + 7 * stripe + 3, sigma))
      .sum();
    for px in 0..WIDTH as i32 {
      let alpha = coverage(px, x, x + width, sigma) * rows;
      frame.push(f32::from(FILL) * alpha + f32::from(BACKGROUND) * (1.0 - alpha));
    }
  }
  frame
}

fn assert_matches(frame: &[u8], expected: &[f32], tolerance: f32, what: &str) {
  let mut worst = (0.0_f32, (0, 0));
  for y in 0..HEIGHT {
    for x in 0..WIDTH {
      let want = expected[(y * WIDTH + x) as usize];
      for channel in pixel(frame, x, y) {
        let diff = (f32::from(channel) - want).abs();
        if diff > worst.0 {
          worst = (diff, (x, y));
        }
      }
    }
  }
  println!("{what}: worst {:.2} at {:?}", worst.0, worst.1);
  assert!(
    worst.0 <= tolerance,
    "{what}: off by {:.2} at {:?}: {:?}, expected {:.1}",
    worst.0,
    worst.1,
    pixel(frame, worst.1.0, worst.1.1),
    expected[(worst.1.1 * WIDTH + worst.1.0) as usize]
  );
}

fn layers_of(list: &RenderList) -> &[LayerCmd] {
  &list.layers
}

/// Checks one engine: a narrow blur and a wide one (run on a reduced copy)
/// against the Gaussian, the blur with an opacity, a blurred layer inside a
/// faded one, and content that runs past the window on every side.
fn assert_layer_blur<E: RenderEngine>(new_engine: fn() -> E, backend: &str) {
  // Sigma 4: both passes read the layer as painted.
  let narrow = boxed(1.0, 8.0);
  let blur = layers_of(&narrow)[0].blur.expect("a blurred layer");
  assert_eq!(blur.sigma, 4.0);
  let frame = render(&mut new_engine(), &narrow);
  assert_matches(&frame, &expected_box(4.0, 1.0), 2.5, &format!("{backend}: sigma 4"));
  // The box's middle keeps its colour, its edge is half way to the
  // background, and the blur is gone three deviations out.
  let middle = pixel(&frame, 80, 48)[0];
  assert!(middle >= FILL - 2, "{backend}: middle is {middle}");
  let outside = pixel(&frame, 60 - 14, 48)[0];
  assert!(outside <= BACKGROUND + 1, "{backend}: 14 px out is {outside}");

  // Sigma 20: reduced by four, blurred, and scaled back up.
  let frame = render(&mut new_engine(), &boxed(1.0, 40.0));
  assert_matches(&frame, &expected_box(20.0, 1.0), 2.5, &format!("{backend}: sigma 20"));

  // Detail finer than the reduction leaves no stripes in a wide blur: at
  // sigma 9 the layer is halved, at sigma 20 quartered, and the pass that
  // scales it back up reads between the reduced rows.
  for sigma in [9.0, 12.0, 20.0] {
    let frame = render(&mut new_engine(), &striped(sigma * 2.0));
    assert_matches(
      &frame,
      &expected_stripes(sigma),
      2.5,
      &format!("{backend}: stripes at sigma {sigma}"),
    );
  }

  // A rounded clip the node is under cuts the blurred layer, corners
  // included: the clip here lies inside the box, so without its corners
  // every pixel of it would be the box's colour.
  let mut clipped = group(0, 1, 1.0, 8.0);
  clipped.clip = Some([64.0, 32.0, 32.0, 32.0]);
  clipped.clip_radii = [12.0; 4];
  let frame = render(
    &mut new_engine(),
    &list(vec![rect(0, BOX, Color::new(FILL, FILL, FILL, 255))], &[clipped]),
  );
  for (x, y) in [
    (64, 32),
    (65, 33),
    (95, 32),
    (64, 63),
    (95, 63),
    (63, 48),
    (96, 48),
    (80, 31),
  ] {
    assert_eq!(
      pixel(&frame, x, y),
      [BACKGROUND; 3],
      "{backend}: ({x}, {y}) outside the rounded clip"
    );
  }
  // Inside it the layer is the blurred box, as without a clip.
  let unclipped = expected_box(4.0, 1.0);
  for (x, y) in [(80, 48), (64, 48), (80, 32), (68, 36), (91, 59)] {
    let (value, want) = (f32::from(pixel(&frame, x, y)[0]), unclipped[(y * WIDTH + x) as usize]);
    assert!(
      (value - want).abs() <= 2.5,
      "{backend}: ({x}, {y}) inside the rounded clip is {value}, expected {want}"
    );
  }

  // The layer's opacity applies to the blurred result.
  let frame = render(&mut new_engine(), &boxed(0.5, 8.0));
  assert_matches(
    &frame,
    &expected_box(4.0, 0.5),
    2.5,
    &format!("{backend}: sigma 4 at 0.5"),
  );

  // A blurred layer inside a faded one: the same pixels, composited through
  // the outer layer's texture instead of the window.
  let mut filler = rect(
    0,
    [4.0, 4.0, 6.0, 6.0],
    Color::new(BACKGROUND, BACKGROUND, BACKGROUND, 255),
  );
  filler.order = 0;
  let nested = list(
    vec![filler, rect(1, BOX, Color::new(FILL, FILL, FILL, 255))],
    &[group(1, 2, 1.0, 8.0), group(0, 2, 0.5, 0.0)],
  );
  assert_eq!(layers_of(&nested).len(), 2);
  let frame = render(&mut new_engine(), &nested);
  assert_matches(
    &frame,
    &expected_box(4.0, 0.5),
    2.5,
    &format!("{backend}: nested in a faded layer"),
  );

  // Content past every edge of the window: what lies outside bleeds in, so
  // the window's edges keep the colour instead of fading to nothing.
  for blur in [12.0, 64.0] {
    let covering = list(
      vec![rect(
        0,
        [-300.0, -300.0, WIDTH as f32 + 600.0, HEIGHT as f32 + 600.0],
        Color::new(200, 90, 40, 255),
      )],
      &[group(0, 1, 1.0, blur)],
    );
    let painted = layers_of(&covering)[0].blur.expect("a blurred layer");
    assert!(painted.x < 0 && painted.y < 0, "{backend}: painted past the window");
    let frame = render(&mut new_engine(), &covering);
    for (x, y) in [
      (0, 0),
      (WIDTH - 1, 0),
      (0, HEIGHT - 1),
      (WIDTH - 1, HEIGHT - 1),
      (80, 48),
    ] {
      let actual = pixel(&frame, x, y);
      assert!(
        actual.iter().zip([200, 90, 40]).all(|(a, e)| a.abs_diff(e) <= 2),
        "{backend}: blur {blur} at ({x}, {y}) is {actual:?}"
      );
    }
  }
}

#[cfg(feature = "wgpu")]
#[test]
#[ignore = "requires a Windows desktop and a GPU adapter; creates a hidden window"]
fn wgpu_layer_blur_is_a_gaussian_of_the_layer() {
  assert_layer_blur(crate::app::wgpu_render::WgpuRenderEngine::new, "wgpu");
}

#[cfg(feature = "dx12")]
#[test]
#[ignore = "requires a Windows desktop and a DX12 adapter; creates a hidden window"]
fn dx12_layer_blur_is_a_gaussian_of_the_layer() {
  assert_layer_blur(crate::app::dx12_render::Dx12RenderEngine::new, "dx12");
}

#[cfg(all(feature = "wgpu", feature = "dx12"))]
#[test]
#[ignore = "requires a Windows desktop, a GPU adapter and DX12; creates hidden windows"]
fn wgpu_and_dx12_layer_blur_match_each_other() {
  for (scene, list) in [("narrow", boxed(0.7, 8.0)), ("wide", boxed(1.0, 40.0))] {
    let a = render(&mut crate::app::wgpu_render::WgpuRenderEngine::new(), &list);
    let b = render(&mut crate::app::dx12_render::Dx12RenderEngine::new(), &list);
    let worst = a
      .chunks_exact(4)
      .zip(b.chunks_exact(4))
      .map(|(a, b)| (0..3).map(|channel| a[channel].abs_diff(b[channel])).max().unwrap_or(0))
      .max()
      .unwrap_or(0);
    println!("{scene}: wgpu vs dx12, worst {worst}");
    assert!(worst <= 2, "{scene}: wgpu and dx12 differ by {worst}");
  }
}
