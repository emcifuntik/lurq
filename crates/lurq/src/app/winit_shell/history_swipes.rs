//! Whether the platform turns trackpad swipes between pages on, read when a scroll gesture starts so a change in the
//! system's settings applies to the next swipe.

/// macOS: System Settings › Trackpad › More Gestures › "Swipe between pages" set to scroll with two fingers (or with
/// two or three). Set to three fingers only, AppKit sends a swipe event instead, which winit does not report.
#[cfg(target_os = "macos")]
pub(super) fn enabled() -> bool {
  objc2_app_kit::NSEvent::isSwipeTrackingFromScrollEventsEnabled()
}

/// Elsewhere winit reports no scroll phases, which the swipes are recognized from: not supported yet.
#[cfg(not(target_os = "macos"))]
pub(super) fn enabled() -> bool {
  false
}
