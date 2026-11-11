use std::time::{Duration, Instant};

use crate::{animation::Easing, node::color::Color};

const DEFAULT_SCROLLBAR_WIDTH: f32 = 8.0;
const DEFAULT_SCROLLBAR_MIN_THUMB_LENGTH: f32 = 24.0;
const DEFAULT_SCROLLBAR_TRACK_COLOR: Color = Color::new(0, 0, 0, 0);
const DEFAULT_SCROLLBAR_THUMB_COLOR: Color = Color::new(0, 0, 0, 80);
const DEFAULT_SCROLLBAR_THUMB_RADIUS: f32 = 4.0;
const DEFAULT_SCROLLBAR_TRACK_RADIUS: f32 = 4.0;
const DEFAULT_SCROLLBAR_PADDING: f32 = 2.0;
const DEFAULT_SCROLLBAR_FADE_DELAY: Duration = Duration::from_millis(600);
const DEFAULT_SCROLLBAR_FADE_DURATION: Duration = Duration::from_millis(200);
/// How long a fading bar keeps one opacity: a frame of a 60 Hz display.
const FADE_FRAME: Duration = Duration::from_millis(16);

const THIN_SCROLLBAR_WIDTH: f32 = 4.0;
const THIN_SCROLLBAR_MIN_THUMB_LENGTH: f32 = 16.0;
const THIN_SCROLLBAR_THUMB_RADIUS: f32 = 2.0;
const THIN_SCROLLBAR_TRACK_RADIUS: f32 = 2.0;
const THIN_SCROLLBAR_PADDING: f32 = 1.0;

const WIDE_SCROLLBAR_WIDTH: f32 = 12.0;
const WIDE_SCROLLBAR_MIN_THUMB_LENGTH: f32 = 32.0;
const WIDE_SCROLLBAR_THUMB_RADIUS: f32 = 6.0;
const WIDE_SCROLLBAR_TRACK_RADIUS: f32 = 6.0;
const WIDE_SCROLLBAR_PADDING: f32 = 2.0;

#[derive(Clone, Debug, PartialEq, lurq_macros::Accessors)]
pub struct ScrollBarStyle {
  pub width: f32,
  pub min_thumb_length: f32,
  pub track_color: Color,
  pub thumb_color: Color,
  pub thumb_radius: f32,
  pub track_radius: f32,
  /// The gap around the bar: from the edge it runs along and from the two
  /// edges its track ends at. [`Self::edge_inset`] and [`Self::end_inset`]
  /// override each side.
  pub padding: f32,
  /// The gap between the bar and the edge it runs along (the right edge for
  /// a vertical bar, the bottom edge for a horizontal one), measured from the
  /// scroll container's outer bounds. `None` uses [`Self::padding`].
  pub edge_inset: Option<f32>,
  /// The gap between each end of the track and the container's outer edge
  /// (top and bottom for a vertical bar), e.g. to clear rounded corners: the
  /// track is the container's length minus twice this. `None` uses
  /// [`Self::padding`].
  pub end_inset: Option<f32>,
  pub visible: ScrollBarVisibility,
  pub placement: ScrollBarPlacement,
  /// How long a [`ScrollBarVisibility::OnHover`] bar stays after the pointer
  /// left its container, the content stopped scrolling and its thumb was
  /// let go, before it starts to fade.
  pub fade_delay: Duration,
  /// How long a [`ScrollBarVisibility::OnHover`] bar takes to fade out. Zero
  /// hides it at once when [`Self::fade_delay`] ends.
  pub fade_duration: Duration,
  /// The curve a [`ScrollBarVisibility::OnHover`] bar fades out along.
  pub fade_easing: Easing,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollBarVisibility {
  /// The bar shows while the content overflows.
  #[default]
  Auto,
  /// The bar shows whether or not the content overflows.
  Always,
  /// No bar is painted and none takes the pointer; the wheel, the keyboard
  /// and scroll requests still scroll the content.
  Never,
  /// The bar of overflowing content shows while the pointer is over its
  /// scroll container, while the content scrolls and while its thumb is
  /// dragged. Then it stays for [`ScrollBarStyle::fade_delay`] and fades out
  /// over [`ScrollBarStyle::fade_duration`]. Hidden, it is not painted and
  /// takes no pointer, like [`Self::Never`]; shown, it is an [`Self::Auto`]
  /// bar.
  OnHover,
  /// A phone's scroll indicator: an [`Self::OnHover`] bar that the pointer does not show, only content that
  /// scrolls and a thumb that is dragged. A finger that rests on a list shows nothing.
  WhileScrolling,
}

impl ScrollBarVisibility {
  /// Whether the bar comes and goes by itself: shown, it is an [`Self::Auto`] bar.
  pub(crate) fn fades(self) -> bool {
    matches!(self, Self::OnHover | Self::WhileScrolling)
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollBarPlacement {
  #[default]
  Overlay,
  Reserved,
}

impl Default for ScrollBarStyle {
  /// A desktop's bar; on iOS, [`Self::indicator`], the bar a list of the platform has.
  fn default() -> Self {
    if cfg!(target_os = "ios") {
      return Self::indicator();
    }
    Self::bar()
  }
}

impl ScrollBarStyle {
  /// A phone's scroll indicator: a thin rounded bar over the content, clear of the corners, that shows while
  /// the content scrolls and fades out after. Grey, to show over light content and dark.
  pub fn indicator() -> Self {
    Self {
      width: 3.0,
      min_thumb_length: 36.0,
      thumb_color: Color::new(128, 128, 128, 170),
      thumb_radius: 1.5,
      track_radius: 1.5,
      padding: 3.0,
      end_inset: Some(6.0),
      visible: ScrollBarVisibility::WhileScrolling,
      fade_delay: Duration::from_millis(350),
      fade_duration: Duration::from_millis(250),
      ..Self::bar()
    }
  }

  fn bar() -> Self {
    Self {
      width: DEFAULT_SCROLLBAR_WIDTH,
      min_thumb_length: DEFAULT_SCROLLBAR_MIN_THUMB_LENGTH,
      track_color: DEFAULT_SCROLLBAR_TRACK_COLOR,
      thumb_color: DEFAULT_SCROLLBAR_THUMB_COLOR,
      thumb_radius: DEFAULT_SCROLLBAR_THUMB_RADIUS,
      track_radius: DEFAULT_SCROLLBAR_TRACK_RADIUS,
      padding: DEFAULT_SCROLLBAR_PADDING,
      edge_inset: None,
      end_inset: None,
      visible: ScrollBarVisibility::Auto,
      placement: ScrollBarPlacement::Overlay,
      fade_delay: DEFAULT_SCROLLBAR_FADE_DELAY,
      fade_duration: DEFAULT_SCROLLBAR_FADE_DURATION,
      fade_easing: Easing::EASE_OUT,
    }
  }
}

impl ScrollBarStyle {
  pub fn thin() -> Self {
    Self {
      width: THIN_SCROLLBAR_WIDTH,
      min_thumb_length: THIN_SCROLLBAR_MIN_THUMB_LENGTH,
      thumb_radius: THIN_SCROLLBAR_THUMB_RADIUS,
      track_radius: THIN_SCROLLBAR_TRACK_RADIUS,
      padding: THIN_SCROLLBAR_PADDING,
      ..Self::default()
    }
  }

  pub fn wide() -> Self {
    Self {
      width: WIDE_SCROLLBAR_WIDTH,
      min_thumb_length: WIDE_SCROLLBAR_MIN_THUMB_LENGTH,
      thumb_radius: WIDE_SCROLLBAR_THUMB_RADIUS,
      track_radius: WIDE_SCROLLBAR_TRACK_RADIUS,
      padding: WIDE_SCROLLBAR_PADDING,
      ..Self::default()
    }
  }

  /// Sets [`Self::edge_inset`] and [`Self::end_inset`].
  pub fn insets(mut self, edge: f32, end: f32) -> Self {
    self.edge_inset = Some(edge);
    self.end_inset = Some(end);
    self
  }

  /// The resolved gap between the bar and the edge it runs along.
  pub fn resolved_edge_inset(&self) -> f32 {
    self.edge_inset.unwrap_or(self.padding)
  }

  /// The resolved gap between each track end and the container's edge.
  pub fn resolved_end_inset(&self) -> f32 {
    self.end_inset.unwrap_or(self.padding)
  }

  pub fn hidden() -> Self {
    Self {
      visible: ScrollBarVisibility::Never,
      ..Self::default()
    }
  }

  /// The default bar, shown only while the pointer is over its container,
  /// the content scrolls or its thumb is dragged
  /// ([`ScrollBarVisibility::OnHover`]).
  pub fn on_hover() -> Self {
    Self {
      visible: ScrollBarVisibility::OnHover,
      ..Self::default()
    }
  }

  /// Sets [`Self::fade_delay`] and [`Self::fade_duration`].
  pub fn fade(mut self, delay: Duration, duration: Duration) -> Self {
    self.fade_delay = delay;
    self.fade_duration = duration;
    self
  }
}

/// What a [`ScrollBarVisibility::OnHover`] bar shows in one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollBarRevealFrame {
  /// 0 for a hidden bar, 1 for a shown one, between them while it fades.
  pub(crate) opacity: f32,
  /// When the bar next looks different with nothing else happening: the end
  /// of its delay or the next frame of its fade.
  pub(crate) next_change: Option<Instant>,
}

impl ScrollBarRevealFrame {
  const HIDDEN: Self = Self {
    opacity: 0.0,
    next_change: None,
  };
  const SHOWN: Self = Self {
    opacity: 1.0,
    next_change: None,
  };
}

/// Whether a [`ScrollBarVisibility::OnHover`] bar is shown, and since when
/// nothing holds it there.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ScrollBarReveal {
  shown: bool,
  idle_since: Option<Instant>,
}

impl ScrollBarReveal {
  /// Whether the bar is painted, at any opacity, and so takes the pointer.
  pub(crate) fn is_shown(&self) -> bool {
    self.shown
  }

  /// Advances to the frame at `now`. `held` says the pointer is over the
  /// container or drags the thumb, `scrolled` that the content moved since
  /// the frame before; either shows the bar. A bar nothing holds stays for
  /// the style's delay from the first frame that found it so, then fades.
  /// Content that fits has no bar to show.
  pub(crate) fn advance(
    &mut self,
    style: &ScrollBarStyle,
    now: Instant,
    held: bool,
    scrolled: bool,
    overflows: bool,
  ) -> ScrollBarRevealFrame {
    if !overflows {
      *self = Self::default();
      return ScrollBarRevealFrame::HIDDEN;
    }
    if held {
      self.shown = true;
      self.idle_since = None;
      return ScrollBarRevealFrame::SHOWN;
    }
    if scrolled {
      self.shown = true;
      self.idle_since = Some(now);
    }
    if !self.shown {
      return ScrollBarRevealFrame::HIDDEN;
    }
    let idle_since = *self.idle_since.get_or_insert(now);
    let idle = now.saturating_duration_since(idle_since);
    if idle < style.fade_delay {
      return ScrollBarRevealFrame {
        opacity: 1.0,
        next_change: Some(idle_since + style.fade_delay),
      };
    }
    let fading = idle - style.fade_delay;
    if fading >= style.fade_duration {
      *self = Self::default();
      return ScrollBarRevealFrame::HIDDEN;
    }
    let progress = fading.as_secs_f64() / style.fade_duration.as_secs_f64();
    let faded = style.fade_easing.evaluate(progress).clamp(0.0, 1.0);
    ScrollBarRevealFrame {
      opacity: (1.0 - faded) as f32,
      next_change: Some(now + FADE_FRAME),
    }
  }
}

/// How far the area that takes the pointer for a scrollbar reaches beyond
/// what is painted, so a press just off a thin bar still lands on it.
const SCROLLBAR_HIT_SLOP: f32 = 4.0;

pub struct ScrollBarGeometry {
  pub track_x: f32,
  pub track_y: f32,
  pub track_width: f32,
  pub track_height: f32,
  pub thumb_x: f32,
  pub thumb_y: f32,
  pub thumb_width: f32,
  pub thumb_height: f32,
}

/// The part of a scrollbar under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollBarPart {
  Thumb,
  /// The track between its start and the thumb.
  TrackBefore,
  /// The track between the thumb and its end.
  TrackAfter,
}

impl ScrollBarGeometry {
  /// The part of the scrollbar at `(x, y)` for a horizontal or a vertical bar,
  /// or `None` where the pointer belongs to the content.
  ///
  /// Where the bar visibly owns its lane, a `Reserved` gutter or an overlay
  /// bar with a painted track, the whole lane takes the pointer: the track
  /// along the axis and, across it, [`SCROLLBAR_HIT_SLOP`] or the edge inset
  /// beyond the track on both sides, whichever is larger, which covers the
  /// gutter and reaches the container edge. A plain overlay bar (transparent
  /// track) shows only its thumb over the content, so only the thumb takes the
  /// pointer, with [`SCROLLBAR_HIT_SLOP`] around it on every side; the rest of
  /// the lane is content.
  pub(crate) fn hit_part_at(&self, horizontal: bool, style: &ScrollBarStyle, x: f32, y: f32) -> Option<ScrollBarPart> {
    let (along, across) = if horizontal { (x, y) } else { (y, x) };
    let (track_start, track_length, cross_start, thickness) = if horizontal {
      (self.track_x, self.track_width, self.track_y, self.track_height)
    } else {
      (self.track_y, self.track_height, self.track_x, self.track_width)
    };
    let (thumb_start, thumb_length) = if horizontal {
      (self.thumb_x, self.thumb_width)
    } else {
      (self.thumb_y, self.thumb_height)
    };

    let owns_lane = style.placement == ScrollBarPlacement::Reserved || style.track_color.a() > 0;
    if !owns_lane {
      let on_thumb = across >= cross_start - SCROLLBAR_HIT_SLOP
        && across <= cross_start + thickness + SCROLLBAR_HIT_SLOP
        && along >= thumb_start - SCROLLBAR_HIT_SLOP
        && along <= thumb_start + thumb_length + SCROLLBAR_HIT_SLOP;
      return on_thumb.then_some(ScrollBarPart::Thumb);
    }

    let reach = SCROLLBAR_HIT_SLOP.max(style.resolved_edge_inset());
    let in_lane = across >= cross_start - reach
      && across <= cross_start + thickness + reach
      && along >= track_start
      && along <= track_start + track_length;
    if !in_lane {
      return None;
    }
    Some(if along < thumb_start {
      ScrollBarPart::TrackBefore
    } else if along > thumb_start + thumb_length {
      ScrollBarPart::TrackAfter
    } else {
      ScrollBarPart::Thumb
    })
  }
}

pub fn compute_vertical_scrollbar(
  style: &ScrollBarStyle,
  viewport_x: f32,
  viewport_y: f32,
  viewport_width: f32,
  viewport_height: f32,
  content_height: f32,
  scroll_y: f32,
) -> Option<ScrollBarGeometry> {
  match style.visible {
    ScrollBarVisibility::Never => return None,
    ScrollBarVisibility::Auto | ScrollBarVisibility::OnHover | ScrollBarVisibility::WhileScrolling
      if content_height <= viewport_height =>
    {
      return None;
    }
    _ => {}
  }

  let track_x = viewport_x + viewport_width - style.width - style.resolved_edge_inset();
  let track_y = viewport_y + style.resolved_end_inset();
  let track_width = style.width;
  let track_height = viewport_height - style.resolved_end_inset() * 2.0;

  let ratio = viewport_height / content_height.max(1.0);
  let thumb_height = (track_height * ratio).max(style.min_thumb_length).min(track_height);
  let max_scroll = (content_height - viewport_height).max(0.0);
  let scroll_ratio = if max_scroll > 0.0 { scroll_y / max_scroll } else { 0.0 };
  let thumb_y = track_y + (track_height - thumb_height) * scroll_ratio;

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

pub fn compute_horizontal_scrollbar(
  style: &ScrollBarStyle,
  viewport_x: f32,
  viewport_y: f32,
  viewport_width: f32,
  viewport_height: f32,
  content_width: f32,
  scroll_x: f32,
) -> Option<ScrollBarGeometry> {
  match style.visible {
    ScrollBarVisibility::Never => return None,
    ScrollBarVisibility::Auto | ScrollBarVisibility::OnHover | ScrollBarVisibility::WhileScrolling
      if content_width <= viewport_width =>
    {
      return None;
    }
    _ => {}
  }

  let track_x = viewport_x + style.resolved_end_inset();
  let track_y = viewport_y + viewport_height - style.width - style.resolved_edge_inset();
  let track_width = viewport_width - style.resolved_end_inset() * 2.0;
  let track_height = style.width;

  let ratio = viewport_width / content_width.max(1.0);
  let thumb_width = (track_width * ratio).max(style.min_thumb_length).min(track_width);
  let max_scroll = (content_width - viewport_width).max(0.0);
  let scroll_ratio = if max_scroll > 0.0 { scroll_x / max_scroll } else { 0.0 };
  let thumb_x = track_x + (track_width - thumb_width) * scroll_ratio;

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
