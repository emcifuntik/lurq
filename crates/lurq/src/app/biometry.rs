//! Asking the person to prove who they are, with their face or their finger, before an app opens or shows a
//! part of itself.
//!
//! [`available`] says what the device can check. [`authenticate`] asks: the system shows its own prompt, and
//! the answer comes to the callback, on a thread of the system's. A phone answers; elsewhere there is nothing
//! to ask and the answer is [`AuthError::Unavailable`] at once.
//!
//! An app that asks on a phone says why in its `Info.plist` (`NSFaceIDUsageDescription`), or the system ends
//! it at the first ask.

/// What a device checks a person by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Biometry {
  /// Nothing: no sensor, none set up, or not a phone. The person's passcode may still be asked for.
  None,
  FaceId,
  TouchId,
  OpticId,
}

/// Whether the device's passcode may stand in for the face or the finger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AuthPolicy {
  /// The face or the finger, and the passcode when that fails or there is none: what an app opens with.
  #[default]
  BiometryOrPasscode,
  /// The face or the finger alone.
  BiometryOnly,
}

/// Why a person was not let through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthError {
  /// The person, the app or the system put the prompt away.
  Cancelled,
  /// The face or the finger was not theirs, as often as the system lets it be tried.
  Failed,
  /// The device has nothing to check by, or nothing is set up on it.
  Unavailable,
  /// Too many tries: the sensor is locked until the passcode is entered.
  LockedOut,
  /// Anything else, as the system says it.
  Other(String),
}

/// What the device can check a person by now.
pub fn available() -> Biometry {
  platform::available()
}

/// Asks the person to prove who they are. `reason` is what the system's prompt says the app wants it for.
/// `done` is called once, on a thread of the system's, with nothing when they did.
pub fn authenticate(reason: &str, policy: AuthPolicy, done: impl FnOnce(Result<(), AuthError>) + Send + 'static) {
  platform::authenticate(reason, policy, Box::new(done));
}

type Done = Box<dyn FnOnce(Result<(), AuthError>) + Send>;

#[cfg(not(target_os = "ios"))]
mod platform {
  use super::{AuthError, AuthPolicy, Biometry, Done};

  pub(super) fn available() -> Biometry {
    Biometry::None
  }

  pub(super) fn authenticate(_reason: &str, _policy: AuthPolicy, done: Done) {
    done(Err(AuthError::Unavailable));
  }
}

#[cfg(target_os = "ios")]
mod platform {
  use std::{ffi::CString, sync::Mutex};

  use block2::RcBlock;
  use objc2::{
    class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
  };

  use super::{AuthError, AuthPolicy, Biometry, Done};

  // The framework is not one of UIKit's: nothing else names it for the linker.
  #[link(name = "LocalAuthentication", kind = "framework")]
  unsafe extern "C" {}

  /// `LAPolicyDeviceOwnerAuthenticationWithBiometrics` and `LAPolicyDeviceOwnerAuthentication`.
  fn policy_number(policy: AuthPolicy) -> isize {
    match policy {
      AuthPolicy::BiometryOnly => 1,
      AuthPolicy::BiometryOrPasscode => 2,
    }
  }

  fn context() -> Retained<AnyObject> {
    unsafe { msg_send![class!(LAContext), new] }
  }

  pub(super) fn available() -> Biometry {
    unsafe {
      let context = context();
      // The kind of sensor is known only after the context was asked whether it can check.
      let can: Bool = msg_send![
        &*context,
        canEvaluatePolicy: policy_number(AuthPolicy::BiometryOnly),
        error: std::ptr::null_mut::<*mut AnyObject>()
      ];
      if !can.as_bool() {
        return Biometry::None;
      }
      // `LABiometryType`.
      let kind: isize = msg_send![&*context, biometryType];
      match kind {
        1 => Biometry::TouchId,
        2 => Biometry::FaceId,
        4 => Biometry::OpticId,
        _ => Biometry::None,
      }
    }
  }

  /// `LAError`'s codes.
  fn error_of(error: *mut AnyObject) -> AuthError {
    let Some(error) = (unsafe { error.as_ref() }) else {
      return AuthError::Failed;
    };
    let code: isize = unsafe { msg_send![error, code] };
    match code {
      // The app, the system or the person cancelled; or the person chose the fallback the app does not have.
      -2 | -3 | -4 | -9 => AuthError::Cancelled,
      -1 => AuthError::Failed,
      // No passcode, no sensor, nothing enrolled.
      -5 | -6 | -7 => AuthError::Unavailable,
      -8 => AuthError::LockedOut,
      _ => {
        let said: Retained<AnyObject> = unsafe { msg_send![error, localizedDescription] };
        let bytes: *const std::ffi::c_char = unsafe { msg_send![&*said, UTF8String] };
        let said = if bytes.is_null() {
          String::new()
        } else {
          unsafe { std::ffi::CStr::from_ptr(bytes) }
            .to_string_lossy()
            .into_owned()
        };
        AuthError::Other(format!("{said} ({code})"))
      }
    }
  }

  pub(super) fn authenticate(reason: &str, policy: AuthPolicy, done: Done) {
    let reason = CString::new(reason.replace('\0', " ")).unwrap_or_default();
    // The reply is called once; the block may be called from any thread.
    let done = Mutex::new(Some(done));
    unsafe {
      let context = context();
      let reason: Retained<AnyObject> = msg_send![class!(NSString), stringWithUTF8String: reason.as_ptr()];
      // The context is held by the block until the reply: let go before, the prompt is put away.
      let held = context.clone();
      let reply = RcBlock::new(move |success: Bool, error: *mut AnyObject| {
        let _held = &held;
        if let Some(done) = done.lock().ok().and_then(|mut done| done.take()) {
          done(if success.as_bool() {
            Ok(())
          } else {
            Err(error_of(error))
          });
        }
      });
      let _: () = msg_send![
        &*context,
        evaluatePolicy: policy_number(policy),
        localizedReason: &*reason,
        reply: &*reply
      ];
    }
  }
}

#[cfg(all(test, not(target_os = "ios")))]
mod tests {
  use std::sync::{Arc, Mutex};

  use super::*;

  #[test]
  fn where_there_is_nothing_to_ask_the_answer_comes_at_once() {
    assert_eq!(available(), Biometry::None);
    let answer = Arc::new(Mutex::new(None));
    let heard = answer.clone();
    authenticate("a reason", AuthPolicy::default(), move |answer| {
      *heard.lock().unwrap() = Some(answer);
    });
    assert_eq!(*answer.lock().unwrap(), Some(Err(AuthError::Unavailable)));
  }
}
