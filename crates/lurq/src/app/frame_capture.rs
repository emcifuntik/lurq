//! Shared CPU-side post-processing for GPU screenshots: repacking
//! padded readback rows into tight RGBA, applying the borderless-window
//! rounded-corner clip, and saving the PNG. Used by every render engine that
//! supports frame capture.

use crate::app::render_engine::RenderFrameCapture;

/// Round a row byte size up to the copy row-pitch alignment the GPU API
/// requires for texture-to-buffer copies.
pub(crate) fn align_capture_row_pitch(value: u32, alignment: u32) -> u32 {
  value.div_ceil(alignment) * alignment
}

/// Repack GPU readback rows (padded to the API's row-pitch alignment) into a
/// tight RGBA8 buffer, swizzling from BGRA when the source format requires it.
pub(crate) fn capture_rows_to_rgba(
  data: &[u8],
  padded_bytes_per_row: usize,
  width: u32,
  height: u32,
  bgra: bool,
) -> Vec<u8> {
  let unpadded_bytes_per_row = width as usize * 4;
  let mut pixels = vec![0_u8; unpadded_bytes_per_row * height as usize];
  for y in 0..height as usize {
    let source_start = y * padded_bytes_per_row;
    let source_row = &data[source_start..source_start + unpadded_bytes_per_row];
    let target_row = &mut pixels[y * unpadded_bytes_per_row..(y + 1) * unpadded_bytes_per_row];
    if bgra {
      for (source, target) in source_row.chunks_exact(4).zip(target_row.chunks_exact_mut(4)) {
        target[0] = source[2];
        target[1] = source[1];
        target[2] = source[0];
        target[3] = source[3];
      }
    } else {
      target_row.copy_from_slice(source_row);
    }
  }
  pixels
}

/// Apply the window rounded-corner clip and deliver the captured pixels to
/// the capture's target: saved as a PNG file, or handed to a bytes callback.
pub(crate) fn finish_capture(mut pixels: Vec<u8>, capture: &RenderFrameCapture) {
  apply_capture_window_clip(&mut pixels, capture);

  match &capture.target {
    crate::app::render_engine::RenderCaptureTarget::Path(path) => {
      save_capture_png(path, &pixels, capture.width, capture.height);
    }
    crate::app::render_engine::RenderCaptureTarget::Bytes(callback) => {
      callback(Ok(crate::app::render_engine::CapturedFrame {
        width: capture.width,
        height: capture.height,
        rgba: pixels,
      }));
    }
  }
}

/// Save tight RGBA8 capture pixels as a PNG, creating the parent directory.
/// A failure is logged: a file capture has no caller waiting for it.
pub(crate) fn save_capture_png(output_path: &std::path::Path, pixels: &[u8], width: u32, height: u32) {
  if let Some(parent) = output_path.parent().filter(|parent| !parent.as_os_str().is_empty())
    && let Err(error) = std::fs::create_dir_all(parent)
  {
    tracing::warn!(
      "failed to create screenshot output directory {}: {error}",
      parent.display()
    );
    return;
  }
  if let Err(error) = image::save_buffer_with_format(
    output_path,
    pixels,
    width,
    height,
    image::ColorType::Rgba8,
    image::ImageFormat::Png,
  ) {
    tracing::warn!("failed to save screenshot to {}: {error}", output_path.display());
  } else {
    tracing::info!("Saved screenshot here: {}", output_path.display());
  }
}

/// Map a logical-pixel capture region onto the physical viewport: scale,
/// clamp to the viewport bounds, and round outward so the requested area is
/// fully covered. `None` when the region has no visible area.
pub(crate) fn physical_capture_region(
  region: crate::app::window::ScreenshotRegion,
  scale_factor: f32,
  viewport_width: f32,
  viewport_height: f32,
) -> Option<(u32, u32, u32, u32)> {
  if !region.width.is_finite() || !region.height.is_finite() {
    return None;
  }
  let left = (region.x * scale_factor).floor().clamp(0.0, viewport_width);
  let top = (region.y * scale_factor).floor().clamp(0.0, viewport_height);
  let right = ((region.x + region.width) * scale_factor)
    .ceil()
    .clamp(0.0, viewport_width);
  let bottom = ((region.y + region.height) * scale_factor)
    .ceil()
    .clamp(0.0, viewport_height);
  let width = right - left;
  let height = bottom - top;
  if width < 1.0 || height < 1.0 {
    return None;
  }
  Some((left as u32, top as u32, width as u32, height as u32))
}

pub(crate) fn apply_capture_window_clip(pixels: &mut [u8], capture: &RenderFrameCapture) {
  let Some(clip) = capture.window_clip else {
    return;
  };
  for y in 0..capture.height {
    for x in 0..capture.width {
      let world_x = capture.x as f32 + x as f32 + 0.5;
      let world_y = capture.y as f32 + y as f32 + 0.5;
      if capture_rounded_rect_contains(world_x, world_y, 0.0, 0.0, clip.width, clip.height, clip.radii) {
        continue;
      }
      let index = (y * capture.width + x) as usize * 4;
      pixels[index..index + 4].copy_from_slice(&[0, 0, 0, 0]);
    }
  }
}

fn capture_rounded_rect_contains(
  x: f32,
  y: f32,
  rect_x: f32,
  rect_y: f32,
  width: f32,
  height: f32,
  radii: [f32; 4],
) -> bool {
  if x < rect_x || y < rect_y || x >= rect_x + width || y >= rect_y + height {
    return false;
  }
  let max_radius = width.min(height).max(0.0) * 0.5;
  let radii = radii.map(|radius| radius.max(0.0).min(max_radius));
  let right = rect_x + width;
  let bottom = rect_y + height;
  if radii.iter().all(|radius| *radius <= 0.0) {
    return true;
  }
  if x < rect_x + radii[0] && y < rect_y + radii[0] {
    return capture_point_in_corner(x, y, rect_x + radii[0], rect_y + radii[0], radii[0]);
  }
  if x >= right - radii[1] && y < rect_y + radii[1] {
    return capture_point_in_corner(x, y, right - radii[1], rect_y + radii[1], radii[1]);
  }
  if x >= right - radii[2] && y >= bottom - radii[2] {
    return capture_point_in_corner(x, y, right - radii[2], bottom - radii[2], radii[2]);
  }
  if x < rect_x + radii[3] && y >= bottom - radii[3] {
    return capture_point_in_corner(x, y, rect_x + radii[3], bottom - radii[3], radii[3]);
  }
  true
}

fn capture_point_in_corner(x: f32, y: f32, center_x: f32, center_y: f32, radius: f32) -> bool {
  if radius <= 0.0 {
    return true;
  }
  let dx = x - center_x;
  let dy = y - center_y;
  dx * dx + dy * dy <= radius * radius
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::render_engine::RenderFrameCaptureWindowClip;

  #[test]
  fn align_capture_row_pitch_rounds_up_to_alignment() {
    assert_eq!(align_capture_row_pitch(8, 256), 256);
    assert_eq!(align_capture_row_pitch(256, 256), 256);
    assert_eq!(align_capture_row_pitch(257, 256), 512);
  }

  #[test]
  fn capture_rows_drop_row_padding() {
    #[rustfmt::skip]
    let data = [
      1, 2, 3, 4, 5, 6, 7, 8, 99, 99, 99, 99,
      9, 10, 11, 12, 13, 14, 15, 16, 88, 88, 88, 88,
    ];

    let rgba = capture_rows_to_rgba(&data, 12, 2, 2, false);

    assert_eq!(rgba, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
  }

  #[test]
  fn capture_region_scales_and_clamps_to_the_viewport() {
    let region = crate::app::window::ScreenshotRegion {
      x: 10.0,
      y: 20.0,
      width: 100.0,
      height: 50.0,
    };

    assert_eq!(
      physical_capture_region(region, 2.0, 1000.0, 1000.0),
      Some((20, 40, 200, 100))
    );

    let clipped = crate::app::window::ScreenshotRegion {
      x: -10.0,
      y: 990.0,
      width: 100.0,
      height: 100.0,
    };
    assert_eq!(
      physical_capture_region(clipped, 1.0, 1000.0, 1000.0),
      Some((0, 990, 90, 10))
    );
  }

  #[test]
  fn capture_region_outside_the_viewport_is_skipped() {
    let region = crate::app::window::ScreenshotRegion {
      x: 2000.0,
      y: 0.0,
      width: 100.0,
      height: 100.0,
    };

    assert_eq!(physical_capture_region(region, 1.0, 1000.0, 1000.0), None);

    let empty = crate::app::window::ScreenshotRegion {
      x: 0.0,
      y: 0.0,
      width: 0.0,
      height: 100.0,
    };
    assert_eq!(physical_capture_region(empty, 1.0, 1000.0, 1000.0), None);
  }

  #[test]
  fn capture_rows_swizzle_bgra_to_rgba() {
    let data = [1, 2, 3, 4, 5, 6, 7, 8];

    let rgba = capture_rows_to_rgba(&data, 8, 2, 1, true);

    assert_eq!(rgba, vec![3, 2, 1, 4, 7, 6, 5, 8]);
  }

  #[test]
  fn window_clip_clears_pixels_outside_rounded_corners() {
    let capture = RenderFrameCapture {
      x: 0,
      y: 0,
      width: 4,
      height: 4,
      target: crate::app::render_engine::RenderCaptureTarget::Path(std::path::PathBuf::new()),
      window_clip: Some(RenderFrameCaptureWindowClip {
        width: 4.0,
        height: 4.0,
        radii: [2.0; 4],
      }),
    };
    let mut pixels = vec![255_u8; 4 * 4 * 4];

    apply_capture_window_clip(&mut pixels, &capture);

    assert_eq!(&pixels[0..4], &[0, 0, 0, 0]);
    let center = (4 + 1) * 4;
    assert_eq!(&pixels[center..center + 4], &[255, 255, 255, 255]);
  }
}
