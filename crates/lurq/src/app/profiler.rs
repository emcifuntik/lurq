pub use crate::app::profile_types::{
  CanvasAssetUploadProfile, CanvasProfile, FrameProfile, GlyphEngineProfile, LayerProfile, RenderProfile,
  RuntimeMemoryProfile,
};

mod aggregate;
#[cfg(test)]
mod aggregate_tests;
mod application;
#[cfg(any(feature = "serde", feature = "mcp"))]
mod application_export;
mod application_model;
#[cfg(feature = "perf_profile")]
mod application_state;
#[cfg(test)]
mod application_tests;
#[cfg(all(feature = "canvas", feature = "perf_profile"))]
pub(crate) mod canvas_text;
#[cfg(test)]
mod canvas_upload_tests;
mod collector;
#[cfg(all(test, feature = "perf_profile"))]
mod counter_tests;
#[cfg(feature = "counting_allocator")]
mod counting_allocator;
pub(crate) mod detail;
#[cfg(all(test, feature = "perf_profile"))]
mod detail_tests;
#[cfg(any(feature = "serde", feature = "mcp"))]
mod export;
#[cfg(any(feature = "serde", feature = "mcp", feature = "devtools"))]
mod firefox_export;
#[cfg(all(test, any(feature = "serde", feature = "mcp", feature = "devtools")))]
mod firefox_export_tests;
#[cfg(all(test, feature = "perf_profile"))]
mod latency_tests;
mod model;
mod phase_slot;
#[cfg(all(test, feature = "perf_profile"))]
mod phase_tests;
pub(crate) mod producer;
#[cfg(any(feature = "serde", feature = "mcp", feature = "devtools"))]
mod recording_file;
#[cfg(all(test, any(feature = "serde", feature = "mcp", feature = "devtools")))]
mod recording_file_tests;
#[cfg(all(feature = "screenshot", feature = "perf_profile"))]
pub(crate) mod screenshot;
#[cfg(all(test, feature = "perf_profile", feature = "screenshot"))]
mod screenshot_tests;
#[cfg(test)]
mod session_tests;

pub use aggregate::{
  CallNode, CallSort, CallTree, ForestSpan, PASS_SPAN_NAME, SpanForest, SpanTrack, forest_of, ms_to_ns, span_name,
};
pub use application::{ApplicationScope, ApplicationScopeParent};
pub use application_model::{
  ApplicationInFlight, ApplicationLane, ApplicationRefusals, ApplicationScopeId, ApplicationScopeInfo,
  ApplicationScopeReport, ApplicationScopeSample, ApplicationScopeStatus, MAX_APPLICATION_LABEL_BYTES,
  MAX_APPLICATION_LIVE_SCOPES, MAX_APPLICATION_SCOPES_PER_SESSION,
};
pub use collector::ProfilingHandle;
#[cfg(feature = "counting_allocator")]
pub use counting_allocator::CountingAllocator;
pub use model::{
  BuildAvailability, CacheSizes, CanvasTextProfile, ComponentLayouts, DEFAULT_RECORDING_MAX_BYTES, FrameScreenshot,
  GpuTimestampAvailability, GpuTiming, InFlightObservation, InputDispatchSample, InputKind, MAX_ACTIVE_SESSIONS,
  MAX_COMPONENT_SPANS_PER_PASS, MAX_ENDED_SESSION_IDS, MAX_PENDING_INPUTS, MAX_SAMPLES_PER_SESSION, MAX_SPANS_PER_PASS,
  MAX_TRACKED_WINDOWS, MAX_WINDOW_ID_BYTES, PassAllocations, PassCounters, PassSample, Phase, PhaseSpan, PresentTiming,
  ProfileError, ProfileReport, ProfileSample, RecordingOptions, RecordingStatus, RenderCause, RenderSource,
  RenderTrigger, SampleData, ScreenshotOptions, SessionId, SessionOptions, SessionStarted, SessionStatus, UiUpdateKind,
  UiUpdateSample, WindowStatus,
};
pub use producer::{PhaseGuard, ProfileContext};
#[cfg(any(feature = "serde", feature = "mcp", feature = "devtools"))]
pub use {
  firefox_export::FIREFOX_PROCESSED_PROFILE_VERSION,
  recording_file::{RECORDING_FILE_FORMAT, RECORDING_FILE_VERSION, RecordingFileError},
};

/// A thread's allocation counts since it started, and live bytes then.
#[cfg(feature = "perf_profile")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AllocationSnapshot {
  pub allocations: u64,
  pub frees: u64,
  pub allocated_bytes: u64,
  pub freed_bytes: u64,
  pub live_bytes: u64,
}

#[cfg(feature = "perf_profile")]
impl AllocationSnapshot {
  /// What the thread did from `earlier` to `self`, with live bytes at `self`.
  pub(crate) fn since(self, earlier: Self) -> PassAllocations {
    PassAllocations {
      allocations: self.allocations.wrapping_sub(earlier.allocations),
      frees: self.frees.wrapping_sub(earlier.frees),
      allocated_bytes: self.allocated_bytes.wrapping_sub(earlier.allocated_bytes),
      freed_bytes: self.freed_bytes.wrapping_sub(earlier.freed_bytes),
      live_bytes: self.live_bytes,
    }
  }
}

/// The calling thread's allocation counts, or None when no `CountingAllocator`
/// is installed or lurq was built without its feature.
#[cfg(feature = "perf_profile")]
pub(crate) fn allocation_snapshot() -> Option<AllocationSnapshot> {
  #[cfg(feature = "counting_allocator")]
  {
    counting_allocator::snapshot()
  }
  #[cfg(not(feature = "counting_allocator"))]
  {
    None
  }
}

#[cfg(feature = "perf_profile")]
mod observer {
  use std::sync::OnceLock;

  use super::FrameProfile;

  type FrameProfileObserver = Box<dyn Fn(&FrameProfile) + Send + Sync>;

  static OBSERVER: OnceLock<FrameProfileObserver> = OnceLock::new();

  /// Registers a process-wide observer invoked with every completed frame
  /// profile. Hosts embedding external renderers use this to fold the UI
  /// frame's layout/raster/present timings into their own profilers. Only the
  /// first registration wins; later calls are ignored.
  pub fn set_frame_profile_observer(observer: impl Fn(&FrameProfile) + Send + Sync + 'static) {
    let _ = OBSERVER.set(Box::new(observer));
  }

  pub(crate) fn notify_frame_profile(profile: &FrameProfile) {
    if let Some(observer) = OBSERVER.get() {
      observer(profile);
    }
  }
}

#[cfg(feature = "perf_profile")]
pub(crate) use observer::notify_frame_profile;
#[cfg(feature = "perf_profile")]
pub use observer::set_frame_profile_observer;

#[cfg(all(test, feature = "perf_profile"))]
mod tests {
  use std::{
    sync::{
      Arc,
      atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
  };

  use super::FrameProfile;

  #[test]
  fn frame_profile_observer_receives_completed_frames() {
    let seen = Arc::new(AtomicUsize::new(0));
    let seen_by_observer = seen.clone();
    super::set_frame_profile_observer(move |profile| {
      if profile.layout == Duration::from_millis(3) {
        seen_by_observer.fetch_add(1, Ordering::SeqCst);
      }
    });

    let profile = FrameProfile {
      layout: Duration::from_millis(3),
      ..FrameProfile::default()
    };
    super::notify_frame_profile(&profile);
    super::notify_frame_profile(&profile);

    assert_eq!(seen.load(Ordering::SeqCst), 2);
  }
}
