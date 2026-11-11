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
  /// The id of the next dispatched input (`InputDispatchSample::interaction_id`).
  #[cfg(feature = "perf_profile")]
  next_interaction: u64,
  untracked_windows: u64,
}

pub(super) struct Session {
  pub(super) started: Instant,
  /// A recording's `max_samples` is `usize::MAX`: only its application
  /// scopes are bounded, by `MAX_APPLICATION_SCOPES_PER_SESSION`.
  pub(super) options: SessionOptions,
  #[cfg(feature = "perf_profile")]
  pub(super) application: ApplicationSession,
  samples: Samples,
  completed: u64,
  dropped: u64,
  excluded: u64,
}

enum Samples {
  /// Drops its oldest sample at `SessionOptions::max_samples`.
  Bounded(VecDeque<Arc<ProfileSample>>),
  Recording(Recording),
}

/// Samples a recording keeps per chunk: a long recording grows by whole
/// chunks and never moves the samples it already holds.
const RECORDING_CHUNK: usize = 4096;

struct Recording {
  chunks: Vec<Vec<Arc<ProfileSample>>>,
  size_bytes: u64,
  max_bytes: u64,
  /// When a sample would have passed `max_bytes`; nothing is recorded after.
  stopped_at_limit: Option<Instant>,
}

impl Session {
  /// False for a recording stopped at its limit, which takes no more samples.
  pub(super) fn receiving(&self) -> bool {
    !matches!(&self.samples, Samples::Recording(recording) if recording.stopped_at_limit.is_some())
  }

  /// Keeps `sample`, of `bytes`, unless it began before the session. True
  /// when a recording stopped at its limit instead.
  fn take(&mut self, sample: &Arc<ProfileSample>, bytes: u64, started: Instant, completed: Instant) -> bool {
    if started < self.started {
      self.excluded += 1;
      return false;
    }
    match &mut self.samples {
      Samples::Bounded(samples) => {
        if samples.len() == self.options.max_samples {
          samples.pop_front();
          self.dropped += 1;
        }
        samples.push_back(sample.clone());
      }
      Samples::Recording(recording) => {
        if recording.size_bytes.saturating_add(bytes) > recording.max_bytes {
          recording.stopped_at_limit = Some(completed);
          return true;
        }
        if recording
          .chunks
          .last()
          .is_none_or(|chunk| chunk.len() == RECORDING_CHUNK)
        {
          recording.chunks.push(Vec::with_capacity(RECORDING_CHUNK));
        }
        recording.chunks.last_mut().unwrap().push(sample.clone());
        recording.size_bytes += bytes;
      }
    }
    self.completed += 1;
    false
  }
}

/// An input a window dispatched, before the frame that presents it.
#[cfg(feature = "perf_profile")]
pub(crate) struct DispatchedInput {
  pub(crate) kind: InputKind,
  /// The pass the dispatch nested in, if any.
  pub(crate) frame_id: Option<u64>,
  /// When the shell received the event; the dispatch start when unstamped.
  pub(crate) received: Instant,
  pub(crate) dispatch_started: Instant,
  pub(crate) dispatch: std::time::Duration,
}

#[cfg(feature = "perf_profile")]
pub(super) struct PendingInput {
  interaction_id: u64,
  input: DispatchedInput,
}

#[cfg(feature = "perf_profile")]
impl PendingInput {
  /// The input's sample, starting when it was received, as presented by
  /// `presented` (a frame id and its present).
  fn sample(&self, presented: Option<(u64, Instant)>) -> (Instant, SampleData) {
    let input = &self.input;
    (
      input.received,
      SampleData::InputDispatch(InputDispatchSample {
        frame_id: input.frame_id,
        kind: input.kind,
        total: input.dispatch,
        interaction_id: self.interaction_id,
        queue_delay: input.dispatch_started.saturating_duration_since(input.received),
        presented_frame_id: presented.map(|(frame_id, _)| frame_id),
        input_to_present: presented.map(|(_, at)| at.saturating_duration_since(input.received)),
      }),
    )
  }
}

pub(super) struct WindowState {
  pub(super) status: WindowStatus,
  #[cfg(feature = "perf_profile")]
  pub(super) scope_identity: Arc<str>,
  /// Shared with the window's producer, which publishes phases without this lock.
  phase: Arc<PhaseSlot>,
  /// Inputs dispatched since the window's last presented frame, oldest first.
  #[cfg(feature = "perf_profile")]
  pending_inputs: VecDeque<PendingInput>,
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
          #[cfg(feature = "perf_profile")]
          next_interaction: 1,
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
    let started = self.insert(options, Samples::Bounded(VecDeque::with_capacity(options.max_samples)))?;
    Ok(SessionStarted {
      max_samples: options.max_samples,
      ..started
    })
  }

  /// Starts a recording session (DevTools): it keeps every sample from now
  /// until `end`, and stops by itself, with `RecordingStatus::stopped_at_limit`,
  /// before its size would pass `options.max_bytes`. Counts against
  /// `MAX_ACTIVE_SESSIONS` like any session. This does not request a frame.
  pub fn start_recording(&self, options: RecordingOptions) -> Result<SessionStarted, ProfileError> {
    if !cfg!(feature = "perf_profile") {
      return Err(ProfileError::FeatureDisabled);
    }
    if options.max_bytes == 0 {
      return Err(ProfileError::InvalidByteLimit);
    }
    let started = self.insert(
      SessionOptions {
        max_samples: usize::MAX,
        include_devtools: options.include_devtools,
      },
      Samples::Recording(Recording {
        chunks: Vec::new(),
        size_bytes: 0,
        max_bytes: options.max_bytes,
        stopped_at_limit: None,
      }),
    )?;
    Ok(SessionStarted {
      recording_max_bytes: Some(options.max_bytes),
      ..started
    })
  }

  fn insert(&self, options: SessionOptions, samples: Samples) -> Result<SessionStarted, ProfileError> {
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
        samples,
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
      max_samples: 0,
      build: BuildAvailability::default(),
      recording_max_bytes: None,
    })
  }

  /// Returns immutable owned data without waiting for an event-loop pass.
  pub fn read(&self, id: SessionId) -> Result<ProfileReport, ProfileError> {
    let state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    let session = state.sessions.get(&id).ok_or_else(|| missing(&state, id))?;
    Ok(self.report(&state, id, session, Instant::now(), false))
  }

  /// A live session's progress, without copying its samples as `read` does.
  pub fn status(&self, id: SessionId) -> Result<SessionStatus, ProfileError> {
    let state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    let session = state.sessions.get(&id).ok_or_else(|| missing(&state, id))?;
    Ok(SessionStatus {
      id,
      started_ms: self.millis(session.started),
      observed_ms: self.millis(Instant::now()),
      completed_samples: session.completed,
      recording: self.recording_status(session),
    })
  }

  fn recording_status(&self, session: &Session) -> Option<RecordingStatus> {
    match &session.samples {
      Samples::Bounded(_) => None,
      Samples::Recording(recording) => Some(RecordingStatus {
        max_bytes: recording.max_bytes,
        size_bytes: recording.size_bytes,
        stopped_at_limit: recording.stopped_at_limit.is_some(),
        stopped_ms: recording.stopped_at_limit.map(|stopped| self.millis(stopped)),
      }),
    }
  }

  /// Finalizes only this session. Later producer completions cannot change it.
  /// Inputs still waiting for a frame end in it presented by none.
  pub fn end(&self, id: SessionId) -> Result<ProfileReport, ProfileError> {
    let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    #[cfg_attr(not(feature = "perf_profile"), allow(unused_mut))]
    let mut session = state.sessions.remove(&id).ok_or_else(|| missing(&state, id))?;
    #[cfg(feature = "perf_profile")]
    self.end_pending_inputs(&mut state, &mut session);
    let report = self.report(&state, id, &session, Instant::now(), true);
    state.ended.push_back(id);
    while state.ended.len() > MAX_ENDED_SESSION_IDS {
      state.ended.pop_front();
    }
    self.update_active(&state);
    Ok(report)
  }

  /// Passes are captured (spans kept, samples made) only while a session
  /// takes samples; a recording stopped at its limit takes none.
  fn update_active(&self, state: &State) {
    let active = state.sessions.values().any(Session::receiving);
    self.inner.active.store(active, Ordering::Release);
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
    let (max_samples, samples) = match &session.samples {
      Samples::Bounded(samples) => (session.options.max_samples, samples.iter().cloned().collect()),
      Samples::Recording(recording) => (0, recording.chunks.iter().flatten().cloned().collect()),
    };
    let recording = self.recording_status(session);
    ProfileReport {
      id,
      finalized,
      started_ms: self.millis(session.started),
      ended_ms: finalized.then(|| self.millis(now)),
      observed_ms: self.millis(now),
      build: BuildAvailability::default(),
      max_samples,
      completed_samples: session.completed,
      dropped_samples: session.dropped,
      boundary_excluded_samples: session.excluded,
      untracked_windows: state.untracked_windows,
      windows,
      in_flight,
      samples,
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
      recording,
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
        #[cfg(feature = "perf_profile")]
        pending_inputs: VecDeque::new(),
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
      // A closing window presents nothing more: its inputs still waiting
      // for a frame end presented by none, while it is still open to record.
      let pending = state
        .windows
        .get_mut(id)
        .map(|window| std::mem::take(&mut window.pending_inputs))
        .unwrap_or_default();
      if self.inner.active.load(Ordering::Acquire) {
        for input in &pending {
          let (started, data) = input.sample(None);
          self.record_locked(&mut state, id, started, data);
        }
      }
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
    self.record_locked(&mut state, window, started, data);
  }

  fn record_locked(&self, state: &mut State, window: &str, started: Instant, data: SampleData) {
    let Some(metadata) = state.windows.get(window) else {
      return;
    };
    if !metadata.status.open {
      return;
    }
    let devtools = metadata.status.devtools;
    let completed = Instant::now();
    let sample = self.sample(state, window, started, completed, data);
    let bytes = retained_bytes(&sample);
    let mut stopped = false;
    for session in state.sessions.values_mut() {
      if (devtools && !session.options.include_devtools) || !session.receiving() {
        continue;
      }
      stopped |= session.take(&sample, bytes, started, completed);
    }
    if stopped {
      self.update_active(state);
    }
  }

  fn sample(
    &self,
    state: &mut State,
    window: &str,
    started: Instant,
    completed: Instant,
    data: SampleData,
  ) -> Arc<ProfileSample> {
    let sequence = state.next_sequence;
    state.next_sequence = state.next_sequence.saturating_add(1);
    Arc::new(ProfileSample {
      sequence,
      window: window.into(),
      started_ms: self.millis(started),
      completed_ms: self.millis(completed),
      data,
    })
  }

  /// Takes an input `window` dispatched. One that asked for no frame is
  /// recorded at once, presented by none; the others wait in the window for
  /// its next presented frame (`inputs_presented`).
  #[cfg(feature = "perf_profile")]
  pub(crate) fn input_dispatched(&self, window: &str, input: DispatchedInput, wants_frame: bool) {
    if !self.inner.active.load(Ordering::Acquire) {
      return;
    }
    let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    let interaction_id = state.next_interaction;
    let Some(metadata) = state.windows.get_mut(window) else {
      return;
    };
    if !metadata.status.open {
      return;
    }
    let input = PendingInput { interaction_id, input };
    let ended = if !wants_frame {
      Some(input)
    } else {
      let oldest = (metadata.pending_inputs.len() >= MAX_PENDING_INPUTS)
        .then(|| metadata.pending_inputs.pop_front())
        .flatten();
      metadata.pending_inputs.push_back(input);
      oldest
    };
    state.next_interaction = state.next_interaction.saturating_add(1);
    if let Some(ended) = ended {
      let (started, data) = ended.sample(None);
      self.record_locked(&mut state, window, started, data);
    }
  }

  /// Records the inputs `window` dispatched since its last presented frame,
  /// as presented by `presented` (its frame id and when its present returned);
  /// None ends them presented by none.
  #[cfg(feature = "perf_profile")]
  pub(crate) fn inputs_presented(&self, window: &str, presented: Option<(u64, Instant)>) {
    if !self.inner.active.load(Ordering::Acquire) {
      return;
    }
    let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    let Some(metadata) = state.windows.get_mut(window) else {
      return;
    };
    if metadata.pending_inputs.is_empty() {
      return;
    }
    let mut pending = std::mem::take(&mut metadata.pending_inputs);
    for input in pending.drain(..) {
      let (started, data) = input.sample(presented);
      self.record_locked(&mut state, window, started, data);
    }
    // Keeps the queue's allocation for the next frame's inputs.
    if let Some(metadata) = state.windows.get_mut(window) {
      metadata.pending_inputs = pending;
    }
  }

  /// Ends the inputs still waiting for a frame in the session `end` took out,
  /// presented by none; forgets them once no session takes samples.
  #[cfg(feature = "perf_profile")]
  fn end_pending_inputs(&self, state: &mut State, session: &mut Session) {
    let completed = Instant::now();
    let mut ended = Vec::new();
    for window in state.windows.values() {
      if !window.status.open || (window.status.devtools && !session.options.include_devtools) {
        continue;
      }
      for input in &window.pending_inputs {
        let (started, data) = input.sample(None);
        ended.push((window.status.id.clone(), started, data));
      }
    }
    for (window, started, data) in ended {
      if !session.receiving() {
        break;
      }
      let sample = self.sample(state, &window, started, completed, data);
      session.take(&sample, retained_bytes(&sample), started, completed);
    }
    if !state.sessions.values().any(Session::receiving) {
      for window in state.windows.values_mut() {
        window.pending_inputs.clear();
      }
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

/// What keeping `sample` costs a recording: the shared sample with its Arc
/// counts, its heap parts and the session's pointer to it.
pub(super) fn retained_bytes(sample: &ProfileSample) -> u64 {
  let spans = match &sample.data {
    SampleData::Pass(pass) => pass.spans.capacity() * std::mem::size_of::<PhaseSpan>(),
    SampleData::UiUpdate(_) | SampleData::InputDispatch(_) => 0,
  };
  let bytes = 2 * std::mem::size_of::<usize>()
    + std::mem::size_of::<ProfileSample>()
    + sample.window.capacity()
    + spans
    + std::mem::size_of::<Arc<ProfileSample>>();
  u64::try_from(bytes).unwrap_or(u64::MAX)
}
