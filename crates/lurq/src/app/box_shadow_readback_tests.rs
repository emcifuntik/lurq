// Box shadows through the real render engines, read back from a hidden window
// of the test's own and compared pixel by pixel with the CPU formula in
// `layout::box_shadow` (which a brute-force Gaussian convolution checks in
// `tests/layout/box_shadow_math.rs`), and with each other.
//
// cargo test -p lurq --features wgpu,dx12,screenshot --lib box_shadow_readback -- --ignored --test-threads=1

use std::sync::Arc;

use super::readback_window;
use crate::{
  app::render_engine::RenderEngine,
  layout::{
    box_shadow::{rect_shadow_coverage, rounded_rect_coverage, spread_radius},
    quad::ClipRect,
    render_list::{GlyphAtlas, RectCmd, RectShadow, RenderList},
  },
  node::color::Color,
};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 160;
const CLEAR: Color = Color::new(0xf0, 0xf0, 0xf0, 0xff);

fn fill(x: f32, y: f32, width: f32, height: f32, radius: f32, color: Color) -> RectCmd {
  RectCmd {
    order: 0,
    x,
    y,
    width,
    height,
    color,
    radii: [radius; 4],
    stroke: [0.0; 4],
    stroke_color: Color::new(0, 0, 0, 0),
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [width * 0.5, height * 0.5],
    clip: ClipRect::default(),
    gradient: None,
    shadow: None,
  }
}

#[allow(clippy::too_many_arguments)]
fn shadow(
  element: &RectCmd,
  offset: [f32; 2],
  spread: f32,
  sigma: f32,
  inset: bool,
  color: Color,
  clip: ClipRect,
) -> RectCmd {
  let delta = if inset { -spread } else { spread };
  RectCmd {
    color,
    clip,
    shadow: Some(RectShadow {
      inset,
      offset,
      spread,
      sigma,
      shape_radii: element.radii.map(|radius| spread_radius(radius, delta)),
    }),
    ..element.clone()
  }
}

/// Outer blurred, spread and offset; a hard shadow; an inset shadow; a shadow
/// clipped by its container; and a translucent element whose shadow must not
/// show through it. Ordered as layout emits them: shadows, then the element.
fn scene() -> Vec<RectCmd> {
  let none = ClipRect::default();
  let card = fill(24.0, 24.0, 64.0, 40.0, 8.0, Color::new(255, 255, 255, 255));
  let chip = fill(120.0, 24.0, 48.0, 32.0, 0.0, Color::new(250, 250, 250, 255));
  let well = fill(24.0, 96.0, 80.0, 40.0, 12.0, Color::new(255, 255, 255, 255));
  let clipped = fill(150.0, 90.0, 40.0, 40.0, 6.0, Color::new(255, 255, 255, 255));
  let glass = fill(200.0, 20.0, 40.0, 40.0, 10.0, Color::new(255, 255, 255, 64));
  let clip = ClipRect {
    x: 140.0,
    y: 80.0,
    width: 56.0,
    height: 60.0,
    active: true,
    border_radius: None,
  };
  vec![
    shadow(&card, [4.0, 6.0], 2.0, 6.0, false, Color::new(0, 0, 0, 128), none),
    card,
    shadow(&chip, [6.0, 6.0], 0.0, 0.0, false, Color::new(32, 64, 192, 200), none),
    chip,
    well.clone(),
    shadow(&well, [0.0, 4.0], 2.0, 5.0, true, Color::new(0, 0, 0, 153), none),
    shadow(&clipped, [0.0, 0.0], 0.0, 8.0, false, Color::new(200, 0, 0, 180), clip),
    RectCmd { clip, ..clipped },
    shadow(&glass, [0.0, 8.0], 0.0, 4.0, false, Color::new(0, 0, 0, 200), none),
    glass,
  ]
  .into_iter()
  .enumerate()
  .map(|(order, rect)| RectCmd { order, ..rect })
  .collect()
}

fn render_list(rects: Vec<RectCmd>) -> RenderList {
  RenderList {
    clear_color: CLEAR,
    rects,
    glyphs: Vec::new(),
    #[cfg(feature = "raster")]
    images: Vec::new(),
    #[cfg(feature = "svg")]
    svgs: Vec::new(),
    layers: Vec::new(),
    backdrops: Vec::new(),
    atlas: GlyphAtlas {
      data: Arc::from([0_u8; 4 * 4 * 4].as_slice()),
      width: 4,
      height: 4,
      version: 1,
      dirty_rects: Arc::from(Vec::new()),
      dirty_from_version: 0,
    },
  }
}

fn clip_contains(clip: ClipRect, x: f32, y: f32) -> bool {
  !clip.active || (x >= clip.x && x < clip.x + clip.width && y >= clip.y && y < clip.y + clip.height)
}

/// The scene composited on the CPU: CSS source-over on sRGB channels.
fn reference(rects: &[RectCmd]) -> Vec<[f32; 3]> {
  let mut pixels = vec![[f32::from(CLEAR.r()), f32::from(CLEAR.g()), f32::from(CLEAR.b())]; (WIDTH * HEIGHT) as usize];
  for rect in rects {
    for py in 0..HEIGHT {
      for px in 0..WIDTH {
        let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
        if !clip_contains(rect.clip, x, y) {
          continue;
        }
        let coverage = match &rect.shadow {
          Some(shadow) => rect_shadow_coverage(rect, shadow, x, y),
          None => {
            let half = [rect.width * 0.5, rect.height * 0.5];
            rounded_rect_coverage(x - rect.x - half[0], y - rect.y - half[1], half, rect.radii)
          }
        };
        let alpha = coverage * f32::from(rect.color.a()) / 255.0;
        let pixel = &mut pixels[(py * WIDTH + px) as usize];
        for (channel, source) in pixel.iter_mut().zip([rect.color.r(), rect.color.g(), rect.color.b()]) {
          *channel = f32::from(source) * alpha + *channel * (1.0 - alpha);
        }
      }
    }
  }
  pixels
}

struct Difference {
  worst: f32,
  over_two: usize,
  over_eight: usize,
  at: (u32, u32),
}

fn difference(frame: &[u8], expected: impl Fn(usize) -> [f32; 3]) -> Difference {
  let mut result = Difference {
    worst: 0.0,
    over_two: 0,
    over_eight: 0,
    at: (0, 0),
  };
  for index in 0..(WIDTH * HEIGHT) as usize {
    let expected = expected(index);
    let diff = (0..3)
      .map(|channel| (f32::from(frame[index * 4 + channel]) - expected[channel]).abs())
      .fold(0.0, f32::max);
    if diff > 2.0 {
      result.over_two += 1;
    }
    if diff > 8.0 {
      result.over_eight += 1;
    }
    if diff > result.worst {
      result.worst = diff;
      result.at = (index as u32 % WIDTH, index as u32 / WIDTH);
    }
  }
  result
}

fn render(engine: &mut dyn RenderEngine) -> Vec<u8> {
  let frame = readback_window::capture(engine, &render_list(scene()), WIDTH, HEIGHT);
  assert_eq!((frame.width, frame.height), (WIDTH, HEIGHT));
  frame.rgba
}

fn assert_matches_reference(frame: &[u8], backend: &str) {
  let expected = reference(&scene());
  let diff = difference(frame, |index| expected[index]);
  println!(
    "{backend}: worst {} at {:?}; of {} pixels {} differ by more than 2 levels, {} by more than 8",
    diff.worst,
    diff.at,
    WIDTH * HEIGHT,
    diff.over_two,
    diff.over_eight,
  );
  // The shaders and the reference share the shadow formula. What differs is
  // edge anti-aliasing: the shaders scale their ramp by `fwidth` over 2x2
  // pixel quads, which the reference cannot reproduce, so anti-aliased corners
  // (of fills as much as of shadows) differ by a few levels, and the corners
  // of hard, square shapes by up to about 25. Measured: 73 pixels over 2
  // levels, 5 over 8, worst 24, on both backends.
  assert!(
    diff.over_two <= 100,
    "{backend}: {} pixels differ by more than 2",
    diff.over_two
  );
  assert!(
    diff.over_eight <= 8,
    "{backend}: {} pixels differ by more than 8",
    diff.over_eight
  );
  assert!(
    diff.worst <= 32.0,
    "{backend}: worst difference {} at {:?}",
    diff.worst,
    diff.at
  );
}

fn pixel(frame: &[u8], x: u32, y: u32) -> [u8; 3] {
  let index = ((y * WIDTH + x) * 4) as usize;
  [frame[index], frame[index + 1], frame[index + 2]]
}

/// Spot checks that do not depend on the shared formula.
fn assert_shadow_semantics(frame: &[u8], backend: &str) {
  // Far from every shape: the clear colour.
  assert_eq!(pixel(frame, 110, 150), [0xf0; 3], "{backend}: background");
  // The opaque card covers its own shadow.
  assert_eq!(pixel(frame, 56, 44), [255; 3], "{backend}: card interior");
  // Below the card, inside its blurred shadow: darker than the background.
  assert!(
    pixel(frame, 56, 70)[0] < 0xd8,
    "{backend}: card shadow {:?}",
    pixel(frame, 56, 70)
  );
  // The hard shadow's exposed corner is its own colour over the background.
  let hard = pixel(frame, 171, 59);
  let expected = [32, 64, 192].map(|c: u8| ((f32::from(c) * 200.0 + 240.0 * 55.0) / 255.0).round() as u8);
  assert!(
    hard.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 1),
    "{backend}: hard shadow {hard:?}, expected {expected:?}"
  );
  // The inset shadow darkens the top of the well (offset down), not its middle.
  assert!(
    pixel(frame, 64, 98)[0] < 200,
    "{backend}: inset edge {:?}",
    pixel(frame, 64, 98)
  );
  assert!(
    pixel(frame, 64, 120)[0] > 250,
    "{backend}: inset centre {:?}",
    pixel(frame, 64, 120)
  );
  // The clipped shadow stops at its clip.
  assert_eq!(pixel(frame, 198, 110), [0xf0; 3], "{backend}: clipped shadow leak");
  assert!(
    pixel(frame, 194, 110)[1] < 0xe0,
    "{backend}: clipped shadow inside the clip"
  );
  // Knockout: the translucent element shows the background, not its shadow.
  let glass = pixel(frame, 220, 40);
  let expected = ((255.0 * 64.0 + 240.0 * 191.0) / 255.0_f32).round() as u8;
  assert!(
    glass.iter().all(|c| c.abs_diff(expected) <= 1),
    "{backend}: glass {glass:?}, expected {expected}"
  );
}

#[cfg(feature = "wgpu")]
#[test]
#[ignore = "requires a Windows desktop and a GPU adapter; creates a hidden window"]
fn wgpu_box_shadows_match_the_reference() {
  let frame = render(&mut crate::app::wgpu_render::WgpuRenderEngine::new());
  assert_shadow_semantics(&frame, "wgpu");
  assert_matches_reference(&frame, "wgpu");
}

#[cfg(feature = "dx12")]
#[test]
#[ignore = "requires a Windows desktop and a DX12 adapter; creates a hidden window"]
fn dx12_box_shadows_match_the_reference() {
  let frame = render(&mut crate::app::dx12_render::Dx12RenderEngine::new());
  assert_shadow_semantics(&frame, "dx12");
  assert_matches_reference(&frame, "dx12");
}

#[cfg(all(feature = "wgpu", feature = "dx12"))]
#[test]
#[ignore = "requires a Windows desktop, a GPU adapter and DX12; creates hidden windows"]
fn wgpu_and_dx12_box_shadows_match_each_other() {
  let wgpu = render(&mut crate::app::wgpu_render::WgpuRenderEngine::new());
  let dx12 = render(&mut crate::app::dx12_render::Dx12RenderEngine::new());
  let diff = difference(&wgpu, |index| {
    [0, 1, 2].map(|channel| f32::from(dx12[index * 4 + channel]))
  });
  println!("wgpu vs dx12: worst {} at {:?}", diff.worst, diff.at);
  assert!(diff.worst <= 2.0, "worst difference {} at {:?}", diff.worst, diff.at);
}
