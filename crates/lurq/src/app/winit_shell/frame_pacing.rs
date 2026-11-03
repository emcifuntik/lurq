//! When a window asks for the frames that come from time rather than input: animation and transition frames, the
//! perf overlay's samples and continuous images. A frame is due when the timeline next draws something else, at
//! most one per display refresh, and never while the window cannot be seen.

use std::time::{Duration, Instant};

use winit::{event::WindowEvent, window::Window};

use crate::app::Tree;

/// A refresh at 60 Hz, for a window whose display reports no rate.
const FALLBACK_REFRESH_INTERVAL: Duration = Duration::from_nanos(16_666_667);
/// How long a display's refresh rate is trusted: asking for it reaches the display driver (`EnumDisplaySettingsExW`
/// on Windows), too slow for every frame.
const REFRESH_RECHECK: Duration = Duration::from_secs(1);

/// When a presented frame's pass started and when its present returned.
#[derive(Clone, Copy, Debug)]
pub(super) struct PresentedFrame {
  pub(super) started: Instant,
  pub(super) presented: Instant,
}

#[derive(Debug, Default)]
pub(super) struct FramePacing {
  /// The platform reports the window fully hidden (`WindowEvent::Occluded`, on macOS).
  occluded: bool,
  /// The window was minimized or occluded at the last request.
  hidden: bool,
  /// The display's refresh interval and when it was read.
  refresh: Option<(Duration, Instant)>,
  last_frame: Option<PresentedFrame>,
}

impl FramePacing {
  /// Takes what the window's events say about pacing: whether the platform reports it occluded, and whether it may
  /// now be on a display with another refresh rate.
  pub(super) fn handle_event(&mut self, event: &WindowEvent) {
    match event {
      WindowEvent::Occluded(occluded) => self.occluded = *occluded,
      WindowEvent::Moved(_) | WindowEvent::ScaleFactorChanged { .. } => self.refresh = None,
      _ => {}
    }
  }

  pub(super) fn presented(&mut self, frame: PresentedFrame) {
    self.last_frame = Some(frame);
  }

  /// Schedules the window's next paced frame, if it needs one. `refresh` reads the display's refresh interval; it is
  /// called only for a frame, at most once a second. Call this before the tree's scheduled redraws are ticked.
  ///
  /// A minimized or occluded window gets none, and the frames it had scheduled are dropped (an animation's next
  /// step, a caret's blink, an image's next frame); the perf overlay takes no samples. Its animations keep their
  /// clock: the window asks for a frame when it shows again, which draws them where they are by then and schedules
  /// what follows.
  pub(super) fn request_frame(&mut self, tree: &mut Tree, refresh: impl FnOnce() -> Duration, now: Instant) {
    let hidden = self.occluded || tree.window().info().is_minimized;
    let shown = self.hidden && !hidden;
    self.hidden = hidden;
    if hidden {
      tree.drop_scheduled_redraws();
      return;
    }
    if shown {
      tree.request_redraw();
      return;
    }
    tree.tick_perf_overlay();
    let changes_at = if tree.perf_overlay_enabled() || tree.has_continuous_redraw_image() || tree.autoscroll_moving() {
      Some(now)
    } else {
      tree.next_timeline_change()
    };
    let Some(changes_at) = changes_at else {
      return;
    };
    let refresh = match self.refresh {
      Some((interval, read_at)) if now.duration_since(read_at) < REFRESH_RECHECK => interval,
      _ => {
        let interval = refresh();
        self.refresh = Some((interval, now));
        interval
      }
    };
    let next_frame_at = frame_due(changes_at, self.last_frame, refresh);
    if now >= next_frame_at {
      tree.request_redraw();
    } else {
      tree.request_redraw_at(next_frame_at);
    }
  }
}

/// When the frame for a change at `changes_at` is due, after the `last` frame on a display that refreshes every
/// `refresh`.
///
/// The frame is asked for half a refresh after the last present. With a FIFO swapchain the pass then waits in its
/// acquire for the vblank after the one that shows the last frame, which puts every frame on its own vblank,
/// however much work it holds, up to about half a refresh with the loop's wake latency. A pace counted a whole
/// refresh from the present would add that work and latency to every period and drift past vblanks. The frame is
/// also no sooner than three quarters of a refresh after the last one started, so that a swapchain that does not
/// wait for the display (mailbox, immediate) takes at most four frames in three refreshes.
pub(super) fn frame_due(changes_at: Instant, last: Option<PresentedFrame>, refresh: Duration) -> Instant {
  let Some(last) = last else {
    return changes_at;
  };
  changes_at
    .max(last.presented + refresh / 2)
    .max(last.started + refresh * 3 / 4)
}

/// One refresh of the display the window is on.
pub(super) fn refresh_interval(window: Option<&Window>) -> Duration {
  window
    .and_then(Window::current_monitor)
    .and_then(|monitor| monitor.refresh_rate_millihertz())
    .filter(|rate| *rate > 0)
    .map(|rate| Duration::from_nanos((1_000_000_000_000u64 / rate as u64).max(1)))
    .unwrap_or(FALLBACK_REFRESH_INTERVAL)
}
