//! The chat screen of the `text_chat` benchmark through the wgpu and DX12
//! renderers, on a hidden Win32 window: what the renderers add to a frame of
//! text (encoding, uploads, draw calls and, on wgpu, GPU time), and captures
//! of the frames to compare between two builds.
//!
//! cargo run --release -p lurq --example text_chat_gpu_probe --features screenshot,wgpu,dx12,perf_profile -- <dir>
//!
//! Writes `<dir>/frames.csv` (one row per frame), `<dir>/captures.csv` (one
//! row per capture, with a hash of its pixels) and a PNG per capture. The two
//! backends' captures of a step must agree within two levels a channel, and
//! `scripts/compare-text-chat-captures.py` compares the hashes of two runs.
//! Nothing is shown; the frames are presented to a window that is never made
//! visible, so `present_us` is not a display's frame time.
//!
//! Backends named after the directory (`wgpu`, `dx12`) limit the run to them.
//! The system must be composing windows: with every display asleep the DX12
//! renderer, which waits for the compositor, draws no frame, and the probe
//! stops saying so. `LURQ_PROBE_CAPTURES_ONLY=1` then still compares what is
//! drawn: only the frames that are captured reach the renderer, a few of
//! which a swapchain takes without the compositor. The times in `frames.csv`
//! mean nothing in that mode.
#[cfg(windows)]
#[path = "../benches/text_chat/driver.rs"]
mod driver;
#[cfg(windows)]
#[path = "../benches/text_chat/screen.rs"]
mod screen;

#[cfg(windows)]
mod probe {
  use std::{
    fs::File,
    io::{BufWriter, Write},
    num::NonZeroIsize,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
  };

  use lurq::{
    app::{
      profiler::{PassSample, RecordingOptions, RenderProfile, SampleData},
      render_engine::{CapturedFrame, RenderCaptureTarget, RenderEngine, RenderFrameCapture},
    },
    layout::render_list::RenderList,
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

  use super::{
    driver::{CHANNEL_MESSAGES, Chat, SCROLL_MESSAGES},
    screen::{DRAFT_ID, VIEWPORT},
  };

  /// The largest frame drawn: the viewport at 150%.
  const MAX_SIZE: (u32, u32) = (VIEWPORT.0 * 3 / 2, VIEWPORT.1 * 3 / 2);
  const FRAMES: usize = 40;
  const SCROLL_FRAMES: usize = 80;

  extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
  }

  #[derive(Clone, Copy)]
  struct Surface(HWND);

  impl Surface {
    fn hidden() -> Self {
      static REGISTER: std::sync::Once = std::sync::Once::new();
      let class_name = w!("lurq_text_chat_gpu_probe");
      // SAFETY: registers a class whose procedure forwards to
      // `DefWindowProcW`, then creates a popup of it that is never shown.
      unsafe {
        REGISTER.call_once(|| {
          assert_ne!(
            RegisterClassW(&WNDCLASSW {
              lpfnWndProc: Some(wndproc),
              lpszClassName: class_name,
              ..Default::default()
            }),
            0
          );
        });
        Self(
          CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Text chat GPU probe"),
            WS_POPUP,
            0,
            0,
            MAX_SIZE.0 as i32,
            MAX_SIZE.1 as i32,
            None,
            None,
            None,
            None,
          )
          .expect("a hidden window"),
        )
      }
    }

    fn destroy(self) {
      // SAFETY: the window was created by `hidden` on this thread.
      unsafe {
        let _ = DestroyWindow(self.0);
      }
    }
  }

  impl HasWindowHandle for Surface {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
      let raw = Win32WindowHandle::new(NonZeroIsize::new(self.0.0 as isize).unwrap());
      // SAFETY: the window outlives every tree drawn on it.
      Ok(unsafe { WindowHandle::borrow_raw(raw.into()) })
    }
  }

  impl HasDisplayHandle for Surface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
      Ok(DisplayHandle::windows())
    }
  }

  /// A capture the next rendered frame takes: its size and where it goes.
  type PendingCapture = Arc<Mutex<Option<(u32, u32, RenderCaptureTarget)>>>;

  /// The backend, taking a capture of a frame when one is asked for.
  struct Capturing {
    backend: Box<dyn RenderEngine>,
    pending: PendingCapture,
    /// Frames nobody captures are not drawn (`LURQ_PROBE_CAPTURES_ONLY`).
    captures_only: bool,
  }

  impl RenderEngine for Capturing {
    fn profile_backend(&self) -> &'static str {
      self.backend.profile_backend()
    }

    fn set_profile_context(&mut self, context: lurq::app::profiler::ProfileContext) {
      self.backend.set_profile_context(context);
    }

    fn resize(&mut self, width: u32, height: u32) {
      self.backend.resize(width, height);
    }

    fn render(&mut self, list: &RenderList, window: WindowHandle<'_>, display: DisplayHandle<'_>) -> bool {
      self.render_with_capture(list, window, display, None)
    }

    fn supports_frame_capture(&self) -> bool {
      self.backend.supports_frame_capture()
    }

    fn render_with_capture(
      &mut self,
      list: &RenderList,
      window: WindowHandle<'_>,
      display: DisplayHandle<'_>,
      capture: Option<RenderFrameCapture>,
    ) -> bool {
      let capture = capture.or_else(|| {
        let (width, height, target) = self.pending.lock().unwrap().take()?;
        Some(RenderFrameCapture {
          x: 0,
          y: 0,
          width,
          height,
          target,
          window_clip: None,
        })
      });
      if capture.is_none() && self.captures_only {
        return true;
      }
      self.backend.render_with_capture(list, window, display, capture)
    }

    fn wants_redraw(&self) -> bool {
      self.backend.wants_redraw()
    }

    fn last_profile(&self) -> Option<RenderProfile> {
      self.backend.last_profile()
    }
  }

  fn backend(name: &str) -> Box<dyn RenderEngine> {
    match name {
      // No vsync: a frame's time is its work, not the wait for a refresh.
      "wgpu" => Box::new(
        lurq::app::wgpu_render::WgpuRenderEngine::new()
          .with_present_mode(lurq::app::wgpu_render::WgpuPresentMode::Immediate),
      ),
      _ => Box::new(lurq::app::dx12_render::Dx12RenderEngine::new()),
    }
  }

  /// FNV-1a over the pixels: equal captures, equal hashes.
  fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
      (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
  }

  struct Probe {
    backend: &'static str,
    chat: Chat<Surface>,
    pending: PendingCapture,
    directory: PathBuf,
    frames: BufWriter<File>,
    captures: BufWriter<File>,
    captured: Vec<(String, CapturedFrame)>,
  }

  impl Probe {
    fn new(backend_name: &'static str, messages: usize, surface: Surface, directory: &Path, append: bool) -> Self {
      let pending = PendingCapture::default();
      let slot = pending.clone();
      let chat = Chat::new(messages, surface, move || {
        Box::new(Capturing {
          backend: backend(backend_name),
          pending: slot.clone(),
          captures_only: std::env::var_os("LURQ_PROBE_CAPTURES_ONLY").is_some(),
        })
      });
      let open = |name: &str| {
        let file = std::fs::OpenOptions::new()
          .create(true)
          .write(true)
          .append(append)
          .truncate(!append)
          .open(directory.join(name))
          .expect("the probe's output file");
        BufWriter::new(file)
      };
      Self {
        backend: backend_name,
        chat,
        pending,
        directory: directory.to_owned(),
        frames: open("frames.csv"),
        captures: open("captures.csv"),
        captured: Vec::new(),
      }
    }

    /// Runs `frames` steps in a profiling session, which is what makes the
    /// wgpu renderer time its passes on the GPU, and writes a row for each.
    fn measure(&mut self, scenario: &str, frames: usize, mut step: impl FnMut(&mut Chat<Surface>, usize)) {
      let handle = self.chat.tree.profiling_handle();
      let session = handle
        .start_recording(RecordingOptions::default())
        .expect("a profiling session")
        .id;
      for frame in 0..frames {
        step(&mut self.chat, frame);
      }
      // GPU times arrive a frame or more after their pass.
      let report = (0..8)
        .map(|_| {
          std::thread::sleep(Duration::from_millis(4));
          handle.read(session).expect("the session's report")
        })
        .last()
        .unwrap();
      handle.end(session).expect("the session ends");
      let passes: Vec<&PassSample> = report
        .samples
        .iter()
        .filter_map(|sample| match &sample.data {
          SampleData::Pass(pass) if pass.rendered => Some(pass),
          _ => None,
        })
        .collect();
      assert_eq!(passes.len(), frames, "{scenario}: one rendered pass per step");
      let us = |duration: Duration| duration.as_secs_f64() * 1_000_000.0;
      for (index, pass) in passes.iter().enumerate() {
        let frame = pass.frame.clone().unwrap_or_default();
        let render = frame.render;
        write!(self.frames, "{},{scenario},{index}", self.backend).unwrap();
        for duration in [
          pass.total,
          frame.layout,
          frame.quad_resolve,
          frame.glyph_rasterize,
          frame.gpu_submit,
          render.acquire,
          render.atlas_upload,
          render.buffer_upload,
          render.encode,
          render.submit,
          render.present,
        ] {
          write!(self.frames, ",{:.3}", us(duration)).unwrap();
        }
        let gpu = pass.gpu.map(|gpu| format!("{:.3}", us(gpu.ui))).unwrap_or_default();
        writeln!(
          self.frames,
          ",{gpu},{},{},{},{},{},{}",
          render.draw_calls,
          frame.glyph_count,
          frame.rect_count,
          render.glyph_atlas_upload_bytes,
          render.glyph_atlas_upload_rects,
          render.glyph_atlas_full_uploads,
        )
        .unwrap();
      }
      self.frames.flush().unwrap();
    }

    /// Draws the screen once more and keeps its pixels as `name`.
    fn capture(&mut self, name: &str) {
      let (width, height) = self.chat.physical_size();
      // A frame the renderer gives up on takes its capture with it: asked
      // again, as the readback tests do.
      let frame = (0..10)
        .find_map(|_| {
          let (sender, receiver) = mpsc::channel();
          let target = RenderCaptureTarget::Bytes(Arc::new(move |frame| {
            let _ = sender.send(frame);
          }));
          *self.pending.lock().unwrap() = Some((width, height, target));
          self.chat.pass();
          receiver.recv_timeout(Duration::from_secs(15)).ok()?.ok()
        })
        .unwrap_or_else(|| {
          panic!(
            "{}: no frame for the capture of {name}; is the system composing windows?",
            self.backend
          )
        });
      let png = self.directory.join(format!("{name}-{}.png", self.backend));
      image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
        .expect("the capture's pixels")
        .save(&png)
        .expect("the capture's PNG");
      writeln!(
        self.captures,
        "{},{name},{},{},{:016x}",
        self.backend,
        frame.width,
        frame.height,
        hash(&frame.rgba)
      )
      .unwrap();
      self.captures.flush().unwrap();
      self.captured.push((name.to_owned(), frame));
    }
  }

  fn run_backend(backend: &'static str, directory: &Path, append: bool) -> Vec<(String, CapturedFrame)> {
    let surface = Surface::hidden();
    let mut probe = Probe::new(backend, CHANNEL_MESSAGES, surface, directory, append);
    probe.measure("cold_first_frame", 1, |chat, _| {
      chat.pass();
    });
    probe.measure("cold_second_frame", 1, |chat, _| {
      chat.pass();
    });
    probe.chat.warm();
    probe.capture("settled");
    probe.measure("idle_frame", FRAMES, |chat, _| chat.idle());
    probe.measure("hover", FRAMES, |chat, _| chat.hover());
    probe.capture("hovered");
    probe.chat.focus_input();
    probe.measure("typing", FRAMES, |chat, _| chat.type_key());
    // A focused input blinks its caret: the capture is of the text alone.
    probe
      .chat
      .tree
      .get_element_by_id_mut(DRAFT_ID)
      .expect("the input")
      .blur();
    probe.chat.pass();
    probe.capture("typed");
    probe.measure("switch_channel_new", FRAMES, |chat, _| {
      let props = chat.new_channel();
      chat.show(props);
    });
    probe.capture("switched");
    probe.measure("resize_width", FRAMES, |chat, _| chat.resize());
    probe.capture("resized");
    probe.chat.resize_to(VIEWPORT.0);
    probe.measure("scale_factor_first_150", 1, |chat, _| chat.set_scale(1.5));
    probe.capture("scale_150");
    probe.chat.set_scale(1.0);
    probe.measure("scale_factor_toggle", FRAMES, |chat, frame| {
      chat.set_scale(if frame % 2 == 0 { 1.5 } else { 1.0 })
    });
    probe.capture("scale_100_again");
    let mut captured = std::mem::take(&mut probe.captured);
    drop(probe);
    surface.destroy();

    let surface = Surface::hidden();
    let mut probe = Probe::new(backend, SCROLL_MESSAGES, surface, directory, true);
    probe.chat.warm();
    probe.measure("scroll_10k", SCROLL_FRAMES, |chat, _| chat.scroll());
    probe.capture("scrolled");
    captured.append(&mut probe.captured);
    drop(probe);
    surface.destroy();
    captured
  }

  pub fn run(directory: &Path, backends: &[String]) {
    let runs = |backend: &str| backends.is_empty() || backends.iter().any(|name| name == backend);
    std::fs::create_dir_all(directory).expect("the output directory");
    std::fs::write(
      directory.join("frames.csv"),
      "backend,scenario,frame,pass_us,layout_us,quads_us,glyphs_us,render_us,acquire_us,atlas_upload_us,\
       buffer_upload_us,encode_us,submit_us,present_us,gpu_ui_us,draw_calls,glyph_count,rect_count,\
       atlas_upload_bytes,atlas_upload_rects,atlas_full_uploads\n",
    )
    .expect("frames.csv");
    std::fs::write(directory.join("captures.csv"), "backend,capture,width,height,fnv1a\n").expect("captures.csv");
    let wgpu = if runs("wgpu") {
      run_backend("wgpu", directory, true)
    } else {
      Vec::new()
    };
    let dx12 = if runs("dx12") {
      run_backend("dx12", directory, true)
    } else {
      Vec::new()
    };

    // The two backends draw the same frame.
    for ((name, wgpu), (_, dx12)) in wgpu.iter().zip(&dx12) {
      assert_eq!((wgpu.width, wgpu.height), (dx12.width, dx12.height), "{name}: sizes");
      let mut worst = 0u8;
      let mut over_two = 0usize;
      for (a, b) in wgpu.rgba.chunks_exact(4).zip(dx12.rgba.chunks_exact(4)) {
        let diff = a.iter().zip(b).take(3).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        worst = worst.max(diff);
        over_two += usize::from(diff > 2);
      }
      println!("{name}: wgpu vs dx12 worst channel difference {worst}, {over_two} pixels differ by more than 2");
      assert!(over_two == 0, "{name}: the backends disagree by {worst}");
    }
    println!(
      "text chat GPU probe: {} wgpu and {} dx12 captures in {}",
      wgpu.len(),
      dx12.len(),
      directory.display()
    );
  }
}

#[cfg(windows)]
fn main() {
  let mut arguments = std::env::args().skip(1);
  let directory = arguments
    .next()
    .map(std::path::PathBuf::from)
    .unwrap_or_else(|| "target/text-chat-gpu".into());
  probe::run(&directory, &arguments.collect::<Vec<_>>());
}

#[cfg(not(windows))]
fn main() {
  eprintln!("This probe uses a Win32 surface.");
}
