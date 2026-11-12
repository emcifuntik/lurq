use std::{
  ops::Deref,
  sync::{Arc, RwLock, Weak},
};

use crate::{core::Signal, layout::size::Size, node::color::Color};

/// A snapshot of the window's geometry, returned by `ctx.window()`.
///
/// `resolved_*` values are in physical device pixels; `logical_*` divide those
/// by the scale factor and match the coordinate space layout reasons in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowInfo {
  pub x: i32,
  pub y: i32,
  pub resolved_width: f32,
  pub resolved_height: f32,
  pub scale_factor: f32,
  pub is_minimized: bool,
  pub is_maximized: bool,
  pub is_full_screen: bool,
  pub is_decorated: bool,
  pub is_focused: bool,
  /// The edges of the window the system draws over or cuts off; see [`SafeAreaInsets`].
  pub safe_area: SafeAreaInsets,
}

/// What the system tells an app about itself, heard with `ctx.window().on_app_event(..)`. A phone tells all of
/// it; a desktop shell tells none of it yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppEvent {
  /// The app is in front and takes input.
  Active,
  /// Something of the system's is over the app, or it is on its way out: the app is seen and takes no input.
  Inactive,
  /// The app is no longer seen. It is given a moment to save what it must, and may be ended without notice.
  Background,
  /// The app is coming back into view.
  Foreground,
  /// The app was opened with a link: its own scheme, or a universal link of its site.
  OpenUrl(String),
  /// The app was opened by one of its shortcuts on the home screen, named by the shortcut's type.
  Shortcut(String),
  /// The way the app's content is turned, told once at the start and again whenever the device is turned to
  /// a way the app allows.
  Rotated(Orientation),
}

/// A way a phone is held, and so the way an app's content is turned. Several are joined with `|` into the
/// [`Orientations`] an app allows: `Orientation::Portrait | Orientation::LandscapeLeft`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Orientation {
  /// Upright.
  Portrait,
  /// Upright the other way up, the device's top at the bottom. A phone with no home button does not turn
  /// its content this way whatever an app allows.
  PortraitUpsideDown,
  /// On its side, the device's top, where its camera is, at the left.
  LandscapeLeft,
  /// On its other side, the device's top at the right.
  LandscapeRight,
}

/// The ways an app lets its content be turned: [`Orientation`]s joined with `|`. Given to the window before
/// it runs; a device turned to a way that is not among them leaves the content as it was. An iOS app's
/// `Info.plist` names the ways it supports too (`UISupportedInterfaceOrientations`), and only those among
/// these that it names are ever taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Orientations(u8);

impl Orientations {
  /// Either side.
  pub const LANDSCAPE: Self = Self(0b1100);
  /// Every way.
  pub const ALL: Self = Self(0b1111);

  const fn bit(orientation: Orientation) -> u8 {
    match orientation {
      Orientation::Portrait => 0b0001,
      Orientation::PortraitUpsideDown => 0b0010,
      Orientation::LandscapeLeft => 0b0100,
      Orientation::LandscapeRight => 0b1000,
    }
  }

  pub fn contains(self, orientation: Orientation) -> bool {
    self.0 & Self::bit(orientation) != 0
  }
}

impl From<Orientation> for Orientations {
  fn from(orientation: Orientation) -> Self {
    Self(Self::bit(orientation))
  }
}

impl std::ops::BitOr for Orientation {
  type Output = Orientations;

  fn bitor(self, other: Self) -> Orientations {
    Orientations(Orientations::bit(self) | Orientations::bit(other))
  }
}

impl std::ops::BitOr<Orientation> for Orientations {
  type Output = Self;

  fn bitor(self, other: Orientation) -> Self {
    Self(self.0 | Self::bit(other))
  }
}

impl std::ops::BitOr for Orientations {
  type Output = Self;

  fn bitor(self, other: Self) -> Self {
    Self(self.0 | other.0)
  }
}

impl std::ops::BitOr<Orientations> for Orientation {
  type Output = Orientations;

  fn bitor(self, other: Orientations) -> Orientations {
    Orientations(Orientations::bit(self) | other.0)
  }
}

type AppEventHandler = Arc<dyn Fn(&AppEvent) + Send + Sync>;

/// A tap of the device's haptic engine, played with `ctx.window().haptic(..)`. A phone has one; where there is
/// none, nothing happens.
///
/// The names are the platform's: an impact for something that lands or snaps into place, a selection for a
/// value that changes under the finger, and a notification for how something ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Haptic {
  Light,
  Medium,
  Heavy,
  Selection,
  Success,
  Warning,
  Error,
}

/// How far in from each edge of the window content stays clear of what the device and the system put there: a
/// phone's status bar, camera housing, home indicator and rounded corners. In logical pixels, and zero on a
/// desktop.
///
/// The window covers the whole screen, so a background reaches its edges; content that must be read or
/// touched is padded by these.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SafeAreaInsets {
  pub top: f32,
  pub right: f32,
  pub bottom: f32,
  pub left: f32,
  /// How much more `bottom` is for the on-screen keyboard being up: 0 with the keyboard down. What stands at
  /// the bottom of the window, as a sheet does, rises by it.
  pub keyboard: f32,
}

impl WindowInfo {
  pub fn position(&self) -> (i32, i32) {
    (self.x, self.y)
  }

  pub fn resolved_size(&self) -> Size {
    Size::new(self.resolved_width, self.resolved_height)
  }

  pub fn logical_size(&self) -> Size {
    let scale = self.scale_factor.max(f32::EPSILON);
    Size::new(self.resolved_width / scale, self.resolved_height / scale)
  }

  pub fn logical_width(&self) -> f32 {
    self.logical_size().width
  }

  pub fn logical_height(&self) -> f32 {
    self.logical_size().height
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowResizeDirection {
  East,
  North,
  NorthEast,
  NorthWest,
  South,
  SouthEast,
  SouthWest,
  West,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowCornerRadius {
  Default,
  None,
  Rounded,
  RoundedSmall,
}

/// The 1px frame the OS compositor draws around a window. Windows 11 draws it around undecorated
/// windows too (DWM `DWMWA_BORDER_COLOR`); other platforms ignore it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowBorderColor {
  /// The system color.
  #[default]
  Default,
  /// No compositor border.
  None,
  Color(Color),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowIcon {
  rgba: Vec<u8>,
  width: u32,
  height: u32,
}

impl WindowIcon {
  pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Self {
    assert_eq!(rgba.len(), (width * height * 4) as usize);
    Self { rgba, width, height }
  }

  #[cfg(feature = "raster")]
  pub fn from_image_data(image: &crate::images::ImageData) -> Self {
    Self::from_rgba((*image.data_arc()).clone(), image.width(), image.height())
  }

  pub fn rgba(&self) -> &[u8] {
    &self.rgba
  }

  pub fn width(&self) -> u32 {
    self.width
  }

  pub fn height(&self) -> u32 {
    self.height
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn into_rgba(self) -> (Vec<u8>, u32, u32) {
    (self.rgba, self.width, self.height)
  }
}

/// Origin of a vetoable close request. Programmatic `close()` bypasses this path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseRequestSource {
  Os,
  App,
}

/// A one-shot decision that may be moved to another thread or retained for a dialog.
/// Dropping it, or calling `cancel`, keeps the window open. A stale request cannot
/// close a replacement window: it refers only to its original window's queue.
pub struct CloseRequest {
  window: Weak<RwLock<WindowInner>>,
  source: CloseRequestSource,
}

impl CloseRequest {
  pub fn source(&self) -> CloseRequestSource {
    self.source
  }
  pub fn proceed(self) {
    if let Some(window) = self.window.upgrade() {
      let waker = {
        let mut inner = window.write().unwrap();
        inner.queue_command(WindowCommand::Close);
        inner.waker.clone()
      };
      if let Some(waker) = waker {
        waker();
      }
    }
  }
  pub fn cancel(self) {}
}

type CloseHandler = Arc<dyn Fn(CloseRequest) + Send + Sync>;

mod handle;
pub(crate) mod resize;
#[cfg(test)]
mod resize_tests;
#[cfg(test)]
pub(crate) mod simulated_window;
#[cfg(feature = "mcp")]
pub(crate) use handle::DialogWindow;
pub use handle::WindowHandle;
use resize::ResizeRequest;

/// A logical-pixel window region for a partial frame capture, matching the
/// units of [`crate::core::ElementRect`] bounds.
#[cfg(feature = "screenshot")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenshotRegion {
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
}

// Not `Eq`: synthetic input carries pixel positions.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum WindowCommand {
  /// Play a tap of the device's haptic engine; see [`Haptic`].
  Haptic(Haptic),
  Close,
  RequestClose(CloseRequestSource),
  SetMinimized(bool),
  SetMaximized(bool),
  SetFullScreen(bool),
  SetDecorated(bool),
  SetTitle(String),
  SetTitleBarColor(Option<Color>),
  SetBorderColor(WindowBorderColor),
  SetIcon(Option<WindowIcon>),
  SetCornerRadius(WindowCornerRadius),
  Move {
    x: i32,
    y: i32,
  },
  Resize(ResizeRequest),
  StartDrag,
  StartResize(WindowResizeDirection),
  StopDrag,
  InjectInput(crate::app::synthetic_input::SyntheticInput),
  /// Read the OS clipboard and paste it into the focused text input, as
  /// Ctrl+V does: for an app's own Paste menu item. `synthetic` is whether
  /// the request was made while lurq delivered synthetic input (an MCP
  /// click on that menu item); the shell refuses it then, since the
  /// person's clipboard is theirs.
  #[cfg(feature = "clipboard")]
  PasteFromClipboard {
    synthetic: bool,
  },
  #[cfg(feature = "screenshot")]
  Screenshot(std::path::PathBuf, Option<ScreenshotRegion>),
  OpenDevtools,
  CloseDevtools,
  ToggleDevtools,
}

/// Reactive, per-window geometry handle held by the `Tree` and injected into
/// its root `Ctx`. Reads through `ctx.window()` subscribe to changes via the
/// version signal; the shell pushes resize/scale/move updates here.
#[derive(Clone)]
pub struct Window {
  inner: Arc<RwLock<WindowInner>>,
  version_signal: Signal<u64>,
}

/// Wakes the shell's event loop so a command pushed from another thread is
/// processed without waiting for the next OS event.
pub(crate) type WindowWaker = Arc<dyn Fn() + Send + Sync>;

struct WindowInner {
  #[cfg(feature = "mcp")]
  accepted_close: bool,
  /// How many times the window lost focus; input an MCP client holds is released by the next loss.
  #[cfg(feature = "mcp")]
  focus_losses: u64,
  info: WindowInfo,
  corner_radius: WindowCornerRadius,
  border_color: WindowBorderColor,
  /// `None` until the shell creates the window or the app sets a title.
  title: Option<String>,
  version: u64,
  commands: Vec<WindowCommand>,
  /// Registered by the shell once its event loop exists. Without it, a
  /// cross-thread `push_command` sat unprocessed while the loop idled in
  /// `ControlFlow::Wait`.
  waker: Option<WindowWaker>,
  /// The thread that registered `waker`, which runs the event loop.
  loop_thread: Option<std::thread::ThreadId>,
  /// Set once a platform shell applies this window's commands; a headless tree has none.
  shell_attached: bool,
  resize_reports: resize::ResizeReports,
  close_handler: Option<CloseHandler>,
  app_event_handler: Option<AppEventHandler>,
}

impl WindowInner {
  fn queue_command(&mut self, command: WindowCommand) {
    #[cfg(feature = "mcp")]
    if command == WindowCommand::Close {
      self.accepted_close = true;
    }
    self.commands.push(command);
  }

  /// Queues a resize under the caller's lock; returns the waker to call once the lock is released.
  #[cfg_attr(not(feature = "mcp"), allow(dead_code))]
  fn queue_resize(&mut self, request: ResizeRequest) -> Option<WindowWaker> {
    self.queue_command(WindowCommand::Resize(request));
    self.waker.clone()
  }
}

impl Default for Window {
  fn default() -> Self {
    Self::new()
  }
}

impl Window {
  pub fn new() -> Self {
    Self {
      inner: Arc::new(RwLock::new(WindowInner {
        #[cfg(feature = "mcp")]
        accepted_close: false,
        #[cfg(feature = "mcp")]
        focus_losses: 0,
        info: WindowInfo {
          x: 0,
          y: 0,
          resolved_width: 0.0,
          resolved_height: 0.0,
          scale_factor: 1.0,
          is_minimized: false,
          is_maximized: false,
          is_full_screen: false,
          is_decorated: true,
          is_focused: true,
          safe_area: SafeAreaInsets::default(),
        },
        corner_radius: WindowCornerRadius::Default,
        border_color: WindowBorderColor::Default,
        title: None,
        version: 0,
        commands: Vec::new(),
        waker: None,
        loop_thread: None,
        shell_attached: false,
        resize_reports: resize::ResizeReports::default(),
        close_handler: None,
        app_event_handler: None,
      })),
      version_signal: Signal::new(0),
    }
  }

  pub(crate) fn set_app_event_handler(&self, handler: Option<AppEventHandler>) {
    self.inner.write().unwrap().app_event_handler = handler;
  }

  #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
  pub(crate) fn has_app_event_handler(&self) -> bool {
    self.inner.read().unwrap().app_event_handler.is_some()
  }

  /// Gives the window's handler something the system said of the app.
  #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
  pub(crate) fn dispatch_app_event(&self, event: &AppEvent) {
    // Never hold the state lock across application code.
    let handler = self.inner.read().unwrap().app_event_handler.clone();
    if let Some(handler) = handler {
      handler(event);
    }
  }

  /// The single event-loop entry point for OS, queued app and MCP requests.
  pub(crate) fn dispatch_close_request(&self, source: CloseRequestSource) {
    let handler = self.inner.read().unwrap().close_handler.clone();
    let request = CloseRequest {
      window: Arc::downgrade(&self.inner),
      source,
    };
    // Never hold the state lock across application code (which may close/register).
    if let Some(handler) = handler {
      handler(request);
    } else {
      request.proceed();
    }
  }

  #[cfg(feature = "mcp")]
  pub(crate) fn close_queued(&self) -> bool {
    self.inner.read().unwrap().commands.contains(&WindowCommand::Close)
  }

  /// Whether a close of this window was accepted (queued for the shell), even if already applied.
  #[cfg(feature = "mcp")]
  pub(crate) fn close_accepted(&self) -> bool {
    self.inner.read().unwrap().accepted_close
  }

  /// How many times the window has lost focus.
  #[cfg(feature = "mcp")]
  pub(crate) fn focus_losses(&self) -> u64 {
    self.inner.read().unwrap().focus_losses
  }

  pub(crate) fn track_access(&self) {
    let _ = self.version_signal.get();
  }

  pub(crate) fn info(&self) -> WindowInfo {
    self.inner.read().unwrap().info
  }

  #[allow(dead_code)]
  pub(crate) fn corner_radius(&self) -> WindowCornerRadius {
    self.inner.read().unwrap().corner_radius
  }

  pub(crate) fn handle(&self) -> WindowHandle {
    WindowHandle {
      info: self.info(),
      window: self.clone(),
    }
  }

  /// Records the title a window is created with, unless the app already set
  /// one (its `SetTitle` command is still queued and wins).
  pub(crate) fn record_initial_title(&self, title: &str) {
    let mut inner = self.inner.write().unwrap();
    if inner.title.is_none() {
      inner.title = Some(title.to_owned());
    }
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn take_commands(&self) -> Vec<WindowCommand> {
    std::mem::take(&mut self.inner.write().unwrap().commands)
  }

  /// Resolve one generation of app requests on the event-loop thread. Requests
  /// queued by a callback remain for the next turn, avoiding recursive handlers.
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn take_shell_commands(&self) -> Vec<WindowCommand> {
    let mut result = Vec::new();
    for command in self.take_commands() {
      if let WindowCommand::RequestClose(source) = command {
        self.dispatch_close_request(source);
      } else {
        result.push(command);
      }
    }
    for command in self.take_commands() {
      if matches!(command, WindowCommand::RequestClose(_)) {
        self.push_command(command);
      } else {
        result.push(command);
      }
    }
    result
  }

  #[cfg(all(feature = "winit", target_os = "macos"))]
  pub(crate) fn requeue_command(&self, command: WindowCommand) {
    self.push_command(command);
  }

  fn push_command(&self, command: WindowCommand) {
    let waker = {
      let mut inner = self.inner.write().unwrap();
      inner.queue_command(command);
      inner.waker.clone()
    };
    // Invoked outside the lock: the waker may re-enter window state.
    if let Some(waker) = waker {
      waker();
    }
  }

  /// Registers the shell's event-loop waker; every subsequent `push_command`
  /// invokes it so commands queued from other threads are drained promptly.
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn set_waker(&self, waker: WindowWaker) {
    let mut inner = self.inner.write().unwrap();
    inner.waker = Some(waker);
    inner.loop_thread = Some(std::thread::current().id());
  }

  /// Registers the platform shell that applies this window's commands, with its event-loop waker.
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn attach_shell(&self, waker: WindowWaker) {
    self.set_waker(waker);
    self.inner.write().unwrap().shell_attached = true;
  }

  /// Wakes the host for paint work without queuing a component/window mutation.
  pub(crate) fn wake(&self) {
    let waker = self.inner.read().unwrap().waker.clone();
    if let Some(waker) = waker {
      waker();
    }
  }

  /// A waker for work that finishes outside the event loop (timers started, futures and queries that can make
  /// progress): it wakes the shell attached to the window when it is called, and does nothing once the window is
  /// gone.
  pub(crate) fn loop_waker(&self) -> WindowWaker {
    let window = Arc::downgrade(&self.inner);
    Arc::new(move || {
      let wake = window.upgrade().and_then(|window| window.read().unwrap().waker.clone());
      if let Some(wake) = wake {
        wake();
      }
    })
  }

  /// A waker for writes made off the event loop's thread, such as a `Signal` set from a Tokio task or a
  /// `std::thread`: called from any other thread it wakes the attached shell, so the loop picks the write up at
  /// once instead of at its next event. On the loop's own thread, whose turn picks the write up already, and
  /// once the window is gone, it does nothing.
  pub(crate) fn off_loop_waker(&self) -> WindowWaker {
    let window = Arc::downgrade(&self.inner);
    Arc::new(move || {
      let wake = window.upgrade().and_then(|window| {
        let window = window.read().unwrap();
        if window.loop_thread == Some(std::thread::current().id()) {
          None
        } else {
          window.waker.clone()
        }
      });
      if let Some(wake) = wake {
        wake();
      }
    })
  }

  pub fn version(&self) -> u64 {
    self.inner.read().unwrap().version
  }

  pub(crate) fn set_resolved_size(&self, width: f32, height: f32) {
    let version = {
      let mut inner = self.inner.write().unwrap();
      if inner.info.resolved_width == width && inner.info.resolved_height == height {
        return;
      }
      inner.info.resolved_width = width;
      inner.info.resolved_height = height;
      Self::bump_version(&mut inner)
    };
    self.version_signal.set(version);
  }

  pub(crate) fn set_scale_factor(&self, scale_factor: f32) {
    let version = {
      let mut inner = self.inner.write().unwrap();
      if inner.info.scale_factor == scale_factor {
        return;
      }
      inner.info.scale_factor = scale_factor;
      Self::bump_version(&mut inner)
    };
    self.version_signal.set(version);
  }

  pub(crate) fn set_position(&self, x: i32, y: i32) {
    let version = {
      let mut inner = self.inner.write().unwrap();
      if inner.info.x == x && inner.info.y == y {
        return;
      }
      inner.info.x = x;
      inner.info.y = y;
      Self::bump_version(&mut inner)
    };
    self.version_signal.set(version);
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn set_minimized(&self, minimized: bool) {
    let version = {
      let mut inner = self.inner.write().unwrap();
      if inner.info.is_minimized == minimized {
        return;
      }
      inner.info.is_minimized = minimized;
      Self::bump_version(&mut inner)
    };
    self.version_signal.set(version);
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn set_maximized(&self, maximized: bool) {
    let version = {
      let mut inner = self.inner.write().unwrap();
      if inner.info.is_maximized == maximized {
        return;
      }
      inner.info.is_maximized = maximized;
      Self::bump_version(&mut inner)
    };
    self.version_signal.set(version);
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn set_full_screen(&self, full_screen: bool) {
    let version = {
      let mut inner = self.inner.write().unwrap();
      if inner.info.is_full_screen == full_screen {
        return;
      }
      inner.info.is_full_screen = full_screen;
      Self::bump_version(&mut inner)
    };
    self.version_signal.set(version);
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn set_decorated(&self, decorated: bool) {
    let version = {
      let mut inner = self.inner.write().unwrap();
      if inner.info.is_decorated == decorated {
        return;
      }
      inner.info.is_decorated = decorated;
      Self::bump_version(&mut inner)
    };
    self.version_signal.set(version);
  }

  /// Sets the insets a shell read from the platform. Public for tests and for shells outside the crate.
  pub fn set_safe_area(&self, safe_area: SafeAreaInsets) {
    let version = {
      let mut inner = self.inner.write().unwrap();
      if inner.info.safe_area == safe_area {
        return;
      }
      inner.info.safe_area = safe_area;
      Self::bump_version(&mut inner)
    };
    self.version_signal.set(version);
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn set_focused(&self, focused: bool) {
    let version = {
      let mut inner = self.inner.write().unwrap();
      if inner.info.is_focused == focused {
        return;
      }
      inner.info.is_focused = focused;
      #[cfg(feature = "mcp")]
      if !focused {
        inner.focus_losses += 1;
      }
      Self::bump_version(&mut inner)
    };
    self.version_signal.set(version);
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn set_corner_radius(&self, radius: WindowCornerRadius) {
    let version = {
      let mut inner = self.inner.write().unwrap();
      if inner.corner_radius == radius {
        return;
      }
      inner.corner_radius = radius;
      Self::bump_version(&mut inner)
    };
    self.version_signal.set(version);
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn set_border_color(&self, color: WindowBorderColor) {
    let version = {
      let mut inner = self.inner.write().unwrap();
      if inner.border_color == color {
        return;
      }
      inner.border_color = color;
      Self::bump_version(&mut inner)
    };
    self.version_signal.set(version);
  }

  fn bump_version(inner: &mut WindowInner) -> u64 {
    inner.version = inner.version.wrapping_add(1);
    inner.version
  }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod orientation_tests {
  use super::{Orientation, Orientations};

  #[test]
  fn orientations_joined_with_or_are_the_ones_named() {
    let allowed = Orientation::Portrait | Orientation::LandscapeLeft;
    assert!(allowed.contains(Orientation::Portrait));
    assert!(allowed.contains(Orientation::LandscapeLeft));
    assert!(!allowed.contains(Orientation::LandscapeRight));
    assert!(!allowed.contains(Orientation::PortraitUpsideDown));

    let more = allowed | Orientation::LandscapeRight;
    assert_eq!(more, Orientation::Portrait | Orientations::LANDSCAPE);
    assert_eq!(more | Orientation::PortraitUpsideDown, Orientations::ALL);
    assert_eq!(
      Orientations::from(Orientation::Portrait) | Orientations::LANDSCAPE,
      more
    );
  }
}
