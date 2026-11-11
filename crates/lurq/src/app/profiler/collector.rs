use std::{
  collections::{HashMap, VecDeque},
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
  },
  time::Instant,
};

#[cfg(feature = "perf_profile")]
use super::application_state::{ApplicationSession, ApplicationState};
use super::{model::*, phase_slot::PhaseSlot};

/// Shared capture API. No UI references, callbacks, redraws or per-node data.
#[derive(Clone)]
pub struct ProfilingHandle {
  pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
  pub(crate) epoch: Instant,
  pub(crate) active: AtomicBool,
  pub(super) state: Mutex<State>,
}

pub(super) struct State {
  next_session: u64,
  next_sequence: u64,
  pub(super) closed: bool,
  pub(super) sessions: HashMap<SessionId, Session>,
  ended: VecDeque<SessionId>,
  pub(super) windows: HashMap<String, WindowState>,
  #[cfg(feature = "perf_profile")]
  pub(super) application: ApplicationState,
  untracked_windows: u64,
}

pub(super) struct Session {
  pub(super) started: Instant,
  pub(super) options: SessionOptions,
  #[cfg(feature = "perf_profile")]
  pub(super) application: ApplicationSession,
  samples: VecDeque<Arc<ProfileSample>>,
  completed: u64,
  dropped: u64,
  excluded: u64,
}

pub(super) struct WindowState {
  pub(super) status: WindowStatus,
  #[cfg(feature = "perf_profile")]
  pub(super) scope_identity: Arc<str>,
  /// Shared with the window's producer, which publishes phases without this lock.
  phase: Arc<PhaseSlot>,
}

impl Default for ProfilingHandle {
  fn default() -> Self {
    Self::new()
  }
}

impl ProfilingHandle {
  pub fn new() -> Self {
    Self {
      inner: Arc::new(Inner {
        epoch: Instant::now(),
        active: AtomicBool::new(false),
        state: Mutex::new(State {
          next_session: 1,
          next_sequence: 1,
          closed: false,
          sessions: HashMap::new(),
          ended: VecDeque::new(),
          windows: HashMap::new(),
          untracked_windows: 0,
          #[cfg(feature = "perf_profile")]
          application: ApplicationState::default(),
        }),
      }),
    }
  }

  /// Starts an independent session. This does not request a frame.
  pub fn start(&self, options: SessionOptions) -> Result<SessionStarted, ProfileError> {
    if !cfg!(feature = "perf_profile") {
      return Err(ProfileError::FeatureDisabled);
    }
    if !(1..=MAX_SAMPLES_PER_SESSION).contains(&options.max_samples) {
      return Err(ProfileError::InvalidSampleLimit);
    }
    let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    if state.closed {
      return Err(ProfileError::Closed);
    }
    if state.sessions.len() >= MAX_ACTIVE_SESSIONS {
      return Err(ProfileError::SessionLimit);
    }
    let id = SessionId(state.next_session);
    state.next_session = state.next_session.checked_add(1).ok_or(ProfileError::SessionLimit)?;
    let started = Instant::now();
    state.sessions.insert(
      id,
      Session {
        started,
        options,
        samples: VecDeque::with_capacity(options.max_samples),
        completed: 0,
        dropped: 0,
        excluded: 0,
        #[cfg(feature = "perf_profile")]
        application: ApplicationSession::default(),
      },
    );
    self.inner.active.store(true, Ordering::Release);
    Ok(SessionStarted {
      id,
      started_ms: self.millis(started),
      max_samples: options.max_samples,
      build: BuildAvailability::default(),
    })
  }

  /// Returns immutable owned data without waiting for an event-loop pass.
  pub fn read(&self, id: SessionId) -> Result<ProfileReport, ProfileError> {
    let state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    let session = state.sessions.get(&id).ok_or_else(|| missing(&state, id))?;
    Ok(self.report(&state, id, session, Instant::now(), false))
  }

  /// Finalizes only this session. Later producer completions cannot change it.
  pub fn end(&self, id: SessionId) -> Result<ProfileReport, ProfileError> {
    let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    let session = state.sessions.remove(&id).ok_or_else(|| missing(&state, id))?;
    let report = self.report(&state, id, &session, Instant::now(), true);
    state.ended.push_back(id);
    while state.ended.len() > MAX_ENDED_SESSION_IDS {
      state.ended.pop_front();
    }
    self.inner.active.store(!state.sessions.is_empty(), Ordering::Release);
    Ok(report)
  }

  fn report(&self, state: &State, id: SessionId, session: &Session, now: Instant, finalized: bool) -> ProfileReport {
    let visible = |window: &WindowState| session.options.include_devtools || !window.status.devtools;
    let mut windows: Vec<_> = state
      .windows
      .values()
      .filter(|window| visible(window))
      .map(|window| window.status.clone())
      .collect();
    windows.sort_by(|a, b| a.id.cmp(&b.id));
    let mut in_flight: Vec<_> = state
      .windows
      .values()
      .filter(|window| visible(window) && window.status.open)
      .filter_map(|window| {
        window.phase.read().map(|progress| InFlightObservation {
          window: window.status.id.clone(),
          frame_id: progress.frame_id,
          phase: progress.phase,
          started_ms: self.millis(progress.started),
          elapsed_ms: now.saturating_duration_since(progress.started).as_secs_f64() * 1000.,
          phase_elapsed_ms: now.saturating_duration_since(progress.phase_started).as_secs_f64() * 1000.,
          started_before_session: progress.started < session.started,
        })
      })
      .collect();
    in_flight.sort_by(|a, b| a.window.cmp(&b.window));
    ProfileReport {
      id,
      finalized,
      started_ms: self.millis(session.started),
      ended_ms: finalized.then(|| self.millis(now)),
      observed_ms: self.millis(now),
      build: BuildAvailability::default(),
      max_samples: session.options.max_samples,
      completed_samples: session.completed,
      dropped_samples: session.dropped,
      boundary_excluded_samples: session.excluded,
      untracked_windows: state.untracked_windows,
      windows,
      in_flight,
      samples: session.samples.iter().cloned().collect(),
      application_scopes: {
        #[cfg(feature = "perf_profile")]
        {
          Some(state.application_report(session, now))
        }
        #[cfg(not(feature = "perf_profile"))]
        {
          None
        }
      },
    }
  }

  pub(crate) fn millis(&self, instant: Instant) -> f64 {
    instant.saturating_duration_since(self.inner.epoch).as_secs_f64() * 1000.
  }

  /// Tracks a window and returns the slot its phases are published in; None
  /// when the window is not tracked, whose phases are then not observable.
  pub(crate) fn register(&self, id: &str, devtools: bool) -> Option<Arc<PhaseSlot>> {
    let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    if id.len() > MAX_WINDOW_ID_BYTES {
      state.untracked_windows = state.untracked_windows.saturating_add(1);
      return None;
    }
    // Retain only bounded closed-window metadata; sample identities remain owned.
    if !state.windows.contains_key(id) && state.windows.len() >= MAX_TRACKED_WINDOWS {
      let oldest_closed = state
        .windows
        .iter()
        .find(|(_, window)| !window.status.open)
        .map(|(id, _)| id.clone());
      if let Some(id) = oldest_closed {
        state.windows.remove(&id);
      } else {
        state.untracked_windows = state.untracked_windows.saturating_add(1);
        return None;
      }
    }
    #[cfg(feature = "perf_profile")]
    state.abandon_application(Some(id));
    // A closed producer publishes nothing, as before any window registered.
    let phase = Arc::new(PhaseSlot::new(self.inner.epoch, !state.closed));
    let replaced = state.windows.insert(
      id.into(),
      WindowState {
        status: WindowStatus {
          id: id.into(),
          open: true,
          devtools,
        },
        phase: phase.clone(),
        #[cfg(feature = "perf_profile")]
        scope_identity: id.into(),
      },
    );
    // A producer still holding the replaced slot can no longer be observed.
    if let Some(replaced) = replaced {
      replaced.phase.set_open(false);
    }
    Some(phase)
  }

  pub(crate) fn set_open(&self, id: &str, open: bool) {
    let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    #[cfg(feature = "perf_profile")]
    if !open {
      state.abandon_application(Some(id));
    }
    let closed = state.closed;
    if let Some(window) = state.windows.get_mut(id) {
      window.status.open = open;
      window.phase.set_open(open && !closed);
    }
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn mark_devtools(&self, id: &str) {
    let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    #[cfg(feature = "perf_profile")]
    state.application_mark_devtools(id);
    if let Some(window) = state.windows.get_mut(id) {
      window.status.devtools = true;
    }
  }

  pub(crate) fn record(&self, window: &str, started: Instant, data: SampleData) {
    if !self.inner.active.load(Ordering::Acquire) {
      return;
    }
    let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    let Some(metadata) = state.windows.get(window) else {
      return;
    };
    if !metadata.status.open {
      return;
    }
    let devtools = metadata.status.devtools;
    let completed = Instant::now();
    let sequence = state.next_sequence;
    state.next_sequence = state.next_sequence.saturating_add(1);
    let sample = Arc::new(ProfileSample {
      sequence,
      window: window.into(),
      started_ms: self.millis(started),
      completed_ms: self.millis(completed),
      data,
    });
    for session in state.sessions.values_mut() {
      if devtools && !session.options.include_devtools {
        continue;
      }
      if started < session.started {
        session.excluded += 1;
        continue;
      }
      session.completed += 1;
      if session.samples.len() == session.options.max_samples {
        session.samples.pop_front();
        session.dropped += 1;
      }
      session.samples.push_back(sample.clone());
    }
  }

  pub(crate) fn close(&self) {
    let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    #[cfg(feature = "perf_profile")]
    state.abandon_application(None);
    state.closed = true;
    for window in state.windows.values_mut() {
      window.status.open = false;
      window.phase.set_open(false);
    }
    // Sessions can still be ended/read through surviving handles.
  }
}

fn missing(state: &State, id: SessionId) -> ProfileError {
  if state.ended.contains(&id) {
    ProfileError::AlreadyEnded
  } else {
    ProfileError::UnknownSession
  }
}
