//! Trackpad swipes between pages, recognized from the scroll gesture they arrive as. macOS reports a two-finger swipe
//! as scroll events with phases: a start when the fingers touch, scrolls while they move, an end when they lift, then
//! a momentum gesture of its own (start, scrolls, end) that carries on the motion. A swipe is a finger gesture that
//! went far enough, mostly sideways, without anything under the pointer taking its horizontal part: no scroll
//! container moved with it and no handler prevented its default. Its end then goes back or forward, as browsers do.
//! Mouse wheels send scrolls without a start, so they never make one.

use std::time::{Duration, Instant};

use super::{EventControl, Tree, hit_test_tree};
use crate::{
  app::events::{HistoryDirection, HistorySwipeEvent, ScrollPhase, is_synthetic_input},
  core::NodeId,
};

/// How far the fingers must travel sideways, in logical pixels.
const SWIPE_DISTANCE: f32 = 120.0;
/// How many times larger the sideways travel must be than the vertical.
const HORIZONTAL_RATIO: f32 = 2.0;
/// How long after a finger gesture ends a gesture that starts is taken as its momentum and ignored. AppKit starts the
/// momentum right away; a new swipe takes longer to touch down. The momentum's own end does not count: touching the
/// trackpad again ends it, a moment before the next swipe starts.
const MOMENTUM_GAP: Duration = Duration::from_millis(150);

#[derive(Debug, Default)]
struct Gesture {
  dx: f32,
  dy: f32,
  /// A scroll container or a handler took the gesture's horizontal part.
  taken: bool,
}

/// One window's recognizer.
#[derive(Debug, Default)]
pub(crate) struct HistorySwipes {
  /// The platform has swipes between pages turned on (macOS: "Swipe between pages" with two fingers).
  enabled: bool,
  gesture: Option<Gesture>,
  /// The gesture under way is the momentum of the last finger gesture.
  momentum: bool,
  /// When the last finger gesture ended: the momentum that follows it is not a swipe.
  last_end: Option<Instant>,
}

impl HistorySwipes {
  pub(crate) fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
    if !enabled {
      self.gesture = None;
    }
  }

  /// Takes one scroll event: its phase, its delta in logical pixels and whether something under the pointer took its
  /// horizontal part. Gives the swipe the event ends, if any.
  pub(crate) fn scroll(
    &mut self,
    phase: ScrollPhase,
    (dx, dy): (f32, f32),
    taken: bool,
    now: Instant,
  ) -> Option<HistoryDirection> {
    if matches!(phase, ScrollPhase::Start) {
      self.momentum = self
        .last_end
        .is_some_and(|ended| now.saturating_duration_since(ended) < MOMENTUM_GAP);
      self.gesture = (self.enabled && !self.momentum).then(Gesture::default);
    }
    let Some(gesture) = self.gesture.as_mut() else {
      // A finger gesture nobody tracks (swipes off) still ends: the momentum after it is no swipe either.
      if matches!(phase, ScrollPhase::End) && !std::mem::take(&mut self.momentum) {
        self.last_end = Some(now);
      }
      return None;
    };
    gesture.dx += dx;
    gesture.dy += dy;
    gesture.taken |= taken;
    if !matches!(phase, ScrollPhase::End) {
      return None;
    }
    self.last_end = Some(now);
    let gesture = self.gesture.take()?;
    let swiped =
      !gesture.taken && gesture.dx.abs() >= SWIPE_DISTANCE && gesture.dx.abs() >= gesture.dy.abs() * HORIZONTAL_RATIO;
    swiped.then_some(if gesture.dx > 0.0 {
      HistoryDirection::Back
    } else {
      HistoryDirection::Forward
    })
  }
}

impl Tree {
  /// Whether trackpad swipes between pages are on for this window. The winit shell sets it from the platform's
  /// setting when a scroll gesture starts; off until then. Public for tests.
  #[doc(hidden)]
  pub fn set_history_swipes(&mut self, enabled: bool) {
    self.history_swipes.set_enabled(enabled);
  }

  /// Feeds a dispatched scroll to the recognizer and delivers the swipe it ends. `(x, y)` and the delta are logical.
  pub(super) fn recognize_history_swipe(
    &mut self,
    (x, y): (f32, f32),
    delta: (f32, f32),
    phase: ScrollPhase,
    taken: bool,
  ) {
    // lurq's synthetic input sends a wheel as a whole gesture: an agent's wheel is not a person's swipe.
    if is_synthetic_input() {
      return;
    }
    if let Some(direction) = self.history_swipes.scroll(phase, delta, taken, Instant::now()) {
      self.dispatch_history_swipe(x, y, direction);
    }
  }

  /// Delivers a swipe to the elements under `(x, y)`, innermost first, until one stops its propagation.
  fn dispatch_history_swipe(&mut self, x: f32, y: f32, direction: HistoryDirection) {
    let (Some(root), Some(layout)) = (&self.root, &self.last_layout) else {
      return;
    };
    let mut hits = Vec::new();
    hit_test_tree(root, layout, 0.0, 0.0, x, y, &mut hits);
    let mut event = HistorySwipeEvent {
      direction,
      x,
      y,
      target_id: NodeId::UNASSIGNED,
      control: EventControl::new(),
    };
    'swipe: for (node, _) in &hits {
      event.target_id = node.node_id();
      for handler in &node.events.on_history_swipe {
        handler.call(&event);
        if event.immediate_propagation_stopped() {
          break 'swipe;
        }
      }
      if event.propagation_stopped() {
        break;
      }
    }
  }
}
