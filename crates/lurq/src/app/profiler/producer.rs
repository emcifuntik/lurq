#![cfg_attr(not(feature = "perf_profile"), allow(dead_code))]
use std::{
  sync::{Arc, atomic::Ordering},
  time::{Duration, Instant},
};

use super::{
  FrameProfile, ProfilingHandle, detail,
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

  /// An update; in a detailed recording it records its component renders
  /// and their causes (one flag check otherwise).
  pub(crate) fn update(&self, kind: UiUpdateKind) -> UpdateGuard {
    let phase = self.phase(Phase::Rebuild);
    let detail = self
      .handle
      .inner
      .detailed
      .load(Ordering::Acquire)
      .then(|| detail::UpdateScope::begin(self.phase.clone(), self.frame_id));
    UpdateGuard {
      context: self.clone(),
      kind,
      started: Instant::now(),
      commit_started: None,
      _phase: phase,
      commit_phase: None,
      detail,
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
  /// Some in a detailed recording.
  detail: Option<detail::UpdateScope>,
}

impl UpdateGuard {
  pub(crate) fn commit(&mut self) {
    self.commit_started = Some(Instant::now());
    self.commit_phase = Some(self.context.phase(Phase::Commit));
  }
}

impl Drop for UpdateGuard {
  fn drop(&mut self) {
    let causes = self.detail.take().map(detail::UpdateScope::finish).unwrap_or_default();
    self.context.handle.record(
      &self.context.window,
      self.started,
      SampleData::UiUpdate(UiUpdateSample {
        frame_id: self.context.frame_id,
        kind: self.kind,
        total: self.started.elapsed(),
        commit: self.commit_started.map(|start| start.elapsed()).unwrap_or_default(),
        causes,
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
  /// True from `begin_pass` to `finish_pass` of a pass a detailed recording takes.
  detailed: bool,
  /// What the current detailed pass laid out again, for its sample.
  component_layouts: Vec<ComponentLayouts>,
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
      detailed: false,
      component_layouts: Vec::new(),
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
    let active = self.context.handle.inner.active.load(Ordering::Acquire);
    // The pass's one check for detailed data, made only with a session.
    self.detailed = active && self.context.handle.inner.detailed.load(Ordering::Acquire);
    if let Some(slot) = &self.context.phase {
      slot.begin_spans(active, self.detailed);
    }
    let guard = self.context.phase(Phase::PassSetup);
    self.pass_entry = guard.entered;
    (Instant::now(), guard)
  }

  /// Whether this pass records detailed data, as `begin_pass` found.
  pub(crate) fn detailed(&self) -> bool {
    self.detailed
  }

  /// The spans of this pass's `after_layout` hooks; None unless it records detailed data.
  pub(crate) fn hook_spans(&self) -> Option<detail::HookSpans> {
    if !self.detailed {
      return None;
    }
    let slot = self.context.phase.clone()?;
    Some(detail::HookSpans::new(slot, self.context.frame_id))
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

  /// The layout trace's counts of this detailed pass
  /// (`LayoutEngine::take_component_layouts`), for its sample: nodes laid out
  /// again per nearest component tag and reason.
  pub(crate) fn component_layouts(&mut self, counts: Vec<(Option<Arc<str>>, &'static str, u32)>) {
    self.component_layouts = component_layouts_of(counts);
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
    self.detailed = false;
    let component_layouts = std::mem::take(&mut self.component_layouts);
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
          component_layouts,
        }),
      );
    }
    self.context.frame_id = None;
  }
}

/// The layout trace's counts with registered names, merged per name and
/// reason, most nodes first.
fn component_layouts_of(counts: Vec<(Option<Arc<str>>, &'static str, u32)>) -> Vec<ComponentLayouts> {
  let mut layouts: Vec<ComponentLayouts> = Vec::new();
  for (tag, reason, nodes) in counts {
    let component = tag.as_deref().map(|tag| detail::intern(tag).unwrap_or("(unnamed)"));
    match layouts
      .iter_mut()
      .find(|layout| layout.component == component && layout.reason == reason)
    {
      Some(layout) => layout.nodes = layout.nodes.saturating_add(nodes),
      None => layouts.push(ComponentLayouts {
        component,
        reason,
        nodes,
      }),
    }
  }
  layouts.sort_by(|a, b| b.nodes.cmp(&a.nodes).then(a.component.cmp(&b.component)));
  layouts
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
