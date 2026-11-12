//! Compare saved before/after binaries using the same output-directory argument.
//! Captures synthetic color/coverage glyphs through the actual DX12 pipeline.
//! cargo run -p lurq --example dx12_glyph_atlas_probe --features screenshot,dx12,perf_profile --
//! F:/codex-tmp/atlas-check
#[cfg(windows)]
fn main() {
  use std::{
    fs::File,
    io::Write,
    num::NonZeroIsize,
    sync::{Arc, mpsc},
    time::Duration,
  };

  use lurq::{
    app::{
      dx12_render::Dx12RenderEngine,
      render_engine::{RenderCaptureTarget, RenderEngine, RenderFrameCapture},
    },
    layout::{
      quad::ClipRect,
      render_list::{GlyphAtlas, GlyphCmd, RenderList},
    },
    node::color::Color,
  };
  use raw_window_handle::{DisplayHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
  use windows::{
    Win32::{
      Foundation::{HWND, LPARAM, LRESULT, WPARAM},
      UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW, WS_POPUP,
      },
    },
    core::w,
  };

  extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
  }
  let output = std::path::PathBuf::from(std::env::args_os().nth(1).expect("pass output directory"));
  std::fs::create_dir_all(&output).unwrap();
  let expect_arena = std::env::args().any(|arg| arg == "--expect-arena");
  let hwnd = unsafe {
    let class_name = w!("lurq_glyph_atlas_probe");
    let class = WNDCLASSW {
      lpfnWndProc: Some(wndproc),
      lpszClassName: class_name,
      ..Default::default()
    };
    assert_ne!(RegisterClassW(&class), 0);
    CreateWindowExW(
      WINDOW_EX_STYLE::default(),
      class_name,
      class_name,
      WS_POPUP,
      0,
      0,
      544,
      128,
      None,
      None,
      None,
      None,
    )
    .unwrap()
  };
  let raw = Win32WindowHandle::new(NonZeroIsize::new(hwnd.0 as isize).unwrap());
  let window = unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(raw)) };
  let display = DisplayHandle::windows();
  let mut engine = Dx12RenderEngine::new();
  engine.resize(544, 128);
  let mut csv = File::create(output.join("frames.csv")).unwrap();
  writeln!(
    csv,
    "case,step,atlas_bytes,arena_uploads,dedicated_uploads,atlas_ms,pixels"
  )
  .unwrap();
  let mut version = 0;
  for (case, width, height) in [
    ("aligned", 256u32, 96u32),
    ("padded", 257, 96),
    ("document", 1024, 1024),
    ("oversized", 4096, 2112),
    ("truncated", 257, 96),
    ("empty", 257, 96),
  ] {
    for step in 0..6 {
      version += 1;
      let mut data = vec![0; (width * height * 4) as usize];
      for (i, pixel) in data.chunks_exact_mut(4).enumerate() {
        let x = i as u32 % width;
        let y = i as u32 / width;
        let tile = x / 8 + y / 8 + step;
        pixel.copy_from_slice(&[
          32 + ((x / 8 + step * 3) % 8 * 24) as u8,
          40 + ((y / 8 + step) % 8 * 24) as u8,
          180,
          [0, 128, 255][tile as usize % 3],
        ]);
      }
      if case == "truncated" {
        data.truncate((width * 4 * 63 + 97 * 4 + 2) as usize);
      }
      if case == "empty" {
        data.clear();
      }
      let sample_y = match step % 3 {
        0 => 0,
        1 => (height - 96) / 2,
        _ => height - 96,
      };
      let sample_x = if step % 2 == 0 { 0 } else { width.saturating_sub(257) };
      let sample_width = width.min(257);
      let glyph = |color_glyph| GlyphCmd {
        order: 0,
        x: if color_glyph { 8.0 } else { 280.0 },
        y: 8.0,
        width: sample_width as f32,
        height: 96.0,
        color: [1.0; 4],
        atlas_min: [sample_x as f32, sample_y as f32],
        atlas_max: [(sample_x + sample_width) as f32, (sample_y + 96) as f32],
        transform: [1.0, 0.0, 0.0, 1.0],
        transform_origin: [0.0; 2],
        sharpness: 1.0,
        color_glyph,
        shadow_sigma: 0.0,
        clip: ClipRect::default(),
      };
      let list = RenderList {
        clear_color: Color::new(0, 0, 0, 255),
        rects: vec![],
        glyphs: vec![glyph(true), glyph(false)],
        #[cfg(feature = "raster")]
        images: vec![],
        #[cfg(feature = "svg")]
        svgs: vec![],
        layers: vec![],
        backdrops: Vec::new(),
        atlas: GlyphAtlas {
          data: data.into(),
          width,
          height,
          version,
          dirty_rects: Arc::from([]),
          dirty_from_version: version,
        },
      };
      // The first two updates run without readback, exercising frame reuse.
      if step < 2 {
        assert!(engine.render(&list, window, display));
        continue;
      }
      let (send, receive) = mpsc::channel();
      assert!(engine.render_with_capture(
        &list,
        window,
        display,
        Some(RenderFrameCapture {
          x: 0,
          y: 0,
          width: 544,
          height: 128,
          window_clip: None,
          target: RenderCaptureTarget::Bytes(Arc::new(move |result| {
            send.send(result).unwrap();
          })),
        })
      ));
      let capture = receive.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
      assert_eq!((capture.width, capture.height), (544, 128));
      let mut opaque = 0;
      for y in 0..96u32 {
        for x in 0..sample_width {
          let source = (((sample_y + y) * width + sample_x + x) * 4) as usize;
          let mut rgba = [0; 4];
          for (i, value) in rgba.iter_mut().enumerate() {
            *value = list.atlas.data.get(source + i).copied().unwrap_or(0);
          }
          if rgba[3] != 0 && rgba[3] != 255 {
            continue;
          }
          opaque += usize::from(rgba[3] == 255);
          for (left, color) in [(8, true), (280, false)] {
            let offset = (((8 + y) * 544 + left + x) * 4) as usize;
            for channel in 0..3 {
              let expected = if rgba[3] == 0 {
                0
              } else if color {
                rgba[channel]
              } else {
                255
              };
              let actual = capture.rgba[offset + channel];
              assert!(
                actual.abs_diff(expected) <= 2,
                "{case}/{step}: pixel {x},{y} channel {channel}: {actual} != {expected}"
              );
            }
          }
        }
      }
      assert!(
        case == "empty" || opaque > 1000,
        "fixture did not draw enough opaque glyph pixels"
      );
      let signature = capture.rgba.iter().fold(0xcbf29ce484222325u64, |hash, &byte| {
        (hash ^ byte as u64).wrapping_mul(0x100000001b3)
      });
      image::save_buffer(
        output.join(format!("{case}-{step}.png")),
        &capture.rgba,
        544,
        128,
        image::ColorType::Rgba8,
      )
      .unwrap();
      let p = engine.last_profile().unwrap();
      assert_eq!(p.glyph_atlas_full_uploads, 1);
      if expect_arena {
        assert_eq!(p.glyph_atlas_arena_uploads, usize::from(case != "oversized"));
        assert_eq!(p.glyph_atlas_dedicated_uploads, usize::from(case == "oversized"));
      }
      writeln!(
        csv,
        "{case},{step},{},{},{},{:.6},{signature}",
        p.glyph_atlas_upload_bytes,
        p.glyph_atlas_arena_uploads,
        p.glyph_atlas_dedicated_uploads,
        p.atlas_upload.as_secs_f64() * 1000.0
      )
      .unwrap();
      assert!(engine.render(&list, window, display), "unchanged frame failed");
      let p = engine.last_profile().unwrap();
      assert_eq!(p.glyph_atlas_upload_bytes, 0);
      assert_eq!(p.glyph_atlas_arena_uploads + p.glyph_atlas_dedicated_uploads, 0);
    }
  }
  drop(engine);
  unsafe {
    DestroyWindow(hwnd).unwrap();
  }
  println!("24 captures passed; {}", output.display());
}

#[cfg(not(windows))]
fn main() {
  eprintln!("This probe requires Windows and DX12");
}
