// A hidden window of the test's own for render-engine readback tests: the
// engines render into its swapchain and deliver a capture of the frame.
// Nothing is shown and no other window is read.

use std::{
  num::NonZeroIsize,
  sync::{Arc, Once, mpsc},
  time::Duration,
};

use raw_window_handle::{DisplayHandle, Win32WindowHandle, WindowHandle};
use windows::{
  Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::WindowsAndMessaging::{
      CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW, WS_POPUP,
    },
  },
  core::w,
};

use crate::{
  app::render_engine::{CapturedFrame, RenderCaptureTarget, RenderEngine, RenderFrameCapture},
  layout::render_list::{GlyphAtlas, RenderList},
  node::color::Color,
};

pub(super) struct HiddenWindow(HWND);

impl HiddenWindow {
  pub(super) fn new(width: u32, height: u32) -> Self {
    static REGISTER: Once = Once::new();
    let name = w!("LurqBlendReadbackTest");
    // SAFETY: registers a class whose procedure only forwards to
    // `DefWindowProcW`, then creates a hidden popup of that class. The class
    // name is a static wide string.
    unsafe {
      REGISTER.call_once(|| {
        let class = WNDCLASSW {
          lpfnWndProc: Some(procedure),
          lpszClassName: name,
          ..Default::default()
        };
        assert_ne!(RegisterClassW(&class), 0);
      });
      let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        name,
        name,
        WS_POPUP,
        0,
        0,
        width as i32,
        height as i32,
        None,
        None,
        None,
        None,
      )
      .expect("hidden test window");
      Self(hwnd)
    }
  }

  pub(super) fn window_handle(&self) -> WindowHandle<'_> {
    let raw = Win32WindowHandle::new(NonZeroIsize::new(self.0.0 as isize).expect("non-null HWND"));
    // SAFETY: the HWND stays valid until `self` is dropped, which outlives the
    // borrowed handle.
    unsafe { WindowHandle::borrow_raw(raw.into()) }
  }
}

extern "system" fn procedure(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
  // SAFETY: forwards the message unchanged to the default procedure.
  unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

impl Drop for HiddenWindow {
  fn drop(&mut self) {
    // SAFETY: the window was created by `new` on this thread and is destroyed once.
    // A failure only leaks a hidden test window until the process exits.
    if let Err(error) = unsafe { DestroyWindow(self.0) } {
      eprintln!("failed to destroy the readback test window: {error}");
    }
  }
}

/// A frame with nothing but its clear colour, for tests that render only Canvas.
#[cfg_attr(not(feature = "canvas"), allow(dead_code))]
pub(super) fn empty_list() -> RenderList {
  RenderList {
    clear_color: Color::new(24, 24, 27, 255),
    rects: Vec::new(),
    glyphs: Vec::new(),
    #[cfg(feature = "raster")]
    images: Vec::new(),
    #[cfg(feature = "svg")]
    svgs: Vec::new(),
    layers: Vec::new(),
    atlas: GlyphAtlas {
      data: Arc::from([0_u8; 4].as_slice()),
      width: 1,
      height: 1,
      version: 1,
      dirty_rects: Arc::from(Vec::new()),
      dirty_from_version: 0,
    },
  }
}

/// Renders `list` into a hidden `width` x `height` window until the engine
/// delivers a capture of the whole window.
pub(super) fn capture(engine: &mut dyn RenderEngine, list: &RenderList, width: u32, height: u32) -> CapturedFrame {
  capture_each(engine, &[list], width, height)
    .pop()
    .expect("one capture per list")
}

/// Renders each of `lists` in turn into one hidden `width` x `height` window,
/// as consecutive frames of the same engine, capturing each.
pub(super) fn capture_each(
  engine: &mut dyn RenderEngine,
  lists: &[&RenderList],
  width: u32,
  height: u32,
) -> Vec<CapturedFrame> {
  let window = HiddenWindow::new(width, height);
  engine.resize(width, height);
  lists
    .iter()
    .map(|list| capture_frame(engine, &window, list, width, height))
    .collect()
}

fn capture_frame(
  engine: &mut dyn RenderEngine,
  window: &HiddenWindow,
  list: &RenderList,
  width: u32,
  height: u32,
) -> CapturedFrame {
  let (sender, receiver) = mpsc::channel();
  for _ in 0..5 {
    let sender = sender.clone();
    let target = RenderCaptureTarget::Bytes(Arc::new(move |frame| {
      // The receiver may already have a frame from an earlier attempt.
      let _ = sender.send(frame);
    }));
    let request = RenderFrameCapture {
      x: 0,
      y: 0,
      width,
      height,
      target,
      window_clip: None,
    };
    engine.render_with_capture(list, window.window_handle(), DisplayHandle::windows(), Some(request));
    if let Ok(Ok(frame)) = receiver.recv_timeout(Duration::from_secs(10)) {
      return frame;
    }
  }
  panic!("the engine never delivered a frame capture");
}
