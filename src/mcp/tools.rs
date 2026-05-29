//! MCP tool definitions and dispatch for Phase 1.
//!
//! Three thin proxies over the substrate's seven-tool HTTP surface:
//! `cn_store` → POST /api/v1/tools/store, `cn_retrieve` → .../retrieve,
//! `cn_summarize` → .../summarize. Each handler validates required
//! arguments locally (returning a JSON-RPC `INVALID_PARAMS` error before
//! any network call), forwards the rest verbatim, and pretty-prints the
//! substrate's JSON response as MCP text content.

use reqwest::Client;
use serde_json::{json, Value};

/// Failure modes a tool call can hit. `UnknownTool` / `BadArguments` are
/// caller mistakes (mapped to JSON-RPC protocol errors); `Upstream` is a
/// runtime substrate failure (surfaced as a tool result with `isError`,
/// per the MCP convention that execution errors stay inside the result).
#[derive(Debug)]
pub enum ToolError {
    UnknownTool(String),
    BadArguments(String),
    Upstream(String),
}

/// The `tools/list` payload: `{ "tools": [ {name, description, inputSchema} ] }`.
pub fn list_tools() -> Value {
    json!({ "tools": [store_def(), retrieve_def(), summarize_def()] })
}

fn store_def() -> Value {
    json!({
        "name": "cn_store",
        "description": "Persist a memory fragment into the ContextNest substrate. \
    Use for durable facts, decisions, or learnings the agent should be able to recall \
    in future sessions.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "content": { "type": "string", "description": "The memory text to store." },
                "importance": { "type": "number", "description": "Optional 0..1 salience weight." },
                "session_id": { "type": "string", "description": "Optional owning session id." },
                "metadata": { "type": "object", "description": "Optional free-form key/value metadata." }
            },
            "required": ["content"]
        }
    })
}

fn retrieve_def() -> Value {
    json!({
        "name": "cn_retrieve",
        "description": "Recall the most relevant stored memories for a natural-language \
    query. Returns scored hits with their similarity, importance, and metadata.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Natural-language query." },
                "top_k": { "type": "integer", "description": "Max hits to return (default 5)." },
                "session_id": { "type": "string", "description": "Optional single session to scope to." },
                "metadata_filter": {
                    "type": "object",
                    "description": "Optional exact-match metadata filter, e.g. {\"kind\":\"decision\"}."
                }
            },
            "required": ["query"]
        }
    })
}

fn summarize_def() -> Value {
    json!({
        "name": "cn_summarize",
        "description": "Consolidate a session's memories into a compact summary attractor. \
    Returns how many fragments were merged and the summary id.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Session to summarize." },
                "target_tokens": { "type": "integer", "description": "Optional target summary size." }
            },
            "required": ["session_id"]
        }
    })
}

/// Dispatch a `tools/call` to the matching handler.
pub async fn call_tool(
    http: &Client,
    base_url: &str,
    name: &str,
    args: Value,
) -> Result<String, ToolError> {
    match name {
        "cn_store" => call_store(http, base_url, args).await,
        "cn_retrieve" => call_retrieve(http, base_url, args).await,
        "cn_summarize" => call_summarize(http, base_url, args).await,
        other => Err(ToolError::UnknownTool(other.to_string())),
    }
}

async fn call_store(http: &Client, base: &str, args: Value) -> Result<String, ToolError> {
    let content = require_str(&args, "content", "cn_store")?;
    let mut body = json!({ "content": content });
    forward(&mut body, &args, &["importance", "session_id", "metadata"]);
    post(http, base, "/api/v1/tools/store", body).await
}

async fn call_retrieve(http: &Client, base: &str, args: Value) -> Result<String, ToolError> {
    let query = require_str(&args, "query", "cn_retrieve")?;
    let mut body = json!({ "query": query });
    forward(
        &mut body,
        &args,
        &["top_k", "session_id", "metadata_filter"],
    );
    post(http, base, "/api/v1/tools/retrieve", body).await
}

async fn call_summarize(http: &Client, base: &str, args: Value) -> Result<String, ToolError> {
    let session_id = require_str(&args, "session_id", "cn_summarize")?;
    let mut body = json!({ "session_id": session_id });
    forward(&mut body, &args, &["target_tokens"]);
    post(http, base, "/api/v1/tools/summarize", body).await
}

/// Pull a required string argument or fail with a precise message.
fn require_str(args: &Value, key: &str, tool: &str) -> Result<String, ToolError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ToolError::BadArguments(format!("{tool} requires '{key}' (string)")))
}

/// Copy each present optional field from `args` into the request `body`.
fn forward(body: &mut Value, args: &Value, keys: &[&str]) {
    for key in keys {
        if let Some(v) = args.get(*key) {
            body[*key] = v.clone();
        }
    }
}

/// POST `body` to `<base><path>`; pretty-print a JSON response, or pass the
/// raw text through. Non-2xx is an `Upstream` error carrying the status +
/// body so the agent sees exactly what the substrate said.
async fn post(http: &Client, base: &str, path: &str, body: Value) -> Result<String, ToolError> {
    let url = format!("{base}{path}");
    let resp = http
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| ToolError::Upstream(format!("request to {url} failed: {e}")))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| {
        ToolError::Upstream(format!("reading response body from {url} failed: {e}"))
    })?;
    if !status.is_success() {
        return Err(ToolError::Upstream(format!(
            "substrate {url} returned {status}: {text}"
        )));
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => Ok(serde_json::to_string_pretty(&v).unwrap_or(text)),
        Err(_) => Ok(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_tools_advertises_three_named_tools() {
        let listed = list_tools();
        let tools = listed["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 3);
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"cn_store"));
        assert!(names.contains(&"cn_retrieve"));
        assert!(names.contains(&"cn_summarize"));
        // Each tool must carry an object inputSchema with a properties map.
        for t in tools {
            assert_eq!(t["inputSchema"]["type"], "object");
            assert!(t["inputSchema"]["properties"].is_object());
        }
    }

    #[tokio::test]
    async fn store_without_content_is_bad_arguments() {
        let http = Client::new();
        let err = call_tool(&http, "http://127.0.0.1:1", "cn_store", json!({}))
            .await
            .expect_err("missing content must fail before any network call");
        match err {
            ToolError::BadArguments(msg) => assert!(msg.contains("content")),
            other => panic!("expected BadArguments, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn retrieve_without_query_is_bad_arguments() {
        let http = Client::new();
        let err = call_tool(
            &http,
            "http://127.0.0.1:1",
            "cn_retrieve",
            json!({"top_k": 3}),
        )
        .await
        .expect_err("missing query must fail");
        assert!(matches!(err, ToolError::BadArguments(_)));
    }

    #[tokio::test]
    async fn unknown_tool_name_is_unknown_tool() {
        let http = Client::new();
        let err = call_tool(&http, "http://127.0.0.1:1", "cn_bogus", json!({}))
            .await
            .expect_err("unknown tool must fail");
        match err {
            ToolError::UnknownTool(n) => assert_eq!(n, "cn_bogus"),
            other => panic!("expected UnknownTool, got {other:?}"),
        }
    }

    #[test]
    fn forward_copies_only_present_keys() {
        let mut body = json!({ "content": "x" });
        let args = json!({ "importance": 0.9, "session_id": "s1" });
        forward(&mut body, &args, &["importance", "session_id", "metadata"]);
        assert_eq!(body["importance"], 0.9);
        assert_eq!(body["session_id"], "s1");
        assert!(body.get("metadata").is_none());
    }
}
