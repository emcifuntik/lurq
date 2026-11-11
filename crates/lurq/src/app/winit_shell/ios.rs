//! What the shell asks of UIKit directly, where winit has no word for it.
//!
//! **Frames.** winit redraws a view through `setNeedsDisplay` and `drawRect:`, which UIKit runs at 60 Hz on a
//! phone whatever the display can do. A `CADisplayLink` that asks for the display's full rate fires once per
//! refresh instead, and the shell presents from the turn of the loop it wakes ([`FrameLink`]). It runs only
//! while a frame is wanted: paused, it costs nothing and the display drops to its idle rate. An iPhone goes
//! above 60 Hz only for an app whose `Info.plist` sets `CADisableMinimumFrameDurationOnPhone`.
//!
//! Text input is in `ios_text`.
//!
//! **The keyboard** comes up over the bottom of the window, which winit does not tell. UIKit announces where it
//! will be ([`keyboard_overlap`]), and the shell counts what it covers as part of the safe area and brings the
//! focused input into view.
//!
//! **A tap on the status bar** scrolls a list to its top. UIKit does not deliver that tap as a touch: it asks
//! the `UIScrollView` under the tapped point that scrolls to top. The lists here are not scroll views, so an
//! empty scroll view across the top of the window, as high as the status bar, stands in ([`StatusBarTap`]),
//! declines to scroll, and the shell scrolls its own content instead. Touches in that band go to it and not
//! to the app, whose content the safe area keeps out of there.
//!
//! **Scenes.** UIKit gives an app its windows through scenes, and traps at launch in an app linked against the
//! iOS 27 SDK that has not taken them up. winit starts the app with no delegate and makes its window outside any
//! scene. So the scene's delegate is a class of the shell's ([`register_scene_delegate`]), which an app names in
//! its `Info.plist` (`UIApplicationSceneManifest`, `UISceneDelegateClassName` = `LurqSceneDelegate`), and the
//! shell puts winit's window into the scene UIKit connects ([`join_scene`]). An app whose `Info.plist` names no
//! scene runs as before, where UIKit still lets it.

use std::{
  ffi::c_void,
  ptr::NonNull,
  sync::{
    Mutex,
    atomic::{AtomicBool, AtomicPtr, Ordering},
  },
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
/// How many times the keyboard has set off, up or down.
static KEYBOARD_MOVES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Until when the keyboard is on its way up or down, as UIKit announced it.
static KEYBOARD_MOVES_UNTIL: Mutex<Option<std::time::Instant>> = Mutex::new(None);

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
pub(super) struct CGPoint {
  pub(super) x: f64,
  pub(super) y: f64,
}

unsafe impl Encode for CGPoint {
  const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct CGSize {
  pub(super) width: f64,
  pub(super) height: f64,
}

unsafe impl Encode for CGSize {
  const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct CGRect {
  pub(super) origin: CGPoint,
  pub(super) size: CGSize,
}

unsafe impl Encode for CGRect {
  const ENCODING: Encoding = Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
}

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
  static NSRunLoopCommonModes: *const AnyObject;
}

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
  static UIKeyboardWillChangeFrameNotification: *const AnyObject;
  static UIKeyboardFrameEndUserInfoKey: *const AnyObject;
  static UIKeyboardAnimationDurationUserInfoKey: *const AnyObject;
}

extern "C-unwind" fn keyboard_frame_changes(_this: &AnyObject, _cmd: Sel, notification: *mut AnyObject) {
  let Some(notification) = (unsafe { notification.as_ref() }) else {
    return;
  };
  unsafe {
    let info: Option<Retained<AnyObject>> = msg_send![notification, userInfo];
    let Some(info) = info else {
      return;
    };
    let duration: Option<Retained<AnyObject>> = msg_send![&*info, objectForKey: UIKeyboardAnimationDurationUserInfoKey];
    let duration: f64 = match duration {
      Some(duration) => msg_send![&*duration, doubleValue],
      None => 0.0,
    };
    // A little longer than announced: the last frame must find the keyboard at rest.
    let until = std::time::Instant::now() + std::time::Duration::from_secs_f64(duration.clamp(0.0, 2.0) + 0.1);
    *KEYBOARD_MOVES_UNTIL.lock().unwrap() = Some(until);
    KEYBOARD_MOVES.fetch_add(1, Ordering::AcqRel);
  }
}

/// Which of the keyboard's moves this is: another number each time it sets off.
pub(super) fn keyboard_move() -> u64 {
  KEYBOARD_MOVES.load(Ordering::Acquire)
}

/// Whether the keyboard is on its way up or down: the window is laid out anew at each frame while it is.
pub(super) fn keyboard_moving() -> bool {
  KEYBOARD_MOVES_UNTIL
    .lock()
    .unwrap()
    .is_some_and(|until| std::time::Instant::now() < until)
}

/// Where the keyboard is on screen at this moment, its way up and down included.
///
/// UIKit says where the keyboard will be, not where it is on the way there, and it moves along a curve of its
/// own. A view pinned to the top of the window's keyboard layout guide is moved by UIKit in the keyboard's own
/// animation, and its presentation layer is where that animation has it now. Main thread only.
pub(super) struct KeyboardProbe {
  probe: Retained<AnyObject>,
}

impl KeyboardProbe {
  /// Adds the probe, a view of one point that shows nothing, to the window's view.
  pub(super) fn new(parent: NonNull<c_void>) -> Self {
    let parent: &AnyObject = unsafe { parent.cast::<AnyObject>().as_ref() };
    unsafe {
      let probe: Retained<AnyObject> = msg_send![class!(UIView), new];
      let _: () = msg_send![&*probe, setUserInteractionEnabled: false];
      let _: () = msg_send![&*probe, setTranslatesAutoresizingMaskIntoConstraints: false];
      let _: () = msg_send![parent, addSubview: &*probe];
      let guide: Retained<AnyObject> = msg_send![parent, keyboardLayoutGuide];
      let pin = |anchor: Retained<AnyObject>, to: Retained<AnyObject>| {
        let constraint: Retained<AnyObject> = msg_send![&*anchor, constraintEqualToAnchor: &*to];
        let _: () = msg_send![&*constraint, setActive: true];
      };
      pin(msg_send![&*probe, topAnchor], msg_send![&*guide, topAnchor]);
      pin(msg_send![&*probe, leadingAnchor], msg_send![parent, leadingAnchor]);
      for dimension in [sel!(widthAnchor), sel!(heightAnchor)] {
        let anchor: Retained<AnyObject> = msg_send![&*probe, performSelector: dimension];
        let constraint: Retained<AnyObject> = msg_send![&*anchor, constraintEqualToConstant: 1.0f64];
        let _: () = msg_send![&*constraint, setActive: true];
      }
      Self { probe }
    }
  }

  /// How much of a window `height` points high the keyboard covers now, in points. With no keyboard the guide
  /// rests at the bottom safe area, which is no cover of the keyboard's: the shell takes the larger of the two.
  pub(super) fn overlap(&self, height: f64) -> f64 {
    unsafe {
      let layer: Retained<AnyObject> = msg_send![&*self.probe, layer];
      let shown: Option<Retained<AnyObject>> = msg_send![&*layer, presentationLayer];
      let frame: CGRect = match &shown {
        Some(shown) => msg_send![&**shown, frame],
        None => msg_send![&*layer, frame],
      };
      // Before its first layout the probe is at the window's top, which is no place for a keyboard.
      if frame.origin.y < 1.0 {
        return 0.0;
      }
      (height - frame.origin.y).clamp(0.0, height)
    }
  }
}

/// Starts listening for where the keyboard goes. The listener lives as long as the app.
pub(super) fn watch_keyboard() {
  unsafe {
    let receiver: Retained<AnyObject> = msg_send![receiver_class(), new];
    let center: Retained<AnyObject> = msg_send![class!(NSNotificationCenter), defaultCenter];
    let _: () = msg_send![
      &*center,
      addObserver: &*receiver,
      selector: sel!(lurqKeyboardFrameChanges:),
      name: UIKeyboardWillChangeFrameNotification,
      object: std::ptr::null::<AnyObject>()
    ];
    // The center does not retain its observers.
    std::mem::forget(receiver);
  }
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
      sel!(lurqKeyboardFrameChanges:),
      keyboard_frame_changes as extern "C-unwind" fn(_, _, _),
    );
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

/// Plays a tap of the haptic engine through UIKit's feedback generators. Main thread only.
pub(super) fn play_haptic(haptic: crate::app::window::Haptic) {
  use crate::app::window::Haptic;
  unsafe {
    match haptic {
      Haptic::Light | Haptic::Medium | Haptic::Heavy => {
        // `UIImpactFeedbackStyle`: light 0, medium 1, heavy 2.
        let style: isize = match haptic {
          Haptic::Light => 0,
          Haptic::Medium => 1,
          _ => 2,
        };
        let generator: objc2::rc::Allocated<AnyObject> = msg_send![class!(UIImpactFeedbackGenerator), alloc];
        let generator: Retained<AnyObject> = msg_send![generator, initWithStyle: style];
        let _: () = msg_send![&*generator, impactOccurred];
      }
      Haptic::Selection => {
        let generator: Retained<AnyObject> = msg_send![class!(UISelectionFeedbackGenerator), new];
        let _: () = msg_send![&*generator, selectionChanged];
      }
      Haptic::Success | Haptic::Warning | Haptic::Error => {
        // `UINotificationFeedbackType`: success 0, warning 1, error 2.
        let kind: isize = match haptic {
          Haptic::Success => 0,
          Haptic::Warning => 1,
          _ => 2,
        };
        let generator: Retained<AnyObject> = msg_send![class!(UINotificationFeedbackGenerator), new];
        let _: () = msg_send![&*generator, notificationOccurred: kind];
      }
    }
  }
}

/// The scene UIKit connected, retained; null before it does and in an app that has no scenes.
static SCENE: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());
/// winit's window, which lives as long as the app does; null until the shell has made it.
static SCENE_WINDOW: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());

/// Makes the class UIKit creates as the scene's delegate, by the name an app's `Info.plist` gives:
/// `LurqSceneDelegate`. To be called before the event loop runs, which is when UIKit starts the app.
pub(super) fn register_scene_delegate() {
  if AnyClass::get(c"LurqSceneDelegate").is_some() {
    return;
  }
  extern "C-unwind" fn will_connect(
    _this: &AnyObject,
    _cmd: Sel,
    scene: *mut AnyObject,
    _session: *mut AnyObject,
    _options: *mut AnyObject,
  ) {
    let Some(scene) = (unsafe { Retained::retain(scene) }) else {
      return;
    };
    let is_window_scene: bool = unsafe { msg_send![&*scene, isKindOfClass: class!(UIWindowScene)] };
    if !is_window_scene {
      return;
    }
    let previous = SCENE.swap(Retained::into_raw(scene), Ordering::AcqRel);
    if !previous.is_null() {
      drop(unsafe { Retained::from_raw(previous) });
    }
    show_window_in_scene();
  }
  extern "C-unwind" fn did_disconnect(_this: &AnyObject, _cmd: Sel, scene: *mut AnyObject) {
    if SCENE
      .compare_exchange(scene, std::ptr::null_mut(), Ordering::AcqRel, Ordering::Acquire)
      .is_ok()
    {
      drop(unsafe { Retained::from_raw(scene) });
    }
  }
  extern "C-unwind" fn window(_this: &AnyObject, _cmd: Sel) -> *mut AnyObject {
    SCENE_WINDOW.load(Ordering::Acquire)
  }
  /// The window is winit's to make.
  extern "C-unwind" fn set_window(_this: &AnyObject, _cmd: Sel, _window: *mut AnyObject) {}

  let mut builder = ClassBuilder::new(c"LurqSceneDelegate", class!(UIResponder)).expect("a class name of its own");
  if let Some(protocol) = objc2::runtime::AnyProtocol::get(c"UIWindowSceneDelegate") {
    builder.add_protocol(protocol);
  }
  unsafe {
    builder.add_method(
      sel!(scene:willConnectToSession:options:),
      will_connect as extern "C-unwind" fn(_, _, _, _, _),
    );
    builder.add_method(
      sel!(sceneDidDisconnect:),
      did_disconnect as extern "C-unwind" fn(_, _, _),
    );
    builder.add_method(sel!(window), window as extern "C-unwind" fn(_, _) -> _);
    builder.add_method(sel!(setWindow:), set_window as extern "C-unwind" fn(_, _, _));
  }
  builder.register();
}

/// Puts the window of a view of winit's into the app's scene, now or when UIKit connects one. A window outside
/// a scene is not shown in an app that has scenes.
pub(super) fn join_scene(view: NonNull<c_void>) {
  let view: &AnyObject = unsafe { view.cast::<AnyObject>().as_ref() };
  let window: Option<Retained<AnyObject>> = unsafe { msg_send![view, window] };
  let Some(window) = window else {
    return;
  };
  SCENE_WINDOW.store(Retained::as_ptr(&window).cast_mut(), Ordering::Release);
  if SCENE.load(Ordering::Acquire).is_null() {
    // Connected before the shell's delegate could hear of it, or by a delegate that is not the shell's.
    let scene = unsafe {
      let app: Retained<AnyObject> = msg_send![class!(UIApplication), sharedApplication];
      let scenes: Retained<AnyObject> = msg_send![&*app, connectedScenes];
      let scenes: Retained<AnyObject> = msg_send![&*scenes, allObjects];
      let count: usize = msg_send![&*scenes, count];
      (0..count)
        .map(|index| -> Retained<AnyObject> { msg_send![&*scenes, objectAtIndex: index] })
        .find(|scene| msg_send![&**scene, isKindOfClass: class!(UIWindowScene)])
    };
    if let Some(scene) = scene {
      SCENE.store(Retained::into_raw(scene), Ordering::Release);
    }
  }
  show_window_in_scene();
}

/// Gives winit's window to the scene, once there are both and the window has none.
fn show_window_in_scene() {
  let (scene, window) = (SCENE.load(Ordering::Acquire), SCENE_WINDOW.load(Ordering::Acquire));
  let (Some(scene), Some(window)) = (unsafe { scene.as_ref() }, unsafe { window.as_ref() }) else {
    return;
  };
  unsafe {
    let current: Option<Retained<AnyObject>> = msg_send![window, windowScene];
    if current.is_some() {
      return;
    }
    let _: () = msg_send![window, setWindowScene: scene];
    let _: () = msg_send![window, makeKeyAndVisible];
  }
}
