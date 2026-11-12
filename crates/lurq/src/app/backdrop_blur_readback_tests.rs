// Backdrop blur through the real render engines, read back from a hidden
// window of the test's own. A node with a backdrop blur must look like CSS
// `backdrop-filter: blur()`: what was painted behind its box is convolved
// with a Gaussian inside the box, and nothing else changes.
//
// cargo test -p lurq --features wgpu,dx12,screenshot --lib backdrop_blur_readback -- --ignored --test-threads=1

use std::sync::Arc;

use super::readback_window;
use crate::{
  app::render_engine::RenderEngine,
  layout::{
    backdrop::BackdropCmd,
    opacity_layer::{LayerBounds, LayerCmd},
    quad::ClipRect,
    render_list::{GlyphAtlas, RectCmd, RenderList},
  },
  node::color::Color,
};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 96;
const DARK: u8 = 30;
const LIGHT: u8 = 250;
/// The window is dark left of this column and light from it on.
const EDGE: i32 = 80;
/// The box whose backdrop is blurred: x, y, width, height.
const BOX: [f32; 4] = [40.0, 20.0, 80.0, 56.0];

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
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [width * 0.5, height * 0.5],
    clip: ClipRect::default(),
    gradient: None,
    shadow: None,
  }
}

fn backdrop(order: usize, [x, y, width, height]: [f32; 4], sigma: f32) -> BackdropCmd {
  BackdropCmd {
    order,
    x,
    y,
    width,
    height,
    radii: [0.0; 4],
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [width * 0.5, height * 0.5],
    clip: ClipRect::default(),
    sigma,
    opacity: 1.0,
  }
}

fn list(rects: Vec<RectCmd>, layers: Vec<LayerCmd>, backdrops: Vec<BackdropCmd>) -> RenderList {
  RenderList {
    clear_color: Color::new(DARK, DARK, DARK, 255),
    rects,
    glyphs: Vec::new(),
    #[cfg(feature = "raster")]
    images: Vec::new(),
    #[cfg(feature = "svg")]
    svgs: Vec::new(),
    layers,
    backdrops,
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

/// The light half of the window, at order 0. It runs past the window's
/// edges, so none of its own anti-aliased edges but the one between the
/// halves is in the frame.
fn light_half() -> RectCmd {
  rect(
    0,
    [EDGE as f32, -16.0, WIDTH as f32, HEIGHT as f32 + 32.0],
    Color::new(LIGHT, LIGHT, LIGHT, 255),
  )
}

/// The dark and light halves with `blur` over them at order 1.
fn split(blur: BackdropCmd) -> RenderList {
  list(vec![light_half()], Vec::new(), vec![blur])
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

/// The window's own colour at a column.
fn sharp(x: i32) -> f32 {
  f32::from(if x >= EDGE { LIGHT } else { DARK })
}

/// The colour of a column once the edge between the halves is blurred by
/// `sigma`. The light half runs on past the window's right edge, as the
/// window's edge pixels do for a blur.
fn soft(x: i32, sigma: f32) -> f32 {
  let reach = (sigma * 3.0).ceil() as i32 + 1;
  let weight = |offset: i32| (-(offset * offset) as f32 / (2.0 * sigma * sigma)).exp();
  let total: f32 = (-reach..=reach).map(weight).sum();
  let light: f32 = (-reach..=reach).filter(|offset| x + offset >= EDGE).map(weight).sum();
  f32::from(DARK) + f32::from(LIGHT - DARK) * light / total
}

/// Checks every pixel against `expected(x, y)`; a NaN there leaves the
/// pixel unchecked.
fn assert_frame(frame: &[u8], expected: impl Fn(i32, i32) -> f32, tolerance: f32, what: &str) {
  let mut worst = (0.0_f32, (0, 0), 0.0);
  for y in 0..HEIGHT {
    for x in 0..WIDTH {
      let want = expected(x as i32, y as i32);
      if want.is_nan() {
        continue;
      }
      for channel in pixel(frame, x, y) {
        let diff = (f32::from(channel) - want).abs();
        if diff > worst.0 {
          worst = (diff, (x, y), want);
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
    worst.2
  );
}

fn in_box(x: i32, y: i32, [left, top, width, height]: [f32; 4]) -> bool {
  let (left, top) = (left as i32, top as i32);
  (left..left + width as i32).contains(&x) && (top..top + height as i32).contains(&y)
}

/// Checks one engine: a narrow and a wide blur of the backdrop inside the
/// box and nowhere else, the blur's opacity, rounded corners, draws after
/// the blur, a blur that fills the window, and a blur inside a layer.
fn assert_backdrop_blur<E: RenderEngine>(new_engine: fn() -> E, backend: &str) {
  for sigma in [4.0, 12.0, 20.0] {
    let frame = render(&mut new_engine(), &split(backdrop(1, BOX, sigma)));
    assert_frame(
      &frame,
      |x, y| if in_box(x, y, BOX) { soft(x, sigma) } else { sharp(x) },
      2.5,
      &format!("{backend}: sigma {sigma}"),
    );
  }

  // At half opacity the blurred backdrop is mixed half and half with the
  // sharp one.
  let mut half = backdrop(1, BOX, 4.0);
  half.opacity = 0.5;
  let frame = render(&mut new_engine(), &split(half));
  assert_frame(
    &frame,
    |x, y| {
      if in_box(x, y, BOX) {
        (soft(x, 4.0) + sharp(x)) * 0.5
      } else {
        sharp(x)
      }
    },
    2.5,
    &format!("{backend}: half opacity"),
  );

  // Rounded corners: outside the corner's arc the backdrop stays sharp. The
  // box is moved so its corners sit on the edge between the halves.
  let rounded_box = [EDGE as f32 - 4.0, 20.0, 60.0, 56.0];
  let mut rounded = backdrop(1, rounded_box, 4.0);
  rounded.radii = [16.0; 4];
  let frame = render(&mut new_engine(), &split(rounded));
  for (x, y) in [(EDGE - 4, 20), (EDGE - 3, 21), (EDGE - 4, 75), (EDGE - 2, 73)] {
    let value = f32::from(pixel(&frame, x as u32, y as u32)[0]);
    assert!(
      (value - sharp(x)).abs() <= 1.0,
      "{backend}: ({x}, {y}) outside the rounded corner is {value}"
    );
  }
  // Along the box's left side, between the corners, it is blurred.
  let value = f32::from(pixel(&frame, (EDGE - 4) as u32, 48)[0]);
  assert!(
    (value - soft(EDGE - 4, 4.0)).abs() <= 2.5 && value > sharp(EDGE - 4) + 20.0,
    "{backend}: the rounded box's side is {value}"
  );

  // What is painted after the blur is painted over it, sharp.
  let over = [70.0, 30.0, 20.0, 20.0];
  let frame = render(
    &mut new_engine(),
    &list(
      vec![light_half(), rect(2, over, Color::new(200, 40, 10, 255))],
      Vec::new(),
      vec![backdrop(1, BOX, 4.0)],
    ),
  );
  for (x, y) in [(72, 32), (80, 40), (87, 47)] {
    assert_eq!(pixel(&frame, x, y), [200, 40, 10], "{backend}: a draw after the blur");
  }
  let beside = f32::from(pixel(&frame, 78, 60)[0]);
  assert!((beside - soft(78, 4.0)).abs() <= 2.5, "{backend}: beside it {beside}");

  // A blur that fills the window keeps its edges: the window's edge pixels
  // repeat past it, instead of fading to nothing.
  for sigma in [6.0, 24.0] {
    let whole = [0.0, 0.0, WIDTH as f32, HEIGHT as f32];
    let frame = render(&mut new_engine(), &split(backdrop(1, whole, sigma)));
    assert_frame(
      &frame,
      |x, _| soft(x, sigma),
      2.5,
      &format!("{backend}: the whole window at sigma {sigma}"),
    );
  }

  // Inside a faded layer the blur reads the layer, not the window: here the
  // layer holds nothing before the blur, so nothing is blurred, and the
  // layer's one rect is composited over the sharp halves.
  let inner = [100.0, 30.0, 20.0, 20.0];
  let frame = render(
    &mut new_engine(),
    &list(
      vec![light_half(), rect(2, inner, Color::new(DARK, DARK, DARK, 255))],
      vec![LayerCmd {
        start_order: 1,
        end_order: 3,
        opacity: 0.5,
        bounds: LayerBounds {
          x: 38,
          y: 18,
          width: 84,
          height: 60,
        },
        blur: None,
      }],
      vec![backdrop(1, BOX, 4.0)],
    ),
  );
  assert_frame(
    &frame,
    |x, y| {
      let [left, top, width, height] = inner;
      if in_box(x, y, [left + 1.0, top + 1.0, width - 2.0, height - 2.0]) {
        (sharp(x) + f32::from(DARK)) * 0.5
      } else if in_box(x, y, [left - 1.0, top - 1.0, width + 2.0, height + 2.0]) {
        // The rect's own anti-aliased edge, a pixel to either side.
        f32::NAN
      } else {
        sharp(x)
      }
    },
    1.5,
    &format!("{backend}: in an empty layer"),
  );

  // With the light half inside the layer, the blur softens it there. The
  // layer is composited at full strength here, so the result is the blur.
  let mut inside = light_half();
  inside.order = 1;
  let frame = render(
    &mut new_engine(),
    &list(
      vec![inside],
      vec![LayerCmd {
        start_order: 1,
        end_order: 3,
        opacity: 1.0,
        bounds: LayerBounds {
          x: 0,
          y: 0,
          width: WIDTH,
          height: HEIGHT,
        },
        blur: None,
      }],
      vec![backdrop(2, BOX, 4.0)],
    ),
  );
  // Left of the edge the layer is transparent, so the blurred light half
  // is premultiplied coverage over the window's dark clear colour: the same
  // numbers as blurring the two halves.
  assert_frame(
    &frame,
    |x, y| if in_box(x, y, BOX) { soft(x, 4.0) } else { sharp(x) },
    2.5,
    &format!("{backend}: the layer's own content"),
  );
}

#[cfg(feature = "wgpu")]
#[test]
#[ignore = "requires a Windows desktop and a GPU adapter; creates a hidden window"]
fn wgpu_backdrop_blur_is_a_gaussian_of_what_is_behind() {
  assert_backdrop_blur(crate::app::wgpu_render::WgpuRenderEngine::new, "wgpu");
}

#[cfg(feature = "dx12")]
#[test]
#[ignore = "requires a Windows desktop and a DX12 adapter; creates a hidden window"]
fn dx12_backdrop_blur_is_a_gaussian_of_what_is_behind() {
  assert_backdrop_blur(crate::app::dx12_render::Dx12RenderEngine::new, "dx12");
}

#[cfg(all(feature = "wgpu", feature = "dx12"))]
#[test]
#[ignore = "requires a Windows desktop, a GPU adapter and DX12; creates hidden windows"]
fn wgpu_and_dx12_backdrop_blur_match_each_other() {
  let mut rounded = backdrop(1, BOX, 12.0);
  rounded.radii = [10.0, 0.0, 20.0, 4.0];
  rounded.opacity = 0.8;
  for (scene, list) in [("narrow", split(backdrop(1, BOX, 4.0))), ("rounded", split(rounded))] {
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
