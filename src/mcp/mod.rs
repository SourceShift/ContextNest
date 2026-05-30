//! Model Context Protocol server (stdio transport).
//!
//! Exposes ContextNest's memory tools to any MCP-speaking agent (Claude
//! Code, Cursor, Zed, ...) so the agent can `cn_store` / `cn_retrieve` /
//! `cn_summarize` natively instead of shelling out to `curl`. The server
//! is a stdio subprocess: it reads newline-delimited JSON-RPC 2.0 messages
//! from stdin and writes responses to stdout. **stdout is the protocol
//! channel — nothing else may be written there** (logs go to stderr; see
//! `init_logging` in the binary).
//!
//! Phase 1 ships three tools. Sessions / trajectory / inbox tools land in
//! later phases (see `docs/roadmap/epics/cc-ingest/E-mcp-server.md`).

pub mod protocol;
pub mod tools;

use protocol::{JsonRpcRequest, JsonRpcResponse, INVALID_PARAMS, METHOD_NOT_FOUND, PARSE_ERROR};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub const SERVER_NAME: &str = "contextnest";

/// Protocol version we advertise when a client omits its own. We otherwise
/// echo the client's requested version (lenient negotiation), so this is
/// only a fallback for non-conformant clients.
pub const FALLBACK_PROTOCOL_VERSION: &str = "2025-11-25";

/// A stdio MCP server bound to one substrate base URL.
pub struct McpServer {
    base_url: String,
    http: reqwest::Client,
}

impl McpServer {
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self {
            base_url,
            http: reqwest::Client::new(),
        }
    }

    /// Run the read→dispatch→write loop until stdin closes (client exit).
    pub async fn serve_stdio(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        let mut stdout = tokio::io::stdout();
        tracing::info!(base_url = %self.base_url, "MCP stdio server started");
        while let Some(line) = lines.next_line().await? {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(resp) = self.handle_line(line).await {
                let mut bytes = serde_json::to_vec(&resp)?;
                bytes.push(b'\n');
                stdout.write_all(&bytes).await?;
                stdout.flush().await?;
            }
        }
        tracing::info!("MCP stdin closed; shutting down");
        Ok(())
    }

    /// Decode one line and produce a response, or `None` for notifications
    /// (which JSON-RPC forbids answering).
    async fn handle_line(&self, line: &str) -> Option<JsonRpcResponse> {
        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                return Some(JsonRpcResponse::error(
                    Value::Null,
                    PARSE_ERROR,
                    format!("parse error: {e}"),
                    None,
                ));
            }
        };
        let is_notification = req.id.is_none();
        let id = req.id.clone().unwrap_or(Value::Null);
        match req.method.as_str() {
            "initialize" => Some(self.handle_initialize(id, &req.params)),
            "notifications/initialized" => None,
            "ping" => Some(JsonRpcResponse::success(id, json!({}))),
            "tools/list" => Some(JsonRpcResponse::success(id, tools::list_tools())),
            "tools/call" => Some(self.handle_tools_call(id, &req.params).await),
            other => {
                if is_notification {
                    None
                } else {
                    Some(JsonRpcResponse::error(
                        id,
                        METHOD_NOT_FOUND,
                        format!("method not found: {other}"),
                        None,
                    ))
                }
            }
        }
    }

    fn handle_initialize(&self, id: Value, params: &Value) -> JsonRpcResponse {
        let version = params
            .get("protocolVersion")
            .and_then(|v| v.as_str())
            .unwrap_or(FALLBACK_PROTOCOL_VERSION)
            .to_string();
        JsonRpcResponse::success(
            id,
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "instructions": "ContextNest memory substrate. cn_store persists a memory, \
            cn_retrieve recalls relevant memories for a query, cn_summarize consolidates a session."
            }),
        )
    }

    async fn handle_tools_call(&self, id: Value, params: &Value) -> JsonRpcResponse {
        let name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n.to_string(),
            None => {
                return JsonRpcResponse::error(
                    id,
                    INVALID_PARAMS,
                    "tools/call requires a 'name'",
                    None,
                )
            }
        };
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        match tools::call_tool(&self.http, &self.base_url, &name, arguments).await {
            Ok(text) => JsonRpcResponse::success(
                id,
                json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
            ),
            Err(tools::ToolError::UnknownTool(n)) => {
                JsonRpcResponse::error(id, INVALID_PARAMS, format!("unknown tool: {n}"), None)
            }
            Err(tools::ToolError::BadArguments(msg)) => {
                JsonRpcResponse::error(id, INVALID_PARAMS, msg, None)
            }
            // Execution failures stay inside the result with isError=true so
            // the model can read and react to them, per MCP convention.
            Err(tools::ToolError::Upstream(msg)) => JsonRpcResponse::success(
                id,
                json!({ "content": [{ "type": "text", "text": msg }], "isError": true }),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> McpServer {
        McpServer::new("http://127.0.0.1:1")
    }

    #[tokio::test]
    async fn initialize_echoes_client_protocol_version() {
        let resp = server()
            .handle_line(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{}}}"#,
            )
            .await
            .expect("initialize must respond");
        let result = resp.result.expect("result");
        assert_eq!(result["protocolVersion"], "2025-03-26");
        assert_eq!(result["serverInfo"]["name"], "contextnest");
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn initialize_falls_back_when_version_absent() {
        let resp = server()
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
            .await
            .expect("respond");
        assert_eq!(
            resp.result.unwrap()["protocolVersion"],
            FALLBACK_PROTOCOL_VERSION
        );
    }

    #[tokio::test]
    async fn initialized_notification_gets_no_response() {
        let resp = server()
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .await;
        assert!(resp.is_none(), "notifications must not be answered");
    }

    #[tokio::test]
    async fn tools_list_returns_all_phase_tools() {
        let resp = server()
            .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .await
            .expect("respond");
        let tools = resp.result.unwrap()["tools"].as_array().unwrap().len();
        assert_eq!(tools, 12, "3 Phase-1 + 6 Phase-2 + 3 Phase-3 = 12 tools");
    }

    #[tokio::test]
    async fn unknown_method_is_method_not_found() {
        let resp = server()
            .handle_line(r#"{"jsonrpc":"2.0","id":3,"method":"resources/list"}"#)
            .await
            .expect("respond");
        assert_eq!(resp.error.unwrap().code, METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn unknown_notification_is_silently_dropped() {
        let resp = server()
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/cancelled"}"#)
            .await;
        assert!(resp.is_none());
    }

    #[tokio::test]
    async fn malformed_line_is_parse_error() {
        let resp = server().handle_line("not json").await.expect("respond");
        assert_eq!(resp.error.unwrap().code, PARSE_ERROR);
    }

    #[tokio::test]
    async fn tools_call_missing_name_is_invalid_params() {
        let resp = server()
            .handle_line(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{}}"#)
            .await
            .expect("respond");
        assert_eq!(resp.error.unwrap().code, INVALID_PARAMS);
    }

    #[tokio::test]
    async fn tools_call_bad_arguments_is_invalid_params() {
        let resp = server()
            .handle_line(
                r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"cn_store","arguments":{}}}"#,
            )
            .await
            .expect("respond");
        assert_eq!(resp.error.unwrap().code, INVALID_PARAMS);
    }

    #[tokio::test]
    async fn ping_returns_empty_result() {
        let resp = server()
            .handle_line(r#"{"jsonrpc":"2.0","id":6,"method":"ping"}"#)
            .await
            .expect("respond");
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }
}
