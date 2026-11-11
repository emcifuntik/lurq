//! Bounded synchronous application spans, independent of UI frame samples.
use std::{sync::Arc, time::Duration};

pub const MAX_APPLICATION_LIVE_SCOPES: usize = 64;
pub const MAX_APPLICATION_SCOPES_PER_SESSION: usize = 128;
pub const MAX_APPLICATION_LABEL_BYTES: usize = 64;

/// Application-declared execution lane; this is not a measured OS thread ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ApplicationLane {
  Ui,
  Worker,
}

impl ApplicationLane {
  pub fn name(self) -> &'static str {
    match self {
      Self::Ui => "ui",
      Self::Worker => "worker",
    }
  }
}

/// Collector-local identity. IDs are never recycled within that collector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplicationScopeId(pub u64);

/// Refused instrumentation is inert and must not change application behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplicationScopeStatus {
  Active,
  FeatureDisabled,
  InvalidLabel,
  WindowUnavailable,
  CollectorClosed,
  LiveLimit,
  InvalidParent,
}

impl ApplicationScopeStatus {
  pub fn name(self) -> &'static str {
    match self {
      Self::Active => "active",
      Self::FeatureDisabled => "feature_disabled",
      Self::InvalidLabel => "invalid_label",
      Self::WindowUnavailable => "window_unavailable",
      Self::CollectorClosed => "collector_closed",
      Self::LiveLimit => "live_limit",
      Self::InvalidParent => "invalid_parent",
    }
  }
}

#[derive(Clone, Debug)]
pub struct ApplicationScopeInfo {
  pub id: ApplicationScopeId,
  pub parent_id: Option<ApplicationScopeId>,
  /// Explicit parent-chain depth, independent of other concurrent scopes.
  pub depth: u32,
  /// Registered toolkit identity, never an application document name or path.
  pub window: Arc<str>,
  /// Static content-free ASCII identifier supplied by application source.
  pub label: &'static str,
  pub lane: ApplicationLane,
  pub started_ms: f64,
}

#[derive(Clone, Debug)]
pub struct ApplicationScopeSample {
  pub scope: ApplicationScopeInfo,
  pub completed_ms: f64,
  /// Inclusive synchronous wall time, including lock/I/O waits; not thread CPU.
  pub wall: Duration,
}

#[derive(Clone, Debug)]
pub struct ApplicationInFlight {
  pub scope: ApplicationScopeInfo,
  /// Unfinished elapsed time, never a completed or partial-stage duration.
  pub elapsed_ms: f64,
  pub started_before_session: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ApplicationRefusals {
  pub invalid_label: u64,
  pub window_unavailable: u64,
  pub collector_closed: u64,
  pub live_limit: u64,
  pub invalid_parent: u64,
}

#[derive(Clone, Debug)]
pub struct ApplicationScopeReport {
  pub available: bool,
  pub max_live_scopes: usize,
  pub max_samples: usize,
  pub started_scopes: u64,
  pub completed_scopes: u64,
  pub dropped_scopes: u64,
  pub boundary_excluded_scopes: u64,
  pub abandoned_window_closed: u64,
  pub abandoned_producer_closed: u64,
  pub refused: ApplicationRefusals,
  pub in_flight: Vec<ApplicationInFlight>,
  pub samples: Vec<Arc<ApplicationScopeSample>>,
}

#[cfg(feature = "perf_profile")]
pub(super) fn valid_label(label: &str) -> bool {
  !label.is_empty()
    && label.len() <= MAX_APPLICATION_LABEL_BYTES
    && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
