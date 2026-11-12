pub mod app_state;
#[cfg(all(test, windows, feature = "screenshot", any(feature = "wgpu", feature = "dx12")))]
mod backdrop_blur_readback_tests;
#[cfg(all(
  test,
  windows,
  feature = "raster",
  feature = "screenshot",
  any(feature = "wgpu", feature = "dx12")
))]
mod blend_readback_tests;
#[cfg(all(test, windows, feature = "screenshot", any(feature = "wgpu", feature = "dx12")))]
mod box_shadow_readback_tests;
#[cfg(all(
  test,
  windows,
  feature = "canvas",
  feature = "perf_profile",
  feature = "screenshot",
  feature = "wgpu",
  feature = "dx12"
))]
mod canvas_frame_probe;
#[cfg(feature = "screenshot")]
pub(crate) mod capture_redaction;
#[cfg(all(test, feature = "screenshot"))]
pub(crate) mod capture_test_engine;
pub mod component;
pub mod ctx;
#[cfg(feature = "devtools")]
pub mod devtools;
#[cfg(all(test, windows, feature = "canvas", feature = "dx12", feature = "screenshot"))]
mod dx12_canvas_residency_tests;
#[cfg(all(feature = "dx12", target_os = "windows"))]
pub mod dx12_render;
pub mod events;
#[cfg(feature = "screenshot")]
// Without a render backend nothing performs captures; the helpers stay for
// the mcp feature's capture types.
#[cfg_attr(
  not(any(feature = "wgpu", all(feature = "dx12", target_os = "windows"))),
  allow(dead_code)
)]
pub(crate) mod frame_capture;
pub(crate) mod glyph_engine;
pub(crate) mod hit_test;
#[cfg(feature = "i18n")]
pub mod i18n;
#[cfg(all(target_os = "ios", feature = "raster"))]
pub(crate) mod ios_symbol;
#[cfg(all(test, windows, feature = "screenshot", any(feature = "wgpu", feature = "dx12")))]
mod layer_blur_readback_tests;
#[cfg(all(test, windows, feature = "screenshot", any(feature = "wgpu", feature = "dx12")))]
mod opacity_layer_readback_tests;
pub(crate) mod profile_support;
pub(crate) mod profile_types;
pub mod profiler;
#[cfg(all(test, windows, feature = "screenshot", any(feature = "wgpu", feature = "dx12")))]
mod readback_window;
pub mod render_engine;
pub mod runtime;
#[cfg(all(test, windows, feature = "screenshot", any(feature = "wgpu", feature = "dx12")))]
mod stroke_readback_tests;
pub mod synthetic_input;
pub mod theme;
#[cfg(feature = "wgpu")]
pub mod wgpu_render;
pub mod window;
#[cfg(feature = "winit")]
pub mod winit_shell;

pub use app_state::{App, SecondaryWindowRequest, WindowOpener, WindowOptions};
pub use runtime::{
  CheckboxHandle, ClipboardReader, ElementHandle, PassReasons, PassReport, SelectHandle, SliderHandle, TextInputHandle,
  Tree,
};
pub use synthetic_input::{SyntheticInput, SyntheticInputKind, SyntheticModifiers};
#[cfg(feature = "screenshot")]
pub use window::ScreenshotRegion;
pub use window::{
  CloseRequest, CloseRequestSource, Window, WindowBorderColor, WindowCornerRadius, WindowHandle, WindowIcon,
  WindowInfo, WindowResizeDirection,
};

pub mod menu;
pub use menu::{Accelerator, ApplicationMenu, Menu, MenuAction, MenuBar, MenuBarSupport, MenuController, MenuItem};
#[cfg(all(feature = "winit", target_os = "macos"))]
mod macos_menu;
