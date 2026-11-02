//! Discovery files of processes that no longer run are removed when an MCP
//! server starts; the files of running processes, and anything not named
//! `<pid>.json`, stay.
#![cfg(feature = "mcp")]

use std::{
  path::{Path, PathBuf},
  process::{Child, Command, Stdio},
  time::Duration,
};

use lurq::{
  app::{App, Tree},
  mcp::McpConfig,
};

/// Run by the test below as a separate process that stays alive while checked.
#[test]
#[ignore = "helper process for mcp_discovery_removes_files_of_ended_processes"]
fn mcp_discovery_helper_stays_alive() {
  std::thread::sleep(Duration::from_secs(60));
}

#[test]
fn mcp_discovery_removes_files_of_ended_processes() {
  let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("mcp-discovery-{}", std::process::id()));
  let dir = point_discovery_at(&base);
  std::fs::create_dir_all(&dir).expect("create the discovery directory");

  let ended = ended_pid();
  let mut alive = helper_process();
  let alive_pid = alive.id();
  let files = [
    (format!("{ended}.json"), false),
    (format!("{alive_pid}.json"), true),
    ("notes.json".to_owned(), true),
    (format!("{ended}.txt"), true),
  ];
  for (name, _) in &files {
    std::fs::write(dir.join(name), "{}").expect("write a discovery file");
  }

  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(200, 100);
  tree.pass_headless(&mut app);
  let mcp = tree.enable_mcp(McpConfig::new().app_name("lurq-discovery-test"));
  assert_ne!(mcp.port(), 0, "the MCP server is listening");

  for (name, kept) in &files {
    assert_eq!(dir.join(name).exists(), *kept, "{name} kept: {kept}");
  }
  assert!(
    dir.join(format!("{}.json", std::process::id())).exists(),
    "this process's own discovery file is written"
  );

  tree.shutdown_mcp();
  alive.kill().expect("stop the helper process");
  alive.wait().expect("wait for the helper process");
  std::fs::remove_dir_all(&base).expect("remove the test folder");
}

/// Send the discovery directory into `base` for this test process.
fn point_discovery_at(base: &Path) -> PathBuf {
  let (variable, dir) = if cfg!(windows) {
    ("LOCALAPPDATA", base.join("lurq").join("mcp"))
  } else if cfg!(target_os = "macos") {
    (
      "HOME",
      base
        .join("Library")
        .join("Application Support")
        .join("lurq")
        .join("mcp"),
    )
  } else {
    ("XDG_RUNTIME_DIR", base.join("lurq").join("mcp"))
  };
  // SAFETY: this test binary runs this one test that reads the variable; the
  // helper test is ignored and runs only in its own process.
  unsafe { std::env::set_var(variable, base) };
  dir
}

/// The pid of a process that has ended.
fn ended_pid() -> u32 {
  let mut child = Command::new(std::env::current_exe().expect("test binary path"))
    .arg("--list")
    .stdout(Stdio::null())
    .spawn()
    .expect("start a short process");
  let pid = child.id();
  child.wait().expect("wait for the short process");
  pid
}

fn helper_process() -> Child {
  Command::new(std::env::current_exe().expect("test binary path"))
    .args(["--ignored", "--exact", "mcp_discovery_helper_stays_alive"])
    .stdout(Stdio::null())
    .spawn()
    .expect("start the helper process")
}
