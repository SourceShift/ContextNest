//! Minimal JSON-RPC 2.0 wire types for the MCP stdio transport.
//!
//! We hand-roll the protocol instead of pulling in `rmcp`: Phase 1 only
//! needs `initialize` + `tools/list` + `tools/call` + the `initialized`
//! notification, which is a few dozen lines and adds no dependency tree to
//! the single binary. `reqwest` + `serde_json` (already in the deps graph)
//! cover everything. Swap to `rmcp` only if a later phase needs
//! server-initiated requests (sampling / elicitation), which none of the
//! query tools require.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A decoded JSON-RPC request line. `id` absent ⇒ this is a *notification*
/// (e.g. `notifications/initialized`) and MUST NOT be answered.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// Standard JSON-RPC 2.0 error codes used by this server.
pub const PARSE_ERROR: i32 = -32700;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Value, code: i32, message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data,
            }),
        }
    }
}
