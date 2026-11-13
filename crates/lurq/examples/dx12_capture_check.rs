//! Manual end-to-end check for the DX12 screenshot capture: renders a
//! known scene into a real swapchain, captures a sub-rect through
//! `render_with_capture`, and asserts the saved PNG's pixels.
//!
//! Run with:
//! ```text
//! cargo run -p lurq --example dx12_capture_check --features screenshot,dx12
//! ```

#[cfg(windows)]
use std::num::NonZeroIsize;

#[cfg(windows)]
use lurq::{
  app::{
    dx12_render::Dx12RenderEngine,
    render_engine::{RenderEngine, RenderFrameCapture},
  },
  layout::{
    quad::ClipRect,
    render_list::{GlyphAtlas, RectCmd, RenderList},
  },
  node::color::Color,
};
#[cfg(windows)]
use raw_window_handle::{DisplayHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
#[cfg(windows)]
use windows::{
  Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW, WS_POPUP},
  },
  core::w,
};

#[cfg(windows)]
extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
  unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(windows)]
const SURFACE: u32 = 256;
#[cfg(windows)]
const CAPTURE_ORIGIN: u32 = 32;
#[cfg(windows)]
const CAPTURE_SIZE: u32 = 192;
#[cfg(windows)]
const CLEAR: [u8; 4] = [128, 64, 32, 255];
#[cfg(windows)]
const RECT: [u8; 4] = [40, 120, 200, 255];

#[cfg(windows)]
fn main() {
  let hwnd = unsafe {
    let class_name = w!("lurq_dx12_capture_check");
    let class = WNDCLASSW {
      lpfnWndProc: Some(wndproc),
      lpszClassName: class_name,
      ..Default::default()
    };
    assert_ne!(RegisterClassW(&class), 0, "failed to register window class");
    CreateWindowExW(
      WINDOW_EX_STYLE::default(),
      class_name,
      w!("lurq dx12 capture check"),
      WS_POPUP,
      0,
      0,
      SURFACE as i32,
      SURFACE as i32,
      None,
      None,
      None,
      None,
    )
    .expect("failed to create window")
  };

  let raw_handle = Win32WindowHandle::new(NonZeroIsize::new(hwnd.0 as isize).expect("null hwnd"));
  let window = unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(raw_handle)) };
  let display = DisplayHandle::windows();

  let list = RenderList {
    clear_color: Color::new(CLEAR[0], CLEAR[1], CLEAR[2], CLEAR[3]),
    rects: vec![RectCmd {
      order: 0,
      x: 64.0,
      y: 64.0,
      width: 128.0,
      height: 128.0,
      color: Color::new(RECT[0], RECT[1], RECT[2], RECT[3]),
      radii: [0.0; 4],
      stroke: [0.0; 4],
      stroke_color: Color::new(0, 0, 0, 0),
      transform: [1.0, 0.0, 0.0, 1.0],
      transform_origin: [0.0, 0.0],
      clip: ClipRect::default(),
      gradient: None,
      shadow: None,
    }],
    glyphs: Vec::new(),
    #[cfg(feature = "raster")]
    images: Vec::new(),
    #[cfg(feature = "svg")]
    svgs: Vec::new(),
    layers: Vec::new(),
    backdrops: Vec::new(),
    atlas: GlyphAtlas {
      data: std::sync::Arc::from([].as_slice()),
      width: 0,
      height: 0,
      version: 0,
      dirty_rects: std::sync::Arc::from([].as_slice()),
      dirty_from_version: 0,
    },
  };

  let output_path = std::env::temp_dir().join("lurq_dx12_capture_check.png");
  let _ = std::fs::remove_file(&output_path);

  let mut engine = Dx12RenderEngine::new();
  engine.resize(SURFACE, SURFACE);
  assert!(engine.supports_frame_capture());
  assert!(engine.render(&list, window, display), "warm-up frame failed");
  assert!(
    engine.render_with_capture(
      &list,
      window,
      display,
      Some(RenderFrameCapture {
        x: CAPTURE_ORIGIN,
        y: CAPTURE_ORIGIN,
        width: CAPTURE_SIZE,
        height: CAPTURE_SIZE,
        target: lurq::app::render_engine::RenderCaptureTarget::Path(output_path.clone()),
        window_clip: None,
      }),
    ),
    "capture frame failed"
  );

  let mut png = None;
  for _ in 0..100 {
    std::thread::sleep(std::time::Duration::from_millis(50));
    if let Ok(image) = image::open(&output_path) {
      png = Some(image.into_rgba8());
      break;
    }
  }
  let png = png.expect("capture PNG was not written within 5s");

  assert_eq!(png.width(), CAPTURE_SIZE);
  assert_eq!(png.height(), CAPTURE_SIZE);
  // (10, 10) in the capture is (42, 42) on the surface: clear color.
  assert_pixel(&png, 10, 10, CLEAR);
  // The capture center is (128, 128) on the surface: inside the rect.
  assert_pixel(&png, CAPTURE_SIZE / 2, CAPTURE_SIZE / 2, RECT);
  // (170, 170) in the capture is (202, 202) on the surface: clear again.
  assert_pixel(&png, 170, 170, CLEAR);

  println!("dx12 capture check passed: {}", output_path.display());
}

#[cfg(windows)]
fn assert_pixel(png: &image::RgbaImage, x: u32, y: u32, expected: [u8; 4]) {
  let actual = png.get_pixel(x, y).0;
  for (channel, (actual, expected)) in actual.iter().zip(expected).enumerate() {
    assert!(
      actual.abs_diff(expected) <= 2,
      "pixel ({x}, {y}) channel {channel}: expected {expected} within 2, got {actual} (full pixel {actual:?})",
    );
  }
}

#[cfg(not(windows))]
fn main() {
  eprintln!("This check requires Windows and DX12");
}
