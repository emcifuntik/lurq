use std::path::PathBuf;

use serde_json::{Value, json};

use crate::{
  app::{Tree, Window},
  mcp::{
    FileDialogOperation, FileDialogSelection, McpState,
    file_dialogs::{LIST_TOOL, RESPOND_TOOL, scope},
    registry::{BuiltinTool, RegisteredTool, ToolKind},
    shared::{McpToolOutput, McpToolResult},
  },
};

pub(crate) fn live_windows(tree: &Tree, include_devtools: bool) -> Vec<(String, Window)> {
  let mut windows = vec![("main".into(), tree.window().clone())];
  for index in super::windows::visible_secondary_indexes(tree, include_devtools) {
    if let Some(window) = tree.secondary_window(index) {
      windows.push((format!("w{}", window.id()), window.tree().window().clone()));
    }
  }
  windows
}

impl Tree {
  pub(crate) fn reconcile_file_dialogs(&self) {
    if let Some(state) = &self.mcp {
      let broker = state.shared.file_dialogs.lock().unwrap().clone();
      if let Some(broker) = broker {
        broker.reconcile(&live_windows(self, state.include_devtools));
      }
    }
  }
}

pub(crate) fn execute(kind: BuiltinTool, tree: &Tree, state: &McpState, args: &Value) -> McpToolResult {
  let broker = state
    .shared
    .file_dialogs
    .lock()
    .unwrap()
    .clone()
    .ok_or("file dialog broker was not opted in")?;
  let windows = live_windows(tree, state.include_devtools);
  broker.reconcile(&windows);
  if kind == BuiltinTool::FileDialogs {
    return Ok(McpToolOutput::Json(broker.list(&windows)));
  }
  let required = |key| {
    args
      .get(key)
      .and_then(Value::as_str)
      .ok_or_else(|| format!("{key} required"))
  };
  let id = required("request_id")?;
  let target = required("window")?;
  // Only immutable canonical ids from the list; never resolve "focused"/name aliases.
  let (_, window) = windows
    .iter()
    .find(|(id, _)| id == target)
    .ok_or("window not found or closed")?;
  let operation = FileDialogOperation::parse(required("operation")?).map_err(|e| e.to_string())?;
  let selection = match required("action")? {
    "cancel" => {
      if args.get("paths").is_some() || args.get("overwrite").is_some() {
        return Err("cancel must not include paths or overwrite".into());
      }
      None
    }
    "select" => {
      let paths = args
        .get("paths")
        .and_then(Value::as_array)
        .ok_or("paths array required")?;
      if paths.len() > 64 {
        return Err("at most 64 paths".into());
      }
      let paths = paths
        .iter()
        .map(|path| {
          let path = path.as_str().ok_or("paths must contain strings")?;
          if path.len() > 32768 {
            return Err("path too long");
          }
          Ok(PathBuf::from(path))
        })
        .collect::<Result<Vec<_>, _>>()?;
      let overwrite = match args.get("overwrite") {
        Some(value) => value.as_bool().ok_or("overwrite must be boolean")?,
        None if operation == FileDialogOperation::SaveFile => {
          return Err("save_file requires explicit overwrite boolean".into());
        }
        None => false,
      };
      Some(FileDialogSelection { paths, overwrite })
    }
    _ => return Err("action must be select or cancel".into()),
  };
  broker
    .respond(id, window, operation, selection)
    .map_err(|e| e.to_string())?;
  Ok(McpToolOutput::Json(json!({"request_id":id,"completed":true})))
}

pub(crate) fn registered_tools() -> Vec<RegisteredTool> {
  vec![
    RegisteredTool {
      name: LIST_TOOL.into(), description: "List opted-in pending file selections with unique request/window/operation identity. No filesystem access.".into(),
      scope: scope(), read_only: true,
      input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
      kind: ToolKind::Builtin(BuiltinTool::FileDialogs),
    },
    RegisteredTool {
      name: RESPOND_TOOL.into(), description: "Complete one pending file selection or cancel. Absolute paths only; app owns validation/IO. Save requires explicit overwrite intent; no file-existence check.".into(),
      scope: scope(), read_only: false,
      input_schema: json!({"type":"object","additionalProperties":false,"required":["request_id","window","operation","action"],
        "properties":{
          "request_id":{"type":"string","maxLength":36},"window":{"type":"string","maxLength":64},
          "operation":{"enum":["open_file","open_files","open_folder","save_file"]},
          "action":{"enum":["select","cancel"]},
          "paths":{"type":"array","maxItems":64,"items":{"type":"string","maxLength":32768}},
          "overwrite":{"type":"boolean"}}}),
      kind: ToolKind::Builtin(BuiltinTool::FileDialogRespond),
    },
  ]
}
