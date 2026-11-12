//! One finger as the pointer. winit reports a touch screen as `WindowEvent::Touch` only, with no mouse events, so
//! the shell turns the first finger into the pointer and scroll input the tree already takes. The timings and
//! distances are UIKit's, so a list here answers a finger as a `UIScrollView` does.
//!
//! Where nothing under the finger scrolls, and on a control that is dragged (a slider, a scrollbar), the finger
//! is the pointer with its button down: it presses when it lands, moves the pointer and releases when it lifts.
//!
//! Over content that scrolls, a finger that lands may be about to scroll or to tap, so nothing is pressed yet,
//! and nothing is hovered: a finger has no hover.
//!
//! - Rested for [`PRESS_DELAY`], it presses, so what it is on shows its pressed state.
//! - Lifted within [`TAP_SLOP`], it taps where it landed: the click comes at once, and what was tapped shows
//!   its pressed state for [`TAP_HIGHLIGHT`] even when the tap was quicker than the delay.
//! - Moved past the slop, it scrolls, and a press already sent ends without a click. The content starts from
//!   where it is: the travel that told the scroll from a tap is not caught up with. The scroll goes on after
//!   the finger lifts, slowing down, until it stops or reaches an end of the content.
//! - Rested for [`LONG_PRESS`], it is a long press (`on_long_press`) and stays the pointer whatever it does
//!   next: text selection and `on_drag_*` sessions over a list start this way.
//!
//! A finger that lands while content is still moving, with momentum or springing back from past an end, catches
//! it: the content stops, nothing is pressed, and any move scrolls on from there with no slop.
//!
//! A finger that lands while another scrolls joins the scroll: the content follows the middle of the fingers,
//! and goes on with those that stay when one lifts. Before a scroll starts, further fingers are ignored.
//!
//! The pointer leaves the window once the gesture ends. Positions and deltas are physical pixels, as the tree's pointer input takes them.

use std::{
  collections::VecDeque,
  time::{Duration, Instant},
};

use winit::event::TouchPhase;

use crate::app::{events::ScrollPhase, runtime::TouchMomentum};

/// How far apart two taps may land and still be a double tap, in logical pixels.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(super) const DOUBLE_TAP_DISTANCE: f32 = 40.0;
/// How long after a tap the next one is still a double tap; UIKit's.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(super) const DOUBLE_TAP_INTERVAL: Duration = Duration::from_millis(350);
/// How far a finger may travel and still tap, in logical pixels.
const TAP_SLOP: f64 = 10.0;
/// How long a finger rests over content that scrolls before it presses; a scroll view's delay of content
/// touches.
pub(super) const PRESS_DELAY: Duration = Duration::from_millis(150);
/// How long what a tap landed on goes on showing as pressed, so that a quick tap shows.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(super) const TAP_HIGHLIGHT: Duration = Duration::from_millis(80);
/// How long a finger rests over content that scrolls before it no longer scrolls it.
pub(super) const LONG_PRESS: Duration = Duration::from_millis(500);
/// The time between two steps of content springing back from past an end.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(super) const SPRING_STEP: Duration = Duration::from_millis(4);
/// The time between two steps of momentum. Shorter than any display's frame, so every frame has a step.
const FLING_STEP: Duration = Duration::from_millis(4);

/// What the tree is given for a touch, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum TouchAction {
  /// The pointer moves to the point; nothing is pressed, or the press is held.
  Move { x: f64, y: f64 },
  /// The left button goes down at the point.
  Press { x: f64, y: f64 },
  /// The left button comes up at the point, which clicks, and the pointer leaves.
  Release { x: f64, y: f64 },
  /// A press and its release at the point at once: a tap, whose click is not held back.
  Tap { x: f64, y: f64 },
  /// The finger rested at the point: `on_long_press`. The shell tells back whether a handler took it; see
  /// [`TouchPointer::long_press_taken`].
  LongPress { x: f64, y: f64 },
  /// The held press ends without a click, and the pointer leaves.
  CancelPress { x: f64, y: f64 },
  /// Content a finger caught and held still is let go: past an end, it springs back.
  Settle,
  /// A scroll gesture starts at the point with this first travel: the tree picks what the gesture scrolls.
  ScrollTarget { x: f64, y: f64, dx: f64, dy: f64 },
  /// A scroll of what the gesture started on, sent at the point the finger started from.
  Scroll {
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    phase: ScrollPhase,
  },
}

#[derive(Debug)]
enum State {
  Idle,
  /// The finger is down over content that scrolls, and has neither moved past the slop nor rested for
  /// [`LONG_PRESS`].
  Pending {
    id: u64,
    start: (f64, f64),
    started_at: Instant,
    /// The touch caught content that was moving: it scrolls if it moves at all, and presses nothing.
    caught: bool,
    /// The press was sent, after [`PRESS_DELAY`].
    pressed: bool,
  },
  /// The press was sent and the finger is the pointer until it lifts.
  Dragging {
    id: u64,
    /// Where the finger is.
    at: (f64, f64),
    /// When the finger becomes a long press if it stays near where it landed, and where that is.
    rest: Option<(Instant, (f64, f64))>,
    /// A handler took the long press: the finger clicks nothing when it lifts.
    taken: bool,
  },
  Scrolling {
    /// The fingers on the content and where each one is: the one that started the scroll, and those that
    /// joined it since.
    fingers: Vec<(u64, (f64, f64))>,
    /// Where the finger started: the point every scroll of the gesture is sent at, so one target takes it all.
    anchor: (f64, f64),
    /// The middle of the fingers, which the content follows.
    last: (f64, f64),
    samples: VecDeque<(Instant, (f64, f64))>,
  },
}

#[derive(Debug)]
struct Fling {
  anchor: (f64, f64),
  /// Physical pixels per second.
  velocity: (f64, f64),
  stop_speed: f64,
  last_step: Instant,
}

#[derive(Debug)]
pub(super) struct TouchPointer {
  state: State,
  fling: Option<Fling>,
  /// How a scroll goes on after the finger lifts; the app's, from the tree.
  momentum: TouchMomentum,
}

impl Default for TouchPointer {
  fn default() -> Self {
    Self {
      state: State::Idle,
      fling: None,
      momentum: TouchMomentum::default(),
    }
  }
}

impl TouchPointer {
  /// Takes the app's momentum; a scroll that is under way keeps going by it.
  #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
  pub(super) fn set_momentum(&mut self, momentum: TouchMomentum) {
    self.momentum = momentum;
  }

  /// Takes one touch event. `scale` is the window's scale factor; `springing` tells that the finger landed while
  /// content was springing back from past an end, which the shell has stopped; `scrolls` tells whether a finger
  /// that moves from a point scrolls what is under it.
  pub(super) fn touch(
    &mut self,
    id: u64,
    phase: TouchPhase,
    position: (f64, f64),
    now: Instant,
    scale: f64,
    springing: bool,
    scrolls: impl FnOnce((f64, f64)) -> bool,
  ) -> Vec<TouchAction> {
    match phase {
      TouchPhase::Started => self.started(id, position, now, springing, scrolls),
      TouchPhase::Moved => self.moved(id, position, now, scale),
      TouchPhase::Ended => self.ended(id, position, now, scale),
      TouchPhase::Cancelled => self.cancelled(id, position),
    }
  }

  fn started(
    &mut self,
    id: u64,
    position: (f64, f64),
    now: Instant,
    springing: bool,
    scrolls: impl FnOnce((f64, f64)) -> bool,
  ) -> Vec<TouchAction> {
    if let State::Scrolling {
      fingers, last, samples, ..
    } = &mut self.state
    {
      // Another finger joins the scroll, and may carry it on when the first one lifts. The content follows the
      // middle of the fingers, and does not jump to where that now is.
      fingers.push((id, position));
      *last = middle(fingers);
      samples.clear();
      return Vec::new();
    }
    if !matches!(self.state, State::Idle) {
      return Vec::new();
    }
    let mut actions = Vec::new();
    let fling = self.fling.take();
    if fling.is_some() || springing {
      self.state = State::Pending {
        id,
        start: position,
        started_at: now,
        caught: true,
        pressed: false,
      };
      actions.extend(fling.map(|fling| scroll(fling.anchor, (0.0, 0.0), ScrollPhase::End)));
      return actions;
    }
    if !scrolls(position) {
      self.state = State::Dragging {
        id,
        at: position,
        rest: Some((now + LONG_PRESS, position)),
        taken: false,
      };
      actions.push(TouchAction::Press {
        x: position.0,
        y: position.1,
      });
      return actions;
    }
    self.state = State::Pending {
      id,
      start: position,
      started_at: now,
      caught: false,
      pressed: false,
    };
    actions
  }

  fn moved(&mut self, id: u64, position: (f64, f64), now: Instant, scale: f64) -> Vec<TouchAction> {
    match &mut self.state {
      State::Pending {
        id: held,
        start,
        caught,
        pressed,
        ..
      } if *held == id => {
        let (start, pressed) = (*start, *pressed);
        // Content the finger caught goes on with its first move.
        let slop = if *caught { 0.0 } else { TAP_SLOP * scale };
        if distance(start, position) <= slop {
          return Vec::new();
        }
        self.state = State::Scrolling {
          fingers: vec![(id, position)],
          anchor: start,
          last: position,
          samples: VecDeque::from([(now, position)]),
        };
        let mut actions = Vec::new();
        if pressed {
          actions.push(TouchAction::CancelPress { x: start.0, y: start.1 });
        }
        // The travel so far says which way the gesture goes, and scrolls nothing.
        actions.push(TouchAction::ScrollTarget {
          x: start.0,
          y: start.1,
          dx: position.0 - start.0,
          dy: position.1 - start.1,
        });
        actions.push(scroll(start, (0.0, 0.0), ScrollPhase::Start));
        actions
      }
      State::Dragging { id: held, at, rest, .. } if *held == id => {
        *at = position;
        // A finger that travels is not resting.
        if rest.is_some_and(|(_, start)| distance(start, position) > TAP_SLOP * scale) {
          *rest = None;
        }
        vec![TouchAction::Move {
          x: position.0,
          y: position.1,
        }]
      }
      State::Scrolling {
        fingers,
        anchor,
        last,
        samples,
      } => {
        let Some(finger) = fingers.iter_mut().find(|(held, _)| *held == id) else {
          return Vec::new();
        };
        finger.1 = position;
        let position = middle(fingers);
        let delta = (position.0 - last.0, position.1 - last.1);
        *last = position;
        samples.push_back((now, position));
        while samples
          .front()
          .is_some_and(|(at, _)| now.saturating_duration_since(*at) > self.momentum.velocity_window)
        {
          samples.pop_front();
        }
        vec![scroll(*anchor, delta, ScrollPhase::Scroll)]
      }
      _ => Vec::new(),
    }
  }

  fn ended(&mut self, id: u64, position: (f64, f64), now: Instant, scale: f64) -> Vec<TouchAction> {
    if self.one_of_several_lifted(id) {
      return Vec::new();
    }
    if self.held_id() != Some(id) {
      return Vec::new();
    }
    match std::mem::replace(&mut self.state, State::Idle) {
      State::Pending { caught: true, .. } => vec![TouchAction::Settle],
      State::Pending {
        start, pressed: true, ..
      } => vec![TouchAction::Release { x: start.0, y: start.1 }],
      State::Pending { start, .. } => vec![TouchAction::Tap { x: start.0, y: start.1 }],
      State::Dragging { taken: true, .. } => vec![TouchAction::CancelPress {
        x: position.0,
        y: position.1,
      }],
      State::Dragging { .. } => vec![TouchAction::Release {
        x: position.0,
        y: position.1,
      }],
      State::Scrolling {
        anchor, last, samples, ..
      } => {
        let mut actions = vec![scroll(
          anchor,
          (position.0 - last.0, position.1 - last.1),
          ScrollPhase::End,
        )];
        if let Some(velocity) = lift_velocity(&samples, position, now, scale, &self.momentum) {
          // Momentum is a scroll gesture of its own, as a trackpad reports it.
          actions.push(scroll(anchor, (0.0, 0.0), ScrollPhase::Start));
          self.fling = Some(Fling {
            anchor,
            velocity,
            stop_speed: f64::from(self.momentum.stop_speed) * scale,
            last_step: now,
          });
        }
        actions
      }
      State::Idle => Vec::new(),
    }
  }

  fn cancelled(&mut self, id: u64, position: (f64, f64)) -> Vec<TouchAction> {
    if self.one_of_several_lifted(id) {
      return Vec::new();
    }
    if self.held_id() != Some(id) {
      return Vec::new();
    }
    match std::mem::replace(&mut self.state, State::Idle) {
      State::Pending { caught: true, .. } => vec![TouchAction::Settle],
      State::Pending { pressed: false, .. } | State::Idle => Vec::new(),
      State::Pending { .. } | State::Dragging { .. } => vec![TouchAction::CancelPress {
        x: position.0,
        y: position.1,
      }],
      State::Scrolling { anchor, .. } => vec![scroll(anchor, (0.0, 0.0), ScrollPhase::End)],
    }
  }

  /// Runs what time brings: the press and the long press of a finger that rested, and the next step of
  /// momentum.
  pub(super) fn tick(&mut self, now: Instant) -> Vec<TouchAction> {
    if let State::Dragging { rest, .. } = &mut self.state
      && let Some((due, start)) = *rest
      && now >= due
    {
      *rest = None;
      return vec![TouchAction::LongPress { x: start.0, y: start.1 }];
    }
    if let State::Pending {
      id,
      start,
      started_at,
      caught: false,
      pressed,
    } = &mut self.state
    {
      let rested = now.saturating_duration_since(*started_at);
      let press = (!*pressed && rested >= PRESS_DELAY).then_some(TouchAction::Press { x: start.0, y: start.1 });
      *pressed |= press.is_some();
      let mut actions: Vec<TouchAction> = press.into_iter().collect();
      if rested >= LONG_PRESS {
        actions.push(TouchAction::LongPress { x: start.0, y: start.1 });
        self.state = State::Dragging {
          id: *id,
          at: *start,
          rest: None,
          taken: false,
        };
      }
      return actions;
    }

    let Some(fling) = self.fling.as_mut() else {
      return Vec::new();
    };
    let elapsed = now.saturating_duration_since(fling.last_step);
    if elapsed < FLING_STEP {
      return Vec::new();
    }
    fling.last_step = now;
    let seconds = elapsed.as_secs_f64();
    let kept = f64::from(self.momentum.deceleration_rate).powf(seconds * 1000.0);
    let (vx, vy) = fling.velocity;
    let next = (vx * kept, vy * kept);
    // The mean of the speeds before and after the step, over its time.
    let delta = ((vx + next.0) * 0.5 * seconds, (vy + next.1) * 0.5 * seconds);
    fling.velocity = next;
    let anchor = fling.anchor;
    let mut actions = vec![scroll(anchor, delta, ScrollPhase::Scroll)];
    if next.0.hypot(next.1) < fling.stop_speed {
      self.fling = None;
      actions.push(scroll(anchor, (0.0, 0.0), ScrollPhase::End));
    }
    actions
  }

  /// When [`tick`](Self::tick) next has something to do, for the loop to wake at.
  pub(super) fn next_deadline(&self) -> Option<Instant> {
    let rest = match self.state {
      State::Pending {
        started_at,
        caught: false,
        pressed,
        ..
      } => Some(started_at + if pressed { LONG_PRESS } else { PRESS_DELAY }),
      State::Dragging { rest, .. } => rest.map(|(due, _)| due),
      _ => None,
    };
    let fling = self.fling.as_ref().map(|fling| fling.last_step + FLING_STEP);
    rest.into_iter().chain(fling).min()
  }

  /// The fingers became a gesture's: whatever the pointer was doing ends, without a click and without momentum,
  /// and the fingers are no longer followed.
  #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
  pub(super) fn abandon(&mut self) -> Vec<TouchAction> {
    self.fling = None;
    match std::mem::replace(&mut self.state, State::Idle) {
      State::Idle
      | State::Pending {
        pressed: false,
        caught: false,
        ..
      } => Vec::new(),
      State::Pending { caught: true, .. } => vec![TouchAction::Settle],
      State::Pending { start, .. } => vec![TouchAction::CancelPress { x: start.0, y: start.1 }],
      State::Dragging { at, .. } => vec![TouchAction::CancelPress { x: at.0, y: at.1 }],
      State::Scrolling { anchor, .. } => vec![scroll(anchor, (0.0, 0.0), ScrollPhase::End)],
    }
  }

  /// A handler took the long press the shell was just given: the finger clicks nothing when it lifts.
  #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
  pub(super) fn long_press_taken(&mut self) {
    if let State::Dragging { taken, .. } = &mut self.state {
      *taken = true;
    }
  }

  /// Ends momentum whose last step moved nothing: the content reached its end. Gives the scroll's last event
  /// and the velocity it ended at, in physical pixels per second, which carries the content past that end.
  pub(super) fn end_fling(&mut self) -> Option<(TouchAction, (f64, f64))> {
    let fling = self.fling.take()?;
    Some((scroll(fling.anchor, (0.0, 0.0), ScrollPhase::End), fling.velocity))
  }

  /// Whether the finger is one of several scrolling the content. It leaves the scroll, which goes on with
  /// the others from where the content is.
  fn one_of_several_lifted(&mut self, id: u64) -> bool {
    let State::Scrolling {
      fingers, last, samples, ..
    } = &mut self.state
    else {
      return false;
    };
    if fingers.len() < 2 {
      return false;
    }
    let Some(index) = fingers.iter().position(|(held, _)| *held == id) else {
      return false;
    };
    fingers.remove(index);
    *last = middle(fingers);
    samples.clear();
    true
  }

  fn held_id(&self) -> Option<u64> {
    match &self.state {
      State::Idle => None,
      State::Pending { id, .. } | State::Dragging { id, .. } => Some(*id),
      State::Scrolling { fingers, .. } => fingers.first().map(|(id, _)| *id),
    }
  }
}

fn scroll(anchor: (f64, f64), delta: (f64, f64), phase: ScrollPhase) -> TouchAction {
  TouchAction::Scroll {
    x: anchor.0,
    y: anchor.1,
    dx: delta.0,
    dy: delta.1,
    phase,
  }
}

/// Whether the actions end a scroll: of a finger, or of its momentum.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(super) fn ends_scroll(actions: &[TouchAction]) -> bool {
  actions.iter().any(|action| {
    matches!(
      action,
      TouchAction::Scroll {
        phase: ScrollPhase::End,
        ..
      }
    )
  })
}

/// The middle of the fingers; there is at least one.
fn middle(fingers: &[(u64, (f64, f64))]) -> (f64, f64) {
  let count = fingers.len().max(1) as f64;
  let (x, y) = fingers
    .iter()
    .fold((0.0, 0.0), |sum, (_, at)| (sum.0 + at.0, sum.1 + at.1));
  (x / count, y / count)
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
  (a.0 - b.0).hypot(a.1 - b.1)
}

/// The velocity a scroll ends with, in physical pixels per second, when it is fast enough to leave momentum.
/// Measured from the oldest sample of the momentum's velocity window to the lift, so a finger that stopped
/// before it lifted leaves none.
fn lift_velocity(
  samples: &VecDeque<(Instant, (f64, f64))>,
  position: (f64, f64),
  now: Instant,
  scale: f64,
  momentum: &TouchMomentum,
) -> Option<(f64, f64)> {
  let (at, from) = samples
    .iter()
    .find(|(at, _)| now.saturating_duration_since(*at) <= momentum.velocity_window)?;
  let seconds = now.saturating_duration_since(*at).as_secs_f64();
  if seconds <= 0.0 {
    return None;
  }
  let velocity = ((position.0 - from.0) / seconds, (position.1 - from.1) / seconds);
  let speed = velocity.0.hypot(velocity.1);
  if speed < f64::from(momentum.min_speed) * scale {
    return None;
  }
  let limit = (f64::from(momentum.max_speed) * scale / speed).min(1.0);
  Some((velocity.0 * limit, velocity.1 * limit))
}
