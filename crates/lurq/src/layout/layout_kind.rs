use std::{
  sync::{Arc, Mutex},
  time::Instant,
};

use crate::{
  layout::{
    Alignment, StackAlignment,
    scrollbar::{
      ScrollBarGeometry, ScrollBarPart, ScrollBarReveal, ScrollBarRevealFrame, ScrollBarStyle, ScrollBarVisibility,
    },
  },
  node::{dimension::Dimension, spacing_value::SpacingValue},
};

const DEFAULT_FLEX_GROW: f32 = 1.0;
const DEFAULT_FLEX_SHRINK: f32 = 0.0;
const SCROLL_END_EPSILON: f32 = 2.0;
/// Text taller than its box by less than this fits: rows of a fractional
/// line height add up to a hair more than the box sized from them.
const TEXT_OVERFLOW_EPSILON: f32 = 0.5;

#[derive(Clone)]
pub enum LayoutKind {
  Leaf,
  Row {
    spacing: SpacingValue,
    align: Alignment,
    justify: Justify,
    wrap: FlexWrap,
  },
  Column {
    spacing: SpacingValue,
    align: Alignment,
    justify: Justify,
    wrap: FlexWrap,
  },
  Stack {
    align: StackAlignment,
  },
  LogicalModifier,
  ScrollModifier {
    state: ScrollState,
    direction: ScrollDirection,
    culling: bool,
    /// A middle-button press scrolls the container by how far the pointer
    /// moves from where the button went down; see
    /// [`Node::middle_button_autoscroll`](crate::node::node::Node::middle_button_autoscroll).
    middle_button_autoscroll: bool,
  },
}

#[derive(Clone)]
pub struct ScrollState {
  inner: Arc<Mutex<ScrollStateInner>>,
}

/// A thumb at `start`, `length` long, while the content is drawn `overscroll` past an end: it shortens by as
/// much against that end, as a scroll indicator of the platform does, down to a dot as wide as the bar.
fn squeezed_thumb(start: f32, length: f32, overscroll: f32, bar_width: f32) -> (f32, f32) {
  if overscroll == 0.0 {
    return (start, length);
  }
  let squeezed = (length - overscroll.abs()).max(bar_width.min(length));
  if overscroll < 0.0 {
    (start, squeezed)
  } else {
    (start + length - squeezed, squeezed)
  }
}

#[derive(Clone, Copy)]
enum PendingScroll {
  Offset(f32),
  Start,
  End,
  PreservePrependAnchor {
    previous_content: f32,
    previous_scroll: f32,
  },
  /// Scroll by the least amount that shows `[start, end]` of the content.
  Reveal {
    start: f32,
    end: f32,
  },
  /// The offset that keeps what is shown in place after the content above it
  /// changed height (a virtualized list's anchor row).
  Anchor(f32),
}

impl PendingScroll {
  /// A scroll to a place someone asked for (an offset, an edge, a reveal),
  /// unlike a prepend anchor or a kept anchor, which only keep the current
  /// place.
  fn is_request(self) -> bool {
    !matches!(self, Self::PreservePrependAnchor { .. } | Self::Anchor(_))
  }
}

/// Scroll position and range when the current layout pass first measured
/// this scroll container.
#[derive(Clone, Copy)]
struct ScrollPassStart {
  scroll_x: f32,
  scroll_y: f32,
  max_scroll_x: f32,
  max_scroll_y: f32,
}

struct ScrollStateInner {
  scroll_x: f32,
  scroll_y: f32,
  /// How far the content is drawn past its ends; see [`ScrollState::set_overscroll`].
  overscroll_x: f32,
  overscroll_y: f32,
  /// See [`ScrollState::set_scrolls_to_top`].
  scrolls_to_top: bool,
  pending_scroll_x: Option<PendingScroll>,
  pending_scroll_y: Option<PendingScroll>,
  max_scroll_x: f32,
  max_scroll_y: f32,
  content_width: f32,
  content_height: f32,
  container_width: f32,
  container_height: f32,
  viewport_width: f32,
  viewport_height: f32,
  viewport_abs_x: f32,
  viewport_abs_y: f32,
  /// Viewport top in layout space (the space of `ElementRef` positions), set
  /// when element refs are updated after layout — unlike `viewport_abs_y`,
  /// which is set while painting and lags one frame behind layout.
  layout_viewport_y: f32,
  /// Set by the first measurement of a layout pass; later measurements in the
  /// same pass (a flex parent re-laying out a shrunk or stretched child)
  /// restart from it, so the final measurement decides the offset.
  pass_start: Option<ScrollPassStart>,
  thumb_hovered: bool,
  /// Whether the pointer is over the scroll container, anywhere in it.
  pointer_over: bool,
  /// Whether a [`ScrollBarVisibility::OnHover`] bar is shown.
  reveal: ScrollBarReveal,
  /// The offset the last painted frame showed, which tells a frame that the
  /// content scrolled; `None` before the first frame.
  painted_scroll: Option<(f32, f32)>,
  dragging: bool,
  drag_start_x: f32,
  drag_start_y: f32,
  drag_start_scroll_x: f32,
  drag_start_scroll_y: f32,
  scrollbar_style: ScrollBarStyle,
  scroll_dirty: bool,
  layout_subscribers: crate::core::layout_dirty::LayoutSubscribers,
  equal_overflow_scroll_probe_enabled: bool,
  /// Vertical scroll requests that took effect: `set_scroll` calls and
  /// pending requests (see [`PendingScroll::is_request`]) resolved by a
  /// layout pass. Lets a scroll anchor captured earlier tell that it stands
  /// for a position someone has since scrolled away from.
  scroll_y_requests: u64,
}

impl ScrollState {
  pub(crate) fn track_layout(&self, watch: &Arc<crate::core::layout_dirty::LayoutWatch>) {
    self.inner.lock().unwrap().layout_subscribers.bind(watch);
  }

  pub fn new() -> Self {
    Self {
      inner: Arc::new(Mutex::new(ScrollStateInner {
        scroll_x: 0.0,
        scroll_y: 0.0,
        overscroll_x: 0.0,
        overscroll_y: 0.0,
        scrolls_to_top: true,
        pending_scroll_x: None,
        pending_scroll_y: None,
        max_scroll_x: 0.0,
        max_scroll_y: 0.0,
        content_width: 0.0,
        content_height: 0.0,
        container_width: 0.0,
        container_height: 0.0,
        viewport_width: 0.0,
        viewport_height: 0.0,
        viewport_abs_x: 0.0,
        viewport_abs_y: 0.0,
        layout_viewport_y: 0.0,
        pass_start: None,
        thumb_hovered: false,
        pointer_over: false,
        reveal: ScrollBarReveal::default(),
        painted_scroll: None,
        dragging: false,
        drag_start_x: 0.0,
        drag_start_y: 0.0,
        drag_start_scroll_x: 0.0,
        drag_start_scroll_y: 0.0,
        scrollbar_style: ScrollBarStyle::default(),
        scroll_dirty: false,
        layout_subscribers: Default::default(),
        equal_overflow_scroll_probe_enabled: false,
        scroll_y_requests: 0,
      })),
    }
  }

  pub fn scroll_x(&self) -> f32 {
    self.inner.lock().unwrap().scroll_x
  }

  /// Whether a tap on a phone's status bar scrolls this container to its top; it does unless set otherwise.
  /// The tap scrolls one container: the outermost under the middle of the window that scrolls down, is away
  /// from its top and takes the tap. A screen with more than one such list turns it off on those the tap is
  /// not for.
  pub fn set_scrolls_to_top(&self, scrolls_to_top: bool) {
    self.inner.lock().unwrap().scrolls_to_top = scrolls_to_top;
  }

  pub fn scrolls_to_top(&self) -> bool {
    self.inner.lock().unwrap().scrolls_to_top
  }

  /// How far the content is drawn past its ends, along each axis: negative before the start, positive beyond
  /// the end. The offset itself stays within the content.
  pub fn overscroll(&self) -> (f32, f32) {
    let inner = self.inner.lock().unwrap();
    (inner.overscroll_x, inner.overscroll_y)
  }

  /// Draws the content past its ends: what a touch shell does while a finger pulls a list beyond its end, and
  /// while the list springs back. Drawn in the next frame.
  pub fn set_overscroll(&self, x: f32, y: f32) {
    let mut inner = self.inner.lock().unwrap();
    if (inner.overscroll_x, inner.overscroll_y) == (x, y) {
      return;
    }
    (inner.overscroll_x, inner.overscroll_y) = (x, y);
    inner.scroll_dirty = true;
    inner.layout_subscribers.mark();
  }

  /// Where the content is laid out relative to the viewport: the scroll offset and the overscroll.
  pub(crate) fn content_offset(&self) -> crate::layout::offset::Offset {
    let inner = self.inner.lock().unwrap();
    crate::layout::offset::Offset::new(
      -(inner.scroll_x + inner.overscroll_x),
      -(inner.scroll_y + inner.overscroll_y),
    )
  }

  pub fn scroll_y(&self) -> f32 {
    self.inner.lock().unwrap().scroll_y
  }

  /// Scrolls to `(x, y)`, clamped to the content as last laid out. The new offset is drawn in the next frame,
  /// wherever the call comes from: an event handler, a timer or code outside any event. An offset the content is
  /// already at asks for no frame.
  pub fn set_scroll(&self, x: f32, y: f32) {
    let mut inner = self.inner.lock().unwrap();
    let offset = (x.clamp(0.0, inner.max_scroll_x), y.clamp(0.0, inner.max_scroll_y));
    inner.scroll_y_requests = inner.scroll_y_requests.wrapping_add(1);
    if offset == (inner.scroll_x, inner.scroll_y) {
      return;
    }
    (inner.scroll_x, inner.scroll_y) = offset;
    inner.scroll_dirty = true;
    inner.layout_subscribers.mark();
  }

  /// Queue an absolute scroll offset to resolve after the next layout measurement, which the queued offset asks
  /// for, wherever the call comes from. Queuing the offset the content is already at, with no other scroll
  /// queued, asks for no layout or frame: it resolves in the next layout that comes.
  pub fn set_scroll_pending(&self, x: f32, y: f32) {
    let mut inner = self.inner.lock().unwrap();
    let offset = (x.max(0.0), y.max(0.0));
    let already_there = inner.pending_scroll_x.is_none()
      && inner.pending_scroll_y.is_none()
      && offset == (inner.scroll_x, inner.scroll_y);
    inner.pending_scroll_x = Some(PendingScroll::Offset(offset.0));
    inner.pending_scroll_y = Some(PendingScroll::Offset(offset.1));
    if already_there {
      return;
    }
    inner.scroll_dirty = true;
    inner.layout_subscribers.mark();
  }

  /// Queue the smallest vertical scroll that shows content rows
  /// `start..end`, resolved after the next layout measurement.
  pub(crate) fn reveal_y_pending(&self, start: f32, end: f32) {
    let mut inner = self.inner.lock().unwrap();
    inner.pending_scroll_y = Some(PendingScroll::Reveal { start, end });
    inner.scroll_dirty = true;
    inner.layout_subscribers.mark();
  }

  /// Queue the vertical offset that keeps the shown content in place once the
  /// content above it changed height, resolved after the next layout
  /// measurement. Not a scroll request: a request already queued wins, and
  /// it does not count in `scroll_y_requests`.
  pub(crate) fn keep_scroll_y_pending(&self, y: f32) {
    let mut inner = self.inner.lock().unwrap();
    if inner.pending_scroll_y.is_some_and(PendingScroll::is_request) {
      return;
    }
    inner.pending_scroll_y = Some(PendingScroll::Anchor(y.max(0.0)));
    inner.scroll_dirty = true;
    inner.layout_subscribers.mark();
  }

  /// Queue a vertical scroll to the top after the next layout measurement.
  pub fn scroll_to_top_pending(&self) {
    let mut inner = self.inner.lock().unwrap();
    inner.pending_scroll_y = Some(PendingScroll::Start);
    inner.scroll_dirty = true;
    inner.layout_subscribers.mark();
  }

  /// Queue a vertical scroll to the bottom after the next layout measurement.
  pub fn scroll_to_bottom_pending(&self) {
    let mut inner = self.inner.lock().unwrap();
    inner.pending_scroll_y = Some(PendingScroll::End);
    inner.scroll_dirty = true;
    inner.layout_subscribers.mark();
  }

  /// Whether a scroll to the bottom (`scroll_to_bottom_pending`, a stick to
  /// the bottom) waits for the next layout measurement.
  pub(crate) fn opens_at_end(&self) -> bool {
    matches!(self.inner.lock().unwrap().pending_scroll_y, Some(PendingScroll::End))
  }

  /// The vertical offset a queued `set_scroll_pending` waits to land on.
  pub(crate) fn pending_scroll_y_offset(&self) -> Option<f32> {
    match self.inner.lock().unwrap().pending_scroll_y {
      Some(PendingScroll::Offset(offset)) => Some(offset),
      _ => None,
    }
  }

  /// Drops a queued vertical scroll when it is to `offset` (a
  /// `set_scroll_pending` whose reason is gone); any other request stays.
  pub(crate) fn drop_pending_scroll_y_offset(&self, offset: f32) {
    let mut inner = self.inner.lock().unwrap();
    if matches!(inner.pending_scroll_y, Some(PendingScroll::Offset(pending)) if pending == offset) {
      inner.pending_scroll_y = None;
    }
  }

  /// Whether a vertical scroll request (an offset, an edge or a reveal, not a
  /// prepend anchor) waits for the next layout measurement.
  pub(crate) fn has_pending_scroll_y_request(&self) -> bool {
    self
      .inner
      .lock()
      .unwrap()
      .pending_scroll_y
      .is_some_and(PendingScroll::is_request)
  }

  /// The vertical offset the next layout measurement settles on when it
  /// measures `content_height` of content in the current viewport, as
  /// [`Self::update_layout_with_container`] resolves it: a pending scroll
  /// lands, an offset at the bottom stays at the bottom, any other keeps its
  /// place within the new range.
  pub(crate) fn next_layout_scroll_y(&self, content_height: f32) -> f32 {
    let inner = self.inner.lock().unwrap();
    let max_scroll = (content_height - inner.viewport_height).max(0.0);
    let was_at_bottom = inner.max_scroll_y > 0.0 && inner.max_scroll_y - inner.scroll_y <= SCROLL_END_EPSILON;
    let target = match inner.pending_scroll_y {
      Some(pending) => resolve_pending_scroll(
        pending,
        content_height,
        max_scroll,
        inner.scroll_y,
        inner.viewport_height,
      ),
      None if was_at_bottom => max_scroll,
      None => inner.scroll_y,
    };
    target.clamp(0.0, max_scroll)
  }

  /// Count of vertical scroll requests that took effect; see
  /// `ScrollStateInner::scroll_y_requests`.
  pub(crate) fn scroll_y_requests(&self) -> u64 {
    self.inner.lock().unwrap().scroll_y_requests
  }

  pub(crate) fn equal_overflow_scroll_probe_enabled(&self) -> bool {
    self.inner.lock().unwrap().equal_overflow_scroll_probe_enabled
  }

  /// Queue a horizontal scroll to the left edge after the next layout measurement.
  pub fn scroll_to_left_pending(&self) {
    let mut inner = self.inner.lock().unwrap();
    inner.pending_scroll_x = Some(PendingScroll::Start);
    inner.scroll_dirty = true;
    inner.layout_subscribers.mark();
  }

  /// Queue a horizontal scroll to the right edge after the next layout measurement.
  pub fn scroll_to_right_pending(&self) {
    let mut inner = self.inner.lock().unwrap();
    inner.pending_scroll_x = Some(PendingScroll::End);
    inner.scroll_dirty = true;
    inner.layout_subscribers.mark();
  }

  /// Queue a bottom scroll only when the current vertical offset is within `threshold` of the bottom.
  pub fn stick_to_bottom_if_near_end(&self, threshold: f32) -> bool {
    let mut inner = self.inner.lock().unwrap();
    if inner.max_scroll_y - inner.scroll_y <= threshold.max(0.0) {
      inner.pending_scroll_y = Some(PendingScroll::End);
      inner.scroll_dirty = true;
      inner.layout_subscribers.mark();
      true
    } else {
      false
    }
  }

  /// Queue a right-edge scroll only when the current horizontal offset is within `threshold` of the right edge.
  pub fn stick_to_right_if_near_end(&self, threshold: f32) -> bool {
    let mut inner = self.inner.lock().unwrap();
    if inner.max_scroll_x - inner.scroll_x <= threshold.max(0.0) {
      inner.pending_scroll_x = Some(PendingScroll::End);
      inner.scroll_dirty = true;
      inner.layout_subscribers.mark();
      true
    } else {
      false
    }
  }

  /// Preserve the current vertical viewport position when content is prepended before the next layout.
  pub fn preserve_prepend_anchor_pending(&self) {
    let mut inner = self.inner.lock().unwrap();
    inner.pending_scroll_y = Some(PendingScroll::PreservePrependAnchor {
      previous_content: inner.content_height,
      previous_scroll: inner.scroll_y,
    });
    inner.scroll_dirty = true;
    inner.layout_subscribers.mark();
  }

  /// Preserve the current horizontal viewport position when content is prepended before the next layout.
  pub fn preserve_horizontal_prepend_anchor_pending(&self) {
    let mut inner = self.inner.lock().unwrap();
    inner.pending_scroll_x = Some(PendingScroll::PreservePrependAnchor {
      previous_content: inner.content_width,
      previous_scroll: inner.scroll_x,
    });
    inner.scroll_dirty = true;
    inner.layout_subscribers.mark();
  }

  pub fn scroll_by(&self, dx: f32, dy: f32) {
    self.scroll_by_with_overflow(dx, dy);
  }

  pub fn scroll_by_with_overflow(&self, dx: f32, dy: f32) -> (f32, f32) {
    let mut inner = self.inner.lock().unwrap();
    if dx != 0.0 {
      inner.pending_scroll_x = None;
    }
    if dy != 0.0 {
      inner.pending_scroll_y = None;
    }

    let old_x = inner.scroll_x;
    let old_y = inner.scroll_y;
    inner.scroll_x = (inner.scroll_x + dx).clamp(0.0, inner.max_scroll_x);
    inner.scroll_y = (inner.scroll_y + dy).clamp(0.0, inner.max_scroll_y);

    let consumed_x = inner.scroll_x - old_x;
    let consumed_y = inner.scroll_y - old_y;
    if consumed_x != 0.0 || consumed_y != 0.0 {
      inner.scroll_dirty = true;
      inner.layout_subscribers.mark();
    }

    (dx - consumed_x, dy - consumed_y)
  }

  pub fn content_width(&self) -> f32 {
    self.inner.lock().unwrap().content_width
  }

  pub fn content_height(&self) -> f32 {
    self.inner.lock().unwrap().content_height
  }

  pub fn viewport_width(&self) -> f32 {
    self.inner.lock().unwrap().viewport_width
  }

  pub fn viewport_height(&self) -> f32 {
    self.inner.lock().unwrap().viewport_height
  }

  /// Whether two handles share the same underlying scroll state.
  pub(crate) fn ptr_eq(&self, other: &ScrollState) -> bool {
    Arc::ptr_eq(&self.inner, &other.inner)
  }

  pub fn style(&self) -> ScrollBarStyle {
    self.inner.lock().unwrap().scrollbar_style.clone()
  }

  pub fn set_style(&self, style: ScrollBarStyle) {
    self.inner.lock().unwrap().scrollbar_style = style;
  }

  pub fn is_thumb_hovered(&self) -> bool {
    self.inner.lock().unwrap().thumb_hovered
  }

  pub fn is_dragging(&self) -> bool {
    self.inner.lock().unwrap().dragging
  }

  pub fn set_thumb_hovered(&self, hovered: bool) {
    self.inner.lock().unwrap().thumb_hovered = hovered;
  }

  /// Records whether the pointer is over the scroll container, which holds
  /// a [`ScrollBarVisibility::OnHover`] bar shown.
  pub(crate) fn set_pointer_over(&self, over: bool) {
    self.inner.lock().unwrap().pointer_over = over;
  }

  /// What the bars styled `style` show in the frame painted at `now`. A bar
  /// of any visibility but [`ScrollBarVisibility::OnHover`] is fully shown
  /// and never changes by itself.
  pub(crate) fn reveal_scrollbar(&self, style: &ScrollBarStyle, now: Instant) -> ScrollBarRevealFrame {
    let mut inner = self.inner.lock().unwrap();
    // Content pulled past an end moves as content that scrolls does.
    let offset = (inner.scroll_x + inner.overscroll_x, inner.scroll_y + inner.overscroll_y);
    let scrolled = inner
      .painted_scroll
      .replace(offset)
      .is_some_and(|painted| painted != offset);
    if !style.visible.fades() {
      inner.reveal = ScrollBarReveal::default();
      return ScrollBarRevealFrame {
        opacity: 1.0,
        next_change: None,
      };
    }
    // A finger's indicator is shown by content that moves, not by a pointer over it.
    let held = (style.visible == ScrollBarVisibility::OnHover && inner.pointer_over) || inner.dragging;
    let overflows = inner.content_width > inner.viewport_width || inner.content_height > inner.viewport_height;
    inner.reveal.advance(style, now, held, scrolled, overflows)
  }

  pub fn begin_drag(&self, mouse_y: f32) {
    self.begin_drag_axis(ScrollAxis::Vertical, 0.0, mouse_y);
  }

  pub fn begin_drag_axis(&self, axis: ScrollAxis, mouse_x: f32, mouse_y: f32) {
    let mut inner = self.inner.lock().unwrap();
    match axis {
      ScrollAxis::Horizontal => inner.pending_scroll_x = None,
      ScrollAxis::Vertical => inner.pending_scroll_y = None,
    }
    inner.dragging = true;
    inner.drag_start_x = mouse_x;
    inner.drag_start_y = mouse_y;
    inner.drag_start_scroll_x = inner.scroll_x;
    inner.drag_start_scroll_y = inner.scroll_y;
  }

  pub fn end_drag(&self) {
    self.inner.lock().unwrap().dragging = false;
  }

  pub fn drag_to(&self, mouse_y: f32, style: &crate::layout::scrollbar::ScrollBarStyle) {
    self.drag_to_axis(ScrollAxis::Vertical, 0.0, mouse_y, style);
  }

  pub fn drag_to_axis(
    &self,
    axis: ScrollAxis,
    mouse_x: f32,
    mouse_y: f32,
    style: &crate::layout::scrollbar::ScrollBarStyle,
  ) {
    let mut inner = self.inner.lock().unwrap();
    if !inner.dragging {
      return;
    }

    match axis {
      ScrollAxis::Horizontal => {
        let track_width = inner.viewport_width - style.resolved_end_inset() * 2.0;
        if track_width <= 0.0 {
          return;
        }
        let ratio = inner.viewport_width / inner.content_width.max(1.0);
        let thumb_width = (track_width * ratio).max(style.min_thumb_length).min(track_width);
        let scrollable_track = track_width - thumb_width;
        if scrollable_track <= 0.0 {
          return;
        }

        let delta_px = mouse_x - inner.drag_start_x;
        let scroll_delta = delta_px / scrollable_track * inner.max_scroll_x;
        inner.scroll_x = (inner.drag_start_scroll_x + scroll_delta).clamp(0.0, inner.max_scroll_x);
      }
      ScrollAxis::Vertical => {
        let track_height = inner.viewport_height - style.resolved_end_inset() * 2.0;
        if track_height <= 0.0 {
          return;
        }
        let ratio = inner.viewport_height / inner.content_height.max(1.0);
        let thumb_height = (track_height * ratio).max(style.min_thumb_length).min(track_height);
        let scrollable_track = track_height - thumb_height;
        if scrollable_track <= 0.0 {
          return;
        }

        let delta_px = mouse_y - inner.drag_start_y;
        let scroll_delta = delta_px / scrollable_track * inner.max_scroll_y;
        inner.scroll_y = (inner.drag_start_scroll_y + scroll_delta).clamp(0.0, inner.max_scroll_y);
      }
    }
    inner.scroll_dirty = true;
    inner.layout_subscribers.mark();
  }

  pub fn thumb_rect(&self, style: &crate::layout::scrollbar::ScrollBarStyle) -> (f32, f32, f32, f32) {
    let inner = self.inner.lock().unwrap();
    let track_x = inner.viewport_abs_x + inner.viewport_width - style.width - style.resolved_edge_inset();
    let track_y = inner.viewport_abs_y + style.resolved_end_inset();
    let track_height = inner.viewport_height - style.resolved_end_inset() * 2.0;

    let ratio = inner.viewport_height / inner.content_height.max(1.0);
    let thumb_height = (track_height * ratio).max(style.min_thumb_length).min(track_height);
    let scroll_ratio = if inner.max_scroll_y > 0.0 {
      inner.scroll_y / inner.max_scroll_y
    } else {
      0.0
    };
    let thumb_y = track_y + (track_height - thumb_height) * scroll_ratio;

    (track_x, thumb_y, style.width, thumb_height)
  }

  pub fn thumb_rect_for_axis(
    &self,
    axis: ScrollAxis,
    style: &crate::layout::scrollbar::ScrollBarStyle,
  ) -> Option<(f32, f32, f32, f32)> {
    let geo = self.scrollbar_geometry_for_axis(axis, style)?;
    Some((geo.thumb_x, geo.thumb_y, geo.thumb_width, geo.thumb_height))
  }

  pub fn scrollbar_geometry_for_axis(
    &self,
    axis: ScrollAxis,
    style: &crate::layout::scrollbar::ScrollBarStyle,
  ) -> Option<ScrollBarGeometry> {
    let inner = self.inner.lock().unwrap();
    match (style.visible, axis) {
      (ScrollBarVisibility::Never, _) => return None,
      (
        ScrollBarVisibility::Auto | ScrollBarVisibility::OnHover | ScrollBarVisibility::WhileScrolling,
        ScrollAxis::Horizontal,
      ) if inner.content_width <= inner.viewport_width => {
        return None;
      }
      (
        ScrollBarVisibility::Auto | ScrollBarVisibility::OnHover | ScrollBarVisibility::WhileScrolling,
        ScrollAxis::Vertical,
      ) if inner.content_height <= inner.viewport_height => {
        return None;
      }
      _ => {}
    }

    match axis {
      ScrollAxis::Horizontal => {
        let track_x = inner.viewport_abs_x + style.resolved_end_inset();
        let track_y = inner.viewport_abs_y + inner.container_height - style.width - style.resolved_edge_inset();
        let track_width = inner.viewport_width - style.resolved_end_inset() * 2.0;
        let track_height = style.width;
        if track_width <= 0.0 || track_height <= 0.0 {
          return None;
        }

        let ratio = inner.viewport_width / inner.content_width.max(1.0);
        let thumb_width = (track_width * ratio).max(style.min_thumb_length).min(track_width);
        let max_scroll = (inner.content_width - inner.viewport_width).max(0.0);
        let scroll_ratio = if max_scroll > 0.0 {
          inner.scroll_x / max_scroll
        } else {
          0.0
        };
        let thumb_x = track_x + (track_width - thumb_width) * scroll_ratio;
        let (thumb_x, thumb_width) = squeezed_thumb(thumb_x, thumb_width, inner.overscroll_x, style.width);

        Some(ScrollBarGeometry {
          track_x,
          track_y,
          track_width,
          track_height,
          thumb_x,
          thumb_y: track_y,
          thumb_width,
          thumb_height: track_height,
        })
      }
      ScrollAxis::Vertical => {
        let track_x = inner.viewport_abs_x + inner.container_width - style.width - style.resolved_edge_inset();
        let track_y = inner.viewport_abs_y + style.resolved_end_inset();
        let track_width = style.width;
        let track_height = inner.viewport_height - style.resolved_end_inset() * 2.0;
        if track_width <= 0.0 || track_height <= 0.0 {
          return None;
        }

        let ratio = inner.viewport_height / inner.content_height.max(1.0);
        let thumb_height = (track_height * ratio).max(style.min_thumb_length).min(track_height);
        let max_scroll = (inner.content_height - inner.viewport_height).max(0.0);
        let scroll_ratio = if max_scroll > 0.0 {
          inner.scroll_y / max_scroll
        } else {
          0.0
        };
        let thumb_y = track_y + (track_height - thumb_height) * scroll_ratio;
        let (thumb_y, thumb_height) = squeezed_thumb(thumb_y, thumb_height, inner.overscroll_y, style.width);

        Some(ScrollBarGeometry {
          track_x,
          track_y,
          track_width,
          track_height,
          thumb_x: track_x,
          thumb_y,
          thumb_width: track_width,
          thumb_height,
        })
      }
    }
  }

  /// The part of `axis`'s scrollbar that takes the pointer at `(x, y)` in
  /// window coordinates (see [`ScrollBarGeometry::hit_part_at`]), or `None`
  /// where the pointer belongs to the content. A bar shown while the content
  /// fits along its axis (`ScrollBarVisibility::Always`) cannot scroll
  /// anything, so it takes no pointer, and neither does a
  /// `ScrollBarVisibility::OnHover` bar that the last frame did not paint.
  pub(crate) fn scrollbar_part_at(
    &self,
    axis: ScrollAxis,
    style: &ScrollBarStyle,
    x: f32,
    y: f32,
  ) -> Option<ScrollBarPart> {
    let overflows = match axis {
      ScrollAxis::Horizontal => self.content_width() > self.viewport_width(),
      ScrollAxis::Vertical => self.content_height() > self.viewport_height(),
    };
    if !overflows {
      return None;
    }
    if style.visible.fades() && !self.inner.lock().unwrap().reveal.is_shown() {
      return None;
    }
    self
      .scrollbar_geometry_for_axis(axis, style)?
      .hit_part_at(axis == ScrollAxis::Horizontal, style, x, y)
  }

  /// Scrolls one viewport along `axis`, forward or back, clamped to the
  /// content: what a press on the scrollbar track does.
  pub(crate) fn page_axis(&self, axis: ScrollAxis, forward: bool) {
    let page = match axis {
      ScrollAxis::Horizontal => self.viewport_width(),
      ScrollAxis::Vertical => self.viewport_height(),
    };
    let delta = if forward { page } else { -page };
    match axis {
      ScrollAxis::Horizontal => self.scroll_by(delta, 0.0),
      ScrollAxis::Vertical => self.scroll_by(0.0, delta),
    }
  }

  pub(crate) fn take_scroll_dirty(&self) -> bool {
    let mut inner = self.inner.lock().unwrap();
    let dirty = inner.scroll_dirty;
    inner.scroll_dirty = false;
    dirty
  }

  pub(crate) fn has_scroll_dirty(&self) -> bool {
    self.inner.lock().unwrap().scroll_dirty
  }

  /// Record one measurement of the scroll container. A layout pass can
  /// measure a container more than once (a flex parent re-lays out a child it
  /// shrank or stretched); every measurement resolves the pending scroll and
  /// the stick-to-end behaviour from the state the pass started with, so only
  /// the final measurement counts. [`Self::finish_layout_pass`] then drops the
  /// resolved pending scroll.
  pub(crate) fn update_layout_with_container(
    &self,
    content_w: f32,
    content_h: f32,
    viewport_w: f32,
    viewport_h: f32,
    container_w: f32,
    container_h: f32,
  ) {
    let mut inner = self.inner.lock().unwrap();
    let start = inner.pass_start.unwrap_or(ScrollPassStart {
      scroll_x: inner.scroll_x,
      scroll_y: inner.scroll_y,
      max_scroll_x: inner.max_scroll_x,
      max_scroll_y: inner.max_scroll_y,
    });
    inner.pass_start = Some(start);
    let was_at_right = start.max_scroll_x > 0.0 && start.max_scroll_x - start.scroll_x <= SCROLL_END_EPSILON;
    let was_at_bottom = start.max_scroll_y > 0.0 && start.max_scroll_y - start.scroll_y <= SCROLL_END_EPSILON;

    inner.content_width = content_w;
    inner.content_height = content_h;
    inner.container_width = container_w;
    inner.container_height = container_h;
    inner.viewport_width = viewport_w.max(0.0);
    inner.viewport_height = viewport_h.max(0.0);
    inner.max_scroll_x = (content_w - inner.viewport_width).max(0.0);
    inner.max_scroll_y = (content_h - inner.viewport_height).max(0.0);
    inner.scroll_x = match inner.pending_scroll_x {
      Some(pending) => resolve_pending_scroll(
        pending,
        inner.content_width,
        inner.max_scroll_x,
        start.scroll_x,
        inner.viewport_width,
      ),
      None if was_at_right => inner.max_scroll_x,
      None => start.scroll_x,
    };
    inner.scroll_y = match inner.pending_scroll_y {
      Some(pending) => resolve_pending_scroll(
        pending,
        inner.content_height,
        inner.max_scroll_y,
        start.scroll_y,
        inner.viewport_height,
      ),
      None if was_at_bottom => inner.max_scroll_y,
      None => start.scroll_y,
    };
    inner.scroll_x = inner.scroll_x.clamp(0.0, inner.max_scroll_x);
    inner.scroll_y = inner.scroll_y.clamp(0.0, inner.max_scroll_y);
  }

  /// End the current layout pass: the last measurement's offset stands and
  /// the pending scroll it resolved is consumed. No-op when the container was
  /// not measured in this pass (its pending scroll keeps waiting).
  pub(crate) fn finish_layout_pass(&self) {
    let mut inner = self.inner.lock().unwrap();
    if inner.pass_start.take().is_some() {
      inner.pending_scroll_x = None;
      if inner.pending_scroll_y.take().is_some_and(PendingScroll::is_request) {
        inner.scroll_y_requests = inner.scroll_y_requests.wrapping_add(1);
      }
    }
  }

  /// Record the layout of a multiline text input, which scrolls its own text:
  /// its box, `viewport_width` by `viewport_height`, showing `content_height`
  /// of text and padding from `scroll_y`, clamped to that content. The box is
  /// the input's whole box, as for any scroll container, so the bar is placed
  /// from the box's edges; the padding is part of what scrolls. Unlike
  /// [`Self::update_layout_with_container`], no pending scroll or
  /// stick-to-end applies: the input decides its offset itself.
  pub(crate) fn set_text_layout(&self, viewport_width: f32, viewport_height: f32, content_height: f32, scroll_y: f32) {
    let mut inner = self.inner.lock().unwrap();
    inner.viewport_width = viewport_width.max(0.0);
    inner.viewport_height = viewport_height.max(0.0);
    inner.container_width = inner.viewport_width;
    inner.container_height = inner.viewport_height;
    inner.content_width = inner.viewport_width;
    inner.content_height = if content_height - inner.viewport_height < TEXT_OVERFLOW_EPSILON {
      content_height.min(inner.viewport_height)
    } else {
      content_height
    };
    inner.max_scroll_x = 0.0;
    inner.max_scroll_y = (inner.content_height - inner.viewport_height).max(0.0);
    inner.scroll_x = 0.0;
    inner.scroll_y = scroll_y.clamp(0.0, inner.max_scroll_y);
  }

  pub(crate) fn set_layout_viewport_y(&self, y: f32) {
    self.inner.lock().unwrap().layout_viewport_y = y;
  }

  /// Viewport top in layout space; see `ScrollStateInner::layout_viewport_y`.
  pub(crate) fn layout_viewport_y(&self) -> f32 {
    self.inner.lock().unwrap().layout_viewport_y
  }

  pub(crate) fn set_viewport_position(&self, x: f32, y: f32) {
    let mut inner = self.inner.lock().unwrap();
    inner.viewport_abs_x = x;
    inner.viewport_abs_y = y;
  }
}

fn resolve_pending_scroll(
  pending: PendingScroll,
  content_size: f32,
  max_scroll: f32,
  current: f32,
  viewport: f32,
) -> f32 {
  match pending {
    PendingScroll::Offset(offset) | PendingScroll::Anchor(offset) => offset,
    PendingScroll::Start => 0.0,
    PendingScroll::End => max_scroll,
    PendingScroll::PreservePrependAnchor {
      previous_content,
      previous_scroll,
    } => previous_scroll + (content_size - previous_content),
    PendingScroll::Reveal { start, end } => {
      if start < current || end - start > viewport {
        start
      } else if end > current + viewport {
        end - viewport
      } else {
        current
      }
    }
  }
}

#[derive(Clone, Copy, PartialEq)]
pub struct FlexParams {
  pub grow: f32,
  pub shrink: f32,
  pub basis: Option<f32>,
}

impl FlexParams {
  pub fn grow(factor: f32) -> Self {
    Self {
      grow: factor,
      shrink: 0.0,
      basis: None,
    }
  }
}

impl Default for FlexParams {
  fn default() -> Self {
    Self {
      grow: DEFAULT_FLEX_GROW,
      shrink: DEFAULT_FLEX_SHRINK,
      basis: None,
    }
  }
}

/// How far a shrinking child of a single-line `Row`/`Column` gives way when
/// the line overflows. Set with `.shrink_limit(...)`; it applies only to a
/// child with a `flex_shrink` factor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShrinkLimit {
  /// Shrink down to the child's `min_width`/`min_height` (0 without one).
  #[default]
  MinSize,
  /// Shrink down to the child's content minimum, and never below an explicit
  /// `min_width`/`min_height`. The content minimum of a `Row` in a row (or
  /// a `Column` in a column) is its padding and spacing, the natural size of
  /// every child that does not shrink and the limit of every child that does.
  /// Anything else (text, rect, stack, scroll container, wrapping line, line
  /// on the other axis) keeps its natural size, like a flex item with CSS
  /// `min-width: auto`.
  Content,
  /// Keep the natural size or drop out of the line: a dropped child is laid
  /// out at zero size, takes no spacing, and is neither drawn nor hit.
  /// Nothing inside it is a Tab stop or takes focus, focus inside it is
  /// blurred, keys never activate it, and its element refs report a
  /// zero-size rect where the next child starts. It stays mounted and keeps
  /// its state.
  Drop,
}

/// When and how far a shrinking flex child gives way: a node's
/// `.shrink_order(...)`, `.shrink_limit(...)` and `.shrink_drop_below(...)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ShrinkRule {
  pub(crate) order: i32,
  pub(crate) limit: ShrinkLimit,
  /// Shrink no further than this main size, then drop (see
  /// `.shrink_drop_below`).
  pub(crate) drop_below: Option<f32>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum FlexWrap {
  #[default]
  NoWrap,
  Wrap,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Justify {
  #[default]
  Start,
  End,
  Center,
  SpaceBetween,
  SpaceAround,
  SpaceEvenly,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Overflow {
  Visible,
  #[default]
  Hidden,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollDirection {
  Horizontal,
  #[default]
  Vertical,
  Both,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
  Horizontal,
  Vertical,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub struct FrameConstraints {
  pub width: Option<Dimension>,
  pub height: Option<Dimension>,
  pub min_width: Option<Dimension>,
  pub max_width: Option<Dimension>,
  pub min_height: Option<Dimension>,
  pub max_height: Option<Dimension>,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub enum Position {
  #[default]
  Static,
  Absolute {
    x: f32,
    y: f32,
    width: Option<Dimension>,
    height: Option<Dimension>,
  },
}

impl FrameConstraints {
  pub fn with_width(mut self, value: Option<Dimension>) -> Self {
    self.width = value;
    self
  }
  pub fn with_height(mut self, value: Option<Dimension>) -> Self {
    self.height = value;
    self
  }
  pub fn with_min_width(mut self, value: Option<Dimension>) -> Self {
    self.min_width = value;
    self
  }
  pub fn with_max_width(mut self, value: Option<Dimension>) -> Self {
    self.max_width = value;
    self
  }
  pub fn with_min_height(mut self, value: Option<Dimension>) -> Self {
    self.min_height = value;
    self
  }
  pub fn with_max_height(mut self, value: Option<Dimension>) -> Self {
    self.max_height = value;
    self
  }
}
