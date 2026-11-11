//! Where a window's frames come from: the one place the shell asks for a frame, learns that one is due and says
//! that one was shown, whatever paces them on the platform.
//!
//! - On a desktop a frame is a redraw winit is asked for, and the frames that come from time (an animation's,
//!   a transition's) are held to the display's refresh by [`FramePacing`].
//! - On iOS a `CADisplayLink` fires once a refresh while frames are wanted and for a moment after, and the shell presents from the
//!   turn of the loop it wakes. It is the only clock: a frame from time is asked for as soon as the timeline
//!   changes and waits for the next refresh like any other, with no pacing of the shell's own on top, which
//!   went by a refresh rate the monitor reports as 60 Hz on a display that runs at 120.

use std::time::Instant;

use winit::{event::WindowEvent, window::Window};

use super::frame_pacing::{FramePacing, PresentedFrame};
use crate::app::Tree;

#[derive(Default)]
pub(super) struct FrameSource {
  pacing: FramePacing,
  /// A frame was asked for and has not started.
  pending: bool,
  #[cfg(target_os = "ios")]
  link: Option<super::ios::FrameLink>,
  /// Since when the display has refreshed with no frame wanted.
  #[cfg(target_os = "ios")]
  idle_since: Option<Instant>,
}

/// How long the display's refreshes are still listened to after the last frame that was wanted.
#[cfg(target_os = "ios")]
const IDLE_REFRESHES: std::time::Duration = std::time::Duration::from_millis(500);

impl FrameSource {
  /// The window was made: what paces its frames starts.
  #[cfg(target_os = "ios")]
  pub(super) fn attach(&mut self) {
    self.link = Some(super::ios::FrameLink::new());
  }

  /// Takes what the window's events say about its frames: whether it can be seen, and whether it may now be on
  /// another display.
  pub(super) fn handle_event(&mut self, event: &WindowEvent) {
    self.pacing.handle_event(event);
  }

  /// Asks for a frame, once until it starts.
  pub(super) fn request(&mut self, window: &Window) {
    if self.pending {
      return;
    }
    self.pending = true;
    #[cfg(target_os = "ios")]
    if let Some(link) = &mut self.link {
      link.set_running(true);
      return;
    }
    window.request_redraw();
  }

  /// Asks the tree for the next frame its timelines need, now or when it is due. To be called before the
  /// tree's scheduled redraws are ticked.
  pub(super) fn request_timeline_frame(&mut self, tree: &mut Tree, window: Option<&Window>, now: Instant) {
    #[cfg(target_os = "ios")]
    if self.link.is_some() {
      self.pacing.request_frame(tree, || std::time::Duration::ZERO, now);
      return;
    }
    self
      .pacing
      .request_frame(tree, || super::frame_pacing::refresh_interval(window), now);
  }

  /// Whether the shell is to present a frame in this turn of the loop, without waiting for an event: the
  /// display refreshed and a frame is wanted, `wanted` by the tree or asked for. Where winit delivers frames
  /// as events it never is. Stops what paces the frames once none is wanted.
  pub(super) fn take_due(&mut self, wanted: bool) -> bool {
    #[cfg(target_os = "ios")]
    if let Some(link) = &mut self.link {
      let wanted = self.pending || wanted;
      if !wanted {
        // The refreshes go on for a while with no frame wanted. A finger's move may reach the loop after the
        // refresh it is for: the refresh is then still there to be taken, and the frame is not put off to the
        // next one. And a display that is presented to in fits and starts lowers its rate, after which a frame
        // waits for a drawable: measured on an iPhone 17, a screen dragged by a finger drew 90 frames a second
        // with the refreshes stopped at once, a third of them 8 ms late.
        let now = Instant::now();
        if now.duration_since(*self.idle_since.get_or_insert(now)) >= IDLE_REFRESHES {
          link.set_running(false);
          self.idle_since = None;
        }
        return false;
      }
      self.idle_since = None;
      return link.take_due();
    }
    let _ = wanted;
    false
  }

  /// Whether a frame was asked for and has not started.
  pub(super) fn pending(&self) -> bool {
    self.pending
  }

  /// A frame starts, or the one asked for will not come: the next request asks anew.
  pub(super) fn settle(&mut self) {
    self.pending = false;
  }

  pub(super) fn presented(&mut self, frame: PresentedFrame) {
    self.pacing.presented(frame);
  }
}
