// The CPU blurs of a devtools node screenshot, which stand in for the render
// engines' layer blur and backdrop blur.

use super::{DevtoolsScreenshotBounds, blur_screenshot_backdrop, blur_screenshot_layer};
use crate::layout::{backdrop::BackdropCmd, quad::ClipRect};

const SIZE: usize = 41;

fn layer_with_block(color: [u8; 4]) -> Vec<u8> {
  let mut layer = vec![0_u8; SIZE * SIZE * 4];
  for y in 16..25 {
    for x in 16..25 {
      layer[(y * SIZE + x) * 4..][..4].copy_from_slice(&color);
    }
  }
  layer
}

fn pixel(layer: &[u8], x: usize, y: usize) -> [u8; 4] {
  let index = (y * SIZE + x) * 4;
  [layer[index], layer[index + 1], layer[index + 2], layer[index + 3]]
}

#[test]
fn a_blurred_layer_spreads_its_alpha_and_keeps_its_colour() {
  let mut layer = layer_with_block([200, 40, 10, 255]);
  let before: u32 = layer.chunks_exact(4).map(|pixel| u32::from(pixel[3])).sum();
  blur_screenshot_layer(&mut layer, SIZE as u32, 2.0);

  let after: u32 = layer.chunks_exact(4).map(|pixel| u32::from(pixel[3])).sum();
  assert!(after.abs_diff(before) * 50 < before, "{before} -> {after}");
  // The middle stays the most opaque, the edge of the block is about half
  // way, and three deviations out nothing is left.
  let middle = pixel(&layer, 20, 20)[3];
  let edge = pixel(&layer, 16, 20)[3];
  assert!(middle > 200 && (100..190).contains(&edge), "{middle}, {edge}");
  assert_eq!(pixel(&layer, 16 - 8, 20)[3], 0);
  // Symmetric, and transparent pixels do not darken the colour.
  assert_eq!(pixel(&layer, 14, 20), pixel(&layer, 26, 20));
  assert_eq!(pixel(&layer, 20, 14), pixel(&layer, 20, 26));
  for (x, y) in [(20, 20), (16, 20), (14, 14)] {
    let [r, g, b, a] = pixel(&layer, x, y);
    assert!(a > 0, "({x}, {y})");
    assert!(
      r.abs_diff(200) <= 2 && g.abs_diff(40) <= 2 && b.abs_diff(10) <= 2,
      "({x}, {y}): {r}, {g}, {b}"
    );
  }
}

#[test]
fn a_blur_below_a_pixel_leaves_the_layer_alone() {
  let mut layer = layer_with_block([200, 40, 10, 128]);
  let untouched = layer.clone();
  blur_screenshot_layer(&mut layer, SIZE as u32, 0.2);
  assert_eq!(layer, untouched);
}

/// A capture that starts at window pixel (100, 50): opaque, dark left of
/// its column 20 and light from it on.
fn split_capture() -> (Vec<u8>, DevtoolsScreenshotBounds) {
  let mut capture = vec![0_u8; SIZE * SIZE * 4];
  for (index, pixel) in capture.chunks_exact_mut(4).enumerate() {
    let value = if index % SIZE >= 20 { 240 } else { 20 };
    pixel.copy_from_slice(&[value, value, value, 255]);
  }
  let bounds = DevtoolsScreenshotBounds {
    x: 100,
    y: 50,
    width: SIZE as u32,
    height: SIZE as u32,
    window_clip: None,
  };
  (capture, bounds)
}

#[test]
fn a_backdrop_blur_softens_the_capture_inside_its_rounded_box_only() {
  let (mut capture, bounds) = split_capture();
  let sharp = capture.clone();
  // Capture pixels 8..32 by 10..30, with round corners.
  let backdrop = BackdropCmd {
    order: 0,
    x: 108.0,
    y: 60.0,
    width: 24.0,
    height: 20.0,
    radii: [6.0; 4],
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [12.0, 10.0],
    clip: ClipRect::default(),
    sigma: 2.0,
    opacity: 1.0,
  };
  blur_screenshot_backdrop(&mut capture, bounds, &backdrop);

  // The edge between the halves is soft inside the box and stays opaque.
  let (dark_side, light_side) = (pixel(&capture, 19, 20), pixel(&capture, 20, 20));
  assert!((60..130).contains(&dark_side[0]), "{dark_side:?}");
  assert!((130..200).contains(&light_side[0]), "{light_side:?}");
  assert_eq!((dark_side[3], light_side[3]), (255, 255));
  // Outside the box, and outside its corner's arc, nothing changed.
  for (x, y) in [(19, 9), (19, 30), (7, 20), (32, 20), (8, 10), (31, 29)] {
    assert_eq!(pixel(&capture, x, y), pixel(&sharp, x, y), "({x}, {y})");
  }
  // Half opacity is half way between.
  let (mut half, _) = split_capture();
  blur_screenshot_backdrop(
    &mut half,
    bounds,
    &BackdropCmd {
      opacity: 0.5,
      ..backdrop
    },
  );
  let mixed = pixel(&half, 19, 20)[0];
  assert!(mixed.abs_diff((dark_side[0] + 20) / 2) <= 1, "{mixed}");
}
