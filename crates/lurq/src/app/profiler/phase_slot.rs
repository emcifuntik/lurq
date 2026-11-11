//! The current phase of one window, published without the collector lock.
//!
//! Every coarse phase entry and exit writes here, with or without a session,
//! so a session started in the middle of a slow pass still sees what the
//! window is doing. Reports read it under the collector lock; the pass never
//! takes that lock for a phase.
//!
//! A pass that starts while a session is active also keeps each phase's span
//! in the slot's span buffer: allocated once per window by its first captured
//! pass, written with atomics only, and read back when the pass finishes. A
//! pass of a detailed recording keeps its component spans in a second buffer,
//! allocated by the window's first detailed pass, so they never take the
//! cells of the pass's phases.
use std::{
  sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU64, AtomicUsize, Ordering, fence},
  },
  time::{Duration, Instant},
};

use super::{
  detail,
  model::{MAX_COMPONENT_SPANS_PER_PASS, MAX_SPANS_PER_PASS, Phase, PhaseSpan},
};

/// Every phase in declaration order: `PHASES[phase as usize] == phase`.
const PHASES: [Phase; 24] = [
  Phase::PassSetup,
  Phase::PassNotifications,
  Phase::LayoutUpdate,
  Phase::LayoutCompute,
  Phase::ComponentAfterLayout,
  Phase::Rebuild,
  Phase::Commit,
  Phase::QuadResolve,
  Phase::GlyphRasterize,
  Phase::CanvasRecording,
  Phase::CanvasPreparation,
  Phase::Render,
  Phase::CanvasBackend,
  Phase::CanvasTessellation,
  Phase::CanvasAssetUpload,
  Phase::CanvasBufferUpload,
  Phase::CanvasSubmission,
  Phase::CanvasCommands,
  Phase::RenderInit,
  Phase::RenderAcquire,
  Phase::RenderEncode,
  Phase::RenderSubmit,
  Phase::RenderPresent,
  Phase::InputDispatch,
];

/// Set in the phase code when the phase belongs to a frame.
const FRAME_BIT: u8 = 0x80;

/// A sequence lock: a writer makes `sequence` odd, writes the words and makes
/// it even again; a reader retries until it read the words between two equal
/// even values. Writers claim the odd value with a compare-exchange, so a
/// phase entered from another thread (a render backend's context clone) cannot
/// tear the record either.
pub(crate) struct PhaseSlot {
  epoch: Instant,
  sequence: AtomicU64,
  /// False once the window closed: phases are neither published nor reported.
  open: AtomicBool,
  /// 0 without a phase in flight, else the phase index + 1, with `FRAME_BIT`.
  phase: AtomicU8,
  /// Phases in flight around the current one; 0 for the outermost.
  depth: AtomicU8,
  frame_id: AtomicU64,
  /// Nanoseconds since `epoch` the outermost phase started.
  started: AtomicU64,
  phase_started: AtomicU64,
  spans: SpanBuffer,
  /// The component spans of a detailed pass.
  components: SpanBuffer,
}

/// The slot's words, as a phase guard keeps them to restore on exit.
#[derive(Clone, Copy, Default)]
pub(crate) struct Published {
  phase: u8,
  depth: u8,
  frame_id: u64,
  started: u64,
  phase_started: u64,
}

/// One phase entry: what it replaced, and its own start for its span.
#[derive(Clone, Copy, Default)]
pub(crate) struct Entered {
  pub(crate) previous: Published,
  /// The phase index + 1; 0 when the slot was closed, which records nothing.
  code: u8,
  depth: u8,
  start: u64,
  /// The component's label (`detail::label_of`) for a component span; 0 for a phase.
  label: u16,
}

/// The phase in flight, as a report reads it.
#[derive(Clone, Copy)]
pub(crate) struct Progress {
  pub(crate) started: Instant,
  pub(crate) phase_started: Instant,
  pub(crate) frame_id: Option<u64>,
  pub(crate) phase: Phase,
}

impl PhaseSlot {
  pub(crate) fn new(epoch: Instant, open: bool) -> Self {
    Self {
      epoch,
      sequence: AtomicU64::new(0),
      open: AtomicBool::new(open),
      phase: AtomicU8::new(0),
      depth: AtomicU8::new(0),
      frame_id: AtomicU64::new(0),
      started: AtomicU64::new(0),
      phase_started: AtomicU64::new(0),
      spans: SpanBuffer::default(),
      components: SpanBuffer::default(),
    }
  }

  /// Publishes `phase` as current and returns what it replaces; the outermost
  /// phase's start is kept. A closed slot stays empty.
  pub(crate) fn enter(&self, frame_id: Option<u64>, phase: Phase) -> Entered {
    self.enter_labeled(frame_id, phase, 0)
  }

  /// [`Self::enter`] for the span of component `label` (`detail::label_of`)
  /// in `phase`, which is published as the phase in flight.
  pub(crate) fn enter_component(&self, frame_id: Option<u64>, phase: Phase, label: u16) -> Entered {
    self.enter_labeled(frame_id, phase, label)
  }

  fn enter_labeled(&self, frame_id: Option<u64>, phase: Phase, label: u16) -> Entered {
    let now = self.nanos(Instant::now());
    self.write(|slot| {
      if !slot.open.load(Ordering::Relaxed) {
        return Entered::default();
      }
      let previous = slot.load();
      let depth = if previous.phase == 0 {
        0
      } else {
        previous.depth.saturating_add(1)
      };
      slot.store(Published {
        phase: (phase as u8 + 1) | if frame_id.is_some() { FRAME_BIT } else { 0 },
        depth,
        frame_id: frame_id.unwrap_or(0),
        started: if previous.phase == 0 { now } else { previous.started },
        phase_started: now,
      });
      Entered {
        previous,
        code: phase as u8 + 1,
        depth,
        start: now,
        label,
      }
    })
  }

  /// Ends a phase entry: puts back what it replaced, unless the slot closed
  /// since, and keeps its span when the pass is captured.
  pub(crate) fn exit(&self, entered: Entered) {
    self.restore(entered.previous);
    let buffer = if entered.label == 0 {
      &self.spans
    } else {
      &self.components
    };
    if entered.code != 0 && buffer.capturing.load(Ordering::Relaxed) {
      let end = self.nanos(Instant::now());
      buffer.push(entered.code, entered.depth, entered.start, end, entered.label);
    }
  }

  /// Puts back what a phase entry replaced, unless the slot closed since.
  pub(crate) fn restore(&self, previous: Published) {
    self.write(|slot| {
      if slot.open.load(Ordering::Relaxed) {
        slot.store(previous);
      }
    });
  }

  /// Opens or closes the slot. Closing drops the phase in flight, and a closed
  /// slot publishes nothing, so it opens again empty.
  pub(crate) fn set_open(&self, open: bool) {
    self.write(|slot| {
      slot.open.store(open, Ordering::Relaxed);
      if !open {
        slot.store(Published::default());
      }
    });
  }

  /// Starts a pass's span capture, or stops it when no session is active;
  /// `detailed` captures its component spans too. Each buffer is allocated by
  /// the window's first pass that captures into it, never after.
  pub(crate) fn begin_spans(&self, capture: bool, detailed: bool) {
    if capture {
      self.spans.begin(MAX_SPANS_PER_PASS);
    } else {
      self.spans.capturing.store(false, Ordering::Relaxed);
    }
    if capture && detailed {
      self.components.begin(MAX_COMPONENT_SPANS_PER_PASS);
    } else {
      self.components.capturing.store(false, Ordering::Relaxed);
    }
  }

  /// Stops the pass's span capture and, when `keep`, returns its spans with
  /// `pass` (the entry of the pass's own guard, still open) as the root span
  /// ending now, depths relative to it, ordered by start; None when the pass
  /// was not captured. Returns the spans that did not fit too.
  pub(crate) fn finish_spans(&self, pass: Entered, keep: bool) -> Option<(Vec<PhaseSpan>, u32)> {
    let components = self.components.capturing.swap(false, Ordering::Acquire);
    if !self.spans.capturing.swap(false, Ordering::Acquire) || !keep || pass.code == 0 {
      return None;
    }
    let end = self.nanos(Instant::now());
    let mut spans = Vec::new();
    spans.push(PhaseSpan {
      phase: phase_of(pass.code)?,
      depth: 0,
      start_ns: pass.start,
      end_ns: end,
      component: None,
    });
    let mut dropped = self.spans.collect(pass.depth, &mut spans);
    if components {
      dropped = dropped.saturating_add(self.components.collect(pass.depth, &mut spans));
    }
    spans.sort_by_key(|span| (span.start_ns, span.depth));
    Some((spans, dropped))
  }

  #[cfg(all(test, feature = "perf_profile"))]
  pub(crate) fn span_buffer_allocated(&self) -> bool {
    self.spans.cells.get().is_some()
  }

  #[cfg(all(test, feature = "perf_profile"))]
  pub(crate) fn span_capture_active(&self) -> bool {
    self.spans.capturing.load(Ordering::Relaxed)
  }
  /// The phase in flight, read without blocking its writer.
  pub(crate) fn read(&self) -> Option<Progress> {
    loop {
      let before = self.sequence.load(Ordering::Acquire);
      if before & 1 == 0 {
        let open = self.open.load(Ordering::Relaxed);
        let value = self.load();
        fence(Ordering::Acquire);
        if self.sequence.load(Ordering::Relaxed) == before {
          return open.then(|| self.progress(value)).flatten();
        }
      }
      std::hint::spin_loop();
    }
  }

  fn progress(&self, value: Published) -> Option<Progress> {
    let phase = phase_of(value.phase & !FRAME_BIT)?;
    Some(Progress {
      started: self.epoch + Duration::from_nanos(value.started),
      phase_started: self.epoch + Duration::from_nanos(value.phase_started),
      frame_id: (value.phase & FRAME_BIT != 0).then_some(value.frame_id),
      phase,
    })
  }

  fn write<R>(&self, update: impl FnOnce(&Self) -> R) -> R {
    let mut sequence = self.sequence.load(Ordering::Relaxed);
    loop {
      if sequence & 1 == 0 {
        match self
          .sequence
          .compare_exchange_weak(sequence, sequence + 1, Ordering::Acquire, Ordering::Relaxed)
        {
          Ok(_) => break,
          Err(current) => sequence = current,
        }
      } else {
        std::hint::spin_loop();
        sequence = self.sequence.load(Ordering::Relaxed);
      }
    }
    // Words written below are seen by a reader only with the odd sequence.
    fence(Ordering::Release);
    let result = update(self);
    self.sequence.store(sequence + 2, Ordering::Release);
    result
  }

  fn load(&self) -> Published {
    Published {
      phase: self.phase.load(Ordering::Relaxed),
      depth: self.depth.load(Ordering::Relaxed),
      frame_id: self.frame_id.load(Ordering::Relaxed),
      started: self.started.load(Ordering::Relaxed),
      phase_started: self.phase_started.load(Ordering::Relaxed),
    }
  }

  fn store(&self, value: Published) {
    self.phase.store(value.phase, Ordering::Relaxed);
    self.depth.store(value.depth, Ordering::Relaxed);
    self.frame_id.store(value.frame_id, Ordering::Relaxed);
    self.started.store(value.started, Ordering::Relaxed);
    self.phase_started.store(value.phase_started, Ordering::Relaxed);
  }

  fn nanos(&self, instant: Instant) -> u64 {
    u64::try_from(instant.saturating_duration_since(self.epoch).as_nanos()).unwrap_or(u64::MAX)
  }
}

/// The phase of a code without `FRAME_BIT`; None for 0 (no phase).
fn phase_of(code: u8) -> Option<Phase> {
  PHASES.get(usize::from(code.checked_sub(1)?)).copied()
}

/// The spans of the pass in capture. Phase guards on any thread claim a cell
/// with one atomic add and publish it by storing its code last.
#[derive(Default)]
struct SpanBuffer {
  capturing: AtomicBool,
  /// Cells claimed this pass, including those past the end.
  len: AtomicUsize,
  cells: OnceLock<Box<[SpanCell]>>,
}

#[derive(Default)]
struct SpanCell {
  /// The phase index + 1 once written; 0 while empty.
  code: AtomicU8,
  depth: AtomicU8,
  /// The component label of a component span; 0 for a phase.
  label: AtomicU16,
  start: AtomicU64,
  end: AtomicU64,
}

impl SpanBuffer {
  /// Starts a capture into `size` cells, allocated by the first.
  fn begin(&self, size: usize) {
    let cells = self
      .cells
      .get_or_init(|| (0..size).map(|_| SpanCell::default()).collect());
    let used = self.len.swap(0, Ordering::Relaxed).min(cells.len());
    for cell in &cells[..used] {
      cell.code.store(0, Ordering::Relaxed);
    }
    self.capturing.store(true, Ordering::Release);
  }

  fn push(&self, code: u8, depth: u8, start: u64, end: u64, label: u16) {
    let Some(cells) = self.cells.get() else {
      return;
    };
    let Some(cell) = cells.get(self.len.fetch_add(1, Ordering::Relaxed)) else {
      return;
    };
    cell.depth.store(depth, Ordering::Relaxed);
    cell.label.store(label, Ordering::Relaxed);
    cell.start.store(start, Ordering::Relaxed);
    cell.end.store(end, Ordering::Relaxed);
    cell.code.store(code, Ordering::Release);
  }

  /// Appends the captured spans below a pass at `pass_depth` to `spans`,
  /// depths relative to it, and returns how many did not fit.
  fn collect(&self, pass_depth: u8, spans: &mut Vec<PhaseSpan>) -> u32 {
    let Some(cells) = self.cells.get() else {
      return 0;
    };
    let claimed = self.len.load(Ordering::Acquire);
    let kept = claimed.min(cells.len());
    spans.reserve(kept);
    for cell in &cells[..kept] {
      // A span claimed on another thread and not yet written is left out.
      let Some(phase) = phase_of(cell.code.load(Ordering::Acquire)) else {
        continue;
      };
      let depth = cell.depth.load(Ordering::Relaxed);
      // A phase around the pass (an input dispatch) that ended inside it.
      if depth <= pass_depth {
        continue;
      }
      spans.push(PhaseSpan {
        phase,
        depth: depth - pass_depth,
        start_ns: cell.start.load(Ordering::Relaxed),
        end_ns: cell.end.load(Ordering::Relaxed),
        component: detail::name_of(cell.label.load(Ordering::Relaxed)),
      });
    }
    u32::try_from(claimed - kept).unwrap_or(u32::MAX)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn phase_codes_round_trip_every_phase() {
    for (index, phase) in PHASES.into_iter().enumerate() {
      assert_eq!(phase as usize, index);
      let slot = PhaseSlot::new(Instant::now(), true);
      slot.enter(Some(u64::MAX), phase);
      let progress = slot.read().unwrap();
      assert_eq!((progress.phase, progress.frame_id), (phase, Some(u64::MAX)));
    }
  }

  #[test]
  fn nested_phases_keep_the_outer_start_and_restore_in_order() {
    let slot = PhaseSlot::new(Instant::now(), true);
    let outer = slot.enter(Some(3), Phase::LayoutUpdate);
    let first = slot.read().unwrap();
    let inner = slot.enter(None, Phase::Rebuild);
    let nested = slot.read().unwrap();
    assert_eq!((nested.phase, nested.frame_id), (Phase::Rebuild, None));
    assert_eq!(nested.started, first.started);
    assert!(nested.phase_started >= first.phase_started);
    slot.exit(inner);
    let restored = slot.read().unwrap();
    assert_eq!((restored.phase, restored.frame_id), (Phase::LayoutUpdate, Some(3)));
    assert_eq!(restored.phase_started, first.phase_started);
    slot.exit(outer);
    assert!(slot.read().is_none());
  }

  #[test]
  fn a_closed_slot_publishes_nothing() {
    let slot = PhaseSlot::new(Instant::now(), true);
    let outer = slot.enter(None, Phase::Render);
    slot.set_open(false);
    slot.exit(outer);
    assert!(slot.enter(None, Phase::Render).code == 0);
    assert!(slot.read().is_none());
  }

  #[test]
  fn captured_spans_keep_the_guards_nesting_relative_to_the_pass() {
    let slot = PhaseSlot::new(Instant::now(), true);
    // A phase around the pass, ending inside it, is not one of its spans.
    let around = slot.enter(None, Phase::InputDispatch);
    slot.begin_spans(true, false);
    let pass = slot.enter(Some(1), Phase::PassSetup);
    let layout = slot.enter(Some(1), Phase::LayoutUpdate);
    let rebuild = slot.enter(Some(1), Phase::Rebuild);
    let commit = slot.enter(Some(1), Phase::Commit);
    slot.exit(commit);
    slot.exit(rebuild);
    let compute = slot.enter(Some(1), Phase::LayoutCompute);
    slot.exit(compute);
    slot.exit(layout);
    slot.exit(around);
    let (spans, dropped) = slot.finish_spans(pass, true).unwrap();
    slot.exit(pass);
    assert_eq!(dropped, 0);
    let shape: Vec<_> = spans.iter().map(|span| (span.phase, span.depth)).collect();
    assert_eq!(
      shape,
      [
        (Phase::PassSetup, 0),
        (Phase::LayoutUpdate, 1),
        (Phase::Rebuild, 2),
        (Phase::Commit, 3),
        (Phase::LayoutCompute, 2),
      ]
    );
    // Every span lies inside its parent: the nearest earlier span one level up.
    for (index, span) in spans.iter().enumerate() {
      assert!(span.start_ns <= span.end_ns);
      if let Some(parent) = spans[..index]
        .iter()
        .rev()
        .find(|parent| parent.depth + 1 == span.depth)
      {
        assert!(parent.start_ns <= span.start_ns && span.end_ns <= parent.end_ns);
      }
    }
    // The capture stopped with the pass: later guards record nothing.
    let late = slot.enter(None, Phase::Render);
    slot.exit(late);
    assert!(slot.finish_spans(pass, true).is_none());
  }

  #[test]
  fn an_uncaptured_pass_allocates_no_span_buffer_and_records_nothing() {
    let slot = PhaseSlot::new(Instant::now(), true);
    slot.begin_spans(false, true);
    let pass = slot.enter(Some(1), Phase::PassSetup);
    let layout = slot.enter(Some(1), Phase::LayoutUpdate);
    slot.exit(layout);
    assert!(slot.finish_spans(pass, true).is_none());
    slot.exit(pass);
    assert!(slot.spans.cells.get().is_none());
    assert!(slot.components.cells.get().is_none());
    assert_eq!(slot.spans.len.load(Ordering::Relaxed), 0);
  }

  #[test]
  fn spans_past_the_buffer_are_counted_not_kept() {
    let slot = PhaseSlot::new(Instant::now(), true);
    slot.begin_spans(true, false);
    let pass = slot.enter(Some(1), Phase::PassSetup);
    for _ in 0..MAX_SPANS_PER_PASS + 3 {
      let guard = slot.enter(Some(1), Phase::CanvasCommands);
      slot.exit(guard);
    }
    let (spans, dropped) = slot.finish_spans(pass, true).unwrap();
    assert_eq!((spans.len(), dropped), (MAX_SPANS_PER_PASS + 1, 3));
    // The next captured pass starts empty.
    slot.begin_spans(true, false);
    let (spans, dropped) = slot.finish_spans(pass, true).unwrap();
    assert_eq!((spans.len(), dropped), (1, 0));
  }

  /// Component spans fill their own buffer, nested in the phases around
  /// them: a pass full of them still keeps its later phases. Without a
  /// detailed capture they record nothing and allocate nothing.
  #[test]
  fn component_spans_nest_in_their_phase_and_never_crowd_out_phases() {
    let label = detail::label_of("SpanProbe");
    let slot = PhaseSlot::new(Instant::now(), true);
    slot.begin_spans(true, false);
    let pass = slot.enter(Some(1), Phase::PassSetup);
    let component = slot.enter_component(Some(1), Phase::Rebuild, label);
    slot.exit(component);
    let (spans, _) = slot.finish_spans(pass, true).unwrap();
    assert!(spans.iter().all(|span| span.component.is_none()));
    assert!(slot.components.cells.get().is_none());

    slot.begin_spans(true, true);
    let rebuild = slot.enter(Some(1), Phase::Rebuild);
    for _ in 0..MAX_SPANS_PER_PASS + 3 {
      let component = slot.enter_component(Some(1), Phase::Rebuild, label);
      slot.exit(component);
    }
    slot.exit(rebuild);
    let render = slot.enter(Some(1), Phase::Render);
    slot.exit(render);
    let (spans, dropped) = slot.finish_spans(pass, true).unwrap();
    assert_eq!(dropped, 0);
    assert_eq!(spans.len(), MAX_SPANS_PER_PASS + 6);
    assert_eq!(
      (spans[1].phase, spans[1].depth, spans[1].component),
      (Phase::Rebuild, 1, None)
    );
    assert_eq!(
      (spans[2].phase, spans[2].depth, spans[2].component),
      (Phase::Rebuild, 2, Some("SpanProbe"))
    );
    let last = spans.last().unwrap();
    assert_eq!((last.phase, last.depth, last.component), (Phase::Render, 1, None));
  }

  #[test]
  fn readers_never_see_a_torn_record() {
    let slot = std::sync::Arc::new(PhaseSlot::new(Instant::now(), true));
    let writer = {
      let slot = slot.clone();
      std::thread::spawn(move || {
        for frame in 0..20_000u64 {
          // Odd frame ids only: a frame id torn from an empty record (0) shows.
          let (phase, frame_id) = if frame % 2 == 0 {
            (Phase::LayoutUpdate, Some(frame * 2 + 1))
          } else {
            (Phase::InputDispatch, None)
          };
          let previous = slot.enter(frame_id, phase);
          slot.exit(previous);
        }
      })
    };
    while !writer.is_finished() {
      if let Some(progress) = slot.read() {
        match progress.phase {
          Phase::LayoutUpdate => assert_eq!(progress.frame_id.unwrap() % 2, 1),
          Phase::InputDispatch => assert!(progress.frame_id.is_none()),
          phase => panic!("unexpected {phase:?}"),
        }
        // Each phase here is outermost: its two starts come from one entry.
        assert_eq!(progress.phase_started, progress.started);
      }
    }
    writer.join().unwrap();
  }
}
