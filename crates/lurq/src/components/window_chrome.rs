use std::{
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

use super::WindowControls;
use crate::{
  app::{
    ctx::{Ctx, Modal, ModalTarget},
    events::{MouseButton, MouseEvent},
    window::{WindowBorderColor, WindowHandle, WindowInfo, WindowResizeDirection},
  },
  components::{Column, Row, Stack},
  layout::Alignment,
  node::{BackgroundColor, CursorIcon, Element, HitTestBehavior, border::Border, color::Color, dimension::Dimension},
};

const WINDOWS_CHROME_HEIGHT: f32 = 36.0;
const MACOS_CHROME_HEIGHT: f32 = 28.0;
const RESIZE_HANDLE_SIZE: f32 = 3.0;
const TITLEBAR_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const TITLEBAR_DOUBLE_CLICK_DISTANCE: f32 = 4.0;

#[derive(Clone)]
pub struct WindowChrome {
  props: WindowChromeProps,
  title_bar: ChromeTitleBar,
  frame_background: Option<BackgroundColor>,
  content: Element,
  overlays: Vec<Element>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowChromeProps {
  pub mode: WindowChromeMode,
  pub resize_handles: ResizeHandlePolicy,
  pub resize_placement: ResizeHandlePlacement,
  pub border: ChromeBorderPolicy,
  pub windows_height: f32,
  pub macos_height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowChromeMode {
  PlatformDefault,
  CustomDesktop,
  AlwaysCustom,
  Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResizeHandlePolicy {
  PlatformDefault,
  Enabled { size: f32 },
  Disabled,
}

/// Where resize hit zones sit relative to the application content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResizeHandlePlacement {
  /// Invisible hit zones overlay the outermost pixels of the content; content fills the window.
  #[default]
  Overlay,
  /// Content is inset by the handle size on the left, right and bottom, and the gutter is painted
  /// with the frame background. This was the behavior before `ResizeHandlePlacement` existed.
  Inset,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChromeBorderPolicy {
  PlatformDefault,
  Visible { size: f32, color: BackgroundColor },
  Hidden,
}

#[derive(Clone)]
pub struct ChromeTitleBar {
  leading: Option<Element>,
  title: Option<Element>,
  center: Option<Element>,
  trailing: Option<Element>,
  controls: Option<WindowControls>,
  height: Option<f32>,
  background: BackgroundColor,
  border_bottom: Option<Border>,
}

#[derive(Clone, Copy)]
struct TitlebarClick {
  time: Instant,
  x: f32,
  y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowChromeMetrics {
  pub enabled: bool,
  pub height: f32,
  pub resize_handle_size: f32,
  pub resize_placement: ResizeHandlePlacement,
  pub border_size: f32,
}

impl Default for WindowChromeProps {
  fn default() -> Self {
    Self {
      mode: WindowChromeMode::PlatformDefault,
      resize_handles: ResizeHandlePolicy::PlatformDefault,
      resize_placement: ResizeHandlePlacement::Overlay,
      border: ChromeBorderPolicy::PlatformDefault,
      windows_height: WINDOWS_CHROME_HEIGHT,
      macos_height: MACOS_CHROME_HEIGHT,
    }
  }
}

impl Default for ChromeTitleBar {
  fn default() -> Self {
    Self::new()
  }
}

impl WindowChrome {
  pub fn new() -> Self {
    Self {
      props: WindowChromeProps::default(),
      title_bar: ChromeTitleBar::new(),
      frame_background: None,
      content: Element::new(),
      overlays: Vec::new(),
    }
  }

  pub fn props(mut self, props: WindowChromeProps) -> Self {
    self.props = props;
    self
  }

  pub fn mode(mut self, mode: WindowChromeMode) -> Self {
    self.props.mode = mode;
    self
  }

  pub fn resize_handles(mut self, resize_handles: ResizeHandlePolicy) -> Self {
    self.props.resize_handles = resize_handles;
    self
  }

  pub fn resize_placement(mut self, placement: ResizeHandlePlacement) -> Self {
    self.props.resize_placement = placement;
    self
  }

  pub fn border(mut self, border: ChromeBorderPolicy) -> Self {
    self.props.border = border;
    self
  }

  /// Overrides the color painted behind the client-area inset used by
  /// [`ResizeHandlePlacement::Inset`] resize handles.
  /// By default, chrome uses the content root background and falls back to the
  /// title-bar background when the content root is transparent.
  pub fn frame_background(mut self, background: impl Into<BackgroundColor>) -> Self {
    self.frame_background = Some(background.into());
    self
  }

  pub fn title_bar(mut self, title_bar: ChromeTitleBar) -> Self {
    self.title_bar = title_bar;
    self
  }

  pub fn content(mut self, content: impl Into<Element>) -> Self {
    self.content = content.into();
    self
  }

  pub fn overlay(mut self, overlay: impl Into<Element>) -> Self {
    self.overlays.push(overlay.into());
    self
  }

  pub fn overlays(mut self, overlays: impl IntoIterator<Item = impl Into<Element>>) -> Self {
    self.overlays.extend(overlays.into_iter().map(Into::into));
    self
  }

  pub fn metrics(&self) -> WindowChromeMetrics {
    self.props.metrics()
  }

  pub fn mount(self, ctx: &mut Ctx) -> Element {
    let metrics = self.metrics();
    if !metrics.enabled {
      return self.content;
    }

    let window = ctx.window();
    if window.is_decorated {
      window.set_decorations(false);
    }
    // `ChromeBorderPolicy` owns the frame outline. Windows 11 DWM still draws its own 1px border around
    // undecorated windows, which would show even with `ChromeBorderPolicy::Hidden`.
    if window.border_color() != WindowBorderColor::None {
      window.set_border_color(WindowBorderColor::None);
    }

    // Resize hit zones always sit above content and overlays. `Inset` placement additionally keeps
    // app-owned hit targets inside the resize perimeter; `Overlay` lets content fill the window.
    let resize_inset = metrics.resize_inset(window.info());
    let resize_handle_size = metrics.resize_handle_size_for(window.info());
    let frame_background = self
      .frame_background
      .clone()
      .or_else(|| self.content.node.background_color())
      .unwrap_or_else(|| self.title_bar.background.clone());
    let title_bar = self.title_bar.render(&window, metrics.height);
    let content = Row::new()
      .width(Dimension::Pct(100.0))
      .flex(1.0)
      .clip()
      .child(self.content);

    let mut frame = Stack::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background(frame_background)
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .height(Dimension::Pct(100.0))
          .padding_left(resize_inset)
          .padding_right(resize_inset)
          .padding_bottom(resize_inset)
          .child(chrome_titlebar_spacer(metrics.height))
          .child(content),
      );

    let mut client_overlay = Stack::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .padding_left(resize_inset)
      .padding_right(resize_inset)
      .padding_bottom(resize_inset)
      .hit_test(HitTestBehavior::ContentOnly)
      .child(title_bar);

    for overlay in self.overlays {
      client_overlay = client_overlay.child(overlay);
    }

    let mut chrome_overlay = Stack::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .hit_test(HitTestBehavior::ContentOnly)
      .child(client_overlay);

    for layer in border_layers(&window, &self.props.border) {
      chrome_overlay = chrome_overlay.child(layer);
    }
    for layer in resize_handle_layers(&window, resize_handle_size) {
      chrome_overlay = chrome_overlay.child(layer);
    }

    frame = frame.child(
      Modal::new(chrome_overlay)
        .target(ModalTarget::Parent)
        .dismiss_on_escape(false)
        .window_chrome_layer(),
    );

    frame.into()
  }
}

impl Default for WindowChrome {
  fn default() -> Self {
    Self::new()
  }
}

fn chrome_titlebar_spacer(height: f32) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(height)
    .hit_test(HitTestBehavior::None)
    .into()
}

impl WindowChromeProps {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn mode(mut self, mode: WindowChromeMode) -> Self {
    self.mode = mode;
    self
  }

  pub fn resize_handles(mut self, resize_handles: ResizeHandlePolicy) -> Self {
    self.resize_handles = resize_handles;
    self
  }

  pub fn resize_placement(mut self, placement: ResizeHandlePlacement) -> Self {
    self.resize_placement = placement;
    self
  }

  pub fn border(mut self, border: ChromeBorderPolicy) -> Self {
    self.border = border;
    self
  }

  pub fn windows_height(mut self, height: f32) -> Self {
    self.windows_height = height.max(0.0);
    self
  }

  pub fn macos_height(mut self, height: f32) -> Self {
    self.macos_height = height.max(0.0);
    self
  }

  pub fn metrics(&self) -> WindowChromeMetrics {
    let enabled = match self.mode {
      WindowChromeMode::PlatformDefault | WindowChromeMode::CustomDesktop => platform_custom_chrome_enabled(),
      WindowChromeMode::AlwaysCustom => true,
      WindowChromeMode::Disabled => false,
    };
    if !enabled {
      return WindowChromeMetrics::default();
    }

    let resize_handle_size = match self.resize_handles {
      ResizeHandlePolicy::PlatformDefault => RESIZE_HANDLE_SIZE,
      ResizeHandlePolicy::Enabled { size } => size.max(0.0),
      ResizeHandlePolicy::Disabled => 0.0,
    };

    WindowChromeMetrics {
      enabled,
      height: platform_chrome_height(self.windows_height, self.macos_height),
      resize_handle_size,
      resize_placement: self.resize_placement,
      border_size: self.border.size(),
    }
  }
}

impl ChromeBorderPolicy {
  pub fn size(&self) -> f32 {
    match self {
      Self::PlatformDefault if cfg!(target_os = "windows") => 1.0,
      Self::PlatformDefault => 0.0,
      Self::Visible { size, .. } => size.max(0.0),
      Self::Hidden => 0.0,
    }
  }
}

impl ChromeTitleBar {
  pub fn new() -> Self {
    Self {
      leading: None,
      title: None,
      center: None,
      trailing: None,
      controls: Some(WindowControls::new()),
      height: None,
      background: BackgroundColor::Color(Color::from_hex("#101215")),
      border_bottom: Some(Border::inside(1.0, Color::from_hex("#252a32"))),
    }
  }

  pub fn leading(mut self, element: impl Into<Element>) -> Self {
    self.leading = Some(element.into());
    self
  }

  pub fn title(mut self, element: impl Into<Element>) -> Self {
    self.title = Some(element.into());
    self
  }

  pub fn center(mut self, element: impl Into<Element>) -> Self {
    self.center = Some(element.into());
    self
  }

  pub fn trailing(mut self, element: impl Into<Element>) -> Self {
    self.trailing = Some(element.into());
    self
  }

  pub fn controls(mut self, controls: WindowControls) -> Self {
    self.controls = Some(controls);
    self
  }

  pub fn without_controls(mut self) -> Self {
    self.controls = None;
    self
  }

  pub fn height(mut self, height: f32) -> Self {
    self.height = Some(height.max(0.0));
    self
  }

  pub fn background(mut self, background: impl Into<BackgroundColor>) -> Self {
    self.background = background.into();
    self
  }

  pub fn border_bottom(mut self, border: impl Into<Option<Border>>) -> Self {
    self.border_bottom = border.into();
    self
  }

  fn render(self, window: &WindowHandle, chrome_height: f32) -> Element {
    let height = self.height.unwrap_or(chrome_height);
    let drag_window = window.clone();
    let maximize_window = window.clone();
    let titlebar_click = Arc::new(Mutex::new(None::<TitlebarClick>));
    let titlebar = Row::new()
      .width(Dimension::Pct(100.0))
      .height(height)
      .align_items(Alignment::Center)
      .background(self.background)
      .on_mouse_down(move |event: MouseEvent| {
        if event.button == MouseButton::Left {
          let now = Instant::now();
          let mut titlebar_click = titlebar_click
            .lock()
            .expect("titlebar click state should not be poisoned");
          let is_double_click = titlebar_click.is_some_and(|click| {
            now.duration_since(click.time) <= TITLEBAR_DOUBLE_CLICK_INTERVAL
              && (event.x - click.x).abs() <= TITLEBAR_DOUBLE_CLICK_DISTANCE
              && (event.y - click.y).abs() <= TITLEBAR_DOUBLE_CLICK_DISTANCE
          });

          if is_double_click {
            *titlebar_click = None;
            maximize_window.set_maximized(!maximize_window.is_maximized);
          } else {
            *titlebar_click = Some(TitlebarClick {
              time: now,
              x: event.x,
              y: event.y,
            });
            drag_window.start_drag();
          }

          event.prevent_default();
          event.stop_immediate_propagation();
        }
      });

    let mut titlebar = if let Some(border) = self.border_bottom {
      titlebar.border_bottom(border)
    } else {
      titlebar
    };

    if let Some(leading) = self.leading {
      titlebar = titlebar.child(leading);
    }
    if let Some(title) = self.title {
      titlebar = titlebar.child(title);
    }
    titlebar = titlebar.child(Row::new().flex(1.0).child(self.center.unwrap_or_default()));
    if let Some(trailing) = self.trailing {
      titlebar = titlebar.child(trailing);
    }
    if let Some(controls) = self.controls {
      titlebar = titlebar.child(controls.render(window, height));
    }

    titlebar.into()
  }
}

impl WindowChromeMetrics {
  /// Logical pixels of resize hit zone along each edge in the current window state.
  pub fn resize_handle_size_for(&self, window: WindowInfo) -> f32 {
    active_resize_handle_size(window, self.resize_handle_size)
  }

  /// Logical pixels the content is inset by resize handles in the current window state. Zero for
  /// [`ResizeHandlePlacement::Overlay`].
  pub fn resize_inset(&self, window: WindowInfo) -> f32 {
    match self.resize_placement {
      ResizeHandlePlacement::Overlay => 0.0,
      ResizeHandlePlacement::Inset => self.resize_handle_size_for(window),
    }
  }

  /// Horizontal origin of application content inside the custom frame.
  pub fn content_x(&self, window: WindowInfo) -> f32 {
    self.resize_inset(window)
  }

  /// Width available to application content between the side resize handles.
  pub fn content_width(&self, window: WindowInfo) -> f32 {
    (window.logical_width() - self.resize_inset(window) * 2.0).max(0.0)
  }

  /// Height below the title bar and above the bottom resize handle.
  pub fn content_height(&self, window: WindowInfo) -> f32 {
    (window.logical_height() - self.height - self.resize_inset(window)).max(0.0)
  }

  pub fn content_y(&self) -> f32 {
    self.height
  }

  pub fn modal_y(&self, y: f32) -> f32 {
    (y - self.height).max(0.0)
  }
}

fn border_layers(window: &WindowHandle, policy: &ChromeBorderPolicy) -> Vec<Element> {
  let (size, color) = match policy {
    ChromeBorderPolicy::PlatformDefault if policy.size() > 0.0 => {
      (1.0, BackgroundColor::Color(Color::from_hex("#252a32")))
    }
    ChromeBorderPolicy::PlatformDefault => return Vec::new(),
    ChromeBorderPolicy::Visible { size, color } => (*size, color.clone()),
    ChromeBorderPolicy::Hidden => return Vec::new(),
  };
  if size <= 0.0 {
    return Vec::new();
  }

  let width = window.logical_width();
  let height = window.logical_height();
  let right = (width - size).max(0.0);
  let bottom = (height - size).max(0.0);

  vec![
    border_strip(0.0, 0.0, width, size, color.clone()),
    border_strip(0.0, bottom, width, size, color.clone()),
    border_strip(0.0, 0.0, size, height, color.clone()),
    border_strip(right, 0.0, size, height, color),
  ]
}

fn border_strip(x: f32, y: f32, width: f32, height: f32, color: BackgroundColor) -> Element {
  Row::new().absolute(x, y, width, height).background(color).into()
}

fn active_resize_handle_size(window: WindowInfo, size: f32) -> f32 {
  if size <= 0.0 || window.is_maximized || window.is_full_screen {
    return 0.0;
  }

  let max_size = window.logical_width().min(window.logical_height()) * 0.5;
  size.min(max_size.max(0.0))
}

fn resize_handle_layers(window: &WindowHandle, size: f32) -> Vec<Element> {
  if size <= 0.0 || window.is_maximized || window.is_full_screen {
    return Vec::new();
  }

  let width = window.logical_width();
  let height = window.logical_height();
  let horizontal_width = (width - size * 2.0).max(0.0);
  let vertical_height = (height - size * 2.0).max(0.0);

  vec![
    resize_handle(
      window.clone(),
      WindowResizeDirection::North,
      size,
      0.0,
      horizontal_width,
      size,
      CursorIcon::NResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::South,
      size,
      height - size,
      horizontal_width,
      size,
      CursorIcon::SResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::West,
      0.0,
      size,
      size,
      vertical_height,
      CursorIcon::WResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::East,
      width - size,
      size,
      size,
      vertical_height,
      CursorIcon::EResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::NorthWest,
      0.0,
      0.0,
      size,
      size,
      CursorIcon::NwResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::NorthEast,
      width - size,
      0.0,
      size,
      size,
      CursorIcon::NeResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::SouthWest,
      0.0,
      height - size,
      size,
      size,
      CursorIcon::SwResize,
    ),
    resize_handle(
      window.clone(),
      WindowResizeDirection::SouthEast,
      width - size,
      height - size,
      size,
      size,
      CursorIcon::SeResize,
    ),
  ]
}

fn resize_handle(
  window: WindowHandle,
  direction: WindowResizeDirection,
  x: f32,
  y: f32,
  width: f32,
  height: f32,
  cursor: CursorIcon,
) -> Element {
  Row::new()
    .absolute(x, y, width, height)
    .background(Color::new(0, 0, 0, 0))
    .cursor(cursor)
    .on_mouse_down(move |event: MouseEvent| {
      if event.button == MouseButton::Left {
        window.start_resize(direction);
        event.prevent_default();
        event.stop_immediate_propagation();
      }
    })
    .into()
}

fn platform_custom_chrome_enabled() -> bool {
  cfg!(target_os = "windows") || cfg!(target_os = "macos")
}

fn platform_chrome_height(windows_height: f32, macos_height: f32) -> f32 {
  if cfg!(target_os = "macos") {
    macos_height.max(0.0)
  } else if cfg!(target_os = "windows") {
    windows_height.max(0.0)
  } else {
    windows_height.max(0.0)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::window::WindowCommand;

  fn window_info() -> WindowInfo {
    WindowInfo {
      x: 0,
      y: 0,
      resolved_width: 800.0,
      resolved_height: 600.0,
      scale_factor: 2.0,
      is_minimized: false,
      is_maximized: false,
      is_full_screen: false,
      is_decorated: false,
      is_focused: true,
      safe_area: Default::default(),
    }
  }

  fn mounted_chrome(maximized: bool, placement: ResizeHandlePlacement) -> Element {
    let window = crate::app::window::Window::new();
    window.set_resolved_size(800.0, 600.0);
    window.set_scale_factor(2.0);
    window.set_maximized(maximized);
    window.set_decorated(false);
    let mut ctx = Ctx::new().with_window(window);

    WindowChrome::new()
      .mode(WindowChromeMode::AlwaysCustom)
      .resize_placement(placement)
      .content(Row::new())
      .mount(&mut ctx)
  }

  #[test]
  fn disabled_metrics_are_zeroed() {
    let metrics = WindowChromeProps::new().mode(WindowChromeMode::Disabled).metrics();

    assert!(!metrics.enabled);
    assert_eq!(metrics.height, 0.0);
    assert_eq!(metrics.resize_handle_size, 0.0);
    assert_eq!(metrics.border_size, 0.0);
  }

  #[test]
  fn always_custom_metrics_use_platform_height_and_resize_policy() {
    let metrics = WindowChromeProps::new()
      .mode(WindowChromeMode::AlwaysCustom)
      .windows_height(40.0)
      .macos_height(30.0)
      .resize_handles(ResizeHandlePolicy::Enabled { size: 5.0 })
      .metrics();

    assert!(metrics.enabled);
    assert_eq!(metrics.height, platform_chrome_height(40.0, 30.0));
    assert_eq!(metrics.resize_handle_size, 5.0);
  }

  #[test]
  fn visible_border_policy_contributes_to_metrics() {
    let metrics = WindowChromeProps::new()
      .mode(WindowChromeMode::AlwaysCustom)
      .border(ChromeBorderPolicy::Visible {
        size: 2.0,
        color: BackgroundColor::Color(Color::from_hex("#ffffff")),
      })
      .metrics();

    assert_eq!(metrics.border_size, 2.0);
  }

  #[test]
  fn chrome_metrics_adjust_content_coordinates() {
    let metrics = WindowChromeMetrics {
      enabled: true,
      height: 36.0,
      resize_handle_size: 3.0,
      resize_placement: ResizeHandlePlacement::Inset,
      border_size: 1.0,
    };
    let window = window_info();

    assert_eq!(metrics.resize_inset(window), 3.0);
    assert_eq!(metrics.content_x(window), 3.0);
    assert_eq!(metrics.content_width(window), 394.0);
    assert_eq!(metrics.content_y(), 36.0);
    assert_eq!(metrics.content_height(window), 261.0);
    assert_eq!(metrics.modal_y(50.0), 14.0);
  }

  #[test]
  fn maximized_and_fullscreen_windows_remove_resize_inset() {
    let metrics = WindowChromeProps::new()
      .mode(WindowChromeMode::AlwaysCustom)
      .resize_handles(ResizeHandlePolicy::Enabled { size: 5.0 })
      .metrics();
    let mut maximized = window_info();
    maximized.is_maximized = true;
    let mut fullscreen = window_info();
    fullscreen.is_full_screen = true;

    for window in [maximized, fullscreen] {
      assert_eq!(metrics.resize_inset(window), 0.0);
      assert_eq!(metrics.content_x(window), 0.0);
      assert_eq!(metrics.content_width(window), 400.0);
      assert_eq!(
        metrics.content_height(window),
        if cfg!(target_os = "macos") { 272.0 } else { 264.0 }
      );
    }
  }

  #[test]
  fn disabled_resize_handles_do_not_inset_content() {
    let metrics = WindowChromeProps::new()
      .mode(WindowChromeMode::AlwaysCustom)
      .resize_handles(ResizeHandlePolicy::Disabled)
      .metrics();
    let window = window_info();

    assert_eq!(metrics.resize_inset(window), 0.0);
    assert_eq!(metrics.content_width(window), 400.0);
    assert_eq!(
      metrics.content_height(window),
      if cfg!(target_os = "macos") { 272.0 } else { 264.0 }
    );
  }

  #[test]
  fn inset_chrome_separates_client_layers_from_resize_edges() {
    let root = mounted_chrome(false, ResizeHandlePlacement::Inset).node;
    assert_eq!(root.color(), Some(Color::from_hex("#101215")));
    let expected_inset = crate::node::SpacingValue::from(RESIZE_HANDLE_SIZE);
    let no_top_inset = crate::node::SpacingValue::default();
    let content_layer = &root.children[0];
    assert_eq!(content_layer.padding.left, expected_inset);
    assert_eq!(content_layer.padding.top, no_top_inset);
    assert_eq!(content_layer.padding.right, expected_inset);
    assert_eq!(content_layer.padding.bottom, expected_inset);

    let chrome_overlay = &root.children[1].modal_declaration.as_ref().unwrap().node;
    let client_overlay = &chrome_overlay.children[0];
    assert_eq!(client_overlay.padding.left, expected_inset);
    assert_eq!(client_overlay.padding.top, no_top_inset);
    assert_eq!(client_overlay.padding.right, expected_inset);
    assert_eq!(client_overlay.padding.bottom, expected_inset);
    assert_eq!(
      chrome_overlay
        .children
        .iter()
        .filter(|node| node.cursor.is_some())
        .count(),
      8
    );
  }

  #[test]
  fn mounted_maximized_chrome_has_no_resize_gutter_or_handles() {
    let root = mounted_chrome(true, ResizeHandlePlacement::Inset).node;
    let no_inset = crate::node::SpacingValue::from(0.0);
    let content_layer = &root.children[0];
    assert_eq!(content_layer.padding.left, no_inset);
    assert_eq!(content_layer.padding.right, no_inset);
    assert_eq!(content_layer.padding.bottom, no_inset);

    let chrome_overlay = &root.children[1].modal_declaration.as_ref().unwrap().node;
    let client_overlay = &chrome_overlay.children[0];
    assert_eq!(client_overlay.padding.left, no_inset);
    assert_eq!(client_overlay.padding.right, no_inset);
    assert_eq!(client_overlay.padding.bottom, no_inset);
    assert_eq!(
      chrome_overlay
        .children
        .iter()
        .filter(|node| node.cursor.is_some())
        .count(),
      0
    );
  }

  #[test]
  fn overlay_chrome_keeps_full_content_and_all_resize_handles() {
    let root = mounted_chrome(false, ResizeHandlePlacement::Overlay).node;
    let no_inset = crate::node::SpacingValue::from(0.0);
    let content_layer = &root.children[0];
    assert_eq!(content_layer.padding.left, no_inset);
    assert_eq!(content_layer.padding.right, no_inset);
    assert_eq!(content_layer.padding.bottom, no_inset);

    let chrome_overlay = &root.children[1].modal_declaration.as_ref().unwrap().node;
    assert_eq!(chrome_overlay.children[0].padding.left, no_inset);
    assert_eq!(
      chrome_overlay
        .children
        .iter()
        .filter(|node| node.cursor.is_some())
        .count(),
      8
    );
  }

  #[test]
  fn mounted_chrome_paints_resize_gutter_with_content_background() {
    let window = crate::app::window::Window::new();
    window.set_resolved_size(800.0, 600.0);
    window.set_scale_factor(2.0);
    window.set_decorated(false);
    let mut ctx = Ctx::new().with_window(window);
    let background = Color::from_hex("#0b0e13");

    let root = WindowChrome::new()
      .mode(WindowChromeMode::AlwaysCustom)
      .content(Row::new().background(background))
      .mount(&mut ctx)
      .node;

    assert_eq!(root.color(), Some(background));
  }

  #[test]
  fn explicit_frame_background_overrides_content_background() {
    let window = crate::app::window::Window::new();
    window.set_resolved_size(800.0, 600.0);
    window.set_scale_factor(2.0);
    window.set_decorated(false);
    let mut ctx = Ctx::new().with_window(window);
    let frame_background = Color::from_hex("#20242b");

    let root = WindowChrome::new()
      .mode(WindowChromeMode::AlwaysCustom)
      .frame_background(frame_background)
      .content(Row::new().background("#0b0e13"))
      .mount(&mut ctx)
      .node;

    assert_eq!(root.color(), Some(frame_background));
  }

  #[test]
  fn mounted_chrome_hides_the_compositor_border_once() {
    let window = crate::app::window::Window::new();
    window.set_resolved_size(800.0, 600.0);
    window.set_decorated(false);
    let mut ctx = Ctx::new().with_window(window.clone());
    let chrome = || WindowChrome::new().mode(WindowChromeMode::AlwaysCustom);

    chrome().mount(&mut ctx);
    assert_eq!(
      window.take_commands(),
      vec![WindowCommand::SetBorderColor(WindowBorderColor::None)]
    );

    // The shell records the applied border; later mounts leave it alone.
    window.set_border_color(WindowBorderColor::None);
    chrome().mount(&mut ctx);
    assert_eq!(window.take_commands(), vec![]);
  }

  #[test]
  fn chrome_builder_collects_overlays() {
    let chrome = WindowChrome::new()
      .overlay(Element::new())
      .overlays([Element::new(), Element::new()]);

    assert_eq!(chrome.overlays.len(), 3);
  }
}
