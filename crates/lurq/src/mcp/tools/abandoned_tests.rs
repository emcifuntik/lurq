//! A call whose caller stopped waiting before the event loop reached it is dropped, not run late.

use std::sync::{Arc, Mutex};

use serde_json::json;

use super::tests::{TestSurface, state};
use crate::{
  app::{App, Tree, events::MouseEvent},
  components::{Column, Rect},
  mcp::shared::McpRequest,
};

fn clicked_tree(clicks: &Arc<Mutex<usize>>) -> Tree {
  let mut tree = Tree::new();
  tree.resize(200, 200);
  let clicks = clicks.clone();
  tree.set_root(
    Column::new()
      .width(200.0)
      .height(200.0)
      .child(Rect::new(100.0, 100.0).on_click(move |_: MouseEvent| *clicks.lock().unwrap() += 1)),
  );
  tree
}

#[test]
fn a_call_whose_caller_gave_up_does_not_run() {
  let clicks = Arc::new(Mutex::new(0));
  let mut tree = clicked_tree(&clicks);
  let mut app = App::new();
  tree.pass(&mut app, &TestSurface);
  let mut mcp = state();
  let (sender, receiver) = std::sync::mpsc::channel();
  mcp.receiver = receiver;
  tree.mcp = Some(Box::new(mcp));
  let click = || json!({"action": "click", "x": 50, "y": 50});

  let (reply, answer) = tokio::sync::oneshot::channel();
  sender
    .send(McpRequest {
      tool: "lurq_interact".into(),
      args: click(),
      reply,
      session: None,
    })
    .unwrap();
  // The server drops its receiver when the call's deadline passes.
  drop(answer);
  assert!(!tree.drain_mcp_requests(&mut app), "nothing ran");
  assert_eq!(*clicks.lock().unwrap(), 0);

  let (reply, mut answer) = tokio::sync::oneshot::channel();
  sender
    .send(McpRequest {
      tool: "lurq_interact".into(),
      args: click(),
      reply,
      session: None,
    })
    .unwrap();
  assert!(tree.drain_mcp_requests(&mut app));
  assert!(answer.try_recv().unwrap().is_ok());
  assert_eq!(*clicks.lock().unwrap(), 1, "a call whose caller waits runs");
}
