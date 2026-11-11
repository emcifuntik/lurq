//! A recording's screenshot of a presented frame: the frame read back from
//! the GPU (sensitive text already painted over), averaged down on the readback
//! worker to fit `ScreenshotOptions`. Plain RGBA8 on the CPU, the same on every
//! platform.

/// The size `width` by `height` takes to fit `max_width` by `max_height`,
/// aspect kept, never enlarged; at least 1 by 1.
pub(crate) fn fitted_size(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
  let (width, height) = (width.max(1), height.max(1));
  let scale = (f64::from(max_width.max(1)) / f64::from(width))
    .min(f64::from(max_height.max(1)) / f64::from(height))
    .min(1.0);
  (
    ((f64::from(width) * scale).round() as u32).clamp(1, width),
    ((f64::from(height) * scale).round() as u32).clamp(1, height),
  )
}

/// `rgba`, tight rows `width` by `height`, averaged down to fit `max_width` by
/// `max_height`: each pixel of the result is the mean of the block of pixels
/// it covers. Returned as is when it already fits.
pub(crate) fn downscale_rgba(
  rgba: Vec<u8>,
  width: u32,
  height: u32,
  max_width: u32,
  max_height: u32,
) -> (u32, u32, Vec<u8>) {
  let (target_width, target_height) = fitted_size(width, height, max_width, max_height);
  if (target_width, target_height) == (width, height) || rgba.len() < width as usize * height as usize * 4 {
    return (width, height, rgba);
  }
  let (width, height) = (width as usize, height as usize);
  let (columns, rows) = (target_width as usize, target_height as usize);
  let mut scaled = Vec::with_capacity(columns * rows * 4);
  for row in 0..rows {
    let y0 = row * height / rows;
    let y1 = ((row + 1) * height / rows).max(y0 + 1);
    for column in 0..columns {
      let x0 = column * width / columns;
      let x1 = ((column + 1) * width / columns).max(x0 + 1);
      let mut sum = [0_u64; 4];
      for y in y0..y1 {
        let line = y * width * 4;
        for pixel in rgba[line + x0 * 4..line + x1 * 4].chunks_exact(4) {
          for (total, channel) in sum.iter_mut().zip(pixel) {
            *total += u64::from(*channel);
          }
        }
      }
      let count = ((y1 - y0) * (x1 - x0)) as u64;
      scaled.extend(sum.map(|total| ((total + count / 2) / count) as u8));
    }
  }
  (target_width, target_height, scaled)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_frame_fits_its_bounds_with_its_aspect_and_is_never_enlarged() {
    assert_eq!(fitted_size(1920, 1080, 320, 200), (320, 180));
    assert_eq!(fitted_size(800, 1200, 320, 200), (133, 200));
    assert_eq!(fitted_size(100, 50, 320, 200), (100, 50));
    assert_eq!(fitted_size(5000, 1, 320, 200), (320, 1));
    assert_eq!(fitted_size(0, 0, 0, 0), (1, 1));
  }

  #[test]
  fn downscaling_averages_each_block() {
    // 4 by 2: a white and a black 2 by 2 block, halved to 2 by 1.
    let white = [255, 255, 255, 255];
    let black = [0, 0, 0, 255];
    let row = [white, white, black, black].concat();
    let rgba = [row.clone(), row].concat();
    let (width, height, scaled) = downscale_rgba(rgba, 4, 2, 2, 1);
    assert_eq!((width, height), (2, 1));
    assert_eq!(scaled, [white, black].concat());

    // A frame that fits is kept as it is.
    let small = vec![7; 2 * 2 * 4];
    assert_eq!(downscale_rgba(small.clone(), 2, 2, 320, 200), (2, 2, small));

    // A grey bar stays grey: no pixel it covers mixes with the text under it.
    let grey = crate::app::capture_redaction::REDACTION_FILL;
    let rgba = grey.repeat(8 * 8);
    let (_, _, scaled) = downscale_rgba(rgba, 8, 8, 2, 2);
    assert!(scaled.chunks_exact(4).all(|pixel| pixel == grey));
  }
}
