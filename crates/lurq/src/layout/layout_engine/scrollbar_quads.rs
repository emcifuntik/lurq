//! The scrollbars a scroll container paints over its content, and the time
//! a bar shown only on hover next changes.

use std::time::Instant;

use super::{DEFAULT_QUAD_OPACITY, LayoutEngine, scroll_direction_has_axis};
use crate::{
  layout::{
    layout_kind::{ScrollAxis, ScrollDirection, ScrollState},
    quad::{ClipRect, Quad, QuadContent},
    scrollbar::ScrollBarStyle,
  },
  node::{border::BorderRadius, color::Color, transform::Transform2D},
};

impl LayoutEngine {
  /// Starts the frame painted at `now`: bars shown on hover
  /// ([`crate::layout::scrollbar::ScrollBarVisibility::OnHover`]) fade by
  /// this time.
  pub(crate) fn begin_scrollbar_frame(&self, now: Instant) {
    self.scrollbar_frame_at.set(now);
    self.scrollbar_change_at.set(None);
  }

  /// When the first of the bars painted since [`Self::begin_scrollbar_frame`]
  /// looks different with nothing else happening: the end of its delay or the
  /// next frame of its fade. `None` when no bar is leaving.
  pub(crate) fn take_scrollbar_change(&self) -> Option<Instant> {
    self.scrollbar_change_at.take()
  }

  /// Paints the bars of a container scrolling along `direction`: the vertical
  /// one, then the horizontal one over it.
  pub(super) fn push_scrollbar_quads(
    &self,
    state: &ScrollState,
    direction: ScrollDirection,
    style: &ScrollBarStyle,
    clip: ClipRect,
    quads: &mut Vec<Quad>,
  ) {
    let reveal = state.reveal_scrollbar(style, self.scrollbar_frame_at.get());
    if let Some(change) = reveal.next_change {
      let first = self.scrollbar_change_at.get().map_or(change, |at| at.min(change));
      self.scrollbar_change_at.set(Some(first));
    }
    if reveal.opacity <= 0.0 {
      return;
    }
    let opacity = DEFAULT_QUAD_OPACITY * reveal.opacity;
    for axis in [ScrollAxis::Vertical, ScrollAxis::Horizontal] {
      if !scroll_direction_has_axis(direction, axis) {
        continue;
      }
      let Some(bar) = state.scrollbar_geometry_for_axis(axis, style) else {
        continue;
      };
      if style.track_color.a() > 0 {
        let track = (bar.track_x, bar.track_y, bar.track_width, bar.track_height);
        quads.push(bar_quad(track, style.track_color, style.track_radius, opacity, clip));
      }
      let thumb = (bar.thumb_x, bar.thumb_y, bar.thumb_width, bar.thumb_height);
      quads.push(bar_quad(thumb, style.thumb_color, style.thumb_radius, opacity, clip));
    }
  }
}

fn bar_quad(
  (x, y, width, height): (f32, f32, f32, f32),
  color: Color,
  radius: f32,
  opacity: f32,
  clip: ClipRect,
) -> Quad {
  Quad {
    x,
    y,
    width,
    height,
    opacity,
    transform: Transform2D::IDENTITY,
    transform_origin: None,
    content: QuadContent::Rect { color, gradient: None },
    border_radius: Some(BorderRadius::all(radius)),
    border: None,
    clip,
  }
}
