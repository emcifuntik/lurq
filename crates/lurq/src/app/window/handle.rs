use super::*;

#[derive(Clone)]
pub struct WindowHandle {
  pub(super) info: WindowInfo,
  pub(super) window: Window,
}

impl WindowHandle {
  #[cfg(feature = "mcp")]
  pub(crate) fn dialog_window(&self) -> DialogWindow {
    DialogWindow(Arc::downgrade(&self.window.inner))
  }
  pub fn info(&self) -> WindowInfo {
    self.info
  }

  /// Replace this window's OS/app close handler. Runs on the event-loop thread.
  pub fn on_close_requested(&self, handler: impl Fn(CloseRequest) + Send + Sync + 'static) {
    self.window.inner.write().unwrap().close_handler = Some(Arc::new(handler));
  }

  /// Replace this window's handler of what the system says of the app: that it came to the front or left it,
  /// that it was opened with a link or a shortcut. Runs on the event-loop thread. What the app was started
  /// with is told once a handler is there, so register it in the root component's `create`.
  pub fn on_app_event(&self, handler: impl Fn(&crate::app::window::AppEvent) + Send + Sync + 'static) {
    self.window.set_app_event_handler(Some(Arc::new(handler)));
  }

  pub fn clear_close_requested_handler(&self) {
    self.window.inner.write().unwrap().close_handler = None;
  }

  /// Queue a vetoable request; unlike `close`, this invokes the handler.
  /// Plays a tap of the device's haptic engine, where there is one.
  pub fn haptic(&self, haptic: crate::app::window::Haptic) {
    self.window.push_command(WindowCommand::Haptic(haptic));
  }

  pub fn request_close(&self) {
    self
      .window
      .push_command(WindowCommand::RequestClose(CloseRequestSource::App));
  }

  /// Close unconditionally, without invoking the close handler.
  pub fn close(&self) {
    self.window.push_command(WindowCommand::Close);
  }

  pub fn set_minimized(&self, minimized: bool) {
    self.window.push_command(WindowCommand::SetMinimized(minimized));
  }

  pub fn set_maximized(&self, maximized: bool) {
    self.window.push_command(WindowCommand::SetMaximized(maximized));
  }

  pub fn set_full_screen(&self, full_screen: bool) {
    self.window.push_command(WindowCommand::SetFullScreen(full_screen));
  }

  pub fn set_decorated(&self, decorated: bool) {
    self.window.push_command(WindowCommand::SetDecorated(decorated));
  }

  pub fn set_decorations(&self, decorations: bool) {
    self.set_decorated(decorations);
  }

  /// The window's title as last requested: by `set_title`, or the title the
  /// window was created with (`WinitWindow::with_title`, `open_window`) once
  /// the shell has created it. `None` before then, and in headless trees that
  /// never set one. Reading it does not subscribe the component.
  pub fn title(&self) -> Option<String> {
    self.window.inner.read().unwrap().title.clone()
  }

  /// Sets the OS window title: the title bar text, and the taskbar button and
  /// Alt+Tab on Windows, or the Window menu and Mission Control on macOS. Like
  /// the other commands it is applied by the shell on the event-loop thread.
  /// Setting the title the window already has queues nothing, so calling this
  /// from `render` with an unchanged title does no work.
  pub fn set_title(&self, title: impl Into<String>) {
    let title = title.into();
    {
      let mut inner = self.window.inner.write().unwrap();
      if inner.title.as_deref() == Some(title.as_str()) {
        return;
      }
      inner.title = Some(title.clone());
    }
    self.window.push_command(WindowCommand::SetTitle(title));
  }

  pub fn set_title_bar_color(&self, color: impl Into<Option<Color>>) {
    self.window.push_command(WindowCommand::SetTitleBarColor(color.into()));
  }

  pub fn clear_title_bar_color(&self) {
    self.set_title_bar_color(None);
  }

  /// The compositor border last applied by the shell.
  pub fn border_color(&self) -> WindowBorderColor {
    self.window.inner.read().unwrap().border_color
  }

  /// Sets the compositor border drawn around the window. Supported on Windows 11 (build 22000+).
  pub fn set_border_color(&self, color: WindowBorderColor) {
    self.window.push_command(WindowCommand::SetBorderColor(color));
  }

  /// Sets the window's icon, or clears it with `None`. On Windows it is both
  /// the small icon (title bar) and the big one (taskbar button, Alt+Tab). On
  /// macOS this does nothing: the Dock and the app switcher show the
  /// application bundle's icon.
  pub fn set_icon(&self, icon: impl Into<Option<WindowIcon>>) {
    self.window.push_command(WindowCommand::SetIcon(icon.into()));
  }

  pub fn clear_icon(&self) {
    self.set_icon(None);
  }

  pub fn set_corner_radius(&self, radius: WindowCornerRadius) {
    self.window.push_command(WindowCommand::SetCornerRadius(radius));
  }

  pub fn set_rounded_corners(&self, rounded: bool) {
    self.set_corner_radius(if rounded {
      WindowCornerRadius::Rounded
    } else {
      WindowCornerRadius::None
    });
  }

  pub fn reset_corner_radius(&self) {
    self.set_corner_radius(WindowCornerRadius::Default);
  }

  pub fn r#move(&self, x: i32, y: i32) {
    self.window.push_command(WindowCommand::Move { x, y });
  }

  pub fn move_to(&self, x: i32, y: i32) {
    self.r#move(x, y);
  }

  /// Request a client size in physical pixels. A minimized window is restored first, since Windows does not resize a
  /// minimized window. A maximized or full-screen window stays in its mode: the resize does not take the user out of
  /// it. The size takes effect asynchronously; read it back from `ctx.window()`.
  pub fn resize(&self, width: u32, height: u32) {
    self
      .window
      .push_command(WindowCommand::Resize(super::resize::ResizeRequest {
        width,
        height,
        restore: super::resize::ResizeRestore::Minimized,
        report: None,
      }));
  }

  pub fn start_drag(&self) {
    self.window.push_command(WindowCommand::StartDrag);
  }

  pub fn start_resize(&self, direction: WindowResizeDirection) {
    self.window.push_command(WindowCommand::StartResize(direction));
  }

  pub fn stop_drag(&self) {
    self.window.push_command(WindowCommand::StopDrag);
  }

  /// Queue one synthetic input event, delivered inside the shell's event loop
  /// exactly where the matching OS event would have been.
  ///
  /// Positions are physical pixels in the window's coordinate space — the same
  /// units a captured frame is measured in.
  pub fn inject_input(&self, input: crate::app::synthetic_input::SyntheticInput) {
    self.window.push_command(WindowCommand::InjectInput(input));
  }

  /// Queue several synthetic events in order; see [`Self::inject_input`].
  pub fn inject_inputs(&self, inputs: impl IntoIterator<Item = crate::app::synthetic_input::SyntheticInput>) {
    for input in inputs {
      self.inject_input(input);
    }
  }

  /// Pastes the OS clipboard into the focused text input, as Ctrl+V does:
  /// the input's `on_paste` handlers see the content first. For an app's own
  /// Paste menu item or context menu. The clipboard is read inside the
  /// shell's event loop, as OS input. A request made while lurq delivers
  /// synthetic input (an MCP client's click on the menu item,
  /// [`is_synthetic_input`](crate::app::events::is_synthetic_input)) is
  /// refused: the person's clipboard is theirs, and a synthetic paste
  /// carries its own content ([`SyntheticInput::paste`](crate::app::synthetic_input::SyntheticInput::paste)).
  #[cfg(feature = "clipboard")]
  pub fn paste_from_clipboard(&self) {
    self.window.push_command(WindowCommand::PasteFromClipboard {
      synthetic: crate::app::events::is_synthetic_input(),
    });
  }

  /// Captures the next rendered window frame to a PNG file.
  #[cfg(feature = "screenshot")]
  pub fn screenshot(&self, output_path: impl Into<std::path::PathBuf>) {
    self
      .window
      .push_command(WindowCommand::Screenshot(output_path.into(), None));
  }

  /// Captures the next rendered frame cropped to a logical-pixel window
  /// region. The region is converted to physical pixels and clamped to the
  /// viewport at capture time; a region with no visible area is skipped with
  /// a warning.
  #[cfg(feature = "screenshot")]
  pub fn screenshot_region(&self, output_path: impl Into<std::path::PathBuf>, region: ScreenshotRegion) {
    self
      .window
      .push_command(WindowCommand::Screenshot(output_path.into(), Some(region)));
  }

  /// Captures the next rendered frame cropped to one element's bounds, as
  /// attached with `ref_element`. The bounds are read now, from the last
  /// completed layout pass — capture after the element has painted.
  #[cfg(feature = "screenshot")]
  pub fn screenshot_node(&self, output_path: impl Into<std::path::PathBuf>, node: &crate::core::ElementRef) {
    let bounds = node.bounds();
    self.screenshot_region(
      output_path,
      ScreenshotRegion {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
      },
    );
  }

  pub fn open_devtools(&self) {
    self.window.push_command(WindowCommand::OpenDevtools);
  }

  pub fn close_devtools(&self) {
    self.window.push_command(WindowCommand::CloseDevtools);
  }

  pub fn toggle_devtools(&self) {
    self.window.push_command(WindowCommand::ToggleDevtools);
  }
}

/// A weak, immutable window identity; a same-name replacement never matches.
#[cfg(feature = "mcp")]
#[derive(Clone)]
pub(crate) struct DialogWindow(Weak<RwLock<WindowInner>>);

#[cfg(feature = "mcp")]
impl DialogWindow {
  pub(crate) fn matches(&self, window: &Window) -> bool {
    self.0.ptr_eq(&Arc::downgrade(&window.inner))
  }

  pub(crate) fn closing(&self) -> bool {
    self
      .0
      .upgrade()
      .is_none_or(|window| window.read().unwrap().accepted_close)
  }
}

impl Deref for WindowHandle {
  type Target = WindowInfo;

  fn deref(&self) -> &Self::Target {
    &self.info
  }
}
