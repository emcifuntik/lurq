use super::EventControl;
use crate::core::NodeId;

/// What a finger did: the handler the event is given to says which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
  /// The finger landed: `on_touch_start`.
  Start,
  /// It moved: `on_touch_move`.
  Move,
  /// It lifted: `on_touch_end`.
  End,
  /// The system took it away (a call came in, a system gesture began): `on_touch_cancel`.
  Cancel,
}

/// A finger on a touch screen: `on_touch_start`, `on_touch_move`, `on_touch_end` and `on_touch_cancel`. Every
/// finger sends its own, told apart by `id`, so two fingers can be followed at once.
///
/// The events of one finger all go to what it landed on, wherever it moves, and bubble from the innermost
/// element there to its ancestors. `x` and `y` are logical pixels in the window.
///
/// The first finger is also the pointer: after its touch events come the mouse events, the click and the
/// scrolling it stands for. A handler of `on_touch_start` that calls `prevent_default` takes the finger for
/// itself: none of those follow for this touch, and the app tells from the touch events what it does.
#[derive(Debug, Clone)]
pub struct TouchEvent {
  /// Which finger: the same for all the events of one touch, and unlike that of any other finger down.
  pub id: u64,
  pub phase: TouchPhase,
  pub x: f32,
  pub y: f32,
  /// How hard the finger presses, from 0 to 1, where the screen measures it.
  pub force: Option<f32>,
  pub target_id: NodeId,
  pub(crate) control: EventControl,
}

/// A finger rested on an element: `on_long_press`. It landed, stayed within a few points for half a second and
/// is still down. `x` and `y` are logical pixels in the window. The event bubbles from the innermost element
/// under the finger to its ancestors.
///
/// A long press that a handler took is not a click as well: lifting the finger clicks nothing.
#[derive(Debug, Clone)]
pub struct LongPressEvent {
  pub x: f32,
  pub y: f32,
  pub target_id: NodeId,
  pub(crate) control: EventControl,
}

/// Where a gesture of two fingers is: the same handler takes all three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GesturePhase {
  /// The fingers moved enough to tell the gesture: `scale` is 1 and `rotation` 0.
  Start,
  Change,
  /// A finger lifted, or the system took the touch.
  End,
}

/// Two fingers moving apart or together on an element: `on_pinch`. Both landed on the element or inside it.
/// The event bubbles from the innermost element both fingers are on to its ancestors.
///
/// While a pinch or a rotation is under way the two fingers are the gesture's: they press, click and scroll
/// nothing.
#[derive(Debug, Clone)]
pub struct PinchEvent {
  pub phase: GesturePhase,
  /// How far apart the fingers are, as a share of how far apart they were when the gesture started: above 1
  /// when they moved apart.
  pub scale: f32,
  /// The same since the previous event: multiply what is shown by it.
  pub delta: f32,
  /// The middle of the two fingers, in logical pixels in the window.
  pub x: f32,
  pub y: f32,
  pub target_id: NodeId,
  pub(crate) control: EventControl,
}

/// Two fingers turning about their middle on an element: `on_rotate`. Both landed on the element or inside it.
/// The event bubbles from the innermost element both fingers are on to its ancestors.
///
/// While a pinch or a rotation is under way the two fingers are the gesture's: they press, click and scroll
/// nothing.
#[derive(Debug, Clone)]
pub struct RotateEvent {
  pub phase: GesturePhase,
  /// How far the fingers turned since the gesture started, in radians, clockwise on the screen.
  pub rotation: f32,
  /// The same since the previous event.
  pub delta: f32,
  /// The middle of the two fingers, in logical pixels in the window.
  pub x: f32,
  pub y: f32,
  pub target_id: NodeId,
  pub(crate) control: EventControl,
}

/// One or more fingers touched an element and lifted without travelling: `on_tap`. A tap of one finger is a
/// click as well; `fingers` tells a tap of two or three from it. All the fingers landed on the element or
/// inside it, stayed within a few points and lifted within a third of a second. The event bubbles from the
/// innermost element all the fingers are on to its ancestors.
#[derive(Debug, Clone)]
pub struct TapEvent {
  /// How many fingers were down at once.
  pub fingers: u32,
  /// The middle of where the fingers landed, in logical pixels in the window.
  pub x: f32,
  pub y: f32,
  pub target_id: NodeId,
  pub(crate) control: EventControl,
}

/// Which way fingers went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
  Left,
  Right,
  Up,
  Down,
}

/// Fingers flicked across an element and lifted: `on_swipe`. They went mostly one way, far and fast enough to
/// be meant. The event bubbles from the innermost element all the fingers landed on to its ancestors.
///
/// A swipe is told when the fingers lift, after whatever they scrolled or dragged on the way; for a gesture
/// that follows the fingers while they move there is [`PanEvent`].
#[derive(Debug, Clone)]
pub struct SwipeEvent {
  pub direction: SwipeDirection,
  /// How many fingers were down at once.
  pub fingers: u32,
  /// How fast the fingers went, in logical pixels per second.
  pub velocity: f32,
  /// The middle of where the fingers landed, in logical pixels in the window.
  pub x: f32,
  pub y: f32,
  pub target_id: NodeId,
  pub(crate) control: EventControl,
}

/// Fingers travelling over an element: `on_pan`, from the moment they moved more than a few points until they
/// lift. The event bubbles from the innermost element all the fingers landed on to its ancestors.
///
/// A pan is told alongside what the first finger does as the pointer: its scroll, or its press and mouse
/// moves. A handler that calls `prevent_default` on the event that starts the pan takes the fingers: what the
/// pointer was doing ends without a click, and the fingers scroll and press nothing more.
#[derive(Debug, Clone)]
pub struct PanEvent {
  pub phase: GesturePhase,
  /// How many fingers are down, or were when the last one lifted.
  pub fingers: u32,
  /// How far the middle of the fingers went since they landed, in logical pixels.
  pub translation_x: f32,
  pub translation_y: f32,
  /// The same since the previous event.
  pub delta_x: f32,
  pub delta_y: f32,
  /// How fast it is going, in logical pixels per second.
  pub velocity_x: f32,
  pub velocity_y: f32,
  /// The middle of the fingers, in logical pixels in the window.
  pub x: f32,
  pub y: f32,
  pub target_id: NodeId,
  pub(crate) control: EventControl,
}

/// An edge of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenEdge {
  Left,
  Right,
  Top,
  Bottom,
}

/// A finger that landed at an edge of the window and travelled inward: `on_edge_pan`, from the moment it moved
/// more than a few points until it lifts. The event bubbles from the innermost element the finger landed on
/// to its ancestors, and the finger is the gesture's: it scrolls and presses nothing.
///
/// From the left edge this is how a phone goes back, and from the right edge how a browser on one goes
/// forward. When the finger lifts having gone a third of the way across, or going fast, and no handler
/// prevented the default, `on_history_swipe` follows with `HistoryDirection::Back` or
/// `HistoryDirection::Forward`, as for a trackpad's swipe between pages.
#[derive(Debug, Clone)]
pub struct EdgePanEvent {
  pub phase: GesturePhase,
  pub edge: ScreenEdge,
  /// How far the finger is from the edge it started at, in logical pixels: 0 or more.
  pub translation: f32,
  /// How fast it is going away from that edge, in logical pixels per second.
  pub velocity: f32,
  /// The finger, in logical pixels in the window.
  pub x: f32,
  pub y: f32,
  pub target_id: NodeId,
  pub(crate) control: EventControl,
}

macro_rules! event_control {
  ($event:ty) => {
    impl $event {
      pub fn prevent_default(&self) {
        self.control.prevent_default();
      }

      pub fn default_prevented(&self) -> bool {
        self.control.default_prevented()
      }

      pub fn stop_propagation(&self) {
        self.control.stop_propagation();
      }

      pub fn propagation_stopped(&self) -> bool {
        self.control.propagation_stopped()
      }

      pub fn stop_immediate_propagation(&self) {
        self.control.stop_immediate_propagation();
      }

      pub fn immediate_propagation_stopped(&self) -> bool {
        self.control.immediate_propagation_stopped()
      }
    }
  };
}

event_control!(TouchEvent);
event_control!(LongPressEvent);
event_control!(PinchEvent);
event_control!(RotateEvent);
event_control!(TapEvent);
event_control!(SwipeEvent);
event_control!(PanEvent);
event_control!(EdgePanEvent);
