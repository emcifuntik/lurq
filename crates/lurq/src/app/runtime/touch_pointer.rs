//! A finger as the tree sees it: its own events, and the pointer it stands for.
//!
//! **Touch events.** Every finger sends `on_touch_start`, `on_touch_move`, `on_touch_end` and `on_touch_cancel`
//! to what it landed on ([`Tree::touch_event`]), and one that rests sends `on_long_press`
//! ([`Tree::touch_long_press`]).
//!
//! **Gestures.** Two fingers on one element that listens for them pinch (`on_pinch`) and rotate
//! (`on_rotate`): each is told from the other, and from two fingers that only rest or scroll, once the fingers
//! moved apart or turned by more than a slop. From then on the two fingers are the gesture's, and
//! [`Tree::touch_event`] says so, for the shell to stop treating one of them as the pointer.
//!
//! The fingers that are down together, from the first to land until the last lifts, are also told as a whole
//! ([`Session`]): a tap of however many fingers (`on_tap`), their travel while it lasts (`on_pan`), a flick
//! when they lift (`on_swipe`), and one finger coming in from an edge of the window (`on_edge_pan`), which
//! from the left edge goes back in the history as a phone does, and from the right edge forward.
//!
//! **The pointer.** A touch shell gives the first finger to the tree as the pointer through the methods here
//! rather than through the mouse's, because a finger is not a mouse in three ways a list of the platform shows:
//!
//! - it hovers nothing. The mouse events still come, `on_mouse_enter` and `on_mouse_leave` among them, but no element
//!   takes its hovered style from a finger;
//! - a tap clicks when the finger lifts, and what it tapped stays pressed for a moment after, so that a tap quicker
//!   than a frame still shows;
//! - what a finger holds stays pressed only while the finger is near it. Moved further than [`PRESS_REACH`] from it,
//!   the element no longer shows as pressed, and shows so again when the finger comes back; lifted within that reach,
//!   even beside the element, the finger clicks it.

use std::{
  collections::VecDeque,
  time::{Duration, Instant},
};

use super::{Tree, find_node_by_id, hit_test_tree, set_node_active};
use crate::{
  app::events::{
    EdgePanEvent, EventControl, GesturePhase, HistoryDirection, LongPressEvent, MouseButton, PanEvent, PinchEvent,
    RotateEvent, ScreenEdge, SwipeDirection, SwipeEvent, TapEvent, TouchEvent, TouchPhase,
  },
  core::NodeId,
  node::node::Node,
};

/// How far from what it presses a finger may go and still hold it, in logical pixels: a control's reach in
/// UIKit.
const PRESS_REACH: f32 = 70.0;

/// How far two fingers move apart or together before they pinch, in logical pixels.
const PINCH_SLOP: f32 = 8.0;
/// How far two fingers turn before they rotate, in radians: about six degrees.
const ROTATE_SLOP: f32 = 0.1;

/// How far fingers travel before they pan and are no longer a tap, in logical pixels.
const TRAVEL_SLOP: f32 = 10.0;
/// How long fingers may stay down and still tap.
const TAP_TIME: Duration = Duration::from_millis(500);
/// How far a flick goes at least, in logical pixels, how fast in logical pixels per second, and how much
/// further along its way than across it.
const SWIPE_DISTANCE: f32 = 50.0;
const SWIPE_SPEED: f32 = 300.0;
const SWIPE_STRAIGHTNESS: f32 = 2.0;
/// How near an edge of the window a finger lands to come in from it, in logical pixels.
const EDGE_WIDTH: f32 = 20.0;
/// How far across the window a finger from the left or the right edge goes to move the history, as a share of
/// the window's width, and the speed that moves it from any distance, in logical pixels per second.
const BACK_SHARE: f32 = 1.0 / 3.0;
const BACK_SPEED: f32 = 500.0;
/// The stretch of travel, ending now, a velocity is measured over.
const VELOCITY_WINDOW: Duration = Duration::from_millis(100);

/// A finger of a session: its id, where it landed and where it is, in logical pixels.
type SessionFinger = (u64, (f32, f32), (f32, f32));

/// The fingers that are down together, from the first to land until the last lifts.
struct Session {
  started: Instant,
  /// The fingers down now, with where each landed and is.
  fingers: Vec<SessionFinger>,
  /// The most that were down at once.
  most: u32,
  /// The middle of where they landed, kept as fingers join.
  landed: (f32, f32),
  /// The elements every finger landed on, from the innermost out.
  chain: Vec<NodeId>,
  /// The middle of the fingers, and how far it went since they landed. A finger that joins or leaves moves
  /// the middle, which is not travel.
  middle: (f32, f32),
  translation: (f32, f32),
  /// The translation over the last [`VELOCITY_WINDOW`].
  samples: VecDeque<(Instant, (f32, f32))>,
  /// A finger went further than [`TRAVEL_SLOP`]: this is no tap.
  travelled: bool,
  /// The pan was started, with the translation last told.
  pan: Option<(f32, f32)>,
  /// The edge the one finger landed at, and whether it came in from it.
  edge: Option<(ScreenEdge, bool)>,
  /// A handler of the edge pan prevented the default: lifting does not go back.
  edge_prevented: bool,
}

impl Session {
  fn velocity(&self, now: Instant) -> (f32, f32) {
    let Some((at, from)) = self
      .samples
      .iter()
      .find(|(at, _)| now.saturating_duration_since(*at) <= VELOCITY_WINDOW)
    else {
      return (0.0, 0.0);
    };
    let seconds = now.saturating_duration_since(*at).as_secs_f32();
    if seconds <= 0.0 {
      return (0.0, 0.0);
    }
    (
      (self.translation.0 - from.0) / seconds,
      (self.translation.1 - from.1) / seconds,
    )
  }
}

fn middle_of(fingers: &[SessionFinger]) -> (f32, f32) {
  let count = fingers.len().max(1) as f32;
  let sum = fingers
    .iter()
    .fold((0.0, 0.0), |sum, (_, _, at)| (sum.0 + at.0, sum.1 + at.1));
  (sum.0 / count, sum.1 / count)
}

/// Sends an event to the handlers `$handlers` of the elements of `$chain`, innermost first, until one stops its
/// propagation. Gives the event's control, which tells whether the default was prevented.
macro_rules! bubble {
  ($root:expr, $chain:expr, $handlers:ident, |$target:ident, $control:ident| $event:expr) => {{
    let $control = EventControl::new();
    'bubble: for $target in $chain.iter().copied() {
      let Some(node) = find_node_by_id($root, $target) else {
        continue;
      };
      let event = $event;
      for handler in &node.events.$handlers {
        handler.call(&event);
        if event.immediate_propagation_stopped() {
          break 'bubble;
        }
      }
      if event.propagation_stopped() {
        break;
      }
    }
    $control
  }};
}

/// Whose a finger is, as [`Tree::touch_event`] tells the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchRoute {
  /// The shell's: the first such finger is the pointer.
  Pointer,
  /// The app's: a handler of its `on_touch_start` prevented the default. It is not the pointer.
  Taken,
  /// A gesture's: it pinches or rotates with another finger. Neither is the pointer, and what the pointer was
  /// doing ends without a click.
  Gesture,
}

/// A finger that is down, for its events.
struct ActiveTouch {
  id: u64,
  /// What it landed on, from the innermost element out.
  targets: Vec<NodeId>,
  /// Where it is, in logical pixels.
  position: (f32, f32),
  /// A handler of its `on_touch_start` took it: it is not the pointer.
  taken: bool,
  /// It pinched or rotated: it is not the pointer, even once the other finger lifted.
  gestured: bool,
}

/// Two fingers on one element that listens for a pinch or a rotation.
struct Gesture {
  fingers: [u64; 2],
  /// The elements both fingers are on, from the innermost out.
  chain: Vec<NodeId>,
  /// How far apart the fingers were when the second landed, and which way the line between them pointed.
  landed_distance: f32,
  angle: f32,
  /// How far they turned since, turns past half a circle included.
  turned: f32,
  /// The distance the pinch started at and the scale last told, once they pinch.
  pinch: Option<(f32, f32)>,
  /// The turn the rotation started at and the rotation last told, once they rotate.
  rotate: Option<(f32, f32)>,
}

/// How far apart two points are, which way the line from the first to the second points, and its middle.
fn between(a: (f32, f32), b: (f32, f32)) -> (f32, f32, (f32, f32)) {
  let (dx, dy) = (b.0 - a.0, b.1 - a.1);
  (dx.hypot(dy), dy.atan2(dx), ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0))
}

/// What the finger that is the pointer presses.
struct Press {
  /// Where it landed, in physical pixels.
  position: (f32, f32),
  /// The element it holds, in logical pixels: x, y, width, height.
  rect: (f32, f32, f32, f32),
  /// Whether the elements under the press show as pressed.
  lit: bool,
}

impl Press {
  /// Whether the point, in logical pixels, is on the element (`reach` of 0) or within `reach` of it.
  fn holds(&self, x: f32, y: f32, reach: f32) -> bool {
    let (left, top, width, height) = self.rect;
    x >= left - reach && x <= left + width + reach && y >= top - reach && y <= top + height + reach
  }
}

#[derive(Default)]
pub(super) struct TouchPointerState {
  touches: Vec<ActiveTouch>,
  gesture: Option<Gesture>,
  session: Option<Session>,
  press: Option<Press>,
  /// A finger is the pointer: nothing is hovered.
  pub(super) down: bool,
  /// The elements a tap left showing as pressed, and until when.
  tap_highlight: Option<(Vec<NodeId>, Instant)>,
}

impl Tree {
  /// Gives a finger's event to what the finger landed on: `on_touch_start` and the rest, bubbling from the
  /// innermost element out, and the pinch or rotation it makes with another finger. `x` and `y` are physical
  /// pixels. Tells whose the finger is: the shell makes it the pointer only when it is its own.
  pub fn touch_event(&mut self, id: u64, phase: TouchPhase, x: f32, y: f32, force: Option<f32>) -> TouchRoute {
    self.touch_event_at(id, phase, x, y, force, Instant::now())
  }

  /// [`touch_event`](Self::touch_event) at a time of the caller's, for tests of what depends on how fast
  /// fingers go.
  pub(super) fn touch_event_at(
    &mut self,
    id: u64,
    phase: TouchPhase,
    x: f32,
    y: f32,
    force: Option<f32>,
    now: Instant,
  ) -> TouchRoute {
    let scale = self.scale_factor();
    let (lx, ly) = (x / scale, y / scale);
    if phase == TouchPhase::Start {
      self.rebuild_if_dirty();
      let mut targets = Vec::new();
      if let (Some(root), Some(layout)) = (&self.root, &self.last_layout) {
        let mut hits = Vec::new();
        hit_test_tree(root, layout, 0.0, 0.0, lx, ly, &mut hits);
        targets.extend(hits.iter().map(|(node, _)| node.node_id()));
      }
      self.touch_pointer.touches.retain(|touch| touch.id != id);
      self.touch_pointer.touches.push(ActiveTouch {
        id,
        targets,
        position: (lx, ly),
        taken: false,
        gestured: false,
      });
    }
    let Some(index) = self.touch_pointer.touches.iter().position(|touch| touch.id == id) else {
      return TouchRoute::Pointer;
    };
    self.touch_pointer.touches[index].position = (lx, ly);

    let control = EventControl::new();
    if let Some(root) = &self.root {
      for target in &self.touch_pointer.touches[index].targets {
        let Some(node) = find_node_by_id(root, *target) else {
          continue;
        };
        let event = TouchEvent {
          id,
          phase,
          x: lx,
          y: ly,
          force,
          target_id: *target,
          control: control.clone(),
        };
        let handlers = match phase {
          TouchPhase::Start => &node.events.on_touch_start,
          TouchPhase::Move => &node.events.on_touch_move,
          TouchPhase::End => &node.events.on_touch_end,
          TouchPhase::Cancel => &node.events.on_touch_cancel,
        };
        for handler in handlers {
          handler.call(&event);
          if event.immediate_propagation_stopped() {
            break;
          }
        }
        if event.propagation_stopped() {
          break;
        }
      }
    }

    if phase == TouchPhase::Start {
      self.touch_pointer.touches[index].taken = control.default_prevented();
    }
    match phase {
      TouchPhase::Start => self.touch_gesture_landed(),
      TouchPhase::Move => self.touch_gesture_moved(id),
      TouchPhase::End | TouchPhase::Cancel => self.touch_gesture_lifted(id),
    }
    if !self.touch_pointer.touches[index].taken {
      self.touch_session(id, phase, (lx, ly), now);
    }
    let touch = &self.touch_pointer.touches[index];
    let route = if touch.taken {
      TouchRoute::Taken
    } else if touch.gestured {
      TouchRoute::Gesture
    } else {
      TouchRoute::Pointer
    };
    if matches!(phase, TouchPhase::End | TouchPhase::Cancel) {
      self.touch_pointer.touches.remove(index);
    }
    self.apply_reactive_updates_after_event();
    route
  }

  /// Follows the fingers that are down together: their pan while it lasts, and their tap or swipe, and the
  /// way back from the left edge, when the last one lifts.
  fn touch_session(&mut self, id: u64, phase: TouchPhase, at: (f32, f32), now: Instant) {
    let window = self.window.info().logical_size();
    let state = &mut self.touch_pointer;
    let Some(index) = state.touches.iter().position(|touch| touch.id == id) else {
      return;
    };
    match phase {
      TouchPhase::Start => {
        let targets = &state.touches[index].targets;
        match &mut state.session {
          Some(session) => {
            session.fingers.push((id, at, at));
            session.most = session.most.max(session.fingers.len() as u32);
            session.chain.retain(|target| targets.contains(target));
            session.middle = middle_of(&session.fingers);
            let count = session.fingers.len() as f32;
            session.landed = (
              session.landed.0 + (at.0 - session.landed.0) / count,
              session.landed.1 + (at.1 - session.landed.1) / count,
            );
            session.samples.clear();
            // Only one finger comes in from an edge.
            if session.edge.is_some_and(|(_, came_in)| !came_in) {
              session.edge = None;
            }
          }
          None => {
            let edge = if at.0 <= EDGE_WIDTH {
              Some(ScreenEdge::Left)
            } else if at.0 >= window.width - EDGE_WIDTH {
              Some(ScreenEdge::Right)
            } else if at.1 <= EDGE_WIDTH {
              Some(ScreenEdge::Top)
            } else if at.1 >= window.height - EDGE_WIDTH {
              Some(ScreenEdge::Bottom)
            } else {
              None
            };
            state.session = Some(Session {
              started: now,
              fingers: vec![(id, at, at)],
              most: 1,
              landed: at,
              chain: targets.clone(),
              middle: at,
              translation: (0.0, 0.0),
              samples: VecDeque::new(),
              travelled: false,
              pan: None,
              edge: edge.map(|edge| (edge, false)),
              edge_prevented: false,
            });
          }
        }
      }
      TouchPhase::Move => self.touch_session_moved(id, at, now),
      TouchPhase::End | TouchPhase::Cancel => self.touch_session_lifted(id, phase == TouchPhase::Cancel, now),
    }
  }

  fn touch_session_moved(&mut self, id: u64, at: (f32, f32), now: Instant) {
    let state = &mut self.touch_pointer;
    let Some(session) = &mut state.session else {
      return;
    };
    let Some(finger) = session.fingers.iter_mut().find(|(held, ..)| *held == id) else {
      return;
    };
    finger.2 = at;
    session.travelled |= (at.0 - finger.1.0).hypot(at.1 - finger.1.1) > TRAVEL_SLOP;
    let middle = middle_of(&session.fingers);
    session.translation.0 += middle.0 - session.middle.0;
    session.translation.1 += middle.1 - session.middle.1;
    session.middle = middle;
    session.samples.push_back((now, session.translation));
    while session
      .samples
      .front()
      .is_some_and(|(at, _)| now.saturating_duration_since(*at) > VELOCITY_WINDOW)
    {
      session.samples.pop_front();
    }
    let Some(root) = &self.root else {
      return;
    };
    let (translation, velocity, fingers) = (session.translation, session.velocity(now), session.fingers.len() as u32);

    // One finger coming in from the edge it landed at.
    if let Some((edge, came_in)) = &mut session.edge {
      let (inward, across) = match edge {
        ScreenEdge::Left => (translation.0, translation.1),
        ScreenEdge::Right => (-translation.0, translation.1),
        ScreenEdge::Top => (translation.1, translation.0),
        ScreenEdge::Bottom => (-translation.1, translation.0),
      };
      let listens = || {
        session.chain.iter().any(|target| {
          find_node_by_id(root, *target).is_some_and(|node| {
            !node.events.on_edge_pan.is_empty()
              || (matches!(edge, ScreenEdge::Left | ScreenEdge::Right) && !node.events.on_history_swipe.is_empty())
          })
        })
      };
      let starts = !*came_in && inward > TRAVEL_SLOP && inward > across.abs() && listens();
      if !*came_in && !starts && session.travelled {
        // It went another way: an ordinary touch that happened to land by the edge.
        session.edge = None;
      } else if starts || *came_in {
        let phase = if starts {
          GesturePhase::Start
        } else {
          GesturePhase::Change
        };
        *came_in = true;
        let (edge, inward_speed) = (
          *edge,
          match edge {
            ScreenEdge::Left => velocity.0,
            ScreenEdge::Right => -velocity.0,
            ScreenEdge::Top => velocity.1,
            ScreenEdge::Bottom => -velocity.1,
          },
        );
        let control = bubble!(root, session.chain, on_edge_pan, |target, control| EdgePanEvent {
          phase,
          edge,
          translation: inward.max(0.0),
          velocity: inward_speed,
          x: at.0,
          y: at.1,
          target_id: target,
          control: control.clone(),
        });
        session.edge_prevented |= control.default_prevented();
        if let Some(touch) = state.touches.iter_mut().find(|touch| touch.id == id) {
          touch.gestured = true;
        }
        return;
      }
    }

    // The fingers' travel.
    let starts = session.pan.is_none() && translation.0.hypot(translation.1) > TRAVEL_SLOP;
    if !starts && session.pan.is_none() {
      return;
    }
    let last = session.pan.unwrap_or((0.0, 0.0));
    session.pan = Some(translation);
    let phase = if starts {
      GesturePhase::Start
    } else {
      GesturePhase::Change
    };
    let control = bubble!(root, session.chain, on_pan, |target, control| PanEvent {
      phase,
      fingers,
      translation_x: translation.0,
      translation_y: translation.1,
      delta_x: translation.0 - last.0,
      delta_y: translation.1 - last.1,
      velocity_x: velocity.0,
      velocity_y: velocity.1,
      x: middle.0,
      y: middle.1,
      target_id: target,
      control: control.clone(),
    });
    if starts && control.default_prevented() {
      // The pan's handler took the fingers: they are no longer the pointer's.
      let held: Vec<u64> = session.fingers.iter().map(|(id, ..)| *id).collect();
      for touch in &mut state.touches {
        touch.gestured |= held.contains(&touch.id);
      }
    }
  }

  fn touch_session_lifted(&mut self, id: u64, cancelled: bool, now: Instant) {
    let window = self.window.info().logical_size();
    let Some(session) = &mut self.touch_pointer.session else {
      return;
    };
    let Some(index) = session.fingers.iter().position(|(held, ..)| *held == id) else {
      return;
    };
    let lifted_at = session.fingers[index].2;
    if session.fingers.len() > 1 {
      session.fingers.remove(index);
      session.middle = middle_of(&session.fingers);
      session.samples.clear();
      return;
    }
    let Some(session) = self.touch_pointer.session.take() else {
      return;
    };
    let velocity = session.velocity(now);
    let (translation, middle) = (session.translation, session.middle);
    let mut history = None;
    if let Some(root) = &self.root {
      if let Some((edge, true)) = session.edge {
        let (inward, inward_speed) = match edge {
          ScreenEdge::Left => (translation.0, velocity.0),
          ScreenEdge::Right => (-translation.0, -velocity.0),
          ScreenEdge::Top => (translation.1, velocity.1),
          ScreenEdge::Bottom => (-translation.1, -velocity.1),
        };
        let control = bubble!(root, session.chain, on_edge_pan, |target, control| EdgePanEvent {
          phase: GesturePhase::End,
          edge,
          translation: inward.max(0.0),
          velocity: inward_speed,
          x: lifted_at.0,
          y: lifted_at.1,
          target_id: target,
          control: control.clone(),
        });
        let far_enough = !cancelled
          && !session.edge_prevented
          && !control.default_prevented()
          && (inward > window.width * BACK_SHARE || inward_speed > BACK_SPEED);
        history = match edge {
          ScreenEdge::Left if far_enough => Some(HistoryDirection::Back),
          ScreenEdge::Right if far_enough => Some(HistoryDirection::Forward),
          _ => None,
        };
      } else {
        if session.pan.is_some() {
          let last = session.pan.unwrap_or(translation);
          bubble!(root, session.chain, on_pan, |target, control| PanEvent {
            phase: GesturePhase::End,
            fingers: session.most,
            translation_x: translation.0,
            translation_y: translation.1,
            delta_x: translation.0 - last.0,
            delta_y: translation.1 - last.1,
            velocity_x: velocity.0,
            velocity_y: velocity.1,
            x: middle.0,
            y: middle.1,
            target_id: target,
            control: control.clone(),
          });
        }
        let lasted = now.saturating_duration_since(session.started);
        let (along, across, direction) = if translation.0.abs() >= translation.1.abs() {
          let direction = if translation.0 < 0.0 {
            SwipeDirection::Left
          } else {
            SwipeDirection::Right
          };
          (translation.0.abs(), translation.1.abs(), direction)
        } else {
          let direction = if translation.1 < 0.0 {
            SwipeDirection::Up
          } else {
            SwipeDirection::Down
          };
          (translation.1.abs(), translation.0.abs(), direction)
        };
        let speed = velocity.0.hypot(velocity.1);
        if cancelled {
          // The system took the touch: it was neither a tap nor a swipe.
        } else if !session.travelled && lasted <= TAP_TIME {
          bubble!(root, session.chain, on_tap, |target, control| TapEvent {
            fingers: session.most,
            x: session.landed.0,
            y: session.landed.1,
            target_id: target,
            control: control.clone(),
          });
        } else if along >= SWIPE_DISTANCE && along >= across * SWIPE_STRAIGHTNESS && speed >= SWIPE_SPEED {
          bubble!(root, session.chain, on_swipe, |target, control| SwipeEvent {
            direction,
            fingers: session.most,
            velocity: speed,
            x: session.landed.0,
            y: session.landed.1,
            target_id: target,
            control: control.clone(),
          });
        }
      }
    }
    if let Some(direction) = history {
      self.dispatch_history_swipe(session.landed.0, session.landed.1, direction);
    }
  }

  /// A finger landed: with another that is down, on an element that listens for a pinch or a rotation, it may
  /// start one.
  fn touch_gesture_landed(&mut self) {
    if self.touch_pointer.gesture.is_some() {
      return;
    }
    let Some(root) = &self.root else {
      return;
    };
    let mut free = self
      .touch_pointer
      .touches
      .iter()
      .filter(|touch| !touch.taken && !touch.gestured);
    let (Some(first), Some(second), None) = (free.next(), free.next(), free.next()) else {
      return;
    };
    let chain: Vec<NodeId> = first
      .targets
      .iter()
      .copied()
      .filter(|target| second.targets.contains(target))
      .collect();
    let listens = chain.iter().any(|target| {
      find_node_by_id(root, *target)
        .is_some_and(|node| !node.events.on_pinch.is_empty() || !node.events.on_rotate.is_empty())
    });
    if !listens {
      return;
    }
    let (distance, angle, _) = between(first.position, second.position);
    self.touch_pointer.gesture = Some(Gesture {
      fingers: [first.id, second.id],
      chain,
      landed_distance: distance,
      angle,
      turned: 0.0,
      pinch: None,
      rotate: None,
    });
  }

  fn touch_gesture_moved(&mut self, id: u64) {
    let state = &mut self.touch_pointer;
    let Some(gesture) = &mut state.gesture else {
      return;
    };
    if !gesture.fingers.contains(&id) {
      return;
    }
    let position = |finger: u64| {
      state
        .touches
        .iter()
        .find(|touch| touch.id == finger)
        .map(|touch| touch.position)
    };
    let (Some(first), Some(second)) = (position(gesture.fingers[0]), position(gesture.fingers[1])) else {
      return;
    };
    let (distance, angle, middle) = between(first, second);
    // The shorter way round from the last angle: the line between the fingers turns a little at a time.
    let turn = (angle - gesture.angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    gesture.angle = angle;
    gesture.turned += turn;

    let mut pinch = None;
    match &mut gesture.pinch {
      None if (distance - gesture.landed_distance).abs() > PINCH_SLOP && distance > 0.0 => {
        gesture.pinch = Some((distance, 1.0));
        pinch = Some((GesturePhase::Start, 1.0, 1.0));
      }
      Some((from, last)) if *from > 0.0 => {
        let scale = distance / *from;
        if scale != *last && *last > 0.0 {
          pinch = Some((GesturePhase::Change, scale, scale / *last));
          *last = scale;
        }
      }
      _ => {}
    }
    let mut rotate = None;
    match &mut gesture.rotate {
      None if gesture.turned.abs() > ROTATE_SLOP => {
        gesture.rotate = Some((gesture.turned, 0.0));
        rotate = Some((GesturePhase::Start, 0.0, 0.0));
      }
      Some((from, last)) => {
        let rotation = gesture.turned - *from;
        if rotation != *last {
          rotate = Some((GesturePhase::Change, rotation, rotation - *last));
          *last = rotation;
        }
      }
      None => {}
    }
    if pinch.is_none() && rotate.is_none() {
      return;
    }
    let (fingers, chain) = (gesture.fingers, gesture.chain.clone());
    for touch in &mut state.touches {
      touch.gestured |= fingers.contains(&touch.id);
    }
    if let Some(root) = &self.root {
      if let Some((phase, scale, delta)) = pinch {
        send_pinch(root, &chain, phase, scale, delta, middle);
      }
      if let Some((phase, rotation, delta)) = rotate {
        send_rotate(root, &chain, phase, rotation, delta, middle);
      }
    }
  }

  /// One of a gesture's fingers lifted: what was under way ends where it was.
  fn touch_gesture_lifted(&mut self, id: u64) {
    if !self
      .touch_pointer
      .gesture
      .as_ref()
      .is_some_and(|gesture| gesture.fingers.contains(&id))
    {
      return;
    }
    let Some(gesture) = self.touch_pointer.gesture.take() else {
      return;
    };
    let position = |finger: u64| {
      self
        .touch_pointer
        .touches
        .iter()
        .find(|touch| touch.id == finger)
        .map(|touch| touch.position)
    };
    let (Some(first), Some(second), Some(root)) =
      (position(gesture.fingers[0]), position(gesture.fingers[1]), &self.root)
    else {
      return;
    };
    let (_, _, middle) = between(first, second);
    if let Some((_, scale)) = gesture.pinch {
      send_pinch(root, &gesture.chain, GesturePhase::End, scale, 1.0, middle);
    }
    if let Some((_, rotation)) = gesture.rotate {
      send_rotate(root, &gesture.chain, GesturePhase::End, rotation, 0.0, middle);
    }
  }

  /// A finger rested at the point, in physical pixels: `on_long_press`, bubbling from the innermost element
  /// there out. `true` when a handler took it, and the finger then clicks nothing when it lifts.
  pub fn touch_long_press(&mut self, x: f32, y: f32) -> bool {
    let scale = self.scale_factor();
    let (lx, ly) = (x / scale, y / scale);
    self.rebuild_if_dirty();
    let mut taken = false;
    if let (Some(root), Some(layout)) = (&self.root, &self.last_layout) {
      let mut hits = Vec::new();
      hit_test_tree(root, layout, 0.0, 0.0, lx, ly, &mut hits);
      let control = EventControl::new();
      'bubble: for (node, _) in &hits {
        let event = LongPressEvent {
          x: lx,
          y: ly,
          target_id: node.node_id(),
          control: control.clone(),
        };
        for handler in &node.events.on_long_press {
          handler.call(&event);
          taken = true;
          if event.immediate_propagation_stopped() {
            break 'bubble;
          }
        }
        if event.propagation_stopped() {
          break;
        }
      }
    }
    self.apply_reactive_updates_after_event();
    taken
  }

  /// The finger that is the pointer presses at the point, in physical pixels.
  pub fn touch_press(&mut self, x: f32, y: f32) {
    self.end_tap_highlight();
    self.touch_pointer.down = true;
    self.mouse_move(x, y);
    self.mouse_down(x, y, MouseButton::Left);
    self.touch_pointer.press = self.touch_press_at(x, y);
    self.unhover_for_touch();
  }

  /// The finger that is the pointer moves to the point with its press held.
  pub fn touch_pointer_move(&mut self, x: f32, y: f32) {
    self.mouse_move(x, y);
    self.unhover_for_touch();
    // What a drag holds (a slider, a selection, a drag session) is not let go by distance.
    if self.has_active_input_interaction() {
      return;
    }
    let scale = self.scale_factor();
    let Some(press) = &mut self.touch_pointer.press else {
      return;
    };
    let lit = press.holds(x / scale, y / scale, PRESS_REACH);
    if lit == press.lit {
      return;
    }
    press.lit = lit;
    if let Some(root) = &self.root {
      for target in &self.active_path {
        if let Some(node) = find_node_by_id(root, *target) {
          set_node_active(node, lit);
        }
      }
    }
    self.cached_render_list = None;
    self.needs_redraw = true;
  }

  /// The finger that is the pointer lifts at the point: the press ends, with a click when the finger is on
  /// what it pressed or within reach of it, and the pointer leaves.
  pub fn touch_release(&mut self, x: f32, y: f32) {
    let scale = self.scale_factor();
    let press = self.touch_pointer.press.take();
    // Beside the element but within reach, the finger still clicks it: the release is the element's.
    let at = match press {
      Some(press)
        if !self.has_active_input_interaction()
          && !press.holds(x / scale, y / scale, 0.0)
          && press.holds(x / scale, y / scale, PRESS_REACH) =>
      {
        press.position
      }
      _ => (x, y),
    };
    self.mouse_up(at.0, at.1, MouseButton::Left);
    self.touch_pointer.down = false;
    self.mouse_leave_window();
  }

  /// The press of the finger that is the pointer ends without a click, and the pointer leaves: a scroll
  /// started, a long press was taken, or the system took the touch.
  pub fn touch_cancel_press(&mut self, x: f32, y: f32) {
    self.touch_pointer.press = None;
    self.mouse_press_taken_by_os(x, y, MouseButton::Left);
    self.touch_pointer.down = false;
    self.mouse_leave_window();
  }

  /// A tap at the point, in physical pixels: a press and its release at once, so the click is not held back,
  /// while what was tapped goes on showing as pressed for `highlight`.
  pub fn touch_tap(&mut self, x: f32, y: f32, highlight: Duration) {
    self.touch_press(x, y);
    let pressed = self.active_path.clone();
    self.touch_pointer.press = None;
    self.mouse_up(x, y, MouseButton::Left);
    self.touch_pointer.down = false;
    self.mouse_leave_window();
    // The click may have rebuilt what was tapped: only what is still there shows.
    let mut shown = Vec::new();
    if let Some(root) = &self.root {
      for target in pressed {
        if let Some(node) = find_node_by_id(root, target) {
          set_node_active(node, true);
          shown.push(target);
        }
      }
    }
    if !shown.is_empty() {
      self.touch_pointer.tap_highlight = Some((shown, Instant::now() + highlight));
      self.cached_render_list = None;
      self.needs_redraw = true;
    }
  }

  /// Ends a tap's pressed look once its time is up. `true` while one is still showing: the shell runs another
  /// turn for it.
  pub(super) fn touch_tap_highlight_tick(&mut self, now: Instant) -> bool {
    match &self.touch_pointer.tap_highlight {
      Some((_, until)) if now >= *until => {
        self.end_tap_highlight();
        false
      }
      Some(_) => true,
      None => false,
    }
  }

  pub(super) fn touch_tap_highlight_showing(&self) -> bool {
    self.touch_pointer.tap_highlight.is_some()
  }

  fn end_tap_highlight(&mut self) {
    let Some((shown, _)) = self.touch_pointer.tap_highlight.take() else {
      return;
    };
    if let Some(root) = &self.root {
      for target in shown {
        if let Some(node) = find_node_by_id(root, target) {
          set_node_active(node, false);
        }
      }
    }
    self.cached_render_list = None;
    self.needs_redraw = true;
  }

  /// What a press at the point holds: the innermost element there that takes clicks, or the innermost one.
  fn touch_press_at(&self, x: f32, y: f32) -> Option<Press> {
    let scale = self.scale_factor();
    let (root, layout) = (self.root.as_ref()?, self.last_layout.as_ref()?);
    let mut hits = Vec::new();
    hit_test_tree(root, layout, 0.0, 0.0, x / scale, y / scale, &mut hits);
    let (_, rect) = hits
      .iter()
      .find(|(node, _)| !node.events.on_click.is_empty())
      .or_else(|| hits.first())?;
    Some(Press {
      position: (x, y),
      rect: (rect.x, rect.y, rect.width, rect.height),
      lit: true,
    })
  }

  /// Takes the hovered style off what the pointer is over: a finger hovers nothing.
  fn unhover_for_touch(&mut self) {
    let Some(root) = &self.root else {
      return;
    };
    let mut changed = false;
    for target in &self.hover_path {
      if let Some(node) = find_node_by_id(root, *target) {
        changed |= node.style_state.is_hovered();
        node.set_style_hovered(false);
        if let Some((state, _)) = node.scroll_container() {
          state.set_pointer_over(false);
        }
      }
    }
    if changed {
      self.cached_render_list = None;
      self.needs_redraw = true;
    }
  }
}

fn send_pinch(root: &Node, chain: &[NodeId], phase: GesturePhase, scale: f32, delta: f32, middle: (f32, f32)) {
  let control = EventControl::new();
  for target in chain {
    let Some(node) = find_node_by_id(root, *target) else {
      continue;
    };
    let event = PinchEvent {
      phase,
      scale,
      delta,
      x: middle.0,
      y: middle.1,
      target_id: *target,
      control: control.clone(),
    };
    for handler in &node.events.on_pinch {
      handler.call(&event);
      if event.immediate_propagation_stopped() {
        return;
      }
    }
    if event.propagation_stopped() {
      return;
    }
  }
}

fn send_rotate(root: &Node, chain: &[NodeId], phase: GesturePhase, rotation: f32, delta: f32, middle: (f32, f32)) {
  let control = EventControl::new();
  for target in chain {
    let Some(node) = find_node_by_id(root, *target) else {
      continue;
    };
    let event = RotateEvent {
      phase,
      rotation,
      delta,
      x: middle.0,
      y: middle.1,
      target_id: *target,
      control: control.clone(),
    };
    for handler in &node.events.on_rotate {
      handler.call(&event);
      if event.immediate_propagation_stopped() {
        return;
      }
    }
    if event.propagation_stopped() {
      return;
    }
  }
}
