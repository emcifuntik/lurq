#![cfg_attr(not(feature = "perf_profile"), allow(dead_code))]
use std::{
  sync::{Arc, atomic::Ordering},
  time::{Duration, Instant},
};

use super::{
  FrameProfile, ProfilingHandle,
  model::*,
  phase_slot::{Entered, PhaseSlot},
};
use crate::app::PassReport;

/// Lightweight context passed to render backends for current-phase publication.
#[derive(Clone)]
pub struct ProfileContext {
  handle: ProfilingHandle,
  window: Arc<str>,
  /// None when the collector does not track this window.
  pub(super) phase: Option<Arc<PhaseSlot>>,
  frame_id: Option<u64>,
}

impl ProfileContext {
  #[cfg(all(feature = "canvas", feature = "perf_profile"))]
  pub(crate) fn capture_active(&self) -> bool {
    self.handle.inner.active.load(Ordering::Acquire)
  }

  #[cfg(all(feature = "canvas", feature = "perf_profile"))]
  pub(crate) fn canvas_text_scope(&self) -> super::canvas_text::PassScope {
    super::canvas_text::PassScope::new(self.capture_active())
  }
  /// Publishes `phase` in the window's slot; takes no collector lock. In a
  /// captured pass, its span is kept when the guard drops.
  pub fn phase(&self, phase: Phase) -> PhaseGuard {
    let entered = self
      .phase
      .as_ref()
      .map(|slot| slot.enter(self.frame_id, phase))
      .unwrap_or_default();
    PhaseGuard {
      slot: self.phase.clone(),
      entered,
    }
  }

  /// Detailed Canvas phases skip their per-command-group slot writes with no capture.
  #[cfg(all(
    feature = "canvas",
    any(feature = "wgpu", all(feature = "dx12", target_os = "windows"))
  ))]
  pub(crate) fn detail_phase(&self, phase: Phase) -> Option<PhaseGuard> {
    self
      .handle
      .inner
      .active
      .load(Ordering::Acquire)
      .then(|| self.phase(phase))
  }

  pub(crate) fn input(&self, kind: InputKind) -> InputGuard {
    InputGuard {
      context: self.clone(),
      kind,
      started: Instant::now(),
      _phase: self.phase(Phase::InputDispatch),
    }
  }

  pub(crate) fn update(&self, kind: UiUpdateKind) -> UpdateGuard {
    UpdateGuard {
      context: self.clone(),
      kind,
      started: Instant::now(),
      commit_started: None,
      _phase: self.phase(Phase::Rebuild),
      commit_phase: None,
    }
  }
}

pub(crate) struct InputGuard {
  context: ProfileContext,
  kind: InputKind,
  started: Instant,
  _phase: PhaseGuard,
}

impl Drop for InputGuard {
  fn drop(&mut self) {
    self.context.handle.record(
      &self.context.window,
      self.started,
      SampleData::InputDispatch(InputDispatchSample {
        frame_id: self.context.frame_id,
        kind: self.kind,
        total: self.started.elapsed(),
      }),
    );
  }
}

pub(crate) struct UpdateGuard {
  context: ProfileContext,
  kind: UiUpdateKind,
  started: Instant,
  commit_started: Option<Instant>,
  _phase: PhaseGuard,
  commit_phase: Option<PhaseGuard>,
}

impl UpdateGuard {
  pub(crate) fn commit(&mut self) {
    self.commit_started = Some(Instant::now());
    self.commit_phase = Some(self.context.phase(Phase::Commit));
  }
}

impl Drop for UpdateGuard {
  fn drop(&mut self) {
    self.context.handle.record(
      &self.context.window,
      self.started,
      SampleData::UiUpdate(UiUpdateSample {
        frame_id: self.context.frame_id,
        kind: self.kind,
        total: self.started.elapsed(),
        commit: self.commit_started.map(|start| start.elapsed()).unwrap_or_default(),
      }),
    );
    // Restore nested scopes in reverse order before the outer field drops.
    self.commit_phase.take();
  }
}

pub struct PhaseGuard {
  slot: Option<Arc<PhaseSlot>>,
  entered: Entered,
}

impl Drop for PhaseGuard {
  fn drop(&mut self) {
    if let Some(slot) = &self.slot {
      slot.exit(self.entered);
    }
  }
}

pub(crate) struct WindowProfiler {
  pub(crate) context: ProfileContext,
  root: bool,
  layout: Duration,
  layout_compute: Duration,
  component_after_layout: Duration,
  canvas_recording: Duration,
  canvas_preparation: Duration,
  /// The pass guard's entry: the root of the pass's spans.
  pass_entry: Entered,
  /// A session took samples when the pass began.
  captured: bool,
  /// The pass thread's allocation counts when a captured pass began.
  #[cfg(feature = "perf_profile")]
  allocations_start: Option<super::AllocationSnapshot>,
  counters: Option<PassCounters>,
}

impl WindowProfiler {
  pub(crate) fn new() -> Self {
    let handle = ProfilingHandle::new();
    let phase = handle.register("main", false);
    Self {
      context: ProfileContext {
        handle,
        window: "main".into(),
        phase,
        frame_id: None,
      },
      root: true,
      layout: Duration::ZERO,
      layout_compute: Duration::ZERO,
      component_after_layout: Duration::ZERO,
      canvas_recording: Duration::ZERO,
      canvas_preparation: Duration::ZERO,
      pass_entry: Entered::default(),
      captured: false,
      #[cfg(feature = "perf_profile")]
      allocations_start: None,
      counters: None,
    }
  }

  pub(crate) fn attach(&mut self, parent: &Self, id: String, devtools: bool) {
    if self.root {
      self.context.handle.close();
    } else {
      self.context.handle.set_open(&self.context.window, false);
    }
    let handle = parent.context.handle.clone();
    let phase = handle.register(&id, devtools);
    self.context = ProfileContext {
      handle,
      window: id.into(),
      phase,
      frame_id: None,
    };
    self.root = false;
  }

  pub(crate) fn handle(&self) -> ProfilingHandle {
    self.context.handle.clone()
  }

  pub(crate) fn child_id(&self, id: u64) -> String {
    if self.context.window.as_ref() == "main" {
      format!("w{id}")
    } else {
      format!("{}/w{id}", self.context.window)
    }
  }

  pub(crate) fn set_open(&self, open: bool) {
    self.context.handle.set_open(&self.context.window, open);
  }
  #[cfg(feature = "devtools")]
  pub(crate) fn mark_devtools(&self) {
    self.context.handle.mark_devtools(&self.context.window);
  }

  pub(crate) fn begin_pass(&mut self, frame_id: u64) -> (Instant, PhaseGuard) {
    self.layout = Duration::ZERO;
    self.layout_compute = Duration::ZERO;
    self.component_after_layout = Duration::ZERO;
    self.canvas_recording = Duration::ZERO;
    self.canvas_preparation = Duration::ZERO;
    self.context.frame_id = Some(frame_id);
    self.captured = self.context.handle.inner.active.load(Ordering::Acquire);
    self.counters = None;
    if let Some(slot) = &self.context.phase {
      slot.begin_spans(self.captured);
    }
    let guard = self.context.phase(Phase::PassSetup);
    self.pass_entry = guard.entered;
    #[cfg(feature = "perf_profile")]
    {
      self.allocations_start = if self.captured {
        super::allocation_snapshot()
      } else {
        None
      };
    }
    (Instant::now(), guard)
  }

  /// A session took samples when the pass began: the runtime then gathers
  /// the pass's counters (`set_counters`).
  pub(crate) fn captured(&self) -> bool {
    self.captured
  }

  /// The pass thread's allocations since `begin_pass`. Read it before
  /// gathering the other counters, whose own allocations are not the pass's.
  #[cfg(feature = "perf_profile")]
  pub(crate) fn pass_allocations(&self) -> Option<PassAllocations> {
    let start = self.allocations_start?;
    Some(super::allocation_snapshot()?.since(start))
  }

  /// Counters the next `finish_pass` records with the pass.
  pub(crate) fn set_counters(&mut self, counters: PassCounters) {
    self.counters = Some(counters);
  }

  pub(crate) fn layout(&mut self, duration: Duration) {
    self.layout = duration;
  }
  pub(crate) fn component_after_layout(&mut self, duration: Duration) {
    self.component_after_layout += duration;
  }
  #[cfg(feature = "canvas")]
  pub(crate) fn canvas_recording(&mut self, duration: Duration) {
    self.canvas_recording += duration;
  }
  #[cfg(feature = "canvas")]
  pub(crate) fn canvas_preparation(&mut self, duration: Duration) {
    self.canvas_preparation += duration;
  }

  pub(crate) fn finish_pass(
    &mut self,
    start: Instant,
    report: &PassReport,
    backend: &'static str,
    frame: Option<&FrameProfile>,
  ) {
    // Always stops the capture; the pass guard, still open, ends its root span here.
    let (spans, dropped_spans) = self
      .context
      .phase
      .as_ref()
      .and_then(|slot| slot.finish_spans(self.pass_entry, report.required))
      .unwrap_or_default();
    if report.required {
      self.context.handle.record(
        &self.context.window,
        start,
        SampleData::Pass(PassSample {
          frame_id: report.rendered.then_some(self.context.frame_id.unwrap()),
          rendered: report.rendered,
          cached_render_list: report.used_cached_render_list,
          total: start.elapsed(),
          layout_update: self.layout,
          layout_compute: self.layout_compute,
          component_after_layout: self.component_after_layout,
          layout_recalculated: report.layout_recalculated,
          canvas_recording: self.canvas_recording,
          canvas_preparation: self.canvas_preparation,
          canvas_text: {
            #[cfg(all(feature = "canvas", feature = "perf_profile"))]
            {
              super::canvas_text::snapshot()
            }
            #[cfg(not(all(feature = "canvas", feature = "perf_profile")))]
            {
              None
            }
          },
          backend,
          frame: frame.cloned(),
          spans,
          dropped_spans,
          counters: self.counters,
        }),
      );
    }
    self.counters = None;
    self.context.frame_id = None;
  }
}

/// Coarse call boundary; no per-node labels, samples or allocations.
#[inline]
pub(crate) fn layout_compute<R>(
  #[cfg(feature = "perf_profile")] profiler: &mut WindowProfiler,
  compute: impl FnOnce() -> R,
) -> R {
  #[cfg(feature = "perf_profile")]
  let started = Instant::now();
  #[cfg(feature = "perf_profile")]
  let _phase = profiler.context.phase(Phase::LayoutCompute);
  let result = compute();
  #[cfg(feature = "perf_profile")]
  {
    profiler.layout_compute += started.elapsed();
  }
  result
}

impl Drop for WindowProfiler {
  fn drop(&mut self) {
    self.set_open(false);
    if self.root {
      self.context.handle.close();
    }
  }
}
