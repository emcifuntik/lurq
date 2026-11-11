pub use crate::app::profile_types::{
  CanvasAssetUploadProfile, CanvasProfile, FrameProfile, GlyphEngineProfile, RenderProfile, RuntimeMemoryProfile,
};

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
#[cfg(any(feature = "serde", feature = "mcp"))]
mod export;
#[cfg(all(test, feature = "perf_profile"))]
mod latency_tests;
mod model;
mod phase_slot;
#[cfg(all(test, feature = "perf_profile"))]
mod phase_tests;
pub(crate) mod producer;
#[cfg(test)]
mod session_tests;

pub use application::{ApplicationScope, ApplicationScopeParent};
pub use application_model::{
  ApplicationInFlight, ApplicationLane, ApplicationRefusals, ApplicationScopeId, ApplicationScopeInfo,
  ApplicationScopeReport, ApplicationScopeSample, ApplicationScopeStatus, MAX_APPLICATION_LABEL_BYTES,
  MAX_APPLICATION_LIVE_SCOPES, MAX_APPLICATION_SCOPES_PER_SESSION,
};
pub use collector::ProfilingHandle;
pub use model::{
  BuildAvailability, CanvasTextProfile, DEFAULT_RECORDING_MAX_BYTES, InFlightObservation, InputDispatchSample,
  InputKind, MAX_ACTIVE_SESSIONS, MAX_ENDED_SESSION_IDS, MAX_PENDING_INPUTS, MAX_SAMPLES_PER_SESSION,
  MAX_SPANS_PER_PASS, MAX_TRACKED_WINDOWS, MAX_WINDOW_ID_BYTES, PassSample, Phase, PhaseSpan, PresentTiming,
  ProfileError, ProfileReport, ProfileSample, RecordingOptions, RecordingStatus, SampleData, SessionId, SessionOptions,
  SessionStarted, SessionStatus, UiUpdateKind, UiUpdateSample, WindowStatus,
};
pub use producer::{PhaseGuard, ProfileContext};

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
