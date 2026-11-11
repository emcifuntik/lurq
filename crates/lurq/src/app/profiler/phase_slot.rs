//! The current phase of one window, published without the collector lock.
//!
//! Every coarse phase entry and exit writes here, with or without a session,
//! so a session started in the middle of a slow pass still sees what the
//! window is doing. Reports read it under the collector lock; the pass never
//! takes that lock for a phase.
use std::{
  sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering, fence},
  time::{Duration, Instant},
};

use super::model::Phase;

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
  frame_id: AtomicU64,
  /// Nanoseconds since `epoch` the outermost phase started.
  started: AtomicU64,
  phase_started: AtomicU64,
}

/// The slot's words, as a phase guard keeps them to restore on exit.
#[derive(Clone, Copy, Default)]
pub(crate) struct Published {
  phase: u8,
  frame_id: u64,
  started: u64,
  phase_started: u64,
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
      frame_id: AtomicU64::new(0),
      started: AtomicU64::new(0),
      phase_started: AtomicU64::new(0),
    }
  }

  /// Publishes `phase` as current and returns what it replaces; the outermost
  /// phase's start is kept. A closed slot stays empty.
  pub(crate) fn enter(&self, frame_id: Option<u64>, phase: Phase) -> Published {
    let now = self.nanos(Instant::now());
    self.write(|slot| {
      if !slot.open.load(Ordering::Relaxed) {
        return Published::default();
      }
      let previous = slot.load();
      slot.store(Published {
        phase: (phase as u8 + 1) | if frame_id.is_some() { FRAME_BIT } else { 0 },
        frame_id: frame_id.unwrap_or(0),
        started: if previous.phase == 0 { now } else { previous.started },
        phase_started: now,
      });
      previous
    })
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
    let phase = *PHASES.get(usize::from((value.phase & !FRAME_BIT).checked_sub(1)?))?;
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
      frame_id: self.frame_id.load(Ordering::Relaxed),
      started: self.started.load(Ordering::Relaxed),
      phase_started: self.phase_started.load(Ordering::Relaxed),
    }
  }

  fn store(&self, value: Published) {
    self.phase.store(value.phase, Ordering::Relaxed);
    self.frame_id.store(value.frame_id, Ordering::Relaxed);
    self.started.store(value.started, Ordering::Relaxed);
    self.phase_started.store(value.phase_started, Ordering::Relaxed);
  }

  fn nanos(&self, instant: Instant) -> u64 {
    u64::try_from(instant.saturating_duration_since(self.epoch).as_nanos()).unwrap_or(u64::MAX)
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
    slot.restore(inner);
    let restored = slot.read().unwrap();
    assert_eq!((restored.phase, restored.frame_id), (Phase::LayoutUpdate, Some(3)));
    assert_eq!(restored.phase_started, first.phase_started);
    slot.restore(outer);
    assert!(slot.read().is_none());
  }

  #[test]
  fn a_closed_slot_publishes_nothing() {
    let slot = PhaseSlot::new(Instant::now(), true);
    let outer = slot.enter(None, Phase::Render);
    slot.set_open(false);
    slot.restore(outer);
    assert!(slot.enter(None, Phase::Render).phase == 0);
    assert!(slot.read().is_none());
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
          slot.restore(previous);
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
