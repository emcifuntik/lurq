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
  #[cfg(all(feature = "perf_profile", any(feature = "canvas", feature = "wgpu")))]
  pub(crate) fn capture_active(&self) -> bool {
    self.handle.inner.active.load(Ordering::Acquire)
  }

  /// The frame id of the pass in progress; None outside a pass.
  #[cfg(all(feature = "wgpu", feature = "perf_profile"))]
  pub(crate) fn frame_id(&self) -> Option<u64> {
    self.frame_id
  }

  /// True when `other` publishes for the same window of the same collector.
  #[cfg(all(
    feature = "perf_profile",
    any(feature = "wgpu", all(feature = "dx12", target_os = "windows"))
  ))]
  pub(crate) fn same_window(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.handle.inner, &other.handle.inner) && Arc::ptr_eq(&self.window, &other.window)
  }

  /// Reports whether this window's renderer measures GPU time on its adapter.
  #[cfg(all(
    feature = "perf_profile",
    any(feature = "wgpu", all(feature = "dx12", target_os = "windows"))
  ))]
  pub(crate) fn report_gpu_timestamps(&self, availability: GpuTimestampAvailability) {
    self.handle.set_gpu_timestamps(&self.window, availability);
  }

  /// Attaches GPU time that arrived late to this window's pass of `frame_id`.
  #[cfg(all(feature = "wgpu", feature = "perf_profile"))]
  pub(crate) fn record_gpu(&self, frame_id: u64, gpu: GpuTiming) {
    self.handle.record_gpu(&self.window, frame_id, gpu);
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

/// One input dispatch (`WindowProfiler::input`). Hand it back to
/// `WindowProfiler::end_input` with whether the input asked for a frame; one
/// dropped instead waits for the window's next presented frame.
#[cfg(feature = "perf_profile")]
pub(crate) struct InputGuard {
  context: ProfileContext,
  kind: InputKind,
  received: Instant,
  started: Instant,
  wants_frame: bool,
  _phase: PhaseGuard,
}

#[cfg(feature = "perf_profile")]
impl Drop for InputGuard {
  fn drop(&mut self) {
    self.context.handle.input_dispatched(
      &self.context.window,
      super::collector::DispatchedInput {
        kind: self.kind,
        frame_id: self.context.frame_id,
        received: self.received,
        dispatch_started: self.started,
        dispatch: self.started.elapsed(),
      },
      self.wants_frame,
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
  /// When the shell received the event it dispatches next (`stamp_input`).
  received: Option<Instant>,
  /// When an input that asked for a frame first came since the last present.
  wanted: Option<Instant>,
  /// The last present, when its pass ended wanting the next frame at once.
  continuing_from: Option<Instant>,
  wants_next_frame: bool,
  /// When this pass's present returned (`presented`).
  presented_at: Option<Instant>,
  refresh: Option<Duration>,
  presents: PresentTracker,
  /// A session took samples when the pass began.
  captured: bool,
  /// The pass's frame is copied for a recording's screenshot.
  screenshot: bool,
  /// The pass thread's allocation counts when a captured pass began.
  #[cfg(feature = "perf_profile")]
  allocations_start: Option<super::AllocationSnapshot>,
  counters: Option<PassCounters>,
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
      received: None,
      wanted: None,
      continuing_from: None,
      wants_next_frame: false,
      presented_at: None,
      refresh: None,
      presents: PresentTracker::default(),
      captured: false,
      screenshot: false,
      #[cfg(feature = "perf_profile")]
      allocations_start: None,
      counters: None,
      detailed: false,
      component_layouts: Vec::new(),
    }
  }

  /// A session takes samples: what a pass or input records is kept.
  pub(crate) fn capturing(&self) -> bool {
    self.context.handle.inner.active.load(Ordering::Acquire)
  }

  /// When the shell received the event it is about to dispatch; None for an
  /// event that is not input, so the stamp never reaches a later input.
  pub(crate) fn stamp_input(&mut self, received: Option<Instant>) {
    self.received = received;
  }

  /// Starts an input dispatch: its queue delay runs from the shell's stamp,
  /// which only this input takes.
  #[cfg(feature = "perf_profile")]
  pub(crate) fn input(&mut self, kind: InputKind) -> InputGuard {
    let started = Instant::now();
    InputGuard {
      context: self.context.clone(),
      kind,
      received: self.received.take().unwrap_or(started),
      started,
      wants_frame: true,
      _phase: self.context.phase(Phase::InputDispatch),
    }
  }

  /// Ends an input dispatch. One that asked for no frame is presented by
  /// none; the others wait for the next presented frame, which they make
  /// part of a continuous run when they came within a refresh of the last.
  #[cfg(feature = "perf_profile")]
  pub(crate) fn end_input(&mut self, mut input: InputGuard, wants_frame: bool) {
    input.wants_frame = wants_frame;
    if wants_frame {
      self.wanted = Some(self.wanted.map_or(input.received, |wanted| wanted.min(input.received)));
    }
  }

  /// The refresh of the window's display, as its shell reads it.
  pub(crate) fn set_refresh_interval(&mut self, refresh: Option<Duration>) {
    self.refresh = refresh;
  }

  /// The pass's present returned at `at`.
  pub(crate) fn presented(&mut self, at: Instant) {
    self.presented_at = Some(at);
  }

  /// The pass ended wanting the next frame at once (an animation, a
  /// continuous image): that frame continues the run.
  pub(crate) fn set_wants_next_frame(&mut self, wants: bool) {
    self.wants_next_frame = wants;
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
    self.screenshot = false;
    self.counters = None;
    // The pass's one check for detailed data, made only with a session.
    self.detailed = self.captured && self.context.handle.inner.detailed.load(Ordering::Acquire);
    if let Some(slot) = &self.context.phase {
      slot.begin_spans(self.captured, self.detailed);
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

  /// Where this captured pass's frame goes when a recording is due a
  /// screenshot of it (`ProfilingHandle::screenshot_due`); None otherwise.
  /// The pass is then marked (`PassSample::screenshot`). The pixels are
  /// downscaled on the readback worker, never on the pass's thread.
  #[cfg(all(feature = "perf_profile", feature = "screenshot"))]
  pub(crate) fn screenshot_target(&mut self) -> Option<crate::app::render_engine::RenderCaptureTarget> {
    if !self.captured {
      return None;
    }
    let requested = Instant::now();
    let options = self.context.handle.screenshot_due(&self.context.window, requested)?;
    self.screenshot = true;
    let handle = self.context.handle.clone();
    let window = self.context.window.clone();
    let frame_id = self.context.frame_id.unwrap_or_default();
    Some(crate::app::render_engine::RenderCaptureTarget::Bytes(Arc::new(
      move |frame: Result<crate::app::render_engine::CapturedFrame, String>| {
        // A frame that was not read back leaves its mark: it still paid.
        let Ok(frame) = frame else {
          return;
        };
        let scaled = super::screenshot::downscale_rgba(
          frame.rgba,
          frame.width,
          frame.height,
          options.max_width,
          options.max_height,
        );
        handle.screenshot_taken(&window, frame_id, requested, scaled);
      },
    )))
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
    let presented_at = self.presented_at.take();
    let frame_id = self.context.frame_id.take();
    let presented = report
      .rendered
      .then(|| (frame_id.unwrap(), presented_at.unwrap_or_else(Instant::now)));
    let present = presented.map(|(_, at)| {
      // First wanted when the frame before ended wanting it, or when the
      // first input that asked for it came.
      let wanted = self.continuing_from.into_iter().chain(self.wanted).min();
      let (interval, continuous, missed_vsyncs) = self.presents.present(at, wanted, self.refresh);
      self.continuing_from = self.wants_next_frame.then_some(at);
      self.wanted = None;
      PresentTiming {
        presented_ms: self.context.handle.millis(at),
        interval,
        refresh_interval: self.refresh,
        continuous,
        missed_vsyncs,
      }
    });
    self.wants_next_frame = false;
    if report.required {
      self.context.handle.record(
        &self.context.window,
        start,
        SampleData::Pass(PassSample {
          frame_id: presented.map(|(frame_id, _)| frame_id),
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
          reasons: report.reasons,
          present,
          counters: self.counters,
          // A frame that was not rendered copied nothing.
          screenshot: self.screenshot && report.rendered,
          gpu: None,
          component_layouts,
        }),
      );
    }
    self.counters = None;
    self.screenshot = false;
    // The inputs since the last present are presented by this frame; a pass
    // that had nothing to do ends them presented by none, and the run of
    // frames with them. A required pass that could not render keeps them.
    if presented.is_some() || !report.required {
      #[cfg(feature = "perf_profile")]
      self.context.handle.inputs_presented(&self.context.window, presented);
      self.wanted = None;
    }
    if !report.required {
      self.continuing_from = None;
    }
  }

  /// Ends the run of frames: the window stopped asking for them (it was
  /// hidden), so its next frame comes after a gap, not late.
  pub(crate) fn break_run(&mut self) {
    self.continuing_from = None;
    self.wanted = None;
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
