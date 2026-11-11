//! Content-free profiling contract shared by MCP and DevTools consumers.
//!
//! A recording holds phase names, times, counts and toolkit window ids. A
//! detailed recording (`RecordingOptions::detailed`) adds static type names:
//! the short type names of components and of the signals, memos and contexts
//! that made them render, taken from `std::any::type_name`. Type names are
//! allowed; values never are: no text, prop values, keys, element or signal
//! ids, input values or paths are recorded.
use std::{sync::Arc, time::Duration};

use super::{ApplicationScopeReport, FrameProfile};
use crate::app::PassReasons;

pub const MAX_ACTIVE_SESSIONS: usize = 8;
pub const MAX_SAMPLES_PER_SESSION: usize = 240;
pub const MAX_TRACKED_WINDOWS: usize = 64;
pub const MAX_WINDOW_ID_BYTES: usize = 256;
pub const MAX_ENDED_SESSION_IDS: usize = 64;
/// Phase spans one pass keeps; later ones are counted in `PassSample::dropped_spans`.
pub const MAX_SPANS_PER_PASS: usize = 1024;
/// Component spans one pass of a detailed recording keeps, besides its phase
/// spans; later ones are counted in `PassSample::dropped_spans`.
pub const MAX_COMPONENT_SPANS_PER_PASS: usize = 8192;
/// Dispatched inputs a window holds until a frame presents them; an older one
/// is recorded as presented by none.
pub const MAX_PENDING_INPUTS: usize = 256;
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
  /// Also records, per component, a span for each render and `after_layout`
  /// hook (`PhaseSpan::component`), the nodes each pass laid out again
  /// (`PassSample::component_layouts`) and what made each update's
  /// components render (`UiUpdateSample::causes`). Off, a pass pays one
  /// flag check for it.
  pub detailed: bool,
  /// Some takes screenshots of presented frames; None (the default) reads
  /// no frame back.
  pub screenshots: Option<ScreenshotOptions>,
}

impl Default for RecordingOptions {
  fn default() -> Self {
    Self {
      max_bytes: DEFAULT_RECORDING_MAX_BYTES,
      include_devtools: false,
      detailed: false,
      screenshots: None,
    }
  }
}

/// Screenshots a recording takes of presented frames. Each one is read back
/// from the GPU after its frame's present, never waited for by the frame,
/// downscaled to fit `max_width` by `max_height`, with sensitive text painted
/// over as in an inspector's screenshot. Its bytes count toward `max_bytes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenshotOptions {
  /// The most screenshots taken in a second, across the handle's windows; 0
  /// counts as 1.
  pub max_per_second: u32,
  pub max_width: u32,
  pub max_height: u32,
}

impl Default for ScreenshotOptions {
  fn default() -> Self {
    Self {
      max_per_second: 4,
      max_width: 320,
      max_height: 200,
    }
  }
}

impl ScreenshotOptions {
  /// The shortest time between two screenshots.
  pub fn min_interval(&self) -> Duration {
    Duration::from_secs(1) / self.max_per_second.max(1)
  }
}

/// A downscaled image of one presented frame, taken for a recording with
/// `RecordingOptions::screenshots`. Holds pixels only: sensitive text in it is
/// a grey bar.
#[derive(Clone)]
pub struct FrameScreenshot {
  pub window: String,
  /// The presented frame it shows: its pass sample's `PassSample::frame_id`.
  pub frame_id: u64,
  /// When the frame's render began, which asked for it.
  pub captured_ms: f64,
  pub width: u32,
  pub height: u32,
  /// Tight RGBA8 rows, `width` by `height`.
  pub rgba: Vec<u8>,
}

/// The state of a recording session; None in a bounded session's report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecordingStatus {
  pub max_bytes: u64,
  /// Estimated memory the session's samples and screenshots hold, counted
  /// per session.
  pub size_bytes: u64,
  /// True once a sample or screenshot would have passed `max_bytes`: the
  /// session keeps what it had and records nothing more.
  pub stopped_at_limit: bool,
  pub stopped_ms: Option<f64>,
  /// `RecordingOptions::detailed` of the session.
  pub detailed: bool,
  /// Screenshots kept, and the part of `size_bytes` they hold.
  pub screenshots: u64,
  pub screenshot_bytes: u64,
}

/// A live session's progress without its samples (`ProfilingHandle::status`),
/// cheap enough to read on every redraw of a recording's status line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionStatus {
  pub id: SessionId,
  pub started_ms: f64,
  pub observed_ms: f64,
  pub completed_samples: u64,
  /// Some for a recording session.
  pub recording: Option<RecordingStatus>,
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
  /// Available when a window's wgpu adapter offers timestamp queries; in a
  /// report, from the windows the session sees.
  pub gpu_timestamps: GpuTimestampAvailability,
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
      gpu_timestamps: GpuTimestampAvailability::default(),
    }
  }
}

/// Whether GPU time is measured, as a window's renderer reported it for its
/// adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuTimestampAvailability {
  pub available: bool,
  /// Why GPU time is not measured; None when it is.
  pub reason: Option<&'static str>,
  /// The wgpu backend of the adapter ("vulkan", "metal", "dx12", "gl", ...);
  /// None before a wgpu adapter reported.
  pub adapter_backend: Option<&'static str>,
}

impl GpuTimestampAvailability {
  #[cfg_attr(not(all(feature = "wgpu", feature = "perf_profile")), allow(dead_code))]
  pub(crate) const NO_TIMESTAMP_QUERY: &'static str = "the wgpu adapter does not offer timestamp queries";
  #[cfg_attr(
    not(all(feature = "dx12", target_os = "windows", feature = "perf_profile")),
    allow(dead_code)
  )]
  pub(crate) const NATIVE_DX12: &'static str = "the native DX12 renderer does not measure GPU time";
  const NO_PERF_PROFILE: &'static str = "GPU timestamp queries require the perf_profile Cargo feature";
  const NO_WGPU: &'static str = "only the wgpu renderer measures GPU time";
  const NOT_RENDERED: &'static str = "no window has rendered with wgpu yet";
  /// Every `reason` of any build, which a recording file reads back.
  #[cfg_attr(not(any(feature = "serde", feature = "mcp", feature = "devtools")), allow(dead_code))]
  pub(crate) const REASONS: [&'static str; 5] = [
    Self::NO_TIMESTAMP_QUERY,
    Self::NATIVE_DX12,
    Self::NO_PERF_PROFILE,
    Self::NO_WGPU,
    Self::NOT_RENDERED,
  ];

  pub(crate) fn unavailable(reason: &'static str) -> Self {
    Self {
      available: false,
      reason: Some(reason),
      adapter_backend: None,
    }
  }
}

impl Default for GpuTimestampAvailability {
  /// Before any renderer reported: why this build cannot measure, or that no
  /// wgpu frame has rendered yet.
  fn default() -> Self {
    Self::unavailable(if !cfg!(feature = "perf_profile") {
      Self::NO_PERF_PROFILE
    } else if !cfg!(feature = "wgpu") {
      Self::NO_WGPU
    } else {
      Self::NOT_RENDERED
    })
  }
}

/// GPU execution time of one presented frame, from wgpu timestamp queries.
/// Arrives one or more frames after its pass; a pass whose result never
/// arrived keeps `PassSample::gpu` None.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuTiming {
  /// The UI encoder: start of its first render pass (an opacity layer, or the
  /// window) to the end of the window pass.
  pub ui: Duration,
  /// The Canvas renderer's command buffers this frame, each from its start to
  /// its end, summed (render passes and tile copies; no explicit readbacks).
  /// None when the frame submitted no Canvas work or not all of it was measured.
  pub canvas: Option<Duration>,
}

impl GpuTiming {
  pub fn total(&self) -> Duration {
    self.ui + self.canvas.unwrap_or_default()
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
  /// As the window's renderer reported it for its adapter; None before it did.
  pub gpu_timestamps: Option<GpuTimestampAvailability>,
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

/// One input event, from when the shell received it to the first frame
/// presented after it. Its sample starts when the shell received it. Holds
/// the kind only: never a pointer position, key, code or text.
#[derive(Clone)]
pub struct InputDispatchSample {
  /// The pass the dispatch nested in; None for an event dispatched between passes.
  pub frame_id: Option<u64>,
  pub kind: InputKind,
  /// The dispatch: from the tree's handler entry to its return.
  pub total: Duration,
  /// Numbers the inputs of one profiling handle from 1, in dispatch order.
  pub interaction_id: u64,
  /// From when the shell received the event to its dispatch. Zero for input
  /// the shell did not stamp (synthetic input, a tree driven without a shell).
  pub queue_delay: Duration,
  /// The first frame presented after the dispatch; None when none followed:
  /// the event asked for no frame, or the session ended first.
  pub presented_frame_id: Option<u64>,
  /// From when the shell received the event to when that frame's present
  /// call returned (CPU time; see `PresentTiming`).
  pub input_to_present: Option<Duration>,
}

/// When a presented frame showed, against the window's frame before it.
/// `presented_ms` is when the backend's present call returned on the CPU: no
/// backend reports when the display scanned the frame out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresentTiming {
  pub presented_ms: f64,
  /// Since the window's previous presented frame; None for its first.
  pub interval: Option<Duration>,
  /// The refresh of the window's display as its shell last read it; None
  /// without a shell that reports it.
  pub refresh_interval: Option<Duration>,
  /// The frame was wanted within one refresh of the previous present (a
  /// running animation, a drag, a scroll): it continues a run of frames.
  /// False after an idle gap, which is not a dropped frame.
  pub continuous: bool,
  /// Refreshes the frame missed, counted from when it was wanted, or from the
  /// previous present when wanted before it; 0 outside a continuous run.
  pub missed_vsyncs: u32,
}

impl PresentTiming {
  pub fn missed_vsync(&self) -> bool {
    self.missed_vsyncs > 0
  }
}

/// The present of a window's last frame, to time the next one against.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PresentTracker {
  last: Option<std::time::Instant>,
}

impl PresentTracker {
  /// Times a frame presented at `presented`, first wanted at `wanted`, on a
  /// display refreshing every `refresh`. A frame continues a run when it was
  /// wanted no later than a refresh after the previous present; it missed
  /// vsyncs when it presented over half a refresh after the refresh it was due.
  pub(crate) fn present(
    &mut self,
    presented: std::time::Instant,
    wanted: Option<std::time::Instant>,
    refresh: Option<Duration>,
  ) -> (Option<Duration>, bool, u32) {
    let previous = self.last.replace(presented);
    let interval = previous.map(|previous| presented.saturating_duration_since(previous));
    let (Some(previous), Some(wanted), Some(refresh)) = (previous, wanted, refresh) else {
      return (interval, false, 0);
    };
    if refresh.is_zero() || wanted > previous + refresh {
      return (interval, false, 0);
    }
    let late = presented.saturating_duration_since(wanted.max(previous));
    let refreshes = (late.as_secs_f64() / refresh.as_secs_f64()).round();
    (interval, true, (refreshes as u32).saturating_sub(1))
  }
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
  /// Why the pass ran (`PassReport::reasons`).
  pub reasons: PassReasons,
  /// Some for a presented pass.
  pub present: Option<PresentTiming>,
  /// Sizes taken at the end of the pass; None when the pass started with no
  /// session active.
  pub counters: Option<PassCounters>,
  /// The frame was copied for a recording's screenshot (`FrameScreenshot`):
  /// its render, encode and submit include that copy, so it is slower than
  /// the same frame without one.
  pub screenshot: bool,
  /// GPU time of this frame, matched by window and frame id when its query
  /// results arrive, frames later; None until then, or for good when they never do.
  pub gpu: Option<GpuTiming>,
  /// In a detailed recording, the nodes the pass laid out again (without
  /// their layout cache), per nearest component and reason, most first.
  /// Empty otherwise.
  pub component_layouts: Vec<ComponentLayouts>,
}

/// One phase guard's lifetime, in nanoseconds since the collector epoch (the
/// origin of every `*_ms` value in a report).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhaseSpan {
  pub phase: Phase,
  pub depth: u8,
  pub start_ns: u64,
  pub end_ns: u64,
  /// In a detailed recording, the component a span of `Phase::Rebuild` (its
  /// render) or `Phase::ComponentAfterLayout` (its hook) belongs to: its
  /// short static type name. None for the phases themselves.
  pub component: Option<&'static str>,
}

/// Nodes one pass laid out again under one component, for one reason (the
/// reasons of `LURQ_LAYOUT_TRACE`, such as `local-dirty`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComponentLayouts {
  /// The short type name of the nearest component at or above the nodes;
  /// None for nodes outside any mounted component (the root's own output).
  pub component: Option<&'static str>,
  pub reason: &'static str,
  pub nodes: u32,
}

/// What a component render was for, as a detailed recording saw it happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RenderTrigger {
  /// A signal the component created or read in its last render was written.
  Signal,
  /// A memo the component read recomputed to a new value.
  Memo,
  /// A store the component created was written.
  Store,
  /// A reactive context it uses changed, or a context value it read was provided anew.
  Context,
  /// A form it created changed.
  Form,
  /// Its parent rendered it with other props.
  Props,
  /// Its parent rendered it with slot children, which always render again.
  SlotChildren,
  /// Its first render, or its first since it came back from offstage.
  Mount,
  /// A `for_each` item, which renders with its list.
  ListItem,
  /// Its parent asked for a root rebuild, or no notification was seen (one
  /// sent before the recording started).
  Unknown,
}

impl RenderTrigger {
  pub fn name(self) -> &'static str {
    match self {
      Self::Signal => "signal",
      Self::Memo => "memo",
      Self::Store => "store",
      Self::Context => "context",
      Self::Form => "form",
      Self::Props => "props",
      Self::SlotChildren => "slot_children",
      Self::Mount => "mount",
      Self::ListItem => "list_item",
      Self::Unknown => "unknown",
    }
  }
}

/// A reactive source as a cause names it: its kind and the short static type
/// name of its value (`Signal<Vec<Row>>` is `signal Vec`). Never its value or id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RenderSource {
  pub trigger: RenderTrigger,
  pub value_type: Option<&'static str>,
}

/// One cause of a component render in an update.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RenderCause {
  /// The short type name of the component that rendered.
  pub component: &'static str,
  pub source: RenderSource,
  /// For a memo, the notification that made it recompute, when it recomputed
  /// while that notification ran.
  pub via: Option<RenderSource>,
}

/// Counts and sizes of a captured pass, taken when it ends. Draw calls, quads,
/// rects, glyphs and Canvas batches of a presented frame are in its
/// `FrameProfile` (`render.draw_calls`, `quad_count`, ..., `render.canvas.batches`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassCounters {
  /// Nodes of the window's live tree.
  pub nodes: usize,
  /// The runtime memory estimate (`RuntimeMemoryProfile::total_bytes`), taken
  /// at the end of this pass rather than the cached once-per-second value.
  pub memory_bytes: usize,
  pub caches: CacheSizes,
  /// The pass thread's allocations during the pass; None when no
  /// `CountingAllocator` is installed (or lurq lacks its feature).
  pub allocations: Option<PassAllocations>,
}

/// Sizes of the caches a pass draws from, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheSizes {
  /// The shared glyph atlas's RGBA pixels (CPU copy; each window's renderer
  /// holds a GPU texture of the same size).
  pub glyph_atlas_bytes: usize,
  /// The UI GlyphEngine's estimate of its text caches and buffers.
  pub text_cache_bytes: usize,
  /// Charged bytes of the shared Canvas text engine's shape cache; 0 before
  /// any Canvas drew text.
  pub canvas_text_cache_bytes: usize,
  /// Charged bytes of the textures this window's renderer keeps for Canvas
  /// images and text, as of its last presented frame.
  pub canvas_asset_cache_bytes: usize,
}

/// What the pass thread allocated during one pass, counted by an installed
/// `CountingAllocator`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassAllocations {
  pub allocations: u64,
  pub frees: u64,
  pub allocated_bytes: u64,
  pub freed_bytes: u64,
  /// Bytes allocated and not yet freed by every thread, at the end of the pass.
  pub live_bytes: u64,
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
  /// In a detailed recording, why each component the update rendered did
  /// render, in render order, each distinct cause once; the notifications
  /// were recorded as the sources sent them. Empty otherwise.
  pub causes: Vec<RenderCause>,
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
  /// A recording's screenshots, in the order they arrived; empty without
  /// `RecordingOptions::screenshots`.
  pub screenshots: Vec<Arc<FrameScreenshot>>,
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
