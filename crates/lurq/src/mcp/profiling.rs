//! Server-thread adapter over the toolkit's shared profiling contract.
use serde_json::{Value, json};

use super::{
  Scope,
  registry::{BuiltinTool, RegisteredTool, ToolKind},
  shared::{McpShared, McpToolOutput, McpToolResult},
};
use crate::app::profiler::{
  MAX_ACTIVE_SESSIONS, MAX_ENDED_SESSION_IDS, MAX_SAMPLES_PER_SESSION, MAX_TRACKED_WINDOWS, MAX_WINDOW_ID_BYTES,
  ProfileError, SessionId, SessionOptions,
};

pub(crate) const TOOL_NAMES: [&str; 3] = ["lurq_profile_start", "lurq_profile_read", "lurq_profile_end"];

pub(crate) fn registered_tools() -> Vec<RegisteredTool> {
  [
    (TOOL_NAMES[0], BuiltinTool::ProfileStart,
      "Start an independent bounded CPU profiling session. No redraw or UI roundtrip. Requires perf_profile; overlapping sessions are independent.",
      json!({"type":"object", "additionalProperties":false, "properties":{
        "max_samples":{"type":"integer","minimum":1,"maximum":MAX_SAMPLES_PER_SESSION,"default":SessionOptions::default().max_samples}
      }})),
    (TOOL_NAMES[1], BuiltinTool::ProfileRead,
      "Read an immutable profiling session snapshot, including unfinished current phases, without waiting for the UI. Without id, read build availability and limits.",
      json!({"type":"object", "additionalProperties":false, "properties":{"id":{"type":"string"}}})),
    (TOOL_NAMES[2], BuiltinTool::ProfileEnd,
      "End and export only the named profiling session. Other sessions continue. Unfinished/boundary-crossing work is explicit; CPU submit/present are not GPU execution.",
      json!({"type":"object", "additionalProperties":false, "properties":{"id":{"type":"string"}}, "required":["id"]})),
  ].into_iter().map(|(name, builtin, description, schema)| RegisteredTool {
    name: name.into(), description: description.into(), scope: Scope::Observe,
    read_only: true, input_schema: schema, kind: ToolKind::Builtin(builtin),
  }).collect()
}

pub(crate) fn execute(shared: &McpShared, builtin: BuiltinTool, args: &Value) -> McpToolResult {
  let name = match builtin {
    BuiltinTool::ProfileStart => TOOL_NAMES[0],
    BuiltinTool::ProfileRead => TOOL_NAMES[1],
    BuiltinTool::ProfileEnd => TOOL_NAMES[2],
    _ => return Err("not a profiling tool".into()),
  };
  // Lock ownership across the permission recheck and the operation. Revocation
  // then cancels every MCP-owned session, including a racing start.
  let mut owned = shared.profile_sessions.lock().unwrap();
  if !shared.is_enabled() || !shared.has_scope(&Scope::Observe) || shared.is_denied(name) {
    return Err(format!("tool {name} is not currently available"));
  }
  let handle = shared.profiling.read().unwrap().clone();
  if builtin == BuiltinTool::ProfileRead && args.get("id").is_none() {
    return Ok(McpToolOutput::Json(json!({
      "schema_version":1, "status":if cfg!(feature="perf_profile") {"available"} else {"feature_disabled"},
      "build":handle.availability().to_json(), "max_active_sessions":MAX_ACTIVE_SESSIONS,
      "max_samples_per_session":MAX_SAMPLES_PER_SESSION, "max_tracked_windows":MAX_TRACKED_WINDOWS,
      "max_window_id_bytes":MAX_WINDOW_ID_BYTES, "requests_redraw":false
    })));
  }
  if builtin == BuiltinTool::ProfileStart {
    let max_samples = match args.get("max_samples") {
      None => SessionOptions::default().max_samples,
      Some(value) => value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| ProfileError::InvalidSampleLimit.to_string())?,
    };
    let started = handle
      .start(SessionOptions {
        max_samples,
        include_devtools: shared
          .include_profile_devtools
          .load(std::sync::atomic::Ordering::Relaxed),
      })
      .map_err(|error| error.to_string())?;
    owned.insert(started.id);
    return Ok(McpToolOutput::Json(started.to_json()));
  }
  let id = parse_id(args)?;
  if !owned.contains(&id) {
    // Never disclose or end sessions owned by an in-process DevTools consumer.
    return Err(if shared.profile_ended.lock().unwrap().contains(&id) {
      "MCP profiling session already ended or revoked".into()
    } else {
      "unknown MCP profiling session id".into()
    });
  }
  let report = if builtin == BuiltinTool::ProfileEnd {
    let report = handle.end(id).map_err(|error| error.to_string())?;
    owned.remove(&id);
    remember_ended(shared, id);
    report
  } else {
    handle.read(id).map_err(|error| error.to_string())?
  };
  drop(owned);
  Ok(McpToolOutput::Json(report.to_json()))
}

fn parse_id(args: &Value) -> Result<SessionId, String> {
  args
    .get("id")
    .and_then(Value::as_str)
    .and_then(|id| id.strip_prefix("profile_"))
    .and_then(|id| id.parse::<u64>().ok())
    .filter(|id| *id > 0)
    .map(SessionId)
    .ok_or_else(|| "id must be a profiling id returned by lurq_profile_start".into())
}

pub(crate) fn permission_changed(shared: &McpShared) {
  let mut owned = shared.profile_sessions.lock().unwrap();
  if shared.is_enabled() && shared.has_scope(&Scope::Observe) && TOOL_NAMES.iter().all(|name| !shared.is_denied(name)) {
    return;
  }
  let handle = shared.profiling.read().unwrap().clone();
  for id in owned.drain() {
    let _ = handle.end(id);
    remember_ended(shared, id);
  }
}

fn remember_ended(shared: &McpShared, id: SessionId) {
  let mut ended = shared.profile_ended.lock().unwrap();
  ended.push_back(id);
  while ended.len() > MAX_ENDED_SESSION_IDS {
    ended.pop_front();
  }
}

#[cfg(test)]
mod tests;
