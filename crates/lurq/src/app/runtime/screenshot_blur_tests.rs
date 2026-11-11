// The CPU blur of a devtools node screenshot's layers, which stands in for
// the render engines' layer blur.

use super::blur_screenshot_layer;

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
