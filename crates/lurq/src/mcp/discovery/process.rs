//! Whether a process id still names a running process, so discovery files left
//! by apps that were killed (and never ran their shutdown) can be removed.
//!
//! Answers err on the side of "running": a file is removed only when the
//! operating system says plainly that no such process exists. A reused pid keeps
//! a stale file until that process ends too, which is harmless.

/// `true` unless the operating system reports that `pid` names no process.
#[cfg(windows)]
pub(super) fn is_running(pid: u32) -> bool {
  use windows::Win32::{
    Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, STILL_ACTIVE},
    System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
  };

  // SAFETY: OpenProcess takes no pointers; the returned handle is owned here and
  // closed below on every path.
  let handle = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
    Ok(handle) => handle,
    // No process has this id. Any other failure (access denied for another
    // user's process, for example) means it may well be running.
    Err(error) => return error.code() != ERROR_INVALID_PARAMETER.to_hresult(),
  };
  let mut code = 0u32;
  // SAFETY: `handle` is a valid process handle opened above with query rights;
  // `code` is a live, writable u32 for the duration of the call.
  let queried = unsafe { GetExitCodeProcess(handle, &mut code) };
  // SAFETY: `handle` was opened above and is not used after this call.
  // Losing a close error is fine: the handle is gone either way and nothing
  // the caller decides depends on it.
  let _ = unsafe { CloseHandle(handle) };
  match queried {
    Ok(()) => code == STILL_ACTIVE.0 as u32,
    Err(_) => true,
  }
}

/// `true` unless the operating system reports that `pid` names no process.
#[cfg(unix)]
pub(super) fn is_running(pid: u32) -> bool {
  let Ok(pid) = libc::pid_t::try_from(pid) else {
    return true;
  };
  if pid <= 0 {
    return true;
  }
  // SAFETY: signal 0 only checks that `pid` exists and may be signalled; it
  // sends nothing and touches no memory.
  if unsafe { libc::kill(pid, 0) } == 0 {
    return true;
  }
  std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Without a way to ask, every process counts as running and nothing is removed.
#[cfg(not(any(windows, unix)))]
pub(super) fn is_running(_pid: u32) -> bool {
  true
}
