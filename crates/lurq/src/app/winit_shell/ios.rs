//! What the shell asks of UIKit directly, where winit has no word for it.
//!
//! **Frames.** winit redraws a view through `setNeedsDisplay` and `drawRect:`, which UIKit runs at 60 Hz on a
//! phone whatever the display can do. A `CADisplayLink` that asks for the display's full rate fires once per
//! refresh instead, and the shell presents from the turn of the loop it wakes ([`FrameLink`]). It runs only
//! while a frame is wanted: paused, it costs nothing and the display drops to its idle rate. An iPhone goes
//! above 60 Hz only for an app whose `Info.plist` sets `CADisableMinimumFrameDurationOnPhone`.
//!
//! **A tap on the status bar** scrolls a list to its top. UIKit does not deliver that tap as a touch: it asks
//! the `UIScrollView` under the tapped point that scrolls to top. The lists here are not scroll views, so an
//! empty scroll view across the top of the window, as high as the status bar, stands in ([`StatusBarTap`]),
//! declines to scroll, and the shell scrolls its own content instead. Touches in that band go to it and not
//! to the app, whose content the safe area keeps out of there.

use std::{
  ffi::c_void,
  ptr::NonNull,
  sync::atomic::{AtomicBool, Ordering},
};

use objc2::{
  class,
  encode::{Encode, Encoding},
  msg_send,
  rc::Retained,
  runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel},
  sel,
};

/// Set by the display link at each refresh while it runs.
static FRAME_DUE: AtomicBool = AtomicBool::new(false);
/// Set when the status bar was tapped.
static STATUS_BAR_TAPPED: AtomicBool = AtomicBool::new(false);

#[repr(C)]
#[derive(Clone, Copy)]
struct CAFrameRateRange {
  minimum: f32,
  maximum: f32,
  preferred: f32,
}

unsafe impl Encode for CAFrameRateRange {
  const ENCODING: Encoding = Encoding::Struct("CAFrameRateRange", &[f32::ENCODING, f32::ENCODING, f32::ENCODING]);
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
  x: f64,
  y: f64,
}

unsafe impl Encode for CGPoint {
  const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
  width: f64,
  height: f64,
}

unsafe impl Encode for CGSize {
  const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
  origin: CGPoint,
  size: CGSize,
}

unsafe impl Encode for CGRect {
  const ENCODING: Encoding = Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
}

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
  static NSRunLoopCommonModes: *const AnyObject;
}

extern "C-unwind" fn frame_due(_this: &AnyObject, _cmd: Sel, _link: *mut AnyObject) {
  FRAME_DUE.store(true, Ordering::Release);
}

extern "C-unwind" fn should_scroll_to_top(_this: &AnyObject, _cmd: Sel, _scroll_view: *mut AnyObject) -> Bool {
  STATUS_BAR_TAPPED.store(true, Ordering::Release);
  // The stand-in itself has nowhere to scroll.
  Bool::NO
}

/// The class of the object both callbacks are sent to, registered once.
fn receiver_class() -> &'static AnyClass {
  if let Some(class) = AnyClass::get(c"LurqShellReceiver") {
    return class;
  }
  let mut builder = ClassBuilder::new(c"LurqShellReceiver", class!(NSObject)).expect("a class name of its own");
  unsafe {
    builder.add_method(sel!(lurqFrameDue:), frame_due as extern "C-unwind" fn(_, _, _));
    builder.add_method(
      sel!(scrollViewShouldScrollToTop:),
      should_scroll_to_top as extern "C-unwind" fn(_, _, _) -> _,
    );
  }
  builder.register()
}

/// The display's refreshes, as turns of the event loop. Main thread only, as the window is.
pub(super) struct FrameLink {
  link: Retained<AnyObject>,
  running: bool,
}

impl FrameLink {
  pub(super) fn new() -> Self {
    let link: Retained<AnyObject> = unsafe {
      let receiver: Retained<AnyObject> = msg_send![receiver_class(), new];
      // The link retains its target.
      let link: Retained<AnyObject> = msg_send![
        class!(CADisplayLink),
        displayLinkWithTarget: &*receiver,
        selector: sel!(lurqFrameDue:)
      ];
      // Whatever the display gives up to 120 Hz; a 60 Hz display gives 60.
      let range = CAFrameRateRange {
        minimum: 60.0,
        maximum: 120.0,
        preferred: 120.0,
      };
      let _: () = msg_send![&*link, setPreferredFrameRateRange: range];
      let _: () = msg_send![&*link, setPaused: true];
      let run_loop: Retained<AnyObject> = msg_send![class!(NSRunLoop), mainRunLoop];
      let _: () = msg_send![&*link, addToRunLoop: &*run_loop, forMode: NSRunLoopCommonModes];
      link
    };
    Self { link, running: false }
  }

  /// Starts or stops the refreshes. Stopped, a refresh already noted is forgotten.
  pub(super) fn set_running(&mut self, running: bool) {
    if self.running == running {
      return;
    }
    self.running = running;
    if !running {
      FRAME_DUE.store(false, Ordering::Release);
    }
    let _: () = unsafe { msg_send![&*self.link, setPaused: !running] };
  }

  /// Whether the display refreshed since the last call, while running.
  pub(super) fn take_due(&mut self) -> bool {
    FRAME_DUE.swap(false, Ordering::AcqRel)
  }
}

impl Drop for FrameLink {
  fn drop(&mut self) {
    let _: () = unsafe { msg_send![&*self.link, invalidate] };
  }
}

/// The scroll view that takes the status bar's tap. Main thread only.
pub(super) struct StatusBarTap {
  scroll_view: Retained<AnyObject>,
  _receiver: Retained<AnyObject>,
  height: f64,
}

impl StatusBarTap {
  /// Adds the stand-in to the window's view, a `UIView` from the window's handle.
  pub(super) fn new(view: NonNull<c_void>) -> Self {
    let view: &AnyObject = unsafe { view.cast::<AnyObject>().as_ref() };
    unsafe {
      let receiver: Retained<AnyObject> = msg_send![receiver_class(), new];
      let scroll_view: Retained<AnyObject> = msg_send![class!(UIScrollView), new];
      // UIKit asks only a scroll view that is away from its top: more content than fits, scrolled by a point.
      let bounds: CGRect = msg_send![view, bounds];
      let frame = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
          width: bounds.size.width,
          height: 0.0,
        },
      };
      let _: () = msg_send![&*scroll_view, setFrame: frame];
      // `UIViewAutoresizingFlexibleWidth`: it stays across the top when the window turns.
      let _: () = msg_send![&*scroll_view, setAutoresizingMask: 2usize];
      let _: () = msg_send![&*scroll_view, setContentSize: CGSize { width: 1.0, height: 10_000.0 }];
      let _: () = msg_send![&*scroll_view, setContentOffset: CGPoint { x: 0.0, y: 1.0 }];
      let _: () = msg_send![&*scroll_view, setScrollsToTop: true];
      let _: () = msg_send![&*scroll_view, setShowsVerticalScrollIndicator: false];
      let _: () = msg_send![&*scroll_view, setShowsHorizontalScrollIndicator: false];
      // A finger must not scroll it to its top, where UIKit would stop asking it.
      let pan: Retained<AnyObject> = msg_send![&*scroll_view, panGestureRecognizer];
      let _: () = msg_send![&*pan, setEnabled: false];
      // The delegate is not retained by the scroll view: `_receiver` keeps it.
      let _: () = msg_send![&*scroll_view, setDelegate: &*receiver];
      let _: () = msg_send![view, addSubview: &*scroll_view];
      Self {
        scroll_view,
        _receiver: receiver,
        height: 0.0,
      }
    }
  }

  /// Makes the band as high as the status bar: the safe area's top inset, in points.
  pub(super) fn set_height(&mut self, height: f64) {
    if self.height == height {
      return;
    }
    self.height = height;
    unsafe {
      let mut frame: CGRect = msg_send![&*self.scroll_view, frame];
      frame.size.height = height;
      let _: () = msg_send![&*self.scroll_view, setFrame: frame];
      let _: () = msg_send![&*self.scroll_view, setContentOffset: CGPoint { x: 0.0, y: 1.0 }];
    }
  }

  /// Whether the status bar was tapped since the last call.
  pub(super) fn take_tapped(&mut self) -> bool {
    STATUS_BAR_TAPPED.swap(false, Ordering::AcqRel)
  }
}
