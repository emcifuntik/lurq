//! Asking the person to prove who they are, with their face or their finger, before an app opens or shows a
//! part of itself.
//!
//! [`available`] says what the device can check. [`authenticate`] asks: the system shows its own prompt, and
//! the answer comes to the callback, on a thread of the system's. A phone answers; elsewhere there is nothing
//! to ask and the answer is [`AuthError::Unavailable`] at once.
//!
//! An app that asks on a phone says why in its `Info.plist` (`NSFaceIDUsageDescription`), or the system ends
//! it at the first ask.
//!
//! A secret, a token or a key an app logs in with, is kept behind the same check: [`keep_secret`] puts it in
//! the device's keychain, where it is the system that asks for the face or the finger before
//! [`read_secret`] gets it back, and not the app, which could be made to skip its own ask.

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

/// Why a secret was not kept, read or forgotten.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretError {
  /// No secret is kept under the name. One kept for the face or the finger alone is gone, too, once the faces
  /// or the fingers the device knows are others than they were.
  NotFound,
  /// The person was not let through to it, or the device has nothing to check by.
  Auth(AuthError),
}

/// Keeps a secret under a name in the device's keychain, in place of the one kept under it before. It stays
/// on this device, out of its backups, and is read only after the system has checked the person as `policy`
/// says; a device with no passcode keeps none ([`AuthError::Unavailable`]). The person is not asked now.
pub fn keep_secret(name: &str, secret: &[u8], policy: AuthPolicy) -> Result<(), SecretError> {
  platform::keep_secret(name, secret, policy)
}

/// Reads the secret kept under a name. The system asks the person to prove who they are, saying `reason`,
/// and `done` is called once, on another thread, with the secret when they did.
pub fn read_secret(name: &str, reason: &str, done: impl FnOnce(Result<Vec<u8>, SecretError>) + Send + 'static) {
  platform::read_secret(name, reason, Box::new(done));
}

/// Whether a secret is kept under a name. The person is not asked.
pub fn has_secret(name: &str) -> bool {
  platform::has_secret(name)
}

/// Forgets the secret kept under a name; there being none is no error. The person is not asked.
pub fn forget_secret(name: &str) -> Result<(), SecretError> {
  platform::forget_secret(name)
}

type SecretDone = Box<dyn FnOnce(Result<Vec<u8>, SecretError>) + Send>;

#[cfg(not(target_os = "ios"))]
mod platform {
  use super::{AuthError, AuthPolicy, Biometry, Done, SecretDone, SecretError};

  pub(super) fn available() -> Biometry {
    Biometry::None
  }

  pub(super) fn authenticate(_reason: &str, _policy: AuthPolicy, done: Done) {
    done(Err(AuthError::Unavailable));
  }

  const NOWHERE: SecretError = SecretError::Auth(AuthError::Unavailable);

  pub(super) fn keep_secret(_name: &str, _secret: &[u8], _policy: AuthPolicy) -> Result<(), SecretError> {
    Err(NOWHERE)
  }

  pub(super) fn read_secret(_name: &str, _reason: &str, done: SecretDone) {
    done(Err(NOWHERE));
  }

  pub(super) fn has_secret(_name: &str) -> bool {
    false
  }

  pub(super) fn forget_secret(_name: &str) -> Result<(), SecretError> {
    Err(NOWHERE)
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

  use super::{AuthError, AuthPolicy, Biometry, Done, SecretDone, SecretError};

  // The framework is not one of UIKit's: nothing else names it for the linker.
  #[link(name = "LocalAuthentication", kind = "framework")]
  unsafe extern "C" {}

  type Ref = *const std::ffi::c_void;

  // The keychain. Its dictionaries are Core Foundation's, which are Foundation's too.
  #[link(name = "Security", kind = "framework")]
  #[allow(non_upper_case_globals)]
  unsafe extern "C" {
    static kSecClass: Ref;
    static kSecClassGenericPassword: Ref;
    static kSecAttrService: Ref;
    static kSecAttrAccount: Ref;
    static kSecAttrAccessControl: Ref;
    static kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly: Ref;
    static kSecValueData: Ref;
    static kSecReturnData: Ref;
    static kSecMatchLimit: Ref;
    static kSecMatchLimitOne: Ref;
    static kSecUseAuthenticationContext: Ref;
    fn SecAccessControlCreateWithFlags(allocator: Ref, protection: Ref, flags: usize, error: *mut Ref) -> Ref;
    fn SecItemAdd(attributes: Ref, result: *mut Ref) -> i32;
    fn SecItemCopyMatching(query: Ref, result: *mut Ref) -> i32;
    fn SecItemDelete(query: Ref) -> i32;
    fn SecCopyErrorMessageString(status: i32, reserved: Ref) -> Ref;
  }

  #[link(name = "CoreFoundation", kind = "framework")]
  unsafe extern "C" {
    fn CFRelease(object: Ref);
  }

  // `OSStatus`: what the keychain says.
  const SUCCESS: i32 = 0;
  const USER_CANCELED: i32 = -128;
  const NOT_AVAILABLE: i32 = -25291;
  const AUTH_FAILED: i32 = -25293;
  const ITEM_NOT_FOUND: i32 = -25300;
  const INTERACTION_NOT_ALLOWED: i32 = -25308;

  /// `kSecAccessControlUserPresence` and `kSecAccessControlBiometryCurrentSet`: the face or the finger with
  /// the passcode to fall back on, and the faces or the fingers the device knows now, alone.
  fn access_flags(policy: AuthPolicy) -> usize {
    match policy {
      AuthPolicy::BiometryOrPasscode => 1 << 0,
      AuthPolicy::BiometryOnly => 1 << 3,
    }
  }

  fn string(text: &str) -> Retained<AnyObject> {
    let text = CString::new(text.replace('\0', " ")).unwrap_or_default();
    unsafe { msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()] }
  }

  fn said_of(text: *const AnyObject) -> String {
    let Some(text) = (unsafe { text.as_ref() }) else {
      return String::new();
    };
    let bytes: *const std::ffi::c_char = unsafe { msg_send![text, UTF8String] };
    if bytes.is_null() {
      String::new()
    } else {
      unsafe { std::ffi::CStr::from_ptr(bytes) }
        .to_string_lossy()
        .into_owned()
    }
  }

  /// Puts a value of Core Foundation's or Foundation's under one of the keychain's keys.
  unsafe fn set(item: &AnyObject, key: Ref, value: *const AnyObject) {
    let _: () = unsafe { msg_send![item, setObject: value, forKey: key.cast::<AnyObject>()] };
  }

  /// What names a secret in the keychain: a password that is no site's, of this app, under the name.
  fn item(name: &str) -> Retained<AnyObject> {
    unsafe {
      let item: Retained<AnyObject> = msg_send![class!(NSMutableDictionary), new];
      let bundle: Retained<AnyObject> = msg_send![class!(NSBundle), mainBundle];
      let app: Option<Retained<AnyObject>> = msg_send![&*bundle, bundleIdentifier];
      let app = app.unwrap_or_else(|| string("lurq"));
      set(&item, kSecClass, kSecClassGenericPassword.cast());
      set(&item, kSecAttrService, Retained::as_ptr(&app));
      set(&item, kSecAttrAccount, Retained::as_ptr(&string(name)));
      item
    }
  }

  fn secret_error(status: i32) -> SecretError {
    match status {
      ITEM_NOT_FOUND => SecretError::NotFound,
      USER_CANCELED => SecretError::Auth(AuthError::Cancelled),
      AUTH_FAILED => SecretError::Auth(AuthError::Failed),
      NOT_AVAILABLE => SecretError::Auth(AuthError::Unavailable),
      _ => {
        let said = unsafe {
          let said = SecCopyErrorMessageString(status, std::ptr::null());
          let text = said_of(said.cast());
          if !said.is_null() {
            CFRelease(said);
          }
          text
        };
        SecretError::Auth(AuthError::Other(format!("{said} ({status})")))
      }
    }
  }

  pub(super) fn keep_secret(name: &str, secret: &[u8], policy: AuthPolicy) -> Result<(), SecretError> {
    unsafe {
      let mut error: Ref = std::ptr::null();
      let access = SecAccessControlCreateWithFlags(
        std::ptr::null(),
        kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly,
        access_flags(policy),
        &mut error,
      );
      if access.is_null() {
        if !error.is_null() {
          CFRelease(error);
        }
        return Err(SecretError::Auth(AuthError::Unavailable));
      }
      let item = item(name);
      // One name keeps one secret: adding beside the old one is refused.
      SecItemDelete(Retained::as_ptr(&item).cast());
      let data: Retained<AnyObject> = msg_send![
        class!(NSData),
        dataWithBytes: secret.as_ptr().cast::<std::ffi::c_void>(),
        length: secret.len()
      ];
      set(&item, kSecValueData, Retained::as_ptr(&data));
      set(&item, kSecAttrAccessControl, access.cast());
      let status = SecItemAdd(Retained::as_ptr(&item).cast(), std::ptr::null_mut());
      CFRelease(access);
      match status {
        SUCCESS => Ok(()),
        // With no passcode on the device nothing can be kept behind one: it says the check failed.
        AUTH_FAILED => Err(SecretError::Auth(AuthError::Unavailable)),
        status => Err(secret_error(status)),
      }
    }
  }

  pub(super) fn read_secret(name: &str, reason: &str, done: SecretDone) {
    let (name, reason) = (name.to_owned(), reason.to_owned());
    // The keychain answers only after the person did, and the main thread, which shows the prompt, may not
    // wait for it.
    let asked = std::thread::Builder::new()
      .name("lurq-keychain".to_owned())
      .spawn(move || {
        let answer = unsafe {
          let query = item(&name);
          let context = context();
          let _: () = msg_send![&*context, setLocalizedReason: &*string(&reason)];
          let yes: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithBool: true];
          set(&query, kSecReturnData, Retained::as_ptr(&yes));
          set(&query, kSecMatchLimit, kSecMatchLimitOne.cast());
          set(&query, kSecUseAuthenticationContext, Retained::as_ptr(&context));
          let mut found: Ref = std::ptr::null();
          let status = SecItemCopyMatching(Retained::as_ptr(&query).cast(), &mut found);
          if status == SUCCESS && !found.is_null() {
            let data = found.cast::<AnyObject>();
            let bytes: *const u8 = msg_send![data, bytes];
            let length: usize = msg_send![data, length];
            let secret = if bytes.is_null() {
              Vec::new()
            } else {
              std::slice::from_raw_parts(bytes, length).to_vec()
            };
            CFRelease(found);
            Ok(secret)
          } else {
            Err(secret_error(status))
          }
        };
        done(answer);
      });
    if let Err(error) = asked {
      // The reply went with the thread that was not made: nothing more can be said to it.
      eprintln!("lurq: no thread to read a secret on: {error}");
    }
  }

  pub(super) fn has_secret(name: &str) -> bool {
    unsafe {
      let query = item(name);
      // A context that may show nothing: the keychain says the secret is there by saying it may not ask.
      let context = context();
      let _: () = msg_send![&*context, setInteractionNotAllowed: true];
      set(&query, kSecUseAuthenticationContext, Retained::as_ptr(&context));
      let status = SecItemCopyMatching(Retained::as_ptr(&query).cast(), std::ptr::null_mut());
      matches!(status, SUCCESS | INTERACTION_NOT_ALLOWED)
    }
  }

  pub(super) fn forget_secret(name: &str) -> Result<(), SecretError> {
    let status = unsafe { SecItemDelete(Retained::as_ptr(&item(name)).cast()) };
    match status {
      SUCCESS | ITEM_NOT_FOUND => Ok(()),
      status => Err(secret_error(status)),
    }
  }

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
        let said = said_of(Retained::as_ptr(&said));
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

  #[test]
  fn where_there_is_no_keychain_behind_a_check_no_secret_is_kept() {
    let nowhere = SecretError::Auth(AuthError::Unavailable);
    assert_eq!(
      keep_secret("token", b"secret", AuthPolicy::default()),
      Err(nowhere.clone())
    );
    assert!(!has_secret("token"));
    let answer = Arc::new(Mutex::new(None));
    let heard = answer.clone();
    read_secret("token", "a reason", move |answer| {
      *heard.lock().unwrap() = Some(answer);
    });
    assert_eq!(*answer.lock().unwrap(), Some(Err(nowhere.clone())));
    assert_eq!(forget_secret("token"), Err(nowhere));
  }
}
