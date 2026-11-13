//! The system's "reduce motion" setting: what the user asked of every app in
//! the platform's accessibility settings.
//!
//! The shell reads it when a window opens and each time one takes focus,
//! which is when a user comes back from the system's settings, and gives it
//! to the theme (`Theme::set_system_reduced_motion`). A tree with no shell, a
//! test's for one, never reads it, so its motion does not depend on the
//! machine it runs on.
//!
//! | Platform | Setting | Read through |
//! | --- | --- | --- |
//! | Windows | Settings › Accessibility › Visual effects › Animation effects | `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)` |
//! | macOS | System Settings › Accessibility › Display › Reduce motion | `NSWorkspace.accessibilityDisplayShouldReduceMotion` |
//! | iOS | Settings › Accessibility › Motion › Reduce Motion | `UIAccessibilityIsReduceMotionEnabled()` |
//! | Linux and others | none that every desktop has | not read: `None` |

/// Whether the system asks apps for less motion, or `None` where the platform
/// has no setting lurq reads.
#[cfg_attr(not(feature = "winit"), allow(dead_code))]
pub(crate) fn system_reduced_motion() -> Option<bool> {
  platform::read()
}

#[cfg(windows)]
mod platform {
  use windows::{
    Win32::UI::WindowsAndMessaging::{
      SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    },
    core::BOOL,
  };

  pub(super) fn read() -> Option<bool> {
    let mut animations = BOOL::from(true);
    // SAFETY: `SPI_GETCLIENTAREAANIMATION` writes one `BOOL` through `pvParam`, which points at one.
    unsafe {
      SystemParametersInfoW(
        SPI_GETCLIENTAREAANIMATION,
        0,
        Some((&raw mut animations).cast()),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
      )
    }
    .ok()?;
    // The setting is "animation effects": off is the request for less motion.
    Some(!animations.as_bool())
  }
}

#[cfg(target_os = "macos")]
mod platform {
  use objc2::{class, msg_send, runtime::AnyObject};

  // `NSWorkspace` is a class of AppKit, and nothing links AppKit into a binary that names no symbol
  // of it: one built without the winit shell, a test's for one.
  #[link(name = "AppKit", kind = "framework")]
  unsafe extern "C" {}

  pub(super) fn read() -> Option<bool> {
    // SAFETY: `+[NSWorkspace sharedWorkspace]` returns the process's workspace, which lives as long as
    // the process, and `-accessibilityDisplayShouldReduceMotion` (macOS 10.12) takes no argument and
    // returns a `BOOL`. Neither needs the main thread.
    unsafe {
      let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
      if workspace.is_null() {
        return None;
      }
      let reduce: bool = msg_send![workspace, accessibilityDisplayShouldReduceMotion];
      Some(reduce)
    }
  }
}

#[cfg(target_os = "ios")]
mod platform {
  #[link(name = "UIKit", kind = "framework")]
  unsafe extern "C" {
    fn UIAccessibilityIsReduceMotionEnabled() -> bool;
  }

  pub(super) fn read() -> Option<bool> {
    // SAFETY: a C function of UIKit that takes no argument and returns a `BOOL`, which is `bool` on
    // every iOS target.
    Some(unsafe { UIAccessibilityIsReduceMotionEnabled() })
  }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "ios")))]
mod platform {
  pub(super) fn read() -> Option<bool> {
    None
  }
}

#[cfg(test)]
mod tests {
  use crate::app::theme::Theme;

  #[cfg(any(windows, target_os = "macos", target_os = "ios"))]
  #[test]
  fn the_platform_has_the_setting() {
    assert!(super::system_reduced_motion().is_some());
  }

  #[test]
  fn the_app_decides_over_the_system() {
    let theme = Theme::default();
    assert!(!theme.reduced_motion(), "a theme no shell wrote to reduces nothing");

    theme.set_system_reduced_motion(true);
    assert!(theme.reduced_motion(), "the system's setting is followed");

    theme.set_reduced_motion(Some(false));
    assert!(!theme.reduced_motion());

    theme.set_system_reduced_motion(false);
    theme.set_reduced_motion(Some(true));
    assert!(theme.reduced_motion());

    theme.set_reduced_motion(None);
    assert!(
      !theme.reduced_motion(),
      "and followed again once the app stops deciding"
    );
  }

  #[test]
  fn a_setting_that_does_not_change_does_not_change_the_theme() {
    let theme = Theme::default();
    theme.set_system_reduced_motion(true);
    let version = theme.version();

    theme.set_system_reduced_motion(true);
    theme.set_reduced_motion(None);

    assert_eq!(theme.version(), version);
  }
}
