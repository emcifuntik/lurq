//! The streamable-HTTP MCP server: one background thread with a small tokio
//! runtime, an rmcp `ServerHandler` bridging tool calls onto the event loop
//! through the request channel, and mandatory bearer-token auth.

use std::{
  sync::{Arc, mpsc as std_mpsc},
  time::Duration,
};

use http_body_util::{BodyExt, combinators::BoxBody};
use hyper::body::Bytes;
use rmcp::{
  ErrorData as McpError, ServerHandler,
  model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, JsonObject, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
  },
  service::RequestContext,
  transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
  },
};

use super::{
  registry::{BuiltinTool, RegisteredTool, ToolKind, ToolRegistry},
  sessions::SessionLease,
  shared::{McpRequest, McpShared, McpToolOutput},
};

#[cfg(test)]
mod profiling_tests;

/// Per-tool reply deadline: how long the HTTP side waits for the event loop
/// to execute and resolve a call before reporting a stall. Frame-deferred
/// tools get more headroom. A call still queued when its deadline passes is
/// dropped by [`Tree::drain_mcp_requests`](crate::app::Tree::drain_mcp_requests)
/// instead of running after its caller was told it failed.
fn tool_timeout(tool: &RegisteredTool, args: &serde_json::Value) -> Duration {
  match tool.kind {
    ToolKind::Builtin(BuiltinTool::Wait) => {
      let requested = args
        .get("timeout_ms")
        .and_then(|value| value.as_u64())
        .unwrap_or(10_000);
      Duration::from_millis(requested.min(115_000) + 5_000)
    }
    ToolKind::Builtin(BuiltinTool::Screenshot) => Duration::from_secs(30),
    _ => Duration::from_secs(15),
  }
}

/// lurq's tools for one MCP client session: the rmcp [`ServerHandler`] of lurq's
/// own server, and what a host app's server mounts or delegates to when the
/// tools are [hosted](super::McpConfig::hosted). Get one per session from
/// [`McpHandle::service`](super::McpHandle::service).
///
/// Calls that reach the event loop are executed by
/// [`Tree::drain_mcp_requests`](crate::app::Tree::drain_mcp_requests), which the
/// winit shell runs every loop turn. A call counts as part of a session, so it
/// may hold input, when its request carries `Mcp-Session-Id` in the
/// `hyper::http::request::Parts` that rmcp's streamable-HTTP service puts in
/// the request context.
#[derive(Clone)]
pub struct McpService {
  shared: Arc<McpShared>,
  registry: Arc<ToolRegistry>,
  sender: std_mpsc::Sender<McpRequest>,
  /// The client session this handler serves; rmcp builds a handler for each session.
  session: Arc<SessionLease>,
}

impl McpService {
  pub(crate) fn new(shared: Arc<McpShared>, registry: Arc<ToolRegistry>, sender: std_mpsc::Sender<McpRequest>) -> Self {
    Self {
      session: Arc::new(SessionLease::new(shared.clone())),
      shared,
      registry,
      sender,
    }
  }

  /// The handler for a new client session.
  fn for_new_session(&self) -> Self {
    Self {
      session: Arc::new(SessionLease::new(self.shared.clone())),
      ..self.clone()
    }
  }

  fn visible(&self, tool: &RegisteredTool) -> bool {
    self.shared.is_enabled() && self.shared.has_scope(&tool.scope) && !self.shared.is_denied(&tool.name)
  }

  fn to_mcp_tool(tool: &RegisteredTool) -> Tool {
    let schema: JsonObject = match tool.input_schema.clone() {
      serde_json::Value::Object(map) => map,
      _ => JsonObject::new(),
    };
    Tool::new(tool.name.clone(), tool.description.clone(), schema)
      .with_annotations(ToolAnnotations::new().read_only(tool.read_only).open_world(false))
  }

  fn output_to_result(output: McpToolOutput) -> CallToolResult {
    match output {
      McpToolOutput::Text(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
      McpToolOutput::Json(value) => {
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
        result.structured_content = Some(value);
        result
      }
      McpToolOutput::Image { data, mime } => {
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        CallToolResult::success(vec![ContentBlock::image(encoded, mime)])
      }
    }
  }

  /// Tools answered on the server thread, with no event-loop roundtrip.
  fn serve_locally(&self, tool: &RegisteredTool, args: &serde_json::Value) -> Option<Result<McpToolOutput, String>> {
    match tool.kind {
      ToolKind::Builtin(builtin @ (BuiltinTool::ProfileStart | BuiltinTool::ProfileRead | BuiltinTool::ProfileEnd)) => {
        Some(super::profiling::execute(&self.shared, builtin, args))
      }
      ToolKind::Builtin(BuiltinTool::Find) => {
        let Some(query) = args.get("query").and_then(|value| value.as_str()) else {
          return Some(Err("`query` is required".into()));
        };
        let needle = query.to_lowercase();
        let refs = self.shared.refs.lock().unwrap();
        let mut lines = Vec::new();
        for record in &refs.records {
          let haystack = format!(
            "{} {} {} {} {} {} {}",
            record.tag,
            record.role,
            record.name.as_deref().unwrap_or(""),
            record.element_id.as_deref().unwrap_or(""),
            record.classes.join(" "),
            record.text.as_deref().unwrap_or(""),
            record
              .attrs
              .iter()
              .map(|(name, value)| format!("{name}={value}"))
              .collect::<Vec<_>>()
              .join(" ")
          );
          if haystack.to_lowercase().contains(&needle) {
            lines.push(super::tools::format_ref_line(record));
          }
        }
        if lines.is_empty() {
          Some(Ok(McpToolOutput::Text(format!(
            "No matches for {query:?}. Refs come from the last lurq_inspect or lurq_read_tree per window — call one first or broaden the query."
          ))))
        } else {
          Some(Ok(McpToolOutput::Text(lines.join("\n"))))
        }
      }
      ToolKind::Builtin(BuiltinTool::Logs) => {
        let max_lines = args.get("lines").and_then(|value| value.as_u64()).unwrap_or(200) as usize;
        let filter = args.get("filter").and_then(|value| value.as_str());
        let (installed, lines) = super::logs::recent_lines(max_lines, filter);
        if !installed {
          return Some(Ok(McpToolOutput::Text(
            "Log capture is not installed: the app has not added `lurq::mcp::log_layer()` to its tracing subscriber."
              .into(),
          )));
        }
        Some(Ok(McpToolOutput::Text(if lines.is_empty() {
          "(no matching log lines)".into()
        } else {
          lines.join("\n")
        })))
      }
      _ => None,
    }
  }
}

impl ServerHandler for McpService {
  fn get_info(&self) -> ServerConfig {
    let mut instructions = format!(
      "Embedded MCP server for the lurq app {:?}. Drive and inspect the running UI.\n\
       Conventions:\n\
       - All coordinates and sizes are pixels of the last lurq_screenshot image (physical pixels).\n\
       - Prefer lurq_inspect (optionally with query and role) -> lurq_act by ref -> lurq_inspect \
         to verify. Use lurq_screenshot only when semantic inspection is insufficient.\n\
       - Refs are replaced by each lurq_inspect or lurq_read_tree of the same window; stale refs error.\n\
       - Multi-window: every window-touching tool takes `window` (default \"main\"); \
         list windows with lurq_windows. Ref-based calls need no `window`.",
      self.shared.app_name
    );
    if let Some(extra) = &self.shared.extra_instructions {
      instructions.push_str("\n\n");
      instructions.push_str(extra);
    }

    let mut info = ServerConfig::default();
    info.capabilities = ServerCapabilities::builder().enable_tools().build();
    info.server_info = Implementation::new(
      format!("lurq-{}", self.shared.app_name),
      env!("CARGO_PKG_VERSION").to_owned(),
    );
    info.instructions = Some(instructions);
    info
  }

  async fn list_tools(
    &self,
    _request: Option<PaginatedRequestParams>,
    _context: RequestContext<rmcp::RoleServer>,
  ) -> Result<ListToolsResult, McpError> {
    Ok(ListToolsResult {
      tools: self
        .registry
        .tools
        .iter()
        .filter(|tool| self.visible(tool))
        .map(Self::to_mcp_tool)
        .collect(),
      ..Default::default()
    })
  }

  async fn call_tool(
    &self,
    request: CallToolRequestParams,
    context: RequestContext<rmcp::RoleServer>,
  ) -> Result<CallToolResponse, McpError> {
    let name = request.name.to_string();
    // Tools outside granted scopes are not listed; treat calls to them as
    // unknown rather than leaking their existence.
    let Some(tool) = self.registry.find(&name).filter(|tool| self.visible(tool)) else {
      return Err(McpError::invalid_params(format!("unknown tool: {name}"), None));
    };
    let args = request
      .arguments
      .map(serde_json::Value::Object)
      .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));

    if let Some(result) = self.serve_locally(tool, &args) {
      return Ok(result_to_response(result));
    }

    if let ToolKind::Async(handler) = &tool.kind {
      let result = handler(args).await.map(McpToolOutput::Json);
      return Ok(result_to_response(result));
    }

    let (reply, receiver) = tokio::sync::oneshot::channel();
    let request = McpRequest {
      tool: name.clone(),
      args: args.clone(),
      reply,
      session: in_session(&context).then(|| self.session.route()),
    };
    if self.sender.send(request).is_err() {
      return Err(McpError::internal_error("the app's event loop has shut down", None));
    }
    self.shared.wake();

    match tokio::time::timeout(tool_timeout(tool, &args), receiver).await {
      Ok(Ok(result)) => Ok(result_to_response(result)),
      Ok(Err(_)) => Ok(result_to_response(Err(format!(
        "{name} was dropped before completing (window closed or capture superseded)"
      )))),
      Err(_) => {
        let message = if matches!(tool.kind, ToolKind::Builtin(BuiltinTool::Wait)) {
          format!(
            "{name} timed out: the awaited state never arrived. Continuous animations keep an app \
             from going idle — wait for `frames` instead, or raise timeout_ms."
          )
        } else {
          format!(
            "{name} timed out waiting for the app; the event loop may be blocked. A call the app had not              started is dropped and does not run later."
          )
        };
        Ok(result_to_response(Err(message)))
      }
    }
  }
}

/// Whether a call came through a client session: rmcp routes a request carrying `Mcp-Session-Id` to the handler
/// of that session. Stateless requests carry none and get a handler of their own.
fn in_session(context: &RequestContext<rmcp::RoleServer>) -> bool {
  context
    .extensions
    .get::<hyper::http::request::Parts>()
    .is_some_and(|parts| parts.headers.contains_key("mcp-session-id"))
}

fn result_to_response(result: Result<McpToolOutput, String>) -> CallToolResponse {
  match result {
    Ok(output) => McpService::output_to_result(output).into(),
    Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]).into(),
  }
}

type ServerBody = BoxBody<Bytes, std::io::Error>;

fn text_response(status: hyper::StatusCode, message: &'static str) -> hyper::Response<ServerBody> {
  hyper::Response::builder()
    .status(status)
    .body(
      http_body_util::Full::new(Bytes::from_static(message.as_bytes()))
        .map_err(|never| match never {})
        .boxed(),
    )
    .expect("static response")
}

pub(crate) struct ServerRuntime {
  pub(crate) port: u16,
  pub(crate) join: std::thread::JoinHandle<()>,
  pub(crate) cancel: tokio_util::sync::CancellationToken,
}

/// Spawn the background server thread. Binds `127.0.0.1` only; every request
/// must carry the bearer token from the discovery file. A localhost HTTP
/// server is reachable by any local process — an input-injection endpoint
/// must not be open.
pub(crate) fn spawn(
  shared: Arc<McpShared>,
  registry: Arc<ToolRegistry>,
  sender: std_mpsc::Sender<McpRequest>,
  port: Option<u16>,
) -> Result<ServerRuntime, String> {
  let cancel = tokio_util::sync::CancellationToken::new();
  let cancel_for_thread = cancel.clone();
  let (port_tx, port_rx) = std_mpsc::channel::<Result<u16, String>>();

  let join = std::thread::Builder::new()
    .name("lurq-mcp".into())
    .spawn(move || {
      let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
          let _ = port_tx.send(Err(format!("failed to build MCP tokio runtime: {error}")));
          return;
        }
      };
      runtime.block_on(async move {
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port.unwrap_or(0))).await {
          Ok(listener) => listener,
          Err(error) => {
            let _ = port_tx.send(Err(format!("failed to bind MCP server: {error}")));
            return;
          }
        };
        let bound_port = match listener.local_addr() {
          Ok(addr) => addr.port(),
          Err(error) => {
            let _ = port_tx.send(Err(format!("failed to read MCP server address: {error}")));
            return;
          }
        };
        let _ = port_tx.send(Ok(bound_port));

        let handler = McpService::new(shared.clone(), registry, sender);
        let http_config = StreamableHttpServerConfig::default().with_cancellation_token(cancel_for_thread.clone());
        let service = StreamableHttpService::new(
          move || Ok(handler.for_new_session()),
          Arc::new(LocalSessionManager::default()),
          http_config,
        );
        let token: Arc<str> = shared.token.clone().into();

        loop {
          let (stream, _peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
              Ok(accepted) => accepted,
              Err(error) => {
                tracing::warn!("MCP server accept failed: {error}");
                continue;
              }
            },
            _ = cancel_for_thread.cancelled() => break,
          };
          let io = hyper_util::rt::TokioIo::new(stream);
          let service = service.clone();
          let token = token.clone();
          let connection_cancel = cancel_for_thread.clone();
          tokio::spawn(async move {
            let hyper_service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
              let mut service = service.clone();
              let token = token.clone();
              async move {
                let authorized = req
                  .headers()
                  .get(hyper::header::AUTHORIZATION)
                  .and_then(|value| value.to_str().ok())
                  .and_then(|value| value.strip_prefix("Bearer "))
                  .is_some_and(|presented| constant_time_eq(presented.as_bytes(), token.as_bytes()));
                if !authorized {
                  return Ok::<_, std::convert::Infallible>(text_response(
                    hyper::StatusCode::UNAUTHORIZED,
                    "missing or invalid bearer token; read it from the lurq MCP discovery file",
                  ));
                }
                let response = tower_service::Service::call(&mut service, req)
                  .await
                  .expect("streamable http service is infallible");
                Ok(response.map(|body| body.map_err(std::io::Error::other).boxed()))
              }
            });
            let connection = hyper::server::conn::http1::Builder::new().serve_connection(io, hyper_service);
            tokio::select! {
              result = connection => {
                if let Err(error) = result {
                  tracing::debug!("MCP connection ended: {error}");
                }
              }
              _ = connection_cancel.cancelled() => {}
            }
          });
        }
      });
    })
    .map_err(|error| format!("failed to spawn MCP server thread: {error}"))?;

  match port_rx.recv_timeout(Duration::from_secs(5)) {
    Ok(Ok(port)) => Ok(ServerRuntime { port, join, cancel }),
    Ok(Err(message)) => Err(message),
    Err(_) => Err("MCP server thread did not report a port within 5s".into()),
  }
}

/// Constant-time token comparison; a plain `==` leaks the matching prefix
/// length through timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
  if a.len() != b.len() {
    return false;
  }
  let mut diff = 0u8;
  for (x, y) in a.iter().zip(b) {
    diff |= x ^ y;
  }
  diff == 0
}
