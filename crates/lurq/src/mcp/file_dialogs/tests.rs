use std::{
  collections::HashSet,
  path::PathBuf,
  sync::atomic::{AtomicUsize, Ordering},
  task::{Wake, Waker},
};

use super::*;
use crate::app::{App, Tree};

fn fixture() -> (FileDialogBroker, Arc<McpShared>, Tree) {
  let broker = FileDialogBroker::new();
  let shared = Arc::new(McpShared::new(
    HashSet::from([scope()]),
    HashSet::new(),
    "test".into(),
    "test".into(),
    None,
  ));
  broker.attach(&shared);
  *shared.file_dialogs.lock().unwrap() = Some(broker.clone());
  (broker, shared, Tree::new())
}

fn path() -> PathBuf {
  std::env::current_dir().unwrap().join("Unicode-α.txt")
}
fn selected() -> FileDialogSelection {
  FileDialogSelection {
    paths: vec![path()],
    overwrite: false,
  }
}
fn request(broker: &FileDialogBroker, tree: &Tree) -> FileDialogFuture {
  broker
    .request(
      &tree.window().handle(),
      FileDialogRequest::new(FileDialogOperation::OpenFile),
    )
    .unwrap()
    .unwrap()
}
fn complete(future: FileDialogFuture) -> Result<FileDialogSelection, FileDialogError> {
  tokio::runtime::Builder::new_current_thread()
    .build()
    .unwrap()
    .block_on(future)
}

#[test]
fn file_dialogs_unregistered_native_boundary_and_one_shot_windows() {
  let tree = Tree::new();
  assert!(
    FileDialogBroker::new()
      .request(
        &tree.window().handle(),
        FileDialogRequest::new(FileDialogOperation::OpenFile)
      )
      .unwrap()
      .is_none()
  );
  let (broker, _shared, first) = fixture();
  let second = Tree::new();
  let a = request(&broker, &first);
  let b = request(&broker, &second);
  assert_ne!(a.id, b.id);
  assert_eq!(
    broker.respond(&a.id, second.window(), FileDialogOperation::OpenFile, Some(selected())),
    Err(FileDialogError::WindowClosed)
  );
  assert_eq!(
    broker.respond(&a.id, first.window(), FileDialogOperation::SaveFile, Some(selected())),
    Err(FileDialogError::WrongOperation)
  );
  let id = a.id.clone();
  broker
    .respond(&id, first.window(), FileDialogOperation::OpenFile, Some(selected()))
    .unwrap();
  assert_eq!(complete(a), Ok(selected()));
  assert_eq!(
    broker.respond(&id, first.window(), FileDialogOperation::OpenFile, Some(selected())),
    Err(FileDialogError::StaleRequest)
  );
  broker
    .respond(&b.id, second.window(), FileDialogOperation::OpenFile, None)
    .unwrap();
  assert_eq!(complete(b), Err(FileDialogError::Cancelled));
}

#[test]
fn file_dialogs_future_drop_and_closed_or_replaced_window_retire_capacity() {
  let (broker, _shared, tree) = fixture();
  let pending: Vec<_> = (0..MAX_PENDING).map(|_| request(&broker, &tree)).collect();
  assert!(matches!(
    broker.request(
      &tree.window().handle(),
      FileDialogRequest::new(FileDialogOperation::OpenFile)
    ),
    Err(FileDialogError::TooManyPending)
  ));
  drop(pending);
  assert!(broker.0.lock().unwrap().pending.is_empty());
  let future = request(&broker, &tree);
  let replacement = Tree::new();
  broker.reconcile(&[("main".into(), replacement.window().clone())]);
  assert_eq!(complete(future), Err(FileDialogError::WindowClosed));
  let future = request(&broker, &tree);
  tree.window().handle().close();
  broker.reconcile(&[("main".into(), tree.window().clone())]);
  assert_eq!(complete(future), Err(FileDialogError::WindowClosed));
  assert!(broker.0.lock().unwrap().pending.is_empty());
}

#[test]
fn file_dialogs_vetoed_close_keeps_request_and_accepted_close_cancels() {
  let (broker, _shared, tree) = fixture();
  let future = request(&broker, &tree);
  tree.window().handle().on_close_requested(|request| request.cancel());
  tree.window().handle().request_close();
  assert!(tree.window().take_shell_commands().is_empty());
  broker.reconcile(&[("main".into(), tree.window().clone())]);
  assert_eq!(broker.0.lock().unwrap().pending.len(), 1);
  tree.window().handle().clear_close_requested_handler();
  tree.window().handle().request_close();
  tree.window().take_shell_commands();
  // Root's live-window cohort excludes the actually closed window.
  broker.reconcile(&[]);
  assert_eq!(complete(future), Err(FileDialogError::WindowClosed));
}

#[test]
fn file_dialogs_revocation_shutdown_and_retained_clone_never_fall_back() {
  for mode in 0..4 {
    let (broker, shared, tree) = fixture();
    let future = request(&broker, &tree);
    match mode {
      0 => shared.set_enabled(false),
      1 => shared.remove_scope(&scope()),
      2 => shared.deny_tool(RESPOND_TOOL),
      _ => broker.shutdown(),
    }
    assert_eq!(complete(future), Err(FileDialogError::Unavailable));
    assert!(matches!(
      broker.request(
        &tree.window().handle(),
        FileDialogRequest::new(FileDialogOperation::OpenFile)
      ),
      Err(FileDialogError::Unavailable)
    ));
    assert!(broker.0.lock().unwrap().pending.is_empty());
  }
}

#[test]
fn file_dialogs_accepted_close_is_terminal_after_command_drain_for_retained_handles() {
  for direct in [false, true] {
    let (broker, _shared, tree) = fixture();
    let handle = tree.window().handle();
    let future = request(&broker, &tree);
    if direct {
      handle.close();
    } else {
      handle.request_close();
    }
    assert_eq!(
      tree.window().take_shell_commands(),
      vec![crate::app::window::WindowCommand::Close]
    );
    assert!(matches!(
      broker.request(&handle, FileDialogRequest::new(FileDialogOperation::OpenFile)),
      Err(FileDialogError::WindowClosed)
    ));
    broker.reconcile(&[("main".into(), tree.window().clone())]);
    assert_eq!(complete(future), Err(FileDialogError::WindowClosed));
    let replacement = Tree::new();
    let fresh = request(&broker, &replacement);
    assert!(broker.0.lock().unwrap().pending.contains_key(&fresh.id));
    drop(fresh);
  }
}

#[test]
fn file_dialogs_paths_cardinality_and_bounded_metadata_are_checked_before_completion() {
  let (broker, _shared, tree) = fixture();
  let mut bad = FileDialogRequest::new(FileDialogOperation::OpenFile);
  bad.title = "x".repeat(4097);
  assert!(matches!(
    broker.request(&tree.window().handle(), bad),
    Err(FileDialogError::InvalidMetadata)
  ));
  let future = request(&broker, &tree);
  for paths in [vec![], vec!["relative".into()], vec![path(), path()]] {
    assert_eq!(
      broker.respond(
        &future.id,
        tree.window(),
        FileDialogOperation::OpenFile,
        Some(FileDialogSelection {
          paths,
          overwrite: false
        })
      ),
      Err(FileDialogError::InvalidSelection)
    );
  }
  broker
    .respond(
      &future.id,
      tree.window(),
      FileDialogOperation::OpenFile,
      Some(selected()),
    )
    .unwrap();
  assert_eq!(complete(future), Ok(selected()));
  let save = broker
    .request(
      &tree.window().handle(),
      FileDialogRequest::new(FileDialogOperation::SaveFile),
    )
    .unwrap()
    .unwrap();
  let selection = FileDialogSelection {
    paths: vec![path()],
    overwrite: true,
  };
  broker
    .respond(
      &save.id,
      tree.window(),
      FileDialogOperation::SaveFile,
      Some(selection.clone()),
    )
    .unwrap();
  assert_eq!(complete(save), Ok(selection));
}

struct Reenter {
  broker: FileDialogBroker,
  calls: AtomicUsize,
}
impl Wake for Reenter {
  fn wake(self: Arc<Self>) {
    self.wake_by_ref();
  }
  fn wake_by_ref(self: &Arc<Self>) {
    assert!(self.broker.0.try_lock().is_ok(), "wake must occur outside broker lock");
    self.broker.permission_changed();
    self.calls.fetch_add(1, Ordering::SeqCst);
  }
}

#[test]
fn file_dialogs_response_and_revocation_wake_outside_internal_locks() {
  for cancel in [false, true] {
    let (broker, shared, tree) = fixture();
    let mut future = request(&broker, &tree);
    let reenter = Arc::new(Reenter {
      broker: broker.clone(),
      calls: AtomicUsize::new(0),
    });
    let waker = Waker::from(reenter.clone());
    assert!(
      Pin::new(&mut future)
        .poll(&mut Context::from_waker(&waker))
        .is_pending()
    );
    if cancel {
      shared.set_enabled(false);
    } else {
      broker
        .respond(
          &future.id,
          tree.window(),
          FileDialogOperation::OpenFile,
          Some(selected()),
        )
        .unwrap();
    }
    assert_eq!(reenter.calls.load(Ordering::SeqCst), 1);
  }
}

#[test]
fn file_dialogs_opt_in_tools_and_actual_drain_cancel_without_later_mcp_call() {
  let mut tree = Tree::new();
  let mut app = App::new();
  let broker = FileDialogBroker::new();
  let handle = tree.enable_mcp(
    super::super::McpConfig::new()
      .scope(scope())
      .file_dialogs(broker.clone()),
  );
  let future = request(&broker, &tree);
  tree.window().handle().close();
  tree.drain_mcp_requests(&mut app);
  assert_eq!(complete(future), Err(FileDialogError::WindowClosed));
  let retained = broker.clone();
  tree.shutdown_mcp();
  handle.set_enabled(true);
  assert!(matches!(
    retained.request(
      &Tree::new().window().handle(),
      FileDialogRequest::new(FileDialogOperation::OpenFile)
    ),
    Err(FileDialogError::Unavailable)
  ));
}
