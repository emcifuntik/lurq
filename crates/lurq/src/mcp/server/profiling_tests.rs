#[cfg(feature = "perf_profile")]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
  collections::HashSet,
  io::{Read, Write},
};

use super::*;
#[cfg(feature = "perf_profile")]
use crate::{
  app::App,
  components::{Column, Text},
};
use crate::{app::Tree, mcp::Scope};

fn shared(tree: &Tree) -> Arc<McpShared> {
  let shared = Arc::new(McpShared::new(
    HashSet::from([Scope::Observe]),
    HashSet::new(),
    "secret-for-test".into(),
    "test".into(),
    None,
  ));
  *shared.profiling.write().unwrap() = tree.profiling_handle();
  shared
}

#[test]
fn profiling_server_availability_is_scope_guarded() {
  let tree = Tree::new();
  let shared = shared(&tree);
  let registry = Arc::new(crate::mcp::build_registry(Vec::new()));
  let (sender, _receiver) = std_mpsc::channel();
  let server = McpService::new(shared.clone(), registry, sender);
  let tool = server.registry.find("lurq_profile_start").unwrap();
  assert!(server.visible(tool));
  shared.remove_scope(&Scope::Observe);
  assert!(!server.visible(tool));
  shared.add_scope(Scope::Observe);
  shared.deny_tool(&tool.name);
  assert!(!server.visible(tool));
}

#[cfg(feature = "perf_profile")]
#[test]
fn profiling_server_start_end_never_queue_or_wake_the_ui_and_export_real_layout() {
  let mut tree = Tree::new();
  let mut app = App::new();
  tree.set_root(Column::new().child(Text::new("private fixture text")));
  let shared = shared(&tree);
  let wakes = Arc::new(AtomicUsize::new(0));
  *shared.waker.lock().unwrap() = Some({
    let wakes = wakes.clone();
    Arc::new(move || {
      wakes.fetch_add(1, Ordering::SeqCst);
    })
  });
  let (sender, receiver) = std_mpsc::channel();
  let server = McpService::new(shared, Arc::new(crate::mcp::build_registry(Vec::new())), sender);
  let start = server.registry.find("lurq_profile_start").unwrap();
  let end = server.registry.find("lurq_profile_end").unwrap();
  let extract = |output: McpToolOutput| match output {
    McpToolOutput::Json(value) => value,
    _ => panic!("JSON expected"),
  };
  let before = tree.frame_count();
  let first = extract(server.serve_locally(start, &serde_json::json!({})).unwrap().unwrap());
  let second = extract(server.serve_locally(start, &serde_json::json!({})).unwrap().unwrap());
  assert_eq!(tree.frame_count(), before);
  assert!(matches!(receiver.try_recv(), Err(std_mpsc::TryRecvError::Empty)));
  assert_eq!(wakes.load(Ordering::SeqCst), 0);
  tree.pass_headless(&mut app);
  let report2 = extract(
    server
      .serve_locally(end, &serde_json::json!({"id":second["id"]}))
      .unwrap()
      .unwrap(),
  );
  tree.pass_headless(&mut app);
  let report1 = extract(
    server
      .serve_locally(end, &serde_json::json!({"id":first["id"]}))
      .unwrap()
      .unwrap(),
  );
  assert_eq!(report2["returned_samples"], 1);
  assert_eq!(report1["returned_samples"], 2);
  assert!(
    report1["samples"][0]["data"]["cpu_timings_ms"]["layout_update"]
      .as_f64()
      .unwrap()
      > 0.
  );
  assert!(!report1.to_string().contains("private fixture text"));
  assert!(matches!(receiver.try_recv(), Err(std_mpsc::TryRecvError::Empty)));
  assert_eq!(wakes.load(Ordering::SeqCst), 0);
}

#[test]
fn profiling_transport_still_requires_bearer_auth_before_tool_dispatch() {
  let tree = Tree::new();
  let shared = shared(&tree);
  let (sender, receiver) = std_mpsc::channel();
  let runtime = spawn(shared, Arc::new(crate::mcp::build_registry(Vec::new())), sender, None).unwrap();
  for authorization in ["", "Authorization: Bearer wrong-token\r\n"] {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", runtime.port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let request =
      format!("POST /mcp HTTP/1.1\r\nHost: localhost\r\n{authorization}Content-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    let mut bytes = [0; 512];
    let read = stream.read(&mut bytes).unwrap();
    assert!(String::from_utf8_lossy(&bytes[..read]).starts_with("HTTP/1.1 401"));
  }
  assert!(matches!(receiver.try_recv(), Err(std_mpsc::TryRecvError::Empty)));
  runtime.cancel.cancel();
  runtime.join.join().unwrap();
}
