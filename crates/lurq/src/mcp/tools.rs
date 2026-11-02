//! Built-in tool execution. Everything here runs on the event-loop thread
//! during the shell's drain (or a headless harness's explicit drain), with
//! full `&mut Tree` / `&mut App` access — the same powers and the same
//! no-blocking constraint as event handlers.

#[cfg(test)]
mod abandoned_tests;
#[cfg(test)]
mod attachments_tests;
mod canvas_items;
pub(crate) mod file_dialogs;
mod held;
#[cfg(test)]
mod held_tests;
#[cfg(test)]
mod input_source_tests;
mod inspect;
mod interact;
mod lookup;
mod navigate;
mod read_tree;
mod resolve;
mod screenshot;
mod semantics;
#[cfg(test)]
mod sensitive_screenshot_tests;
#[cfg(test)]
mod sensitive_tests;
mod set_value;
#[cfg(test)]
mod tests;
mod wait;
mod windows;
#[cfg(test)]
mod windows_tests;

#[cfg(all(test, feature = "canvas"))]
mod canvas_items_tests;
#[cfg(all(test, feature = "canvas"))]
mod canvas_visibility_tests;

pub(crate) use held::{release_all_holds, release_unreachable_holds};
use inspect::{act_tool, inspect_tool};
use interact::interact_tool;
use lookup::{find_by_class_tool, find_by_id_tool};
use navigate::navigate_tool;
pub(crate) use read_tree::format_ref_line;
use read_tree::read_tree_tool;
use screenshot::screenshot_tool;
use set_value::set_value_tool;
use wait::wait_tool;
use windows::{menu_json, resize_tool, windows_tool};

use crate::{
  app::{App, Tree},
  mcp::{
    McpState, McpToolCtx,
    registry::{BuiltinTool, ToolKind},
    sessions::SessionId,
    shared::{McpReply, McpRequest, McpToolOutput},
  },
};

pub(crate) fn execute(tree: &mut Tree, app: &mut App, state: &McpState, request: McpRequest) {
  let McpRequest {
    tool,
    args,
    reply,
    session,
  } = request;
  let Some(registered) = state.registry.find(&tool) else {
    let _ = reply.send(Err(format!("unknown tool: {tool}")));
    return;
  };
  // Scopes are runtime-mutable; re-check at execution time, not just listing.
  if !state.shared.is_enabled() || !state.shared.has_scope(&registered.scope) || state.shared.is_denied(&tool) {
    let _ = reply.send(Err(format!("tool {tool} is not currently available")));
    return;
  }

  match &registered.kind {
    ToolKind::Builtin(builtin) => execute_builtin(*builtin, tree, app, state, args, reply, session),
    ToolKind::Sync(handler) => {
      let mut ctx = McpToolCtx { tree, app };
      let result = handler(&mut ctx, args).map(McpToolOutput::Json);
      let _ = reply.send(result);
    }
    // Async tools run on the server runtime and never cross the channel.
    ToolKind::Async(_) => {
      let _ = reply.send(Err(format!("tool {tool} is async and must not be routed to the app")));
    }
  }
}

fn execute_builtin(
  builtin: BuiltinTool,
  tree: &mut Tree,
  app: &mut App,
  state: &McpState,
  args: serde_json::Value,
  reply: McpReply,
  session: Option<SessionId>,
) {
  let _ = app;
  match builtin {
    BuiltinTool::FileDialogs | BuiltinTool::FileDialogRespond => {
      let _ = reply.send(file_dialogs::execute(builtin, tree, state, &args));
    }
    BuiltinTool::Menu => {
      let model = app.shared.menu.model();
      let _ = reply.send(Ok(McpToolOutput::Json(serde_json::json!({
        "support": format!("{:?}", app.menu_bar_support()),
        "model": model.as_ref().map(menu_json),
      }))));
    }
    BuiltinTool::Windows => {
      let _ = reply.send(windows_tool(tree, state));
    }
    BuiltinTool::ReadTree => {
      let _ = reply.send(read_tree_tool(tree, state, &args));
    }
    BuiltinTool::Inspect => {
      let _ = reply.send(inspect_tool(tree, state, &args));
    }
    BuiltinTool::FindById => {
      let _ = reply.send(find_by_id_tool(tree, state, &args));
    }
    BuiltinTool::FindByClass => {
      let _ = reply.send(find_by_class_tool(tree, state, &args));
    }
    BuiltinTool::Screenshot => screenshot_tool(tree, state, &args, reply),
    BuiltinTool::Wait => wait_tool(tree, state, &args, reply),
    BuiltinTool::Interact => {
      let _ = reply.send(interact_tool(tree, app, state, &args, session));
    }
    BuiltinTool::Act => {
      let _ = reply.send(act_tool(tree, app, state, &args));
    }
    BuiltinTool::SetValue => {
      let _ = reply.send(set_value_tool(tree, state, &args));
    }
    BuiltinTool::Resize => resize_tool(tree, state, &args, reply),
    BuiltinTool::Navigate => {
      let _ = reply.send(navigate_tool(state, &args));
    }
    // Served on the server thread; never routed here.
    BuiltinTool::Find
    | BuiltinTool::Logs
    | BuiltinTool::ProfileStart
    | BuiltinTool::ProfileRead
    | BuiltinTool::ProfileEnd => {
      let _ = reply.send(Err("this tool is served without an app roundtrip".into()));
    }
  }
}
