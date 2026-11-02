use std::time::{Duration, Instant};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};
#[cfg(target_os = "macos")]
use winit::platform::macos::EventLoopBuilderExtMacOS;
#[cfg(windows)]
use winit::platform::windows::{
  Color as WinitWindowsColor, CornerPreference as WinitCornerPreference, WindowAttributesExtWindows, WindowExtWindows,
};
use winit::{
  application::ApplicationHandler,
  dpi::{PhysicalPosition, Position},
  event::{ElementState, MouseScrollDelta, TouchPhase, WindowEvent},
  event_loop::{ActiveEventLoop, EventLoop},
  keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
  window::{
    CursorIcon as WinitCursorIcon, Fullscreen, Icon as WinitIcon, ResizeDirection as WinitResizeDirection, Window,
    WindowAttributes, WindowId,
  },
};

use crate::{
  app::{
    App, Tree,
    events::{MouseButton, ScrollPhase},
    runtime::{PassReport, SecondaryWindow, SecondaryWindowMetadata},
    window::{
      CloseRequestSource, WindowBorderColor, WindowCommand, WindowCornerRadius, WindowIcon, WindowResizeDirection,
      resize::SizeReporter,
    },
  },
  node::{CursorIcon, color::Color},
};

mod cursor;
mod file_drag;
mod frame_pacing;
#[cfg(test)]
mod frame_pacing_tests;
mod history_swipes;
mod ime;
mod loop_wait;
#[cfg(test)]
mod loop_wait_tests;
mod native_resize;

use file_drag::FileDragShell;
use frame_pacing::{FramePacing, PresentedFrame, refresh_interval};
use loop_wait::{LoopNeeds, LoopWait};

const CONTINUOUS_REDRAW_GRACE: Duration = Duration::from_millis(500);
const WINIT_SLOW_SCOPE_THRESHOLD: Duration = Duration::from_millis(45);
const WINIT_BREAKDOWN_THRESHOLD: Duration = Duration::from_millis(45);
/// Floor between direct presents from the mouse-move stream (interactive
/// drags and hover restyles; vsync usually paces harder — this guards the
/// vsync-off case).
const INTERACTION_PRESENT_INTERVAL: Duration = Duration::from_millis(7);

type TickFn = Box<dyn FnMut(&mut Tree, Duration)>;
type PaintFn = Box<dyn FnMut(&Tree, Duration, PassReport)>;
type PositionChangedFn = Box<dyn FnMut(i32, i32)>;
type SizeChangedFn = Box<dyn FnMut(u32, u32)>;

pub struct WinitWindow {
  app: App,
  tree: Tree,
  attrs: WindowAttributes,
  start_without_focus: bool,
  corner_radius: Option<WindowCornerRadius>,
  on_tick: Option<TickFn>,
  on_paint: Option<PaintFn>,
  on_position_changed: Option<PositionChangedFn>,
  on_size_changed: Option<SizeChangedFn>,
}

impl WinitWindow {
  pub fn new(app: App, tree: Tree) -> Self {
    Self {
      app,
      tree,
      attrs: WindowAttributes::default(),
      start_without_focus: false,
      corner_radius: None,
      on_tick: None,
      on_paint: None,
      on_position_changed: None,
      on_size_changed: None,
    }
  }

  pub fn with_title(mut self, title: &str) -> Self {
    self.attrs = self.attrs.with_title(title);
    self
  }

  pub fn with_size(mut self, width: u32, height: u32) -> Self {
    self.attrs = self.attrs.with_inner_size(winit::dpi::LogicalSize::new(width, height));
    self
  }

  pub fn with_position(mut self, x: i32, y: i32) -> Self {
    self.attrs = self
      .attrs
      .with_position(Position::Physical(PhysicalPosition::new(x, y)));
    self
  }

  pub fn with_min_size(mut self, width: u32, height: u32) -> Self {
    self.attrs = self
      .attrs
      .with_min_inner_size(winit::dpi::LogicalSize::new(width, height));
    self
  }

  pub fn with_max_size(mut self, width: u32, height: u32) -> Self {
    self.attrs = self
      .attrs
      .with_max_inner_size(winit::dpi::LogicalSize::new(width, height));
    self
  }

  pub fn with_resizable(mut self, resizable: bool) -> Self {
    self.attrs = self.attrs.with_resizable(resizable);
    self
  }

  pub fn with_decorations(mut self, decorations: bool) -> Self {
    self.attrs = self.attrs.with_decorations(decorations);
    self.tree.window().set_decorated(decorations);
    self
  }

  pub fn with_title_bar_color(mut self, color: impl Into<Option<Color>>) -> Self {
    self.attrs = with_title_bar_color(self.attrs, color.into());
    self
  }

  /// The window's icon. On Windows it is both the small icon (title bar) and
  /// the big one (taskbar button, Alt+Tab); Windows scales the image to each
  /// size. On macOS this does nothing: the Dock and the app switcher show the
  /// application bundle's icon.
  pub fn with_icon(mut self, icon: impl Into<Option<WindowIcon>>) -> Self {
    self.attrs = with_window_icons(self.attrs, icon.into().and_then(to_winit_icon));
    self
  }

  pub fn with_corner_radius(mut self, radius: WindowCornerRadius) -> Self {
    self.attrs = with_corner_radius(self.attrs, radius);
    self.corner_radius = Some(radius);
    self.tree.window().set_corner_radius(radius);
    self
  }

  pub fn with_rounded_corners(self, rounded: bool) -> Self {
    self.with_corner_radius(if rounded {
      WindowCornerRadius::Rounded
    } else {
      WindowCornerRadius::None
    })
  }

  pub fn with_transparent(mut self, transparent: bool) -> Self {
    self.attrs = self.attrs.with_transparent(transparent);
    self
  }

  /// Show the initial window without activating the application or taking keyboard focus.
  /// Intended for app instances driven through automation rather than local input.
  /// Winit supports this startup behavior on Windows and macOS.
  pub fn with_start_without_focus(mut self, start_without_focus: bool) -> Self {
    self.start_without_focus = start_without_focus;
    self.attrs = self.attrs.with_active(!start_without_focus);
    self
  }

  /// Runs during event-loop ticking before paint and may mutate the tree.
  pub fn on_tick<F>(mut self, tick: F) -> Self
  where
    F: FnMut(&mut Tree, Duration) + 'static,
  {
    self.on_tick = Some(Box::new(tick));
    self
  }

  /// Runs after a frame is presented. Use this for paint profiling.
  pub fn on_paint<F>(mut self, paint: F) -> Self
  where
    F: FnMut(&Tree, Duration, PassReport) + 'static,
  {
    self.on_paint = Some(Box::new(paint));
    self
  }

  #[deprecated(note = "use on_paint for callbacks after a frame is presented")]
  pub fn on_frame<F>(self, frame: F) -> Self
  where
    F: FnMut(&Tree, Duration, PassReport) + 'static,
  {
    self.on_paint(frame)
  }

  pub fn on_position_changed<F>(mut self, callback: F) -> Self
  where
    F: FnMut(i32, i32) + 'static,
  {
    self.on_position_changed = Some(Box::new(callback));
    self
  }

  pub fn on_size_changed<F>(mut self, callback: F) -> Self
  where
    F: FnMut(u32, u32) + 'static,
  {
    self.on_size_changed = Some(Box::new(callback));
    self
  }

  pub fn run(self) {
    let mut event_loop_builder = EventLoop::builder();
    #[cfg(target_os = "macos")]
    if self.start_without_focus {
      event_loop_builder.with_activate_ignoring_other_apps(false);
    }
    let event_loop = event_loop_builder.build().unwrap();

    // Waker for commands pushed from other threads: without it a queued
    // command waits for the next OS event while the loop idles in
    // `ControlFlow::Wait`. The proxy wakes the loop; the resulting
    // `about_to_wait` drains the queues.
    let waker: crate::app::window::WindowWaker = {
      let proxy = std::sync::Mutex::new(event_loop.create_proxy());
      std::sync::Arc::new(move || {
        if let Ok(proxy) = proxy.lock() {
          let _ = proxy.send_event(());
        }
      })
    };

    self.app.shared.menu.set_waker(waker.clone());
    let tree = self.tree;
    tree.window().attach_shell(waker.clone());
    #[cfg(feature = "mcp")]
    tree.set_mcp_waker(waker.clone());
    let secondaries = (0..tree.secondary_window_count())
      .filter_map(|index| {
        tree
          .secondary_window(index)
          .map(|secondary| ManagedSecondaryWindow::new(index, secondary))
      })
      .collect();

    let mut handler = WinitHandler {
      app: self.app,
      main: ManagedWindow::new(
        tree,
        self.attrs,
        self.start_without_focus,
        self.corner_radius,
        self.on_tick,
        self.on_paint,
        self.on_position_changed,
        self.on_size_changed,
        true,
      ),
      secondaries,
      loop_cadence: WinitLoopCadence::new(Instant::now()),
      window_waker: waker,
    };
    #[cfg(target_os = "macos")]
    let _native =
      crate::app::macos_menu::NativeMenu::install(handler.app.shared.menu.clone(), handler.main.tree.window().clone());
    event_loop.run_app(&mut handler).unwrap();
    #[cfg(feature = "mcp")]
    handler.main.tree.shutdown_mcp();
  }
}

struct ManagedWindow {
  tree: Tree,
  window: Option<Window>,
  cursor_pos: (f64, f64),
  cursor: CursorIcon,
  modifiers: ModifiersState,
  attrs: Option<WindowAttributes>,
  start_without_focus: bool,
  corner_radius: Option<WindowCornerRadius>,
  on_tick: Option<TickFn>,
  on_paint: Option<PaintFn>,
  on_position_changed: Option<PositionChangedFn>,
  on_size_changed: Option<SizeChangedFn>,
  size_reporter: SizeReporter,
  redraw_pending: bool,
  close_exits: bool,
  last_tick: Instant,
  last_paint: Instant,
  pacing: FramePacing,
  continuous_redraw_until: Option<Instant>,
  /// Last direct present from inside the mouse-move stream (interactive
  /// drags paint here — see the CursorMoved arm).
  last_interaction_present: Instant,
  ime: ime::ImeSync,
  file_drag: FileDragShell,
}

impl ManagedWindow {
  fn new(
    tree: Tree,
    attrs: WindowAttributes,
    start_without_focus: bool,
    corner_radius: Option<WindowCornerRadius>,
    on_tick: Option<TickFn>,
    on_paint: Option<PaintFn>,
    on_position_changed: Option<PositionChangedFn>,
    on_size_changed: Option<SizeChangedFn>,
    close_exits: bool,
  ) -> Self {
    Self {
      tree,
      window: None,
      cursor_pos: (0.0, 0.0),
      cursor: CursorIcon::Default,
      modifiers: ModifiersState::empty(),
      attrs: Some(attrs),
      start_without_focus,
      corner_radius,
      on_tick,
      on_paint,
      on_position_changed,
      on_size_changed,
      size_reporter: SizeReporter::default(),
      redraw_pending: false,
      close_exits,
      last_tick: Instant::now(),
      last_paint: Instant::now(),
      pacing: FramePacing::default(),
      continuous_redraw_until: None,
      last_interaction_present: Instant::now(),
      ime: ime::ImeSync::default(),
      file_drag: FileDragShell::default(),
    }
  }

  fn window_id(&self) -> Option<WindowId> {
    self.window.as_ref().map(Window::id)
  }

  /// Whether the loop must poll for this window: `on_tick` runs every turn,
  /// or a file event waits for the turn's end.
  fn needs_polling(&self) -> bool {
    self.on_tick.is_some() || self.tree.needs_polling() || self.file_drag.has_pending()
  }

  /// When the pointer is next read for a file drag over the window.
  fn next_file_drag_poll(&self) -> Option<Instant> {
    self.file_drag.next_poll()
  }

  fn create_window(&mut self, event_loop: &ActiveEventLoop, app: &mut App) {
    if self.window.is_some() {
      return;
    }

    let attrs = self.attrs.take().unwrap_or_default();
    self.tree.window().record_initial_title(&attrs.title);
    // macOS set_visible(true) makes a window key even when it was created inactive.
    let show_after_first_present = attrs.visible && !self.start_without_focus;
    let attrs = if show_after_first_present {
      attrs.with_visible(false)
    } else {
      attrs
    };
    let window = event_loop.create_window(attrs).unwrap();
    if let Some(radius) = self.corner_radius {
      set_corner_radius(&window, radius);
      self.tree.window().set_corner_radius(radius);
    }
    let size = window.inner_size();
    self.tree.set_scale_factor(window.scale_factor() as f32);
    self.tree.resize(size.width, size.height);
    if let Ok(position) = window.outer_position() {
      self.tree.set_window_position(position.x, position.y);
      self.notify_position_changed(position.x, position.y);
    }
    self.window = Some(window);
    self.sync_window_state();
    let presented = show_after_first_present && self.present_now(app, true);
    if show_after_first_present {
      if let Some(window) = &self.window {
        window.set_visible(true);
      }
      self.tree.request_redraw();
      self.check_redraw();
    }
    if !presented {
      self.request_redraw();
    }
  }

  fn sync_window_state(&mut self) -> bool {
    let Some(window) = &self.window else {
      return false;
    };

    let size = window.inner_size();
    let minimized = window.is_minimized();
    let maximized = window.is_maximized();
    let full_screen = window.fullscreen().is_some();
    let focused = window.has_focus();
    let current = self.tree.window().info();
    let size_changed =
      current.resolved_width.round() as u32 != size.width || current.resolved_height.round() as u32 != size.height;

    if size_changed {
      self.tree.resize(size.width, size.height);
    }
    if let Some(minimized) = minimized {
      self.tree.window().set_minimized(minimized);
    }
    self.tree.window().set_maximized(maximized);
    self.tree.window().set_full_screen(full_screen);
    self.tree.window().set_focused(focused);
    self.report_size();

    size_changed
  }

  fn apply_window_commands(&mut self, event_loop: &ActiveEventLoop) -> bool {
    let commands = self.tree.window().take_shell_commands();
    if commands.is_empty() {
      return false;
    }

    let mut closed = false;
    for command in commands {
      match command {
        WindowCommand::RequestClose(_) => unreachable!("resolved by take_shell_commands"),
        WindowCommand::Close => {
          if self.close_exits {
            event_loop.exit();
          } else {
            self.window = None;
            self.redraw_pending = false;
            closed = true;
          }
        }
        WindowCommand::SetMinimized(minimized) => {
          if let Some(window) = &self.window {
            window.set_minimized(minimized);
          }
          self.tree.window().set_minimized(minimized);
        }
        WindowCommand::SetMaximized(maximized) => {
          if let Some(window) = &self.window {
            window.set_maximized(maximized);
          }
          self.tree.window().set_maximized(maximized);
        }
        WindowCommand::SetFullScreen(full_screen) => {
          if let Some(window) = &self.window {
            window.set_fullscreen(full_screen.then(|| Fullscreen::Borderless(None)));
          }
          self.tree.window().set_full_screen(full_screen);
        }
        WindowCommand::SetDecorated(decorated) => {
          if let Some(window) = &self.window {
            window.set_decorations(decorated);
          }
          self.tree.window().set_decorated(decorated);
        }
        WindowCommand::SetTitle(title) => {
          if let Some(window) = &self.window {
            window.set_title(&title);
          }
        }
        WindowCommand::SetTitleBarColor(color) => {
          if let Some(window) = &self.window {
            set_title_bar_color(window, color);
          }
        }
        WindowCommand::SetBorderColor(color) => {
          if let Some(window) = &self.window {
            set_border_color(window, color);
          }
          self.tree.window().set_border_color(color);
        }
        WindowCommand::SetIcon(icon) => {
          if let Some(window) = &self.window {
            set_window_icons(window, icon.and_then(to_winit_icon));
          }
        }
        WindowCommand::SetCornerRadius(radius) => {
          if let Some(window) = &self.window {
            set_corner_radius(window, radius);
          }
          self.tree.window().set_corner_radius(radius);
        }
        WindowCommand::Move { x, y } => {
          if let Some(window) = &self.window {
            window.set_outer_position(PhysicalPosition::new(x, y));
          }
          self.tree.set_window_position(x, y);
          self.notify_position_changed(x, y);
        }
        WindowCommand::Resize(request) => {
          // The size the window took reaches `on_size_changed` from `sync_window_state` below.
          self.tree.window().apply_resize(self.window.as_ref(), request);
        }
        WindowCommand::StartDrag => {
          if self.window.as_ref().is_some_and(start_native_window_drag) {
            let (x, y) = (self.cursor_pos.0 as f32, self.cursor_pos.1 as f32);
            self.tree.mouse_press_taken_by_os(x, y, MouseButton::Left);
          }
        }
        WindowCommand::StartResize(direction) => {
          if self
            .window
            .as_ref()
            .is_some_and(|window| start_native_window_resize(window, direction))
          {
            let (x, y) = (self.cursor_pos.0 as f32, self.cursor_pos.1 as f32);
            self.tree.mouse_press_taken_by_os(x, y, MouseButton::Left);
          }
        }
        WindowCommand::StopDrag => {}
        WindowCommand::InjectInput(input) => {
          crate::app::synthetic_input::apply(&mut self.tree, &input);
        }
        #[cfg(feature = "clipboard")]
        WindowCommand::PasteFromClipboard { synthetic } => {
          self.tree.paste_from_os_clipboard(synthetic);
        }
        #[cfg(feature = "screenshot")]
        WindowCommand::Screenshot(output_path, region) => {
          self.tree.request_screenshot(output_path, region);
        }
        WindowCommand::OpenDevtools => {
          self.tree.open_devtools();
        }
        WindowCommand::CloseDevtools => {
          self.tree.close_devtools();
        }
        WindowCommand::ToggleDevtools => {
          self.tree.toggle_devtools();
        }
      }
    }
    if self.sync_window_state() {
      self.request_redraw();
    }
    closed
  }

  fn check_redraw(&mut self) {
    if self.tree.needs_redraw() {
      self.request_redraw();
    }
    if let Some(window) = &self.window {
      self.ime.apply(window, &self.tree);
    }
  }

  fn request_redraw(&mut self) {
    if self.redraw_pending {
      return;
    }
    if let Some(w) = &self.window {
      self.redraw_pending = true;
      w.request_redraw();
    }
  }

  fn present_now(&mut self, app: &mut App, request_followup_redraw: bool) -> bool {
    if let Some(w) = &self.window {
      let started_at = Instant::now();
      let setup_started_at = Instant::now();
      let size = w.inner_size();
      self.tree.set_scale_factor(w.scale_factor() as f32);
      self.tree.resize(size.width, size.height);
      self.redraw_pending = false;
      let setup = setup_started_at.elapsed();
      let pass_started_at = Instant::now();
      let report = self.tree.pass(app, w);
      let pass = pass_started_at.elapsed();
      // A pass hit-tests the hover again when content moved under a pointer
      // that held still, which can change the cursor without an input event.
      set_window_cursor(w, &mut self.cursor, self.tree.cursor());
      // The pass painted the focused input's caret where it now is.
      self.ime.apply(w, &self.tree);
      let presented = report.rendered;
      let mut paint_duration = Duration::ZERO;
      if presented {
        let now = Instant::now();
        self.pacing.presented(PresentedFrame {
          started: pass_started_at,
          presented: now,
        });
        let delta = now.duration_since(self.last_paint);
        self.last_paint = now;
        if let Some(paint) = &mut self.on_paint {
          let paint_started_at = Instant::now();
          paint(&self.tree, delta, report);
          paint_duration = paint_started_at.elapsed();
        }
      }
      let followup_started_at = Instant::now();
      if request_followup_redraw {
        self.check_redraw();
      }
      let followup = followup_started_at.elapsed();
      let total = started_at.elapsed();
      if total >= WINIT_BREAKDOWN_THRESHOLD {
        crate::app::profile_support::video_log!(debug,
          target: "video::watch::lurq",
          "winit present_now breakdown total_ms={:.2} setup_ms={:.2} pass_ms={:.2} paint_ms={:.2} followup_ms={:.2} rendered={} cached={} request_followup_redraw={} reasons={:?}",
          duration_ms(total),
          duration_ms(setup),
          duration_ms(pass),
          duration_ms(paint_duration),
          duration_ms(followup),
          presented,
          report.used_cached_render_list,
          request_followup_redraw,
          report.reasons
        );
      }
      return presented;
    }
    false
  }

  fn has_continuous_redraw_video_recently(&mut self, now: Instant) -> bool {
    if self.tree.has_continuous_redraw_video() {
      self.continuous_redraw_until = now.checked_add(CONTINUOUS_REDRAW_GRACE);
      return true;
    }

    if self.continuous_redraw_until.is_some_and(|until| now <= until) {
      return true;
    }

    self.continuous_redraw_until = None;
    false
  }

  fn apply_cursor(&mut self) {
    match &self.window {
      Some(window) => set_window_cursor(window, &mut self.cursor, self.tree.cursor()),
      None => self.cursor = self.tree.cursor(),
    }
  }

  fn tick(&mut self) {
    let now = Instant::now();
    let delta = now.duration_since(self.last_tick);
    self.last_tick = now;

    if let Some(tick) = &mut self.on_tick {
      tick(&mut self.tree, delta);
    }
    self.tree.tick_timers();
    self.tree.tick_futures();
    self
      .file_drag
      .flush(&mut self.tree, self.window.as_ref(), self.cursor_pos);
    let has_continuous_video = self.has_continuous_redraw_video_recently(now);
    if has_continuous_video {
      // Video streams are presented directly from about_to_wait. Requesting
      // winit redraw events for the same stream adds a second, irregular presentation
      // path that can reset last_paint and create visible cadence gaps.
    } else {
      // Before the scheduled redraws are ticked: a hidden window drops them.
      let window = self.window.as_ref();
      self
        .pacing
        .request_frame(&mut self.tree, || refresh_interval(window), now);
    }
    self.tree.tick_scheduled_redraw(now);
    if !has_continuous_video {
      self.check_redraw();
    }
  }

  fn notify_position_changed(&mut self, x: i32, y: i32) {
    if let Some(callback) = &mut self.on_position_changed {
      callback(x, y);
    }
  }

  /// Reports the window's size to `on_size_changed`, in logical pixels, unless the window is minimized (its client
  /// area is then not its size) or the size was already reported.
  fn report_size(&mut self) {
    let (Some(window), Some(callback)) = (&self.window, &mut self.on_size_changed) else {
      return;
    };
    let size = window.inner_size();
    let scale = self.tree.window().info().scale_factor.max(f32::EPSILON);
    let logical = (
      ((size.width as f32) / scale).round().max(1.0) as u32,
      ((size.height as f32) / scale).round().max(1.0) as u32,
    );
    if let Some((width, height)) = self.size_reporter.next(window.is_minimized() == Some(true), logical) {
      callback(width, height);
    }
  }

  fn handle_event(&mut self, app: &mut App, event_loop: &ActiveEventLoop, event: WindowEvent) -> bool {
    self.tree.set_app_ref(app);
    self.pacing.handle_event(&event);
    if self.file_drag.on_event(&event) {
      // Delivered at the turn's end, once the burst of one file per event
      // is complete; see `file_drag`.
      return false;
    }
    match event {
      WindowEvent::CloseRequested => {
        self.tree.window().dispatch_close_request(CloseRequestSource::Os);
        self.request_redraw();
      }
      WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
        self.tree.set_scale_factor(scale_factor as f32);
        if let Some(w) = &self.window {
          let size = w.inner_size();
          self.tree.resize(size.width, size.height);
          self.request_redraw();
        }
      }
      WindowEvent::Resized(size) => {
        self.tree.resize(size.width, size.height);
        self.sync_window_state();
        // Paint the new size from inside the event: during a live edge drag Windows runs a modal sizing
        // loop that keeps delivering WM_SIZE, but WM_PAINT (and so `RedrawRequested`) only when the
        // queue is idle, and DWM stretches the previous frame until a new one is presented.
        // Only for the current size: size events queued while winit was busy arrive back to back, and
        // painting each stale one would replay the whole drag.
        let current = self.window.as_ref().is_some_and(|w| w.inner_size() == size);
        if current && size.width > 0 && size.height > 0 && self.present_now(app, false) {
          self.apply_window_commands(event_loop);
          return true;
        }
        self.request_redraw();
      }
      WindowEvent::Focused(focused) => {
        self.tree.window().set_focused(focused);
        self.request_redraw();
      }
      WindowEvent::Moved(position) => {
        self.tree.set_window_position(position.x, position.y);
        self.notify_position_changed(position.x, position.y);
      }
      WindowEvent::CursorMoved { position, .. } => {
        self.cursor_pos = (position.x, position.y);
        self.tree.mouse_move_with_meta(
          position.x as f32,
          position.y as f32,
          self.modifiers.shift_key(),
          self.modifiers.control_key(),
          self.modifiers.alt_key(),
          self.modifiers.super_key(),
        );
        self.apply_cursor();
        self.check_redraw();
        // Any redraw this move produced (interactive drags — scrollbars,
        // sliders, text selection, on_drag_* sessions — but equally hover
        // enter/leave restyles) must paint from inside the event stream: a
        // high-rate mouse never lets the queue drain, so both
        // `about_to_wait` and WM_PAINT are starved for the whole gesture.
        // Gated on drags only, a drag repainted but a hover sweep skipped
        // every row between where the pointer started and where it paused.
        if self.tree.needs_redraw() && self.last_interaction_present.elapsed() >= INTERACTION_PRESENT_INTERVAL {
          self.last_interaction_present = Instant::now();
          let presented = self.present_now(app, false);
          if presented {
            self.apply_window_commands(event_loop);
            return true;
          }
        }
      }
      WindowEvent::CursorLeft { .. } => {
        self.tree.mouse_leave_window();
        self.apply_cursor();
        self.check_redraw();
      }
      WindowEvent::MouseInput { state, button, .. } => {
        let btn = to_lurq_mouse_button(button);
        let (x, y) = (self.cursor_pos.0 as f32, self.cursor_pos.1 as f32);
        match state {
          ElementState::Pressed => self.tree.mouse_down_with_meta(
            x,
            y,
            btn,
            self.modifiers.shift_key(),
            self.modifiers.control_key(),
            self.modifiers.alt_key(),
            self.modifiers.super_key(),
          ),
          ElementState::Released => self.tree.mouse_up_with_meta(
            x,
            y,
            btn,
            self.modifiers.shift_key(),
            self.modifiers.control_key(),
            self.modifiers.alt_key(),
            self.modifiers.super_key(),
          ),
        }
        self.apply_window_commands(event_loop);
        self.apply_cursor();
        self.check_redraw();
        return false;
      }
      WindowEvent::ModifiersChanged(modifiers) => {
        self.modifiers = modifiers.state();
      }
      WindowEvent::Ime(ime) => {
        self.tree.ime(ime::to_ime_event(ime));
        self.check_redraw();
      }
      WindowEvent::KeyboardInput { event, .. } => {
        if is_open_devtools_shortcut(&event, self.modifiers) {
          self.tree.open_devtools();
          self.apply_cursor();
          self.check_redraw();
          return false;
        }
        match event.state {
          ElementState::Pressed => self.tree.key_down_with_meta(
            key_to_string(&event),
            physical_key_to_string(&event.physical_key),
            self.modifiers.shift_key(),
            self.modifiers.control_key(),
            self.modifiers.alt_key(),
            self.modifiers.super_key(),
          ),
          ElementState::Released => self.tree.key_up_with_meta(
            key_to_string(&event),
            physical_key_to_string(&event.physical_key),
            self.modifiers.shift_key(),
            self.modifiers.control_key(),
            self.modifiers.alt_key(),
            self.modifiers.super_key(),
          ),
        }
        self.apply_cursor();
        self.check_redraw();
      }
      WindowEvent::MouseWheel { delta, phase, .. } => {
        let (mut dx, mut dy) = match delta {
          MouseScrollDelta::LineDelta(x, y) => (x * 40.0, y * 40.0),
          MouseScrollDelta::PixelDelta(p) => (p.x as f32, p.y as f32),
        };
        if self.modifiers.shift_key() && dx == 0.0 {
          dx = dy;
          dy = 0.0;
        }
        let scroll_phase = match phase {
          TouchPhase::Started => ScrollPhase::Start,
          TouchPhase::Moved => ScrollPhase::Scroll,
          TouchPhase::Ended | TouchPhase::Cancelled => ScrollPhase::End,
        };
        if phase == TouchPhase::Started {
          self.tree.set_history_swipes(history_swipes::enabled());
        }
        self
          .tree
          .scroll(self.cursor_pos.0 as f32, self.cursor_pos.1 as f32, dx, dy, scroll_phase);
        self.apply_cursor();
        self.check_redraw();
      }
      WindowEvent::RedrawRequested => {
        let presented = self.present_now(app, true);
        self.apply_window_commands(event_loop);
        return presented;
      }
      _ => {}
    }
    self.apply_window_commands(event_loop);
    self.check_redraw();
    false
  }
}

struct ManagedSecondaryWindow {
  index: usize,
  window: Option<Window>,
  cursor_pos: (f64, f64),
  cursor: CursorIcon,
  modifiers: ModifiersState,
  attrs: Option<WindowAttributes>,
  redraw_pending: bool,
  close_requested: bool,
  pacing: FramePacing,
  ime: ime::ImeSync,
  file_drag: FileDragShell,
}

impl ManagedSecondaryWindow {
  fn new(index: usize, secondary: &SecondaryWindow) -> Self {
    Self {
      index,
      window: None,
      cursor_pos: (0.0, 0.0),
      cursor: CursorIcon::Default,
      modifiers: ModifiersState::empty(),
      attrs: Some(
        WindowAttributes::default()
          .with_title(secondary.title())
          .with_decorations(secondary.decorations())
          .with_inner_size(winit::dpi::LogicalSize::new(secondary.width(), secondary.height())),
      ),
      redraw_pending: false,
      close_requested: false,
      pacing: FramePacing::default(),
      ime: ime::ImeSync::default(),
      file_drag: FileDragShell::default(),
    }
  }

  fn index(&self) -> usize {
    self.index
  }

  fn needs_polling(&self) -> bool {
    self.file_drag.has_pending()
  }

  fn next_file_drag_poll(&self) -> Option<Instant> {
    self.file_drag.next_poll()
  }

  fn window_id(&self) -> Option<WindowId> {
    self.window.as_ref().map(Window::id)
  }

  fn request_close(&mut self) {
    self.redraw_pending = false;
    self.close_requested = true;
  }

  fn close_requested(&self) -> bool {
    self.close_requested
  }

  fn create_window(
    &mut self,
    event_loop: &ActiveEventLoop,
    app: &mut App,
    tree: &mut Tree,
  ) -> Option<SecondaryWindowMetadata> {
    if self.window.is_some() {
      return None;
    }

    let attrs = self.attrs.take().unwrap_or_default();
    let show_after_first_present = attrs.visible;
    let attrs = if show_after_first_present {
      attrs.with_visible(false)
    } else {
      attrs
    };
    let window = event_loop.create_window(attrs).unwrap();
    let size = window.inner_size();
    let metadata = secondary_window_metadata(&window);
    tree.set_scale_factor(window.scale_factor() as f32);
    tree.resize(size.width, size.height);
    if let Ok(position) = window.outer_position() {
      tree.set_window_position(position.x, position.y);
    }
    self.window = Some(window);
    self.sync_window_state(tree);
    let presented = show_after_first_present && self.present_now(app, tree);
    if show_after_first_present {
      if let Some(window) = &self.window {
        window.set_visible(true);
      }
    }
    if !presented {
      self.request_redraw();
    }
    Some(metadata)
  }

  fn sync_window_state(&mut self, tree: &mut Tree) {
    if let Some(window) = &self.window {
      if let Some(minimized) = window.is_minimized() {
        tree.window().set_minimized(minimized);
      }
      tree.window().set_maximized(window.is_maximized());
      tree.window().set_full_screen(window.fullscreen().is_some());
      tree.window().set_decorated(window.is_decorated());
      tree.window().set_focused(window.has_focus());
    }
  }

  fn apply_window_commands(&mut self, tree: &mut Tree) -> bool {
    let commands = tree.window().take_shell_commands();
    if commands.is_empty() {
      return false;
    }

    let mut closed = false;
    for command in commands {
      match command {
        WindowCommand::RequestClose(_) => unreachable!("resolved by take_shell_commands"),
        WindowCommand::Close => {
          self.request_close();
          closed = true;
        }
        WindowCommand::SetMinimized(minimized) => {
          if let Some(window) = &self.window {
            window.set_minimized(minimized);
          }
          tree.window().set_minimized(minimized);
        }
        WindowCommand::SetMaximized(maximized) => {
          if let Some(window) = &self.window {
            window.set_maximized(maximized);
          }
          tree.window().set_maximized(maximized);
        }
        WindowCommand::SetFullScreen(full_screen) => {
          if let Some(window) = &self.window {
            window.set_fullscreen(full_screen.then(|| Fullscreen::Borderless(None)));
          }
          tree.window().set_full_screen(full_screen);
        }
        WindowCommand::SetDecorated(decorated) => {
          if let Some(window) = &self.window {
            window.set_decorations(decorated);
          }
          tree.window().set_decorated(decorated);
        }
        WindowCommand::SetTitle(title) => {
          if let Some(window) = &self.window {
            window.set_title(&title);
          }
        }
        WindowCommand::SetTitleBarColor(color) => {
          if let Some(window) = &self.window {
            set_title_bar_color(window, color);
          }
        }
        WindowCommand::SetBorderColor(color) => {
          if let Some(window) = &self.window {
            set_border_color(window, color);
          }
          tree.window().set_border_color(color);
        }
        WindowCommand::SetIcon(icon) => {
          if let Some(window) = &self.window {
            set_window_icons(window, icon.and_then(to_winit_icon));
          }
        }
        WindowCommand::SetCornerRadius(radius) => {
          if let Some(window) = &self.window {
            set_corner_radius(window, radius);
          }
          tree.window().set_corner_radius(radius);
        }
        WindowCommand::Move { x, y } => {
          if let Some(window) = &self.window {
            window.set_outer_position(PhysicalPosition::new(x, y));
          }
          tree.set_window_position(x, y);
        }
        WindowCommand::Resize(request) => {
          tree.window().apply_resize(self.window.as_ref(), request);
        }
        WindowCommand::StartDrag => {
          if self.window.as_ref().is_some_and(start_native_window_drag) {
            let (x, y) = (self.cursor_pos.0 as f32, self.cursor_pos.1 as f32);
            tree.mouse_press_taken_by_os(x, y, MouseButton::Left);
          }
        }
        WindowCommand::StartResize(direction) => {
          if self
            .window
            .as_ref()
            .is_some_and(|window| start_native_window_resize(window, direction))
          {
            let (x, y) = (self.cursor_pos.0 as f32, self.cursor_pos.1 as f32);
            tree.mouse_press_taken_by_os(x, y, MouseButton::Left);
          }
        }
        WindowCommand::StopDrag => {}
        WindowCommand::InjectInput(input) => {
          crate::app::synthetic_input::apply(tree, &input);
        }
        #[cfg(feature = "clipboard")]
        WindowCommand::PasteFromClipboard { synthetic } => {
          tree.paste_from_os_clipboard(synthetic);
        }
        #[cfg(feature = "screenshot")]
        WindowCommand::Screenshot(output_path, region) => {
          tree.request_screenshot(output_path, region);
        }
        WindowCommand::OpenDevtools => {
          tree.open_devtools();
        }
        WindowCommand::CloseDevtools => {
          tree.close_devtools();
        }
        WindowCommand::ToggleDevtools => {
          tree.toggle_devtools();
        }
      }
    }
    self.sync_window_state(tree);
    closed
  }

  fn check_redraw(&mut self, tree: &Tree) {
    if tree.needs_redraw() {
      self.request_redraw();
    }
    if let Some(window) = &self.window {
      self.ime.apply(window, tree);
    }
  }

  fn request_redraw(&mut self) {
    if self.redraw_pending {
      return;
    }
    if let Some(w) = &self.window {
      self.redraw_pending = true;
      w.request_redraw();
    }
  }

  fn present_now(&mut self, app: &mut App, tree: &mut Tree) -> bool {
    if let Some(w) = &self.window {
      let size = w.inner_size();
      tree.set_scale_factor(w.scale_factor() as f32);
      tree.resize(size.width, size.height);
      self.redraw_pending = false;
      let started = Instant::now();
      let presented = tree.pass(app, w).rendered;
      // See the main window's `present_now`.
      set_window_cursor(w, &mut self.cursor, tree.cursor());
      if presented {
        self.pacing.presented(PresentedFrame {
          started,
          presented: Instant::now(),
        });
      }
      self.check_redraw(tree);
      return presented;
    }
    false
  }

  fn apply_cursor(&mut self, tree: &Tree) {
    match &self.window {
      Some(window) => set_window_cursor(window, &mut self.cursor, tree.cursor()),
      None => self.cursor = tree.cursor(),
    }
  }

  fn tick(&mut self, tree: &mut Tree) {
    let now = Instant::now();
    tree.tick_timers();
    tree.tick_futures();
    self.file_drag.flush(tree, self.window.as_ref(), self.cursor_pos);
    let window = self.window.as_ref();
    self.pacing.request_frame(tree, || refresh_interval(window), now);
    tree.tick_scheduled_redraw(now);
    self.check_redraw(tree);
  }

  fn handle_event(
    &mut self,
    app: &mut App,
    _event_loop: &ActiveEventLoop,
    event: WindowEvent,
    tree: &mut Tree,
  ) -> bool {
    tree.set_app_ref(app);
    self.pacing.handle_event(&event);
    if self.file_drag.on_event(&event) {
      return false;
    }
    match event {
      WindowEvent::CloseRequested => {
        tree.window().dispatch_close_request(CloseRequestSource::Os);
        self.request_redraw();
      }
      WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
        tree.set_scale_factor(scale_factor as f32);
        if let Some(w) = &self.window {
          let size = w.inner_size();
          tree.resize(size.width, size.height);
          self.request_redraw();
        }
      }
      WindowEvent::Resized(size) => {
        tree.resize(size.width, size.height);
        self.sync_window_state(tree);
        // Paint inside the event, as the main window does, so a live resize is not stretched.
        let current = self.window.as_ref().is_some_and(|w| w.inner_size() == size);
        if !current || size.width == 0 || size.height == 0 || !self.present_now(app, tree) {
          self.request_redraw();
        }
      }
      WindowEvent::Focused(focused) => {
        tree.window().set_focused(focused);
        self.request_redraw();
      }
      WindowEvent::Moved(position) => {
        tree.set_window_position(position.x, position.y);
      }
      WindowEvent::CursorMoved { position, .. } => {
        self.cursor_pos = (position.x, position.y);
        tree.mouse_move_with_meta(
          position.x as f32,
          position.y as f32,
          self.modifiers.shift_key(),
          self.modifiers.control_key(),
          self.modifiers.alt_key(),
          self.modifiers.super_key(),
        );
        self.apply_cursor(tree);
        self.check_redraw(tree);
      }
      WindowEvent::CursorLeft { .. } => {
        tree.mouse_leave_window();
        self.apply_cursor(tree);
        self.check_redraw(tree);
      }
      WindowEvent::MouseInput { state, button, .. } => {
        let btn = to_lurq_mouse_button(button);
        let (x, y) = (self.cursor_pos.0 as f32, self.cursor_pos.1 as f32);
        match state {
          ElementState::Pressed => tree.mouse_down_with_meta(
            x,
            y,
            btn,
            self.modifiers.shift_key(),
            self.modifiers.control_key(),
            self.modifiers.alt_key(),
            self.modifiers.super_key(),
          ),
          ElementState::Released => tree.mouse_up_with_meta(
            x,
            y,
            btn,
            self.modifiers.shift_key(),
            self.modifiers.control_key(),
            self.modifiers.alt_key(),
            self.modifiers.super_key(),
          ),
        }
        self.apply_window_commands(tree);
        self.apply_cursor(tree);
        self.check_redraw(tree);
        return false;
      }
      WindowEvent::ModifiersChanged(modifiers) => {
        self.modifiers = modifiers.state();
      }
      WindowEvent::Ime(ime) => {
        tree.ime(ime::to_ime_event(ime));
        self.check_redraw(tree);
      }
      WindowEvent::KeyboardInput { event, .. } => {
        match event.state {
          ElementState::Pressed => tree.key_down_with_meta(
            key_to_string(&event),
            physical_key_to_string(&event.physical_key),
            self.modifiers.shift_key(),
            self.modifiers.control_key(),
            self.modifiers.alt_key(),
            self.modifiers.super_key(),
          ),
          ElementState::Released => tree.key_up_with_meta(
            key_to_string(&event),
            physical_key_to_string(&event.physical_key),
            self.modifiers.shift_key(),
            self.modifiers.control_key(),
            self.modifiers.alt_key(),
            self.modifiers.super_key(),
          ),
        }
        self.apply_cursor(tree);
        self.check_redraw(tree);
      }
      WindowEvent::MouseWheel { delta, phase, .. } => {
        let (mut dx, mut dy) = match delta {
          MouseScrollDelta::LineDelta(x, y) => (x * 40.0, y * 40.0),
          MouseScrollDelta::PixelDelta(p) => (p.x as f32, p.y as f32),
        };
        if self.modifiers.shift_key() && dx == 0.0 {
          dx = dy;
          dy = 0.0;
        }
        let scroll_phase = match phase {
          TouchPhase::Started => ScrollPhase::Start,
          TouchPhase::Moved => ScrollPhase::Scroll,
          TouchPhase::Ended | TouchPhase::Cancelled => ScrollPhase::End,
        };
        if phase == TouchPhase::Started {
          tree.set_history_swipes(history_swipes::enabled());
        }
        tree.scroll(self.cursor_pos.0 as f32, self.cursor_pos.1 as f32, dx, dy, scroll_phase);
        self.apply_cursor(tree);
        self.check_redraw(tree);
      }
      WindowEvent::RedrawRequested => {
        let presented = self.present_now(app, tree);
        self.apply_window_commands(tree);
        return presented;
      }
      _ => {}
    }
    self.apply_window_commands(tree);
    self.check_redraw(tree);
    false
  }
}

fn secondary_window_metadata(window: &Window) -> SecondaryWindowMetadata {
  let raw_window = window.window_handle().ok().map(|handle| handle.as_raw());
  let raw_display = window.display_handle().ok().map(|handle| handle.as_raw());
  let hwnd = raw_window.and_then(|handle| match handle {
    RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
    _ => None,
  });

  SecondaryWindowMetadata {
    window_id: Some(format!("{:?}", window.id())),
    raw_window_handle: raw_window.map(|handle| format!("{handle:?}")),
    raw_display_handle: raw_display.map(|handle| format!("{handle:?}")),
    hwnd,
  }
}

struct WinitHandler {
  app: App,
  main: ManagedWindow,
  secondaries: Vec<ManagedSecondaryWindow>,
  loop_cadence: WinitLoopCadence,
  /// Cloned into every tree's `Window` so cross-thread command pushes wake
  /// the loop.
  window_waker: crate::app::window::WindowWaker,
}

struct WinitLoopCadence {
  started_at: Instant,
  ticks: u32,
  direct_presents: u32,
  redraw_requests: u32,
  poll_requests: u32,
  wait_until_requests: u32,
  wait_requests: u32,
  continuous_ticks: u32,
  redraw_pending_ticks: u32,
  last_tick_at: Option<Instant>,
  max_tick_delta: Duration,
}

fn duration_ms(duration: Duration) -> f64 {
  duration.as_secs_f64() * 1000.0
}

impl WinitLoopCadence {
  fn new(now: Instant) -> Self {
    Self {
      started_at: now,
      ticks: 0,
      direct_presents: 0,
      redraw_requests: 0,
      poll_requests: 0,
      wait_until_requests: 0,
      wait_requests: 0,
      continuous_ticks: 0,
      redraw_pending_ticks: 0,
      last_tick_at: None,
      max_tick_delta: Duration::ZERO,
    }
  }

  fn record(
    &mut self,
    now: Instant,
    has_continuous_redraw_image: bool,
    redraw_pending: bool,
    direct_presented: bool,
    redraw_requested: bool,
    control_flow: &'static str,
  ) {
    if let Some(last_tick_at) = self.last_tick_at {
      self.max_tick_delta = self.max_tick_delta.max(now.saturating_duration_since(last_tick_at));
    }
    self.last_tick_at = Some(now);
    self.ticks += 1;
    self.direct_presents += u32::from(direct_presented);
    self.redraw_requests += u32::from(redraw_requested);
    self.continuous_ticks += u32::from(has_continuous_redraw_image);
    self.redraw_pending_ticks += u32::from(redraw_pending);
    match control_flow {
      "poll" => self.poll_requests += 1,
      "wait_until" => self.wait_until_requests += 1,
      _ => self.wait_requests += 1,
    }

    if now.saturating_duration_since(self.started_at) < Duration::from_secs(1) {
      return;
    }

    crate::app::profile_support::video_log!(debug,
      target: "video::watch::lurq",
      "winit loop cadence ticks={} max_tick_delta_ms={:.2} continuous_ticks={} redraw_pending_ticks={} direct_presents={} redraw_requests={} poll={} wait_until={} wait={}",
      self.ticks,
      duration_ms(self.max_tick_delta),
      self.continuous_ticks,
      self.redraw_pending_ticks,
      self.direct_presents,
      self.redraw_requests,
      self.poll_requests,
      self.wait_until_requests,
      self.wait_requests
    );

    *self = Self::new(now);
  }
}

struct WinitSlowScope {
  name: &'static str,
  detail: &'static str,
  started_at: Instant,
}

impl WinitSlowScope {
  fn new(name: &'static str, detail: &'static str) -> Self {
    Self {
      name,
      detail,
      started_at: Instant::now(),
    }
  }
}

impl Drop for WinitSlowScope {
  fn drop(&mut self) {
    let elapsed = self.started_at.elapsed();
    if elapsed < WINIT_SLOW_SCOPE_THRESHOLD {
      return;
    }

    crate::app::profile_support::video_log!(debug,
      target: "video::watch::lurq",
      "winit slow scope name={} detail={} elapsed_ms={:.2}",
      self.name,
      self.detail,
      duration_ms(elapsed)
    );
  }
}

impl WinitHandler {
  fn close_secondary_at_position(&mut self, position: usize) {
    let index = self.secondaries[position].index();
    if self.main.tree.close_secondary_window(index) {
      self.main.request_redraw();
    }
    self.secondaries.remove(position);
  }

  fn sync_secondary_windows(&mut self, event_loop: &ActiveEventLoop) {
    self
      .secondaries
      .retain(|secondary| self.main.tree.secondary_window(secondary.index()).is_some());

    let count = self.main.tree.secondary_window_count();
    for index in 0..count {
      if self.secondaries.iter().any(|secondary| secondary.index() == index) {
        continue;
      }

      let Some(secondary) = self.main.tree.secondary_window(index) else {
        continue;
      };
      secondary.tree().window().attach_shell(self.window_waker.clone());
      let mut managed = ManagedSecondaryWindow::new(index, secondary);
      self.main.tree.ensure_secondary_window_render_engine(index);
      let metadata = self
        .main
        .tree
        .secondary_window_mut(index)
        .and_then(|secondary| managed.create_window(event_loop, &mut self.app, secondary.tree_mut()));
      if let Some(metadata) = metadata {
        self.main.tree.set_secondary_window_metadata(index, metadata);
      }
      self.secondaries.push(managed);
    }
  }

  fn check_secondary_redraw(&mut self) {
    for secondary_window in &mut self.secondaries {
      let index = secondary_window.index();
      if let Some(secondary) = self.main.tree.secondary_window(index) {
        secondary_window.check_redraw(secondary.tree());
      }
    }
  }

  /// Presents each window an MCP call delivered held input to (a press, a move while held), as a real mouse-move
  /// stream presents from inside its events, so each call lands in exactly one pass of its own.
  #[cfg(feature = "mcp")]
  fn present_mcp_input(&mut self, event_loop: &ActiveEventLoop) {
    if self.main.tree.take_mcp_input_present()
      && self.main.tree.needs_redraw()
      && self.main.present_now(&mut self.app, false)
    {
      self.main.apply_window_commands(event_loop);
    }
    for position in 0..self.secondaries.len() {
      let index = self.secondaries[position].index();
      let Some(secondary) = self.main.tree.secondary_window_mut(index) else {
        continue;
      };
      let tree = secondary.tree_mut();
      if tree.take_mcp_input_present() && tree.needs_redraw() {
        self.secondaries[position].present_now(&mut self.app, tree);
      }
    }
  }

  fn present_dirty_secondaries(&mut self) {
    for position in 0..self.secondaries.len() {
      let index = self.secondaries[position].index();
      let should_present = self
        .main
        .tree
        .secondary_window(index)
        .is_some_and(|secondary| secondary.tree().needs_redraw());
      if !should_present {
        continue;
      }

      let secondary_window = &mut self.secondaries[position];
      if let Some(secondary) = self.main.tree.secondary_window_mut(index) {
        secondary_window.present_now(&mut self.app, secondary.tree_mut());
      }
    }
  }

  fn apply_secondary_window_requests(&mut self, event_loop: &ActiveEventLoop) {
    if self.main.tree.apply_secondary_window_requests(&mut self.app) {
      self.main.request_redraw();
    }
    self.sync_secondary_windows(event_loop);
    self.check_secondary_redraw();
  }

  fn handle_secondary_pick_event(&mut self, event: &WindowEvent) -> bool {
    if !self.main.tree.secondary_pick_mode() {
      return false;
    }

    match event {
      WindowEvent::CursorMoved { position, .. } => {
        self.main.cursor_pos = (position.x, position.y);
        true
      }
      WindowEvent::MouseInput { state, button, .. } => {
        if *button != winit::event::MouseButton::Left {
          return true;
        }

        if matches!(state, ElementState::Pressed) {
          return true;
        }

        let (x, y) = (self.main.cursor_pos.0 as f32, self.main.cursor_pos.1 as f32);
        self.main.tree.pick_secondary_node_at(x, y);
        self.main.request_redraw();
        self.check_secondary_redraw();
        true
      }
      WindowEvent::KeyboardInput { event, .. }
        if matches!(event.state, ElementState::Pressed)
          && matches!(event.logical_key, Key::Named(NamedKey::Escape)) =>
      {
        self.main.tree.cancel_secondary_pick();
        self.check_secondary_redraw();
        true
      }
      _ => false,
    }
  }
}

fn window_event_name(event: &WindowEvent) -> &'static str {
  match event {
    WindowEvent::ActivationTokenDone { .. } => "ActivationTokenDone",
    WindowEvent::Resized(_) => "Resized",
    WindowEvent::Moved(_) => "Moved",
    WindowEvent::CloseRequested => "CloseRequested",
    WindowEvent::Destroyed => "Destroyed",
    WindowEvent::DroppedFile(_) => "DroppedFile",
    WindowEvent::HoveredFile(_) => "HoveredFile",
    WindowEvent::HoveredFileCancelled => "HoveredFileCancelled",
    WindowEvent::Focused(_) => "Focused",
    WindowEvent::KeyboardInput { .. } => "KeyboardInput",
    WindowEvent::ModifiersChanged(_) => "ModifiersChanged",
    WindowEvent::Ime(_) => "Ime",
    WindowEvent::CursorMoved { .. } => "CursorMoved",
    WindowEvent::CursorEntered { .. } => "CursorEntered",
    WindowEvent::CursorLeft { .. } => "CursorLeft",
    WindowEvent::MouseWheel { .. } => "MouseWheel",
    WindowEvent::MouseInput { .. } => "MouseInput",
    WindowEvent::PinchGesture { .. } => "PinchGesture",
    WindowEvent::PanGesture { .. } => "PanGesture",
    WindowEvent::DoubleTapGesture { .. } => "DoubleTapGesture",
    WindowEvent::RotationGesture { .. } => "RotationGesture",
    WindowEvent::TouchpadPressure { .. } => "TouchpadPressure",
    WindowEvent::AxisMotion { .. } => "AxisMotion",
    WindowEvent::Touch(_) => "Touch",
    WindowEvent::ScaleFactorChanged { .. } => "ScaleFactorChanged",
    WindowEvent::ThemeChanged(_) => "ThemeChanged",
    WindowEvent::Occluded(_) => "Occluded",
    WindowEvent::RedrawRequested => "RedrawRequested",
  }
}

impl ApplicationHandler for WinitHandler {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    self.main.create_window(event_loop, &mut self.app);
    #[cfg(target_os = "macos")]
    crate::app::macos_menu::sync();
    self.sync_secondary_windows(event_loop);
  }

  fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
    let event_name = window_event_name(&event);
    let _slow_scope = WinitSlowScope::new("window_event", event_name);
    if self.main.window_id() == Some(id) {
      if matches!(event, WindowEvent::RedrawRequested) && self.main.has_continuous_redraw_video_recently(Instant::now())
      {
        self.main.redraw_pending = false;
        return;
      }
      let started_at = Instant::now();
      let pick_started_at = Instant::now();
      if self.handle_secondary_pick_event(&event) {
        let pick = pick_started_at.elapsed();
        let total = started_at.elapsed();
        if total >= WINIT_BREAKDOWN_THRESHOLD {
          crate::app::profile_support::video_log!(debug,
            target: "video::watch::lurq",
            "winit window_event breakdown event={} total_ms={:.2} pick_ms={:.2} handle_ms=0.00 secondary_requests_ms=0.00 post_present_ms=0.00 presented=false picked=true",
            event_name,
            duration_ms(total),
            duration_ms(pick)
          );
        }
        return;
      }
      let pick = pick_started_at.elapsed();
      let handle_started_at = Instant::now();
      let presented = self.main.handle_event(&mut self.app, event_loop, event);
      let handle = handle_started_at.elapsed();
      let requests_started_at = Instant::now();
      self.apply_secondary_window_requests(event_loop);
      let requests = requests_started_at.elapsed();
      let post_present_started_at = Instant::now();
      if presented {
        self.check_secondary_redraw();
        self.present_dirty_secondaries();
      }
      let post_present = post_present_started_at.elapsed();
      let total = started_at.elapsed();
      if total >= WINIT_BREAKDOWN_THRESHOLD {
        crate::app::profile_support::video_log!(debug,
          target: "video::watch::lurq",
          "winit window_event breakdown event={} total_ms={:.2} pick_ms={:.2} handle_ms={:.2} secondary_requests_ms={:.2} post_present_ms={:.2} presented={} picked=false",
          event_name,
          duration_ms(total),
          duration_ms(pick),
          duration_ms(handle),
          duration_ms(requests),
          duration_ms(post_present),
          presented
        );
      }
      return;
    }

    if let Some(position) = self
      .secondaries
      .iter()
      .position(|secondary| secondary.window_id() == Some(id))
    {
      if is_open_devtools_window_event(&event, self.secondaries[position].modifiers) {
        self.main.tree.open_devtools();
        self.apply_secondary_window_requests(event_loop);
        return;
      }
      let index = self.secondaries[position].index();
      let mut closed = false;
      if let Some(secondary) = self.main.tree.secondary_window_mut(index) {
        self.secondaries[position].handle_event(&mut self.app, event_loop, event, secondary.tree_mut());
        closed = self.secondaries[position].close_requested();
      }
      if closed {
        self.close_secondary_at_position(position);
      } else {
        self.apply_secondary_window_requests(event_loop);
      }
    }
  }

  fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
    let _slow_scope = WinitSlowScope::new("about_to_wait", "main");
    let started_at = Instant::now();
    let stage_started_at = Instant::now();
    #[cfg(target_os = "macos")]
    crate::app::macos_menu::sync();
    self.app.shared.menu.drain(self.main.tree.window());
    self.main.tick();
    let tick = stage_started_at.elapsed();
    let stage_started_at = Instant::now();
    self.main.apply_window_commands(event_loop);
    #[cfg(feature = "mcp")]
    if self.main.tree.drain_mcp_requests(&mut self.app) {
      // Tool handlers ran against the trees; pick up any redraws or window
      // commands they produced.
      self.main.apply_window_commands(event_loop);
      // Before any redraw is requested: a requested redraw would paint the same input a second time.
      self.present_mcp_input(event_loop);
      self.main.check_redraw();
      self.check_secondary_redraw();
    }
    let main_commands = stage_started_at.elapsed();
    let stage_started_at = Instant::now();
    self.apply_secondary_window_requests(event_loop);
    let secondary_requests = stage_started_at.elapsed();
    let mut direct_presented = false;
    let stage_started_at = Instant::now();
    let now = Instant::now();
    let main_has_continuous_video = self.main.has_continuous_redraw_video_recently(now);
    // Interactive drags (scrollbars, sliders, text selection, on_drag_*
    // sessions) present directly too: on Windows the WM_MOUSEMOVE stream
    // starves WM_PAINT, so redraws requested from drag handlers would only
    // land once the mouse pauses. Vsync in present caps the rate.
    let main_has_interactive_drag = self.main.tree.has_active_input_interaction() && self.main.tree.needs_redraw();
    let continuous_check = stage_started_at.elapsed();
    let mut present = Duration::ZERO;
    let mut post_present = Duration::ZERO;
    if main_has_continuous_video || main_has_interactive_drag {
      let stage_started_at = Instant::now();
      let presented = self.main.present_now(&mut self.app, false);
      present = stage_started_at.elapsed();
      direct_presented = presented;
      let stage_started_at = Instant::now();
      self.apply_secondary_window_requests(event_loop);
      if presented {
        self.check_secondary_redraw();
        self.present_dirty_secondaries();
      }
      post_present = stage_started_at.elapsed();
    }
    let stage_started_at = Instant::now();
    let mut position = 0;
    while position < self.secondaries.len() {
      let secondary_index = self.secondaries[position].index();
      let mut closed = false;
      if let Some(secondary) = self.main.tree.secondary_window_mut(secondary_index) {
        let secondary_window = &mut self.secondaries[position];
        secondary_window.tick(secondary.tree_mut());
        closed = secondary_window.apply_window_commands(secondary.tree_mut());
      }

      if closed {
        self.close_secondary_at_position(position);
      } else {
        position += 1;
      }
    }
    let secondary_tick = stage_started_at.elapsed();
    let stage_started_at = Instant::now();
    self.sync_secondary_windows(event_loop);
    let sync_secondary = stage_started_at.elapsed();

    let stage_started_at = Instant::now();
    let secondary_needs_polling = self.secondaries.iter().any(|secondary_window| {
      self
        .main
        .tree
        .secondary_window(secondary_window.index())
        .is_some_and(|secondary| secondary.tree().needs_polling())
        || secondary_window.needs_polling()
    });
    let secondary_has_continuous_video = self.secondaries.iter().any(|secondary_window| {
      self
        .main
        .tree
        .secondary_window(secondary_window.index())
        .is_some_and(|secondary| secondary.tree().has_continuous_redraw_video())
    });
    let has_continuous_video = main_has_continuous_video || secondary_has_continuous_video;
    let redraw_pending = self.main.redraw_pending || self.secondaries.iter().any(|secondary| secondary.redraw_pending);
    let redraw_requested = self.main.tree.needs_redraw()
      || self.secondaries.iter().any(|secondary_window| {
        self
          .main
          .tree
          .secondary_window(secondary_window.index())
          .is_some_and(|secondary| secondary.tree().needs_redraw())
      });
    let trees = || {
      self
        .secondaries
        .iter()
        .filter_map(|secondary_window| self.main.tree.secondary_window(secondary_window.index()))
        .map(|secondary| secondary.tree())
        .chain(std::iter::once(&self.main.tree))
    };
    let next_redraw = trees().filter_map(Tree::next_scheduled_redraw).min();
    // A file drag hovering over a window reads the pointer at its poll
    // interval, like a timer.
    let next_file_drag_poll = self
      .secondaries
      .iter()
      .filter_map(ManagedSecondaryWindow::next_file_drag_poll)
      .chain(self.main.next_file_drag_poll())
      .min();
    let next_timer = trees()
      .filter_map(Tree::next_timer_at)
      .chain(next_file_drag_poll)
      .min();

    let wait = LoopWait::of(
      LoopNeeds {
        continuous_video: has_continuous_video,
        redraw_pending,
        polling: self.main.needs_polling() || secondary_needs_polling,
        next_redraw,
        next_timer,
      },
      now,
    );
    event_loop.set_control_flow(wait.control_flow());
    let control_flow = wait.name();
    self.loop_cadence.record(
      now,
      has_continuous_video,
      redraw_pending,
      direct_presented,
      redraw_requested,
      control_flow,
    );
    let control_flow_duration = stage_started_at.elapsed();
    let total = started_at.elapsed();
    if total >= WINIT_BREAKDOWN_THRESHOLD {
      crate::app::profile_support::video_log!(debug,
        target: "video::watch::lurq",
        "winit about_to_wait breakdown total_ms={:.2} tick_ms={:.2} main_commands_ms={:.2} secondary_requests_ms={:.2} continuous_check_ms={:.2} present_ms={:.2} post_present_ms={:.2} secondary_tick_ms={:.2} sync_secondary_ms={:.2} control_flow_ms={:.2} main_video={} secondary_video={} direct_presented={} redraw_pending={} redraw_requested={} control_flow={}",
        duration_ms(total),
        duration_ms(tick),
        duration_ms(main_commands),
        duration_ms(secondary_requests),
        duration_ms(continuous_check),
        duration_ms(present),
        duration_ms(post_present),
        duration_ms(secondary_tick),
        duration_ms(sync_secondary),
        duration_ms(control_flow_duration),
        main_has_continuous_video,
        secondary_has_continuous_video,
        direct_presented,
        redraw_pending,
        redraw_requested,
        control_flow
      );
    }
  }
}

fn to_lurq_mouse_button(button: winit::event::MouseButton) -> MouseButton {
  match button {
    winit::event::MouseButton::Left => MouseButton::Left,
    winit::event::MouseButton::Right => MouseButton::Right,
    winit::event::MouseButton::Middle => MouseButton::Middle,
    winit::event::MouseButton::Back => MouseButton::Other(4),
    winit::event::MouseButton::Forward => MouseButton::Other(5),
    winit::event::MouseButton::Other(id) => MouseButton::Other(id.min(u16::from(u8::MAX)) as u8),
  }
}

fn key_to_string(event: &winit::event::KeyEvent) -> String {
  match &event.logical_key {
    Key::Character(text) => event
      .text
      .as_ref()
      .map(ToString::to_string)
      .unwrap_or_else(|| text.to_string()),
    Key::Named(named) => named_key_to_string(*named).to_owned(),
    Key::Dead(Some(ch)) => ch.to_string(),
    Key::Dead(None) => String::new(),
    Key::Unidentified(_) => event.text.as_ref().map(ToString::to_string).unwrap_or_default(),
  }
}

fn is_open_devtools_shortcut(event: &winit::event::KeyEvent, modifiers: ModifiersState) -> bool {
  event.state == ElementState::Pressed
    && modifiers.control_key()
    && modifiers.shift_key()
    && !modifiers.alt_key()
    && !modifiers.super_key()
    && matches!(event.physical_key, PhysicalKey::Code(KeyCode::F12))
}

fn is_open_devtools_window_event(event: &WindowEvent, modifiers: ModifiersState) -> bool {
  matches!(event, WindowEvent::KeyboardInput { event, .. } if is_open_devtools_shortcut(event, modifiers))
}

fn named_key_to_string(key: NamedKey) -> &'static str {
  match key {
    NamedKey::Backspace => "Backspace",
    NamedKey::Delete => "Delete",
    NamedKey::ArrowLeft => "ArrowLeft",
    NamedKey::ArrowRight => "ArrowRight",
    NamedKey::ArrowUp => "ArrowUp",
    NamedKey::ArrowDown => "ArrowDown",
    NamedKey::Home => "Home",
    NamedKey::End => "End",
    NamedKey::Insert => "Insert",
    NamedKey::Enter => "Enter",
    NamedKey::Space => " ",
    // A key the input method takes (Windows `VK_PROCESSKEY`); see
    // `KeyboardEvent::composing`. Its code is the physical key, such as
    // `Enter` for the one that confirms a composition.
    NamedKey::Process => "Process",
    // An unmapped named key used to deliver `key == ""`, which silently
    // broke every app-side `event.key == "Escape"`-style match.
    NamedKey::Escape => "Escape",
    NamedKey::Tab => "Tab",
    NamedKey::PageUp => "PageUp",
    NamedKey::PageDown => "PageDown",
    NamedKey::Control => "Control",
    NamedKey::Shift => "Shift",
    NamedKey::Alt => "Alt",
    NamedKey::Super => "Super",
    NamedKey::CapsLock => "CapsLock",
    NamedKey::ContextMenu => "ContextMenu",
    NamedKey::F1 => "F1",
    NamedKey::F2 => "F2",
    NamedKey::F3 => "F3",
    NamedKey::F4 => "F4",
    NamedKey::F5 => "F5",
    NamedKey::F6 => "F6",
    NamedKey::F7 => "F7",
    NamedKey::F8 => "F8",
    NamedKey::F9 => "F9",
    NamedKey::F10 => "F10",
    NamedKey::F11 => "F11",
    NamedKey::F12 => "F12",
    _ => "",
  }
}

#[cfg(test)]
mod key_name_tests {
  use super::*;

  #[test]
  fn named_keys_apps_match_on_are_not_empty() {
    // Escape regression: an empty string here breaks every app-side
    // `event.key == "Escape"` shortcut without any visible error.
    for (key, expected) in [
      (NamedKey::Escape, "Escape"),
      (NamedKey::Tab, "Tab"),
      (NamedKey::Enter, "Enter"),
      (NamedKey::PageUp, "PageUp"),
      (NamedKey::PageDown, "PageDown"),
      (NamedKey::F5, "F5"),
      (NamedKey::Control, "Control"),
      (NamedKey::Process, "Process"),
    ] {
      assert_eq!(named_key_to_string(key), expected);
    }
  }
}

fn physical_key_to_string(key: &PhysicalKey) -> String {
  match key {
    PhysicalKey::Code(code) => format!("{code:?}"),
    PhysicalKey::Unidentified(_) => String::new(),
  }
}

/// Shows `cursor` on `window` unless it is the `applied` one already.
fn set_window_cursor(window: &Window, applied: &mut CursorIcon, cursor: CursorIcon) {
  if cursor == *applied {
    return;
  }
  window.set_cursor(to_winit_cursor(cursor));
  *applied = cursor;
}

fn to_winit_cursor(cursor: CursorIcon) -> WinitCursorIcon {
  match cursor {
    CursorIcon::Default => WinitCursorIcon::Default,
    CursorIcon::ContextMenu => WinitCursorIcon::ContextMenu,
    CursorIcon::Help => WinitCursorIcon::Help,
    CursorIcon::Pointer => WinitCursorIcon::Pointer,
    CursorIcon::Progress => WinitCursorIcon::Progress,
    CursorIcon::Wait => WinitCursorIcon::Wait,
    CursorIcon::Cell => WinitCursorIcon::Cell,
    CursorIcon::Crosshair => WinitCursorIcon::Crosshair,
    CursorIcon::Text => WinitCursorIcon::Text,
    CursorIcon::VerticalText => WinitCursorIcon::VerticalText,
    CursorIcon::Alias => WinitCursorIcon::Alias,
    CursorIcon::Copy => WinitCursorIcon::Copy,
    CursorIcon::Move => WinitCursorIcon::Move,
    CursorIcon::NoDrop => WinitCursorIcon::NoDrop,
    CursorIcon::NotAllowed => WinitCursorIcon::NotAllowed,
    CursorIcon::Grab => WinitCursorIcon::Grab,
    CursorIcon::Grabbing => WinitCursorIcon::Grabbing,
    CursorIcon::EResize => WinitCursorIcon::EResize,
    CursorIcon::NResize => WinitCursorIcon::NResize,
    CursorIcon::NeResize => WinitCursorIcon::NeResize,
    CursorIcon::NwResize => WinitCursorIcon::NwResize,
    CursorIcon::SResize => WinitCursorIcon::SResize,
    CursorIcon::SeResize => WinitCursorIcon::SeResize,
    CursorIcon::SwResize => WinitCursorIcon::SwResize,
    CursorIcon::WResize => WinitCursorIcon::WResize,
    CursorIcon::EwResize => WinitCursorIcon::EwResize,
    CursorIcon::NsResize => WinitCursorIcon::NsResize,
    CursorIcon::NeswResize => WinitCursorIcon::NeswResize,
    CursorIcon::NwseResize => WinitCursorIcon::NwseResize,
    CursorIcon::ColResize => WinitCursorIcon::ColResize,
    CursorIcon::RowResize => WinitCursorIcon::RowResize,
    CursorIcon::AllScroll => WinitCursorIcon::AllScroll,
    CursorIcon::ZoomIn => WinitCursorIcon::ZoomIn,
    CursorIcon::ZoomOut => WinitCursorIcon::ZoomOut,
    CursorIcon::DndAsk => WinitCursorIcon::DndAsk,
    CursorIcon::AllResize => WinitCursorIcon::AllResize,
  }
}

fn to_winit_icon(icon: WindowIcon) -> Option<WinitIcon> {
  let (rgba, width, height) = icon.into_rgba();
  WinitIcon::from_rgba(rgba, width, height).ok()
}

/// winit's window icon is only Windows' small icon (`ICON_SMALL`, the title
/// bar); the taskbar button and Alt+Tab show the big one (`ICON_BIG`), which
/// would otherwise stay the executable's icon. Both get the same image.
#[cfg(windows)]
fn with_window_icons(attrs: WindowAttributes, icon: Option<WinitIcon>) -> WindowAttributes {
  attrs.with_window_icon(icon.clone()).with_taskbar_icon(icon)
}

/// macOS ignores window icons (the bundle's icon stands for the app); X11
/// takes this one for the title bar and the task switcher.
#[cfg(not(windows))]
fn with_window_icons(attrs: WindowAttributes, icon: Option<WinitIcon>) -> WindowAttributes {
  attrs.with_window_icon(icon)
}

/// See [`with_window_icons`].
#[cfg(windows)]
fn set_window_icons(window: &Window, icon: Option<WinitIcon>) {
  window.set_taskbar_icon(icon.clone());
  window.set_window_icon(icon);
}

/// See [`with_window_icons`].
#[cfg(not(windows))]
fn set_window_icons(window: &Window, icon: Option<WinitIcon>) {
  window.set_window_icon(icon);
}

#[cfg(windows)]
fn with_title_bar_color(attrs: WindowAttributes, color: Option<Color>) -> WindowAttributes {
  attrs.with_title_background_color(color.map(to_winit_windows_color))
}

#[cfg(not(windows))]
fn with_title_bar_color(attrs: WindowAttributes, color: Option<Color>) -> WindowAttributes {
  let _ = color;
  attrs
}

#[cfg(windows)]
fn set_title_bar_color(window: &Window, color: Option<Color>) {
  window.set_title_background_color(color.map(to_winit_windows_color));
}

#[cfg(not(windows))]
fn set_title_bar_color(window: &Window, color: Option<Color>) {
  let _ = (window, color);
}

#[cfg(windows)]
fn set_border_color(window: &Window, color: WindowBorderColor) {
  window.set_border_color(match color {
    WindowBorderColor::Default => Some(WinitWindowsColor::SYSTEM_DEFAULT),
    WindowBorderColor::None => None,
    WindowBorderColor::Color(color) => Some(to_winit_windows_color(color)),
  });
}

#[cfg(not(windows))]
fn set_border_color(window: &Window, color: WindowBorderColor) {
  let _ = (window, color);
}

#[cfg(windows)]
fn to_winit_windows_color(color: Color) -> WinitWindowsColor {
  WinitWindowsColor::from_rgb(color.r(), color.g(), color.b())
}

#[cfg(windows)]
fn with_corner_radius(attrs: WindowAttributes, radius: WindowCornerRadius) -> WindowAttributes {
  attrs.with_corner_preference(to_winit_corner_preference(radius))
}

#[cfg(not(windows))]
fn with_corner_radius(attrs: WindowAttributes, radius: WindowCornerRadius) -> WindowAttributes {
  let _ = radius;
  attrs
}

#[cfg(windows)]
fn set_corner_radius(window: &Window, radius: WindowCornerRadius) {
  window.set_corner_preference(to_winit_corner_preference(radius));
}

#[cfg(target_os = "macos")]
fn set_corner_radius(window: &Window, radius: WindowCornerRadius) {
  let Ok(handle) = window.window_handle() else {
    return;
  };

  let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
    return;
  };

  let radius = to_macos_corner_radius(radius);
  unsafe {
    use objc2::{msg_send, runtime::AnyObject};

    let view = handle.ns_view.as_ptr().cast::<AnyObject>();
    let wants_layer: bool = msg_send![view, wantsLayer];
    if !wants_layer {
      if radius.is_none() {
        return;
      }
      let _: () = msg_send![view, setWantsLayer: true];
    }

    let layer: *mut AnyObject = msg_send![view, layer];
    if layer.is_null() {
      return;
    }

    let _: () = msg_send![layer, setCornerRadius: radius.unwrap_or(0.0)];
    let _: () = msg_send![layer, setMasksToBounds: radius.is_some()];
  }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn set_corner_radius(window: &Window, radius: WindowCornerRadius) {
  let _ = (window, radius);
}

#[cfg(windows)]
fn to_winit_corner_preference(radius: WindowCornerRadius) -> WinitCornerPreference {
  match radius {
    WindowCornerRadius::Default => WinitCornerPreference::Default,
    WindowCornerRadius::None => WinitCornerPreference::DoNotRound,
    WindowCornerRadius::Rounded => WinitCornerPreference::Round,
    WindowCornerRadius::RoundedSmall => WinitCornerPreference::RoundSmall,
  }
}

#[cfg(target_os = "macos")]
fn to_macos_corner_radius(radius: WindowCornerRadius) -> Option<f64> {
  match radius {
    WindowCornerRadius::Default => None,
    WindowCornerRadius::None => Some(0.0),
    WindowCornerRadius::Rounded => Some(10.0),
    WindowCornerRadius::RoundedSmall => Some(4.0),
  }
}

fn to_winit_resize_direction(direction: WindowResizeDirection) -> WinitResizeDirection {
  match direction {
    WindowResizeDirection::East => WinitResizeDirection::East,
    WindowResizeDirection::North => WinitResizeDirection::North,
    WindowResizeDirection::NorthEast => WinitResizeDirection::NorthEast,
    WindowResizeDirection::NorthWest => WinitResizeDirection::NorthWest,
    WindowResizeDirection::South => WinitResizeDirection::South,
    WindowResizeDirection::SouthEast => WinitResizeDirection::SouthEast,
    WindowResizeDirection::SouthWest => WinitResizeDirection::SouthWest,
    WindowResizeDirection::West => WinitResizeDirection::West,
  }
}

/// Starts the native window move loop. True once the OS owns the left-button press: the loop consumes
/// its release, so lurq never receives it (the caller ends the press with
/// [`Tree::mouse_press_taken_by_os`]). This holds for the Windows loop, `performWindowDragWithEvent` on
/// macOS, and the X11 and Wayland compositor moves behind `drag_window`.
fn start_native_window_drag(window: &Window) -> bool {
  begin_native_window_drag(window) || window.drag_window().is_ok()
}

/// Starts the native window resize loop; see [`start_native_window_drag`]. macOS has none, so a resize
/// handle there keeps its press and gets the real release.
fn start_native_window_resize(window: &Window, direction: WindowResizeDirection) -> bool {
  begin_native_window_resize(window, direction)
    || window.drag_resize_window(to_winit_resize_direction(direction)).is_ok()
}

#[cfg(windows)]
fn begin_native_window_drag(window: &Window) -> bool {
  use windows::Win32::UI::WindowsAndMessaging::HTCAPTION;

  send_native_non_client_mouse_down(window, HTCAPTION)
}

#[cfg(not(windows))]
fn begin_native_window_drag(window: &Window) -> bool {
  let _ = window;
  false
}

#[cfg(windows)]
fn begin_native_window_resize(window: &Window, direction: WindowResizeDirection) -> bool {
  use windows::Win32::UI::WindowsAndMessaging::{
    HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT,
  };

  let hit_test = match direction {
    WindowResizeDirection::North => HTTOP,
    WindowResizeDirection::South => HTBOTTOM,
    WindowResizeDirection::West => HTLEFT,
    WindowResizeDirection::East => HTRIGHT,
    WindowResizeDirection::NorthWest => HTTOPLEFT,
    WindowResizeDirection::NorthEast => HTTOPRIGHT,
    WindowResizeDirection::SouthWest => HTBOTTOMLEFT,
    WindowResizeDirection::SouthEast => HTBOTTOMRIGHT,
  };
  send_native_non_client_mouse_down(window, hit_test)
}

#[cfg(not(windows))]
fn begin_native_window_resize(window: &Window, direction: WindowResizeDirection) -> bool {
  let _ = (window, direction);
  false
}

/// Starts the native move/size loop the way a press on a real frame would.
///
/// The message is posted, not sent: `SendMessageW` ran the whole modal loop inside the lurq event
/// handler that issued the command, where winit buffers every event. No frame was drawn for the
/// entire drag (DWM stretched the last one) and the buffered size events were replayed afterwards.
/// Posted, the loop runs from winit's own message pump, which delivers `Resized` and paints during
/// the drag. The start point is the cursor position in screen coordinates, which the loop uses as
/// the drag origin (for example to place a maximized window restored by dragging its caption).
#[cfg(windows)]
fn send_native_non_client_mouse_down(window: &Window, hit_test: u32) -> bool {
  use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, WPARAM},
    UI::{
      Input::KeyboardAndMouse::ReleaseCapture,
      WindowsAndMessaging::{GetCursorPos, PostMessageW, WM_NCLBUTTONDOWN},
    },
  };

  let Ok(handle) = window.window_handle() else {
    return false;
  };
  let RawWindowHandle::Win32(handle) = handle.as_raw() else {
    return false;
  };
  let hwnd = HWND(handle.hwnd.get() as *mut _);
  if hwnd.is_invalid() {
    return false;
  }

  let mut cursor = POINT::default();
  unsafe {
    if GetCursorPos(&mut cursor).is_err() {
      return false;
    }
    let _ = ReleaseCapture();
    PostMessageW(
      Some(hwnd),
      WM_NCLBUTTONDOWN,
      WPARAM(hit_test as usize),
      LPARAM(screen_point_lparam(cursor.x, cursor.y)),
    )
    .is_ok()
  }
}

/// `MAKELPARAM` of a screen point, as `WM_NC*` mouse messages carry it: signed 16-bit x and y.
#[cfg(windows)]
fn screen_point_lparam(x: i32, y: i32) -> isize {
  let x = x as i16 as u16 as u32;
  let y = y as i16 as u16 as u32;
  ((y << 16) | x) as i32 as isize
}

#[cfg(all(test, windows))]
mod native_drag_tests {
  use super::screen_point_lparam;

  #[test]
  fn screen_point_lparam_packs_signed_coordinates() {
    assert_eq!(screen_point_lparam(0, 0), 0);
    assert_eq!(screen_point_lparam(1440, 900), (900 << 16) | 1440);
    // Monitors left of or above the primary have negative screen coordinates.
    let packed = screen_point_lparam(-100, -20) as u32;
    assert_eq!((packed & 0xffff) as u16 as i16, -100);
    assert_eq!((packed >> 16) as u16 as i16, -20);
  }
}

#[cfg(test)]
mod close_runtime_tests {
  use super::*;
  #[test]
  fn secondary_veto_delayed_acceptance_and_unconditional_close() {
    let mut app = App::new();
    let mut root = Tree::new();
    app.window_opener().open("Preferences", 200, 200, |_, _| {});
    root.apply_secondary_window_requests(&mut app);
    let secondary = root.secondary_window_mut(0).unwrap();
    let mut managed = ManagedSecondaryWindow::new(0, secondary);
    let tree = secondary.tree_mut();
    tree.window().handle().on_close_requested(|r| r.cancel());
    tree.window().dispatch_close_request(CloseRequestSource::Os);
    assert!(!managed.apply_window_commands(tree));
    assert!(!managed.close_requested());
    let pending = std::sync::Arc::new(std::sync::Mutex::new(None));
    let save = pending.clone();
    tree
      .window()
      .handle()
      .on_close_requested(move |r| *save.lock().unwrap() = Some(r));
    tree.window().handle().request_close();
    assert!(!managed.apply_window_commands(tree));
    pending.lock().unwrap().take().unwrap().proceed();
    assert!(managed.apply_window_commands(tree));
    assert!(managed.close_requested());
    tree
      .window()
      .handle()
      .on_close_requested(|_| panic!("unconditional close called handler"));
    tree.window().handle().close();
    assert!(managed.apply_window_commands(tree));
  }
}

#[cfg(all(test, windows))]
mod native_window_tests {
  use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{ICON_BIG, ICON_SMALL, SendMessageW, WM_GETICON},
  };
  use winit::platform::windows::EventLoopBuilderExtWindows;

  use super::*;

  /// The window's `ICON_SMALL` or `ICON_BIG` handle, 0 when unset.
  fn icon_handle(window: &Window, kind: u32) -> isize {
    let RawWindowHandle::Win32(handle) = window.window_handle().expect("window handle").as_raw() else {
      panic!("not a Win32 window");
    };
    let hwnd = HWND(handle.hwnd.get() as *mut std::ffi::c_void);
    // SAFETY: `hwnd` is the test's own live window; WM_GETICON only reads it.
    unsafe { SendMessageW(hwnd, WM_GETICON, Some(WPARAM(kind as usize)), Some(LPARAM(0))).0 }
  }

  /// One test: winit allows one event loop per process.
  #[test]
  fn a_hidden_window_takes_icons_and_runtime_titles() {
    // The test's only event loop: winit allows one per process.
    let event_loop = EventLoop::builder().with_any_thread(true).build().expect("event loop");
    let icon = || to_winit_icon(WindowIcon::from_rgba(vec![200; 32 * 32 * 4], 32, 32));
    let attributes = with_window_icons(WindowAttributes::default().with_visible(false), icon());
    // A hidden window of the test's own, outside `run_app`.
    #[allow(deprecated)]
    let window = event_loop.create_window(attributes).expect("hidden test window");
    assert_ne!(icon_handle(&window, ICON_SMALL), 0, "title bar icon");
    assert_ne!(icon_handle(&window, ICON_BIG), 0, "taskbar and Alt+Tab icon");

    set_window_icons(&window, None);
    assert_eq!(icon_handle(&window, ICON_SMALL), 0);
    assert_eq!(icon_handle(&window, ICON_BIG), 0);

    set_window_icons(&window, icon());
    assert_ne!(icon_handle(&window, ICON_SMALL), 0);
    assert_ne!(icon_handle(&window, ICON_BIG), 0);

    runtime_title_reaches_the_os_window(window);
  }

  /// `WindowHandle::set_title` is applied by the shell to the live window.
  fn runtime_title_reaches_the_os_window(window: Window) {
    let mut app = App::new();
    let mut root = Tree::new();
    app.window_opener().open("Preferences", 200, 200, |_, _| {});
    root.apply_secondary_window_requests(&mut app);
    let secondary = root.secondary_window_mut(0).expect("opened secondary window");
    let mut managed = ManagedSecondaryWindow::new(0, secondary);
    let tree = secondary.tree_mut();
    assert_eq!(tree.window().handle().title().as_deref(), Some("Preferences"));
    managed.window = Some(window);

    tree.window().handle().set_title("Tasks - Orchester");
    managed.apply_window_commands(tree);
    let os_title = |managed: &ManagedSecondaryWindow| managed.window.as_ref().expect("test window").title();
    assert_eq!(os_title(&managed), "Tasks - Orchester");
    assert_eq!(tree.window().handle().title().as_deref(), Some("Tasks - Orchester"));

    tree.window().handle().set_title("Run 12 - Orchester");
    managed.apply_window_commands(tree);
    assert_eq!(os_title(&managed), "Run 12 - Orchester");
  }
}
