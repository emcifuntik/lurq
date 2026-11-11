//! Content-free profiling contract shared by MCP and future DevTools consumers.
use std::{sync::Arc, time::Duration};

use super::{ApplicationScopeReport, FrameProfile};

pub const MAX_ACTIVE_SESSIONS: usize = 8;
pub const MAX_SAMPLES_PER_SESSION: usize = 240;
pub const MAX_TRACKED_WINDOWS: usize = 64;
pub const MAX_WINDOW_ID_BYTES: usize = 256;
pub const MAX_ENDED_SESSION_IDS: usize = 64;
/// Phase spans one pass keeps; later ones are counted in `PassSample::dropped_spans`.
pub const MAX_SPANS_PER_PASS: usize = 1024;
/// Default ceiling of a recording session: 1 GiB.
pub const DEFAULT_RECORDING_MAX_BYTES: u64 = 1 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

#[derive(Clone, Copy, Debug)]
pub struct SessionOptions {
  /// Oldest samples are discarded when this bound is reached.
  pub max_samples: usize,
  pub include_devtools: bool,
}

impl Default for SessionOptions {
  fn default() -> Self {
    Self {
      max_samples: 120,
      include_devtools: false,
    }
  }
}

/// A DevTools recording: runs from `start_recording` to `end` and keeps every
/// sample, until its retained size would pass `max_bytes`; it then stops with
/// `RecordingStatus::stopped_at_limit` instead of dropping samples.
#[derive(Clone, Copy, Debug)]
pub struct RecordingOptions {
  pub max_bytes: u64,
  pub include_devtools: bool,
}

impl Default for RecordingOptions {
  fn default() -> Self {
    Self {
      max_bytes: DEFAULT_RECORDING_MAX_BYTES,
      include_devtools: false,
    }
  }
}

/// The state of a recording session; None in a bounded session's report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecordingStatus {
  pub max_bytes: u64,
  /// Estimated memory the session's samples hold, counted per session.
  pub size_bytes: u64,
  /// True once a sample would have passed `max_bytes`: the session keeps what
  /// it had and records nothing more.
  pub stopped_at_limit: bool,
  pub stopped_ms: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileError {
  FeatureDisabled,
  SessionLimit,
  InvalidSampleLimit,
  /// A recording session's `max_bytes` is zero.
  InvalidByteLimit,
  UnknownSession,
  AlreadyEnded,
  Closed,
}

impl std::fmt::Display for ProfileError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    if *self == Self::InvalidSampleLimit {
      return write!(f, "max_samples must be between 1 and {MAX_SAMPLES_PER_SESSION}");
    }
    f.write_str(match self {
      Self::FeatureDisabled => "profiling requires the perf_profile Cargo feature",
      Self::SessionLimit => "the concurrent profiling session limit is reached",
      Self::InvalidSampleLimit => unreachable!("handled above"),
      Self::InvalidByteLimit => "max_bytes must be above zero",
      Self::UnknownSession => "unknown profiling session id",
      Self::AlreadyEnded => "profiling session already ended",
      Self::Closed => "profiling producer has closed",
    })
  }
}

impl std::error::Error for ProfileError {}

#[derive(Clone, Copy, Debug)]
pub struct BuildAvailability {
  pub perf_profile: bool,
  pub debug_assertions: bool,
  pub canvas: bool,
  pub wgpu: bool,
  pub dx12: bool,
  pub devtools: bool,
}

impl Default for BuildAvailability {
  fn default() -> Self {
    Self {
      perf_profile: cfg!(feature = "perf_profile"),
      debug_assertions: cfg!(debug_assertions),
      canvas: cfg!(feature = "canvas"),
      wgpu: cfg!(feature = "wgpu"),
      dx12: cfg!(all(feature = "dx12", target_os = "windows")),
      devtools: cfg!(feature = "devtools"),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
  PassSetup,
  PassNotifications,
  LayoutUpdate,
  LayoutCompute,
  ComponentAfterLayout,
  Rebuild,
  Commit,
  QuadResolve,
  GlyphRasterize,
  CanvasRecording,
  CanvasPreparation,
  Render,
  CanvasBackend,
  CanvasTessellation,
  CanvasAssetUpload,
  CanvasBufferUpload,
  CanvasSubmission,
  CanvasCommands,
  RenderInit,
  RenderAcquire,
  RenderEncode,
  RenderSubmit,
  RenderPresent,
  InputDispatch,
}

impl Phase {
  pub fn name(self) -> &'static str {
    match self {
      Self::PassSetup => "pass_setup",
      Self::PassNotifications => "pass_notifications",
      Self::LayoutUpdate => "layout_update",
      Self::LayoutCompute => "layout_compute",
      Self::ComponentAfterLayout => "component_after_layout",
      Self::Rebuild => "ui_rebuild",
      Self::Commit => "ui_commit",
      Self::QuadResolve => "quad_resolve",
      Self::GlyphRasterize => "glyph_rasterize",
      Self::CanvasRecording => "canvas_recording",
      Self::CanvasPreparation => "canvas_preparation",
      Self::Render => "render",
      Self::CanvasBackend => "canvas_backend",
      Self::CanvasTessellation => "canvas_tessellation",
      Self::CanvasAssetUpload => "canvas_asset_upload",
      Self::CanvasBufferUpload => "canvas_buffer_upload",
      Self::CanvasSubmission => "canvas_submission",
      Self::CanvasCommands => "canvas_commands",
      Self::RenderInit => "render_init",
      Self::RenderAcquire => "render_acquire",
      Self::RenderEncode => "render_encode",
      Self::RenderSubmit => "render_submit",
      Self::RenderPresent => "render_present",
      Self::InputDispatch => "input_dispatch",
    }
  }
}

#[derive(Clone, Debug)]
pub struct WindowStatus {
  /// Toolkit identity only; never a document path, title or user-supplied name.
  pub id: String,
  pub open: bool,
  pub devtools: bool,
}

#[derive(Clone, Debug)]
pub struct InFlightObservation {
  pub window: String,
  pub frame_id: Option<u64>,
  pub phase: Phase,
  pub started_ms: f64,
  pub elapsed_ms: f64,
  pub phase_elapsed_ms: f64,
  pub started_before_session: bool,
}

#[derive(Clone)]
pub struct ProfileSample {
  pub sequence: u64,
  pub window: String,
  pub started_ms: f64,
  pub completed_ms: f64,
  pub data: SampleData,
}

#[derive(Clone)]
pub enum SampleData {
  Pass(PassSample),
  UiUpdate(UiUpdateSample),
  InputDispatch(InputDispatchSample),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
  Pointer,
  Keyboard,
  Scroll,
}

impl InputKind {
  pub fn name(self) -> &'static str {
    match self {
      Self::Pointer => "pointer",
      Self::Keyboard => "keyboard",
      Self::Scroll => "scroll",
    }
  }
}

#[derive(Clone)]
pub struct InputDispatchSample {
  pub frame_id: Option<u64>,
  pub kind: InputKind,
  pub total: Duration,
}

#[derive(Clone)]
pub struct PassSample {
  pub frame_id: Option<u64>,
  pub rendered: bool,
  pub cached_render_list: bool,
  /// Full Tree::pass wall time; excludes input/event dispatch before the pass.
  pub total: Duration,
  /// Includes UI rebuild, resource work, Canvas recording and actual layout.
  pub layout_update: Duration,
  /// Aggregate runtime-owned layout compute calls; nested in layout_update.
  pub layout_compute: Duration,
  /// Entire root/recursive component hook sweep; may include app Canvas painting.
  pub component_after_layout: Duration,
  pub layout_recalculated: bool,
  /// Nested in layout_update; records bind/replay CPU work only.
  pub canvas_recording: Duration,
  /// Handle selection before render; actual Canvas backend work is render.canvas.
  pub canvas_preparation: Duration,
  /// Synchronous Canvas text calls on this pass's thread, including hook work.
  /// None when Canvas instrumentation is unavailable or this pass was not captured.
  pub canvas_text: Option<CanvasTextProfile>,
  pub backend: &'static str,
  /// Present only after a successful render; never reused from a prior frame.
  pub frame: Option<FrameProfile>,
  /// Every phase guard of the pass, the pass itself first (depth 0), ordered
  /// by start; a span's depth is its guard's nesting below the pass. Empty when
  /// the pass started with no session active.
  pub spans: Vec<PhaseSpan>,
  /// Spans past `MAX_SPANS_PER_PASS`, not kept.
  pub dropped_spans: u32,
}

/// One phase guard's lifetime, in nanoseconds since the collector epoch (the
/// origin of every `*_ms` value in a report).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhaseSpan {
  pub phase: Phase,
  pub depth: u8,
  pub start_ns: u64,
  pub end_ns: u64,
}

/// Aggregate CPU work in CanvasTextEngine, separate from the UI GlyphEngine.
#[derive(Clone, Copy, Debug, Default)]
pub struct CanvasTextProfile {
  pub measure_calls: u64,
  pub fill_calls: u64,
  pub shape_calls: u64,
  pub shape_cache_hits: u64,
  pub shape_cache_misses: u64,
  pub shape_cache_evictions: u64,
  /// Shaped results returned without being kept, because the text of the
  /// current and previous frame filled the cache's ceiling.
  pub shape_cache_uncached: u64,
  /// The most the cache was charged above its budget during the pass; above
  /// zero, the text of the current and previous frame needs more than it.
  pub shape_cache_stretch_bytes: u64,
  /// Newly produced final RGBA data length on misses; excludes cache hits,
  /// transient glyph pixmaps, the second retained pixmap and GPU uploads.
  pub produced_bitmap_bytes: u64,
  /// Inclusive shape call, including cache lookup/eviction/insertion and stages below.
  pub total: Duration,
  pub buffer_font_shape: Duration,
  /// Whole glyph loop: font metrics, cache-budget scans, Swash lookup and clones.
  pub glyph_prepare: Duration,
  /// Whole final bitmap allocation, glyph conversion/composition and RGBA copy.
  pub bitmap_composition: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiUpdateKind {
  RootRebuild,
  SubtreeRefresh,
}

#[derive(Clone)]
pub struct UiUpdateSample {
  /// Some when this update is nested in a pass; None for pre-pass event work.
  pub frame_id: Option<u64>,
  pub kind: UiUpdateKind,
  /// Complete update, including commit; do not add to an enclosing pass.
  pub total: Duration,
  pub commit: Duration,
}

#[derive(Clone)]
pub struct ProfileReport {
  pub id: SessionId,
  pub finalized: bool,
  pub started_ms: f64,
  pub ended_ms: Option<f64>,
  pub observed_ms: f64,
  pub build: BuildAvailability,
  /// 0 for a recording session, which keeps every sample.
  pub max_samples: usize,
  pub completed_samples: u64,
  pub dropped_samples: u64,
  pub boundary_excluded_samples: u64,
  pub untracked_windows: u64,
  pub windows: Vec<WindowStatus>,
  pub in_flight: Vec<InFlightObservation>,
  pub samples: Vec<Arc<ProfileSample>>,
  /// Independent application wall scopes; None when perf_profile is disabled.
  pub application_scopes: Option<ApplicationScopeReport>,
  /// Some for a recording session (`ProfilingHandle::start_recording`).
  pub recording: Option<RecordingStatus>,
}

#[derive(Clone, Debug)]
pub struct SessionStarted {
  pub id: SessionId,
  pub started_ms: f64,
  /// 0 for a recording session, which keeps every sample.
  pub max_samples: usize,
  pub build: BuildAvailability,
  /// The ceiling of a recording session; None for a bounded session.
  pub recording_max_bytes: Option<u64>,
}
