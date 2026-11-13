//! Middle-button autoscroll (lurq#78), for scroll containers that turn it on
//! (`middle_button_autoscroll`). A middle press over such a container marks
//! where the button went down; while the button is held, the content scrolls
//! toward the pointer, faster the farther the pointer is from the mark, on
//! the axes the container scrolls. The release, Esc or the pointer leaving
//! the window ends it. The content moves a step each pass, by the speed times
//! the time since the last step, and the shell asks for a frame at every
//! vblank while it moves, as it does for an animation.

use std::time::{Duration, Instant};

use super::{Tree, find_node_by_id, find_scroll_node_id_by_state, mark_scroll_layout_dirty};
use crate::{
  app::events::{MouseButton, ScrollPhase},
  core::NodeId,
  layout::{
    layout_kind::{ScrollDirection, ScrollState},
    quad::ClipRect,
    render_list::RectCmd,
  },
  node::{color::Color, cursor::CursorIcon, node::Node},
};

/// How far, in logical pixels, the pointer may move from the mark on an axis
/// before that axis scrolls.
const DEAD_ZONE: f32 = 12.0;
/// Logical pixels a second the content scrolls for each pixel past the dead
/// zone...
const SPEED_PER_PIXEL: f32 = 8.0;
/// ...plus this times the square of that distance, so a far pointer runs
/// through a long list.
const SPEED_GROWTH: f32 = 0.04;
/// The fastest the content scrolls, in logical pixels a second.
const MAX_SPEED: f32 = 12_000.0;
/// The longest time one step covers: after a stall (a slow pass, a window
/// that was hidden) the content does not jump by the whole time.
const MAX_STEP: Duration = Duration::from_millis(50);
/// The radius of the mark drawn where the button went down, logical pixels.
const MARK_RADIUS: f32 = 12.0;
const MARK_DOT_RADIUS: f32 = 2.5;

/// The autoscroll under way.
pub(super) struct Autoscroll {
  target_id: NodeId,
  state: ScrollState,
  direction: ScrollDirection,
  /// Where the button went down, logical pixels.
  origin: (f32, f32),
  /// Where the pointer is, logical pixels.
  pointer: (f32, f32),
  last_step: Instant,
  /// The content has moved since the press.
  scrolled: bool,
}

/// The speed one axis scrolls at, in logical pixels a second, for the
/// pointer `offset` pixels from the mark on that axis: none inside the dead
/// zone, then growing with the distance, signed as the offset.
pub(crate) fn axis_speed(offset: f32) -> f32 {
  let past = offset.abs() - DEAD_ZONE;
  if !past.is_finite() || past <= 0.0 {
    return 0.0;
  }
  (past * SPEED_PER_PIXEL + past * past * SPEED_GROWTH)
    .min(MAX_SPEED)
    .copysign(offset)
}

fn scrolls_x(direction: ScrollDirection) -> bool {
  matches!(direction, ScrollDirection::Horizontal | ScrollDirection::Both)
}

fn scrolls_y(direction: ScrollDirection) -> bool {
  matches!(direction, ScrollDirection::Vertical | ScrollDirection::Both)
}

impl Autoscroll {
  /// Logical pixels a second on each axis the container scrolls.
  fn velocity(&self) -> (f32, f32) {
    let x = if scrolls_x(self.direction) {
      axis_speed(self.pointer.0 - self.origin.0)
    } else {
      0.0
    };
    let y = if scrolls_y(self.direction) {
      axis_speed(self.pointer.1 - self.origin.1)
    } else {
      0.0
    };
    (x, y)
  }
}

/// Whether the container has content beyond its viewport on an axis it
/// scrolls.
fn overflows(state: &ScrollState, direction: ScrollDirection) -> bool {
  (scrolls_x(direction) && state.content_width() > state.viewport_width() + 0.5)
    || (scrolls_y(direction) && state.content_height() > state.viewport_height() + 0.5)
}

/// Whether `node` takes the middle button itself: a middle click, or a drag
/// that starts with the middle button.
fn takes_middle_button(node: &Node) -> bool {
  node
    .events
    .on_mouse_click
    .iter()
    .any(|(button, _)| *button == MouseButton::Middle)
    || node.events.start_drag_buttons.contains_button(MouseButton::Middle)
      && (!node.events.on_drag_start.is_empty()
        || !node.events.on_drag_move.is_empty()
        || !node.events.on_drag_end.is_empty())
}

/// The scroll container a middle press on `hits` (innermost first)
/// autoscrolls: the innermost one that has it on and can scroll, unless an
/// element inside it takes the middle button itself.
pub(super) fn autoscroll_target<H>(hits: &[(&Node, H)]) -> Option<(NodeId, ScrollState, ScrollDirection)> {
  for (node, _) in hits {
    if let Some((state, direction)) = node.scroll_container()
      && node.has_middle_button_autoscroll()
      && overflows(&state, direction)
    {
      return Some((node.node_id(), state, direction));
    }
    if takes_middle_button(node) {
      return None;
    }
  }
  None
}

impl Tree {
  /// Starts autoscrolling `target` from a middle press at `(x, y)`, logical
  /// pixels.
  pub(super) fn begin_autoscroll(
    &mut self,
    (target_id, state, direction): (NodeId, ScrollState, ScrollDirection),
    x: f32,
    y: f32,
  ) {
    self.autoscroll = Some(Autoscroll {
      target_id,
      state,
      direction,
      origin: (x, y),
      pointer: (x, y),
      last_step: self.now(),
      scrolled: false,
    });
    self.cursor = match direction {
      ScrollDirection::Vertical => CursorIcon::NsResize,
      ScrollDirection::Horizontal => CursorIcon::EwResize,
      ScrollDirection::Both => CursorIcon::AllScroll,
    };
    self.cached_render_list = None;
    self.needs_redraw = true;
  }

  /// Whether a middle-button autoscroll is under way.
  pub fn is_autoscrolling(&self) -> bool {
    self.autoscroll.is_some()
  }

  /// Whether the autoscroll under way moves the content: the pointer is past
  /// the dead zone on an axis the container scrolls. The shell then asks for
  /// a frame at every vblank.
  pub(crate) fn autoscroll_moving(&self) -> bool {
    self
      .autoscroll
      .as_ref()
      .is_some_and(|autoscroll| autoscroll.velocity() != (0.0, 0.0))
  }

  /// Takes a pointer move during an autoscroll, logical pixels. Returns
  /// whether one is under way: the move is then its, and goes nowhere else.
  pub(super) fn autoscroll_pointer_moved(&mut self, x: f32, y: f32) -> bool {
    let now = self.now();
    let Some(autoscroll) = self.autoscroll.as_mut() else {
      return false;
    };
    if autoscroll.velocity() == (0.0, 0.0) {
      // Time spent in the dead zone is not scrolled when the pointer leaves it.
      autoscroll.last_step = now;
    }
    autoscroll.pointer = (x, y);
    self.needs_redraw = true;
    true
  }

  /// Ends the autoscroll under way, if any. The container's scroll handlers
  /// get the end of the scroll if the content moved. Returns whether one was
  /// under way.
  pub(super) fn end_autoscroll(&mut self) -> bool {
    let Some(autoscroll) = self.autoscroll.take() else {
      return false;
    };
    if autoscroll.scrolled
      && let Some(target_id) = self.autoscroll_node(&autoscroll)
    {
      let (x, y) = autoscroll.pointer;
      self.dispatch_scroll_handlers_for_node(target_id, x, y, 0.0, 0.0, ScrollPhase::End);
    }
    self.cursor = CursorIcon::Default;
    self.cached_render_list = None;
    self.needs_redraw = true;
    true
  }

  /// The node the autoscroll scrolls: its own, or, when a rebuild replaced
  /// it, the node that now holds its scroll state.
  fn autoscroll_node(&self, autoscroll: &Autoscroll) -> Option<NodeId> {
    let root = self.root.as_ref()?;
    if find_node_by_id(root, autoscroll.target_id).is_some() {
      return Some(autoscroll.target_id);
    }
    find_scroll_node_id_by_state(root, &autoscroll.state)
  }

  /// Moves the content by the autoscroll's speed for the time since its last
  /// step, and runs the container's scroll and scroll-reach handlers as the
  /// wheel does. Ends the autoscroll if its container is gone.
  pub(super) fn step_autoscroll(&mut self) {
    let now = self.now();
    let Some(autoscroll) = self.autoscroll.as_mut() else {
      return;
    };
    let elapsed = now.saturating_duration_since(autoscroll.last_step).min(MAX_STEP);
    autoscroll.last_step = now;
    let (vx, vy) = autoscroll.velocity();
    if (vx, vy) == (0.0, 0.0) || elapsed.is_zero() {
      return;
    }
    let seconds = elapsed.as_secs_f32();
    let state = autoscroll.state.clone();
    let (x, y) = autoscroll.pointer;
    let previous = (state.scroll_x(), state.scroll_y());
    let previous_max_scroll_y = (state.content_height() - state.viewport_height()).max(0.0);
    state.scroll_by_with_overflow(vx * seconds, vy * seconds);
    if (state.scroll_x(), state.scroll_y()) == previous {
      return;
    }
    let Some(target_id) = self
      .autoscroll
      .as_ref()
      .and_then(|autoscroll| self.autoscroll_node(autoscroll))
    else {
      self.end_autoscroll();
      return;
    };
    if let Some(autoscroll) = self.autoscroll.as_mut() {
      autoscroll.target_id = target_id;
      if !autoscroll.scrolled {
        autoscroll.scrolled = true;
        self.dispatch_scroll_handlers_for_node(target_id, x, y, 0.0, 0.0, ScrollPhase::Start);
      }
    }
    if let Some(root) = &self.root {
      mark_scroll_layout_dirty(root, target_id);
    }
    self.dispatch_scroll_handlers_for_node(target_id, x, y, 0.0, 0.0, ScrollPhase::Scroll);
    self.dispatch_scroll_reach_handlers_for_node(
      target_id,
      x,
      y,
      0.0,
      0.0,
      ScrollPhase::Scroll,
      previous.1,
      previous_max_scroll_y,
    );
    self.needs_redraw = true;
    self.apply_reactive_updates_after_event();
  }

  /// Draws the mark where the button went down, in physical pixels, over the
  /// content.
  pub(super) fn push_autoscroll_mark(&self, rects: &mut Vec<RectCmd>, order: usize, scale: f32) {
    let Some(autoscroll) = &self.autoscroll else {
      return;
    };
    let (x, y) = (autoscroll.origin.0 * scale, autoscroll.origin.1 * scale);
    let circle = |radius: f32, color: Color, stroke: f32, stroke_color: Color, order: usize| RectCmd {
      order,
      x: x - radius * scale,
      y: y - radius * scale,
      width: radius * 2.0 * scale,
      height: radius * 2.0 * scale,
      color,
      radii: [radius * scale; 4],
      stroke: [stroke * scale; 4],
      stroke_color,
      stroke_style: crate::layout::render_list::StrokeStyle::default(),
      transform: [1.0, 0.0, 0.0, 1.0],
      transform_origin: [0.0, 0.0],
      clip: ClipRect::default(),
      gradient: None,
      shadow: None,
    };
    rects.push(circle(
      MARK_RADIUS,
      Color::new(255, 255, 255, 230),
      1.0,
      Color::new(75, 85, 99, 255),
      order,
    ));
    rects.push(circle(
      MARK_DOT_RADIUS,
      Color::new(75, 85, 99, 255),
      0.0,
      Color::new(0, 0, 0, 0),
      order + 1,
    ));
  }
}
