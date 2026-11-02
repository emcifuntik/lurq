//! Thread-bound HWND and open-session lifetime for immediate text transfer.

use std::{marker::PhantomData, rc::Rc};

use ::windows::{
  Win32::{
    Foundation::{HANDLE, HWND},
    System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
    UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE, IsWindow, WINDOW_EX_STYLE, WINDOW_STYLE},
  },
  core::w,
};

use super::{Platform, Session, memory::Data};

pub(super) struct Native;

pub(super) struct Owner {
  hwnd: HWND,
  _thread: PhantomData<Rc<()>>,
}

pub(super) struct Open {
  opened: bool,
  _thread: PhantomData<Rc<()>>,
}

impl Platform for Native {
  type Owner = Owner;
  type Data = Data;
  type Session = Open;

  fn prepare(&self, text: &str) -> Option<Data> {
    Data::prepare(text)
  }

  fn owner(&self) -> Option<Owner> {
    // SAFETY: creates only this thread's message-only STATIC window. Its
    // built-in procedure requires no borrowed application data or callback.
    let hwnd = unsafe {
      CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        w!("LurqClipboardOwner"),
        WINDOW_STYLE::default(),
        0,
        0,
        0,
        0,
        Some(HWND_MESSAGE),
        None,
        None,
        None,
      )
    }
    .ok()?;
    Some(Owner {
      hwnd,
      _thread: PhantomData,
    })
  }

  fn open(&self, owner: &Owner) -> Option<Open> {
    // SAFETY: owner is held by the writer on its creation thread. Refuse a
    // destroyed window rather than silently reopening with NULL ownership.
    if !unsafe { IsWindow(Some(owner.hwnd)) }.as_bool() {
      return None;
    }
    // SAFETY: a live, process-owned HWND is provided; no retries are made.
    unsafe { OpenClipboard(Some(owner.hwnd)) }.ok()?;
    Some(Open {
      opened: true,
      _thread: PhantomData,
    })
  }
}

impl Session for Open {
  type Data = Data;

  fn empty(&mut self) -> bool {
    // SAFETY: this session holds the clipboard open with its live owner HWND.
    self.opened && unsafe { EmptyClipboard() }.is_ok()
  }

  fn put(&mut self, data: Data) -> bool {
    if !self.opened {
      return false;
    }
    // CF_UNICODETEXT is the documented standard format number 13. All bytes
    // are already encoded/unlocked; no delayed-rendering NULL handle is used.
    // SAFETY: an open session and a live movable UTF16 HGLOBAL are supplied.
    if unsafe { SetClipboardData(13, Some(HANDLE(data.handle().0))) }.is_err() {
      return false;
    }
    data.transfer(); // Windows now owns the handle; Drop must not free it.
    true
  }

  fn close(&mut self) -> bool {
    if !self.opened {
      return true;
    }
    self.opened = false;
    // SAFETY: exactly one Close follows this session's successful Open.
    unsafe { CloseClipboard() }.is_ok()
  }
}

impl Drop for Open {
  fn drop(&mut self) {
    if self.opened && !self.close() {
      tracing::warn!("clipboard session could not be closed");
    }
  }
}

impl Drop for Owner {
  fn drop(&mut self) {
    // SAFETY: !Send/!Sync keeps this HWND on its creation thread. Drop occurs
    // at thread exit, after calls and their open sessions have completed.
    if unsafe { DestroyWindow(self.hwnd) }.is_err() {
      tracing::warn!("clipboard owner window could not be destroyed");
    }
  }
}

#[cfg(test)]
mod tests {
  use ::windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

  use super::*;

  #[test]
  fn windows_clipboard_real_owner_is_process_owned_until_drop() {
    let owner = Native.owner().expect("own message-only test window");
    let hwnd = owner.hwnd;
    let mut pid = 0;
    // SAFETY: only this test's live, process-owned window is inspected.
    assert_ne!(unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) }, 0);
    assert_eq!(pid, std::process::id());
    assert!(unsafe { IsWindow(Some(hwnd)) }.as_bool());
    drop(owner);
    assert!(!unsafe { IsWindow(Some(hwnd)) }.as_bool());
    // No actual clipboard content was read or changed by this test.
  }
}
