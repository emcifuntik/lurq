//! Collector-owned live slots and per-session application history.
use std::{collections::VecDeque, sync::Arc, time::Instant};

use super::{
  ProfilingHandle,
  application::{ApplicationScope, ApplicationScopeParent},
  application_model::*,
  collector::{Session, State},
};

pub(super) struct ApplicationState {
  next_id: u64,
  live: [Option<LiveScope>; MAX_APPLICATION_LIVE_SCOPES],
}

impl Default for ApplicationState {
  fn default() -> Self {
    Self {
      next_id: 1,
      live: std::array::from_fn(|_| None),
    }
  }
}

struct LiveScope {
  info: ApplicationScopeInfo,
  started: Instant,
  devtools: bool,
}

/// No history allocation until an eligible operation completes.
#[derive(Default)]
pub(super) struct ApplicationSession {
  samples: VecDeque<Arc<ApplicationScopeSample>>,
  started: u64,
  completed: u64,
  dropped: u64,
  excluded: u64,
  abandoned_window: u64,
  abandoned_producer: u64,
  refused: ApplicationRefusals,
}

impl ProfilingHandle {
  pub(super) fn begin_application_scope(
    &self,
    window: Option<&str>,
    parent: Option<&ApplicationScopeParent>,
    label: &'static str,
    lane: ApplicationLane,
  ) -> ApplicationScope {
    let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    let admission = (|| {
      if state.closed {
        return Err(ApplicationScopeStatus::CollectorClosed);
      }
      if !valid_label(label) {
        return Err(ApplicationScopeStatus::InvalidLabel);
      }
      let (window, devtools, parent_id, depth) = if let Some(parent) = parent {
        if !Arc::ptr_eq(&self.inner, &parent.handle.inner) {
          return Err(ApplicationScopeStatus::InvalidParent);
        }
        let live = state
          .application
          .live
          .iter()
          .flatten()
          .find(|live| live.info.id == parent.id)
          .ok_or(ApplicationScopeStatus::InvalidParent)?;
        (
          live.info.window.clone(),
          live.devtools,
          Some(parent.id),
          live
            .info
            .depth
            .checked_add(1)
            .ok_or(ApplicationScopeStatus::InvalidParent)?,
        )
      } else {
        let window = state
          .windows
          .get(window.unwrap_or_default())
          .filter(|window| window.status.open)
          .ok_or(ApplicationScopeStatus::WindowUnavailable)?;
        (window.scope_identity.clone(), window.status.devtools, None, 0)
      };
      let slot = state
        .application
        .live
        .iter()
        .position(Option::is_none)
        .ok_or(ApplicationScopeStatus::LiveLimit)?;
      let id = ApplicationScopeId(state.application.next_id);
      state.application.next_id = id.0.checked_add(1).ok_or(ApplicationScopeStatus::LiveLimit)?;
      let started = Instant::now();
      state.application.live[slot] = Some(LiveScope {
        info: ApplicationScopeInfo {
          id,
          parent_id,
          depth,
          window,
          label,
          lane,
          started_ms: self.millis(started),
        },
        started,
        devtools,
      });
      for session in state.sessions.values_mut() {
        if !devtools || session.options.include_devtools {
          session.application.started = session.application.started.saturating_add(1);
        }
      }
      Ok(id)
    })();
    match admission {
      Ok(id) => ApplicationScope::active(self.clone(), id),
      Err(status) => {
        state.refuse_application(
          status,
          window,
          parent.filter(|parent| Arc::ptr_eq(&self.inner, &parent.handle.inner)),
        );
        ApplicationScope::inert(status)
      }
    }
  }

  pub(super) fn finish_application_scope(&self, id: ApplicationScopeId) {
    let completed = Instant::now();
    let mut state = self.inner.state.lock().unwrap_or_else(|error| error.into_inner());
    let Some(slot) = state
      .application
      .live
      .iter()
      .position(|live| live.as_ref().is_some_and(|live| live.info.id == id))
    else {
      return; // Closed/abandoned or already finished; never resurrect old slots.
    };
    let live = state.application.live[slot].take().unwrap();
    let mut sample = None;
    for session in state.sessions.values_mut() {
      if (live.devtools && !session.options.include_devtools) || !session.receiving() {
        continue;
      }
      if live.started < session.started {
        session.application.excluded = session.application.excluded.saturating_add(1);
        continue;
      }
      let sample = sample.get_or_insert_with(|| {
        Arc::new(ApplicationScopeSample {
          scope: live.info.clone(),
          completed_ms: self.millis(completed),
          wall: completed.saturating_duration_since(live.started),
        })
      });
      let app = &mut session.application;
      app.completed = app.completed.saturating_add(1);
      let limit = session.options.max_samples.min(MAX_APPLICATION_SCOPES_PER_SESSION);
      if app.samples.len() == limit {
        app.samples.pop_front();
        app.dropped = app.dropped.saturating_add(1);
      }
      app.samples.push_back(sample.clone());
    }
  }
}

impl State {
  fn refuse_application(
    &mut self,
    status: ApplicationScopeStatus,
    window: Option<&str>,
    parent: Option<&ApplicationScopeParent>,
  ) {
    let devtools = window
      .and_then(|id| self.windows.get(id))
      .map(|window| window.status.devtools)
      .or_else(|| {
        parent.and_then(|parent| {
          self
            .application
            .live
            .iter()
            .flatten()
            .find(|live| live.info.id == parent.id)
            .map(|live| live.devtools)
        })
      })
      .unwrap_or(false);
    for session in self.sessions.values_mut() {
      if devtools && !session.options.include_devtools {
        continue;
      }
      let refused = &mut session.application.refused;
      let counter = match status {
        ApplicationScopeStatus::InvalidLabel => &mut refused.invalid_label,
        ApplicationScopeStatus::WindowUnavailable => &mut refused.window_unavailable,
        ApplicationScopeStatus::CollectorClosed => &mut refused.collector_closed,
        ApplicationScopeStatus::LiveLimit => &mut refused.live_limit,
        ApplicationScopeStatus::InvalidParent => &mut refused.invalid_parent,
        _ => return,
      };
      *counter = counter.saturating_add(1);
    }
  }

  pub(super) fn abandon_application(&mut self, window: Option<&str>) {
    for slot in &mut self.application.live {
      if slot
        .as_ref()
        .is_none_or(|live| window.is_some_and(|id| id != live.info.window.as_ref()))
      {
        continue;
      }
      let live = slot.take().unwrap();
      for session in self.sessions.values_mut() {
        if live.devtools && !session.options.include_devtools {
          continue;
        }
        let app = &mut session.application;
        if live.started < session.started {
          app.excluded = app.excluded.saturating_add(1);
        }
        let abandoned = if window.is_some() {
          &mut app.abandoned_window
        } else {
          &mut app.abandoned_producer
        };
        *abandoned = abandoned.saturating_add(1);
      }
    }
  }

  #[cfg(feature = "devtools")]
  pub(super) fn application_mark_devtools(&mut self, window: &str) {
    for live in self.application.live.iter_mut().flatten() {
      if live.info.window.as_ref() == window {
        live.devtools = true;
      }
    }
  }

  pub(super) fn application_report(&self, session: &Session, now: Instant) -> ApplicationScopeReport {
    let app = &session.application;
    let mut in_flight: Vec<_> = self
      .application
      .live
      .iter()
      .flatten()
      .filter(|live| !live.devtools || session.options.include_devtools)
      .map(|live| ApplicationInFlight {
        scope: live.info.clone(),
        elapsed_ms: now.saturating_duration_since(live.started).as_secs_f64() * 1000.,
        started_before_session: live.started < session.started,
      })
      .collect();
    in_flight.sort_by_key(|live| live.scope.id.0);
    ApplicationScopeReport {
      available: true,
      max_live_scopes: MAX_APPLICATION_LIVE_SCOPES,
      max_samples: session.options.max_samples.min(MAX_APPLICATION_SCOPES_PER_SESSION),
      started_scopes: app.started,
      completed_scopes: app.completed,
      dropped_scopes: app.dropped,
      boundary_excluded_scopes: app.excluded,
      abandoned_window_closed: app.abandoned_window,
      abandoned_producer_closed: app.abandoned_producer,
      refused: app.refused,
      in_flight,
      samples: app.samples.iter().cloned().collect(),
    }
  }
}
