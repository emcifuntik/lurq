//! End-to-end layer-blur check on a hidden Win32 surface: a scene of blurred
//! nodes goes through layout and both native backends, and the two captures
//! must match.
//! cargo run -p lurq --example layer_blur_capture_check --features screenshot,wgpu,dx12
//! Writes target/layer-blur-wgpu.png and target/layer-blur-dx12.png.
#[cfg(windows)]
fn main() {
  use std::{
    num::NonZeroIsize,
    sync::{Arc, mpsc},
    time::Duration,
  };

  use lurq::{
    app::{
      App, Tree,
      render_engine::{CapturedFrame, RenderCaptureTarget, RenderEngine, RenderFrameCapture},
    },
    components::{Column, Rect, Row, Stack, Text},
    layout::{Constraints, Size, render_list::RenderList},
    node::Element,
  };
  use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, Win32WindowHandle, WindowHandle,
  };
  use windows::{
    Win32::{
      Foundation::{HWND, LPARAM, LRESULT, WPARAM},
      UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW, WS_POPUP,
      },
    },
    core::w,
  };

  const SIZE: u32 = 320;

  extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
  }
  struct Surface(HWND);
  impl HasWindowHandle for Surface {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
      let raw = Win32WindowHandle::new(NonZeroIsize::new(self.0.0 as isize).unwrap());
      Ok(unsafe { WindowHandle::borrow_raw(raw.into()) })
    }
  }
  impl HasDisplayHandle for Surface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
      Ok(DisplayHandle::windows())
    }
  }
  impl Drop for Surface {
    fn drop(&mut self) {
      unsafe {
        let _ = DestroyWindow(self.0);
      }
    }
  }
  struct Capture {
    backend: Box<dyn RenderEngine>,
    target: RenderCaptureTarget,
  }
  impl RenderEngine for Capture {
    fn resize(&mut self, w: u32, h: u32) {
      self.backend.resize(w, h);
    }
    fn render(&mut self, list: &RenderList, window: WindowHandle<'_>, display: DisplayHandle<'_>) -> bool {
      let request = RenderFrameCapture {
        x: 0,
        y: 0,
        width: SIZE,
        height: SIZE,
        target: self.target.clone(),
        window_clip: None,
      };
      self.backend.render_with_capture(list, window, display, Some(request))
    }
  }

  const PAGE: [u8; 3] = [0xee, 0xf0, 0xf3];

  fn scene() -> Element {
    // A card of several primitives: fill, border, a label and a bar.
    let card = |radius: f32, opacity: f32| {
      Column::new()
        .size(80.0, 56.0)
        .padding(8.0)
        .spacing(6.0)
        .background("#ffffff")
        .border_inside(1.0, "#c8ccd4")
        .rounded(10.0)
        .child(Text::new("Blur").color("#111827"))
        .child(Rect::new(48.0, 8.0).background("#2563eb").rounded(4.0))
        .opacity(opacity)
        .blur(radius)
    };
    Stack::new()
      .size(SIZE as f32, SIZE as f32)
      .background("#eef0f3")
      .child(
        Column::new()
          .padding(24.0)
          .spacing(36.0)
          .overflow_visible()
          .child(
            Row::new()
              .spacing(24.0)
              .overflow_visible()
              .child(card(0.0, 1.0))
              .child(card(2.0, 1.0))
              .child(card(8.0, 1.0)),
          )
          .child(
            Row::new()
              .spacing(24.0)
              .overflow_visible()
              // A wide blur, which runs on a reduced copy of the layer.
              .child(card(24.0, 1.0))
              // Blurred and faded as one layer.
              .child(card(8.0, 0.5)),
          )
          // A container that clips its blurred child: the blur stays inside,
          // and the part of the child past the clip still bleeds in.
          .child(
            Stack::new()
              .size(200.0, 72.0)
              .clip()
              .background("#dfe3ea")
              .child(Rect::new(320.0, 40.0).background("#dc2626").blur(12.0)),
          ),
      )
      .into()
  }

  let surface = Surface(unsafe {
    let class_name = w!("lurq_layer_blur_capture_check");
    assert_ne!(
      RegisterClassW(&WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        lpszClassName: class_name,
        ..Default::default()
      }),
      0
    );
    CreateWindowExW(
      WINDOW_EX_STYLE::default(),
      class_name,
      w!("Layer blur capture check"),
      WS_POPUP,
      0,
      0,
      SIZE as i32,
      SIZE as i32,
      None,
      None,
      None,
      None,
    )
    .unwrap()
  });

  let capture = |backend: &'static str| -> CapturedFrame {
    let (sender, receiver) = mpsc::channel();
    let target = RenderCaptureTarget::Bytes(Arc::new(move |frame| {
      let _ = sender.send(frame);
    }));
    let mut tree = Tree::new();
    tree.set_render_engine_factory(move || {
      let backend: Box<dyn RenderEngine> = match backend {
        "wgpu" => Box::new(lurq::app::wgpu_render::WgpuRenderEngine::new()),
        _ => Box::new(lurq::app::dx12_render::Dx12RenderEngine::new()),
      };
      Box::new(Capture {
        backend,
        target: target.clone(),
      })
    });
    tree.resize(SIZE, SIZE);
    tree.set_layout_constraints_override(Some(Constraints::tight(Size::new(SIZE as f32, SIZE as f32))));
    let mut app = App::new();
    tree.set_root(scene());
    assert!(tree.pass(&mut app, &surface).rendered, "{backend}: no frame");
    receiver
      .recv_timeout(Duration::from_secs(15))
      .expect("capture")
      .expect("capture ok")
  };

  let wgpu = capture("wgpu");
  let dx12 = capture("dx12");
  for (name, frame) in [("wgpu", &wgpu), ("dx12", &dx12)] {
    let png = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone()).unwrap();
    let output = std::path::PathBuf::from("target").join(format!("layer-blur-{name}.png"));
    png.save(&output).unwrap();
    println!("{name}: {}", output.display());
  }

  let at = |frame: &CapturedFrame, x: u32, y: u32| {
    let i = ((y * frame.width + x) * 4) as usize;
    [frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]]
  };
  for frame in [&wgpu, &dx12] {
    // The unblurred card (x 24..104, y 24..80) keeps its white fill and the
    // page around it.
    assert_eq!(at(frame, 90, 70), [0xff, 0xff, 0xff]);
    assert_eq!(at(frame, 160, 4), PAGE);
    // The `blur(8)` card is the same card 208 pixels to the right. Its bar
    // (x 32..80, y 57..65 in the first card) is paler, its border is gone,
    // and three deviations past its box the page is untouched.
    let (bar, blurred_bar) = (at(frame, 56, 61), at(frame, 56 + 208, 61));
    assert!(
      bar[0] < 0x40 && blurred_bar[0] > bar[0] + 40,
      "{bar:?} -> {blurred_bar:?}"
    );
    assert!(
      at(frame, 24, 52)[0] < 0xd8,
      "the first card's border {:?}",
      at(frame, 24, 52)
    );
    assert!(at(frame, 232, 52)[0] > 0xe8, "blur(8) border {:?}", at(frame, 232, 52));
    assert_eq!(at(frame, 232 - 16, 52), PAGE);
    // The clipped child (clip x 24..224, y 208..280; child y 208..248): red
    // right up to the clip's right edge, where the rest of the child bleeds
    // in, and none of it outside the clip.
    let inside = at(frame, 222, 228);
    assert!(inside[0] > 200 && inside[1] < 60, "clipped blur edge {inside:?}");
    assert_eq!(at(frame, 226, 228), PAGE, "blur leaked out of its clip");
    assert_eq!(at(frame, 100, 284), PAGE, "blur leaked out of its clip");
  }

  let mut worst = 0u8;
  let mut over_one = 0usize;
  for (a, b) in wgpu.rgba.chunks_exact(4).zip(dx12.rgba.chunks_exact(4)) {
    let diff = a.iter().zip(b).take(3).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    worst = worst.max(diff);
    over_one += usize::from(diff > 1);
  }
  println!("wgpu vs dx12: worst channel difference {worst}, {over_one} pixels differ by more than 1");
  assert!(worst <= 2, "backends disagree by {worst}");
  println!("layer blur capture check passed");
}

#[cfg(not(windows))]
fn main() {
  eprintln!("This capture harness uses a Win32 surface.");
}
