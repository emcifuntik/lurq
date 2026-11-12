use raw_window_handle::{DisplayHandle, WindowHandle};

#[cfg(feature = "perf_profile")]
use crate::app::profile_types::RenderProfile;
use crate::layout::render_list::RenderList;

/// A finished CPU-side frame capture: tightly packed RGBA8 pixels.
#[cfg(feature = "screenshot")]
pub struct CapturedFrame {
  pub width: u32,
  pub height: u32,
  pub rgba: Vec<u8>,
}

/// Where a frame capture's pixels go once read back from the GPU.
///
/// `Bytes` completes exactly once — with the pixels, or with an error when the
/// capture is dropped anywhere along the pipeline — so a caller parked on the
/// result is never left waiting. The callback runs on a readback worker
/// thread, never on the event loop.
#[cfg(feature = "screenshot")]
#[derive(Clone)]
pub enum RenderCaptureTarget {
  /// Save a PNG to this path (fire-and-forget; failures are logged).
  Path(std::path::PathBuf),
  /// Hand the raw pixels to this callback.
  Bytes(std::sync::Arc<dyn Fn(Result<CapturedFrame, String>) + Send + Sync>),
}

#[cfg(feature = "screenshot")]
impl RenderCaptureTarget {
  /// Human-readable target for log messages.
  pub fn describe(&self) -> String {
    match self {
      Self::Path(path) => path.display().to_string(),
      Self::Bytes(_) => "<in-memory capture>".to_owned(),
    }
  }

  /// Report a dropped capture: warns for file targets, completes byte
  /// targets with an error so the waiter unblocks.
  pub fn fail(&self, reason: impl Into<String>) {
    let reason = reason.into();
    match self {
      Self::Path(path) => {
        tracing::warn!("dropped frame capture to {}: {reason}", path.display());
      }
      Self::Bytes(callback) => callback(Err(reason)),
    }
  }
}

#[cfg(feature = "screenshot")]
#[derive(Clone)]
pub struct RenderFrameCapture {
  pub x: u32,
  pub y: u32,
  pub width: u32,
  pub height: u32,
  pub target: RenderCaptureTarget,
  pub window_clip: Option<RenderFrameCaptureWindowClip>,
}

#[cfg(feature = "screenshot")]
#[derive(Clone, Copy)]
pub struct RenderFrameCaptureWindowClip {
  pub width: f32,
  pub height: f32,
  pub radii: [f32; 4],
}

pub trait RenderEngine {
  /// Static toolkit backend identity; excludes document or adapter names.
  fn profile_backend(&self) -> &'static str {
    "custom"
  }
  #[cfg(feature = "perf_profile")]
  fn set_profile_context(&mut self, _context: crate::app::profiler::ProfileContext) {}
  fn resize(&mut self, width: u32, height: u32);
  /// Includes culled canvases, which still need queued drawing and readbacks.
  #[cfg(feature = "canvas")]
  fn prepare_canvases(&mut self, canvases: &[crate::canvas::CanvasHandle]) {
    for canvas in canvases {
      canvas.unsupported();
    }
  }
  fn render(&mut self, list: &RenderList, window: WindowHandle<'_>, display: DisplayHandle<'_>) -> bool;

  #[cfg(feature = "screenshot")]
  fn supports_frame_capture(&self) -> bool {
    false
  }

  #[cfg(feature = "screenshot")]
  fn render_with_capture(
    &mut self,
    list: &RenderList,
    window: WindowHandle<'_>,
    display: DisplayHandle<'_>,
    _capture: Option<RenderFrameCapture>,
  ) -> bool {
    self.render(list, window, display)
  }

  fn release_window_surface(&mut self) {}

  fn wants_redraw(&self) -> bool {
    false
  }

  #[cfg(feature = "perf_profile")]
  fn last_profile(&self) -> Option<RenderProfile> {
    None
  }

  /// Counts of the last frame's opacity layers and blurs: passes, copies,
  /// blur draws and pooled textures. None for a backend that does not count
  /// them.
  #[cfg(feature = "perf_profile")]
  fn layer_profile(&self) -> Option<crate::app::profile_types::LayerProfile> {
    None
  }
}

pub type RenderEngineFactory = std::sync::Arc<dyn Fn() -> Box<dyn RenderEngine>>;
