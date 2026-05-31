//! MCP tool definitions and dispatch.
//!
//! Phase 1 (write surface): `cn_store`, `cn_retrieve`, `cn_summarize`
//! proxy the substrate's seven-tool HTTP write/query endpoints.
//!
//! Phase 2 (cross-session read surface): `cn_sessions_list`,
//! `cn_session_summary`, `cn_session_trajectory`, `cn_inbox`, `cn_features`,
//! `cn_prompt_context_atoms` proxy the substrate's read-only GET endpoints
//! so an MCP-speaking agent can ask "what's open / what shipped / which
//! trajectory atoms exist?" natively, without shelling out to `curl`.
//!
//! Phase 3 (search + drill-in): `cn_attention`, `cn_session_get`,
//! `cn_session_find` close the spec's tool list now that the underlying
//! HTTP endpoints exist (PRs #75 + #76). Together they answer "what
//! sessions need attention right now / give me one session's full
//! record / find me the session where I worked on X".
//!
//! Each handler validates required arguments locally (returning a JSON-RPC
//! `INVALID_PARAMS` error before any network call), forwards the rest
//! verbatim, and pretty-prints the substrate's JSON response as MCP text
//! content. Required path parameters (`session_id` in summary/trajectory)
//! are injected into the URL, never into the query string.

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
    json!({
        "tools": [
            // Phase 1 — write / query surface.
            store_def(),
            retrieve_def(),
            summarize_def(),
            // Phase 2 — cross-session read surface.
            sessions_list_def(),
            session_summary_def(),
            session_trajectory_def(),
            inbox_def(),
            features_def(),
            prompt_context_atoms_def(),
            // Phase 3 — attention + drill-in + search.
            attention_def(),
            session_get_def(),
            session_find_def(),
            // Phase 4 — prompt-context aggregation surface.
            prompt_context_clusters_def(),
            prompt_context_capsule_def(),
        ]
    })
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

fn sessions_list_def() -> Value {
    json!({
        "name": "cn_sessions_list",
        "description": "List every session the substrate knows about, newest-first. \
    Returns fragment counts, most-common project_cwd, most-common src_session, \
    and the latest ts per session. Use to discover recent work across projects.",
        "inputSchema": { "type": "object", "properties": {} }
    })
}

fn session_summary_def() -> Value {
    json!({
        "name": "cn_session_summary",
        "description": "Return a single session's high-level summary — kinds histogram, \
    top files touched, key timestamps. Use after `cn_sessions_list` surfaces a candidate.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Session UUID (src_session)." }
            },
            "required": ["session_id"]
        }
    })
}

fn session_trajectory_def() -> Value {
    json!({
        "name": "cn_session_trajectory",
        "description": "Return one session's trajectory: phased goal windows plus the \
    decisions, failures, verifications, risks, prompt directives, and assumptions \
    recorded during it. Includes basin/resonance signals when the substrate has \
    consolidated the session's fragments.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Session UUID (src_session)." }
            },
            "required": ["session_id"]
        }
    })
}

fn inbox_def() -> Value {
    json!({
        "name": "cn_inbox",
        "description": "Return the cross-session attention inbox — todos and user_actions \
    that still need attention, ranked by urgency. Use to discover what's blocking the \
    user across all projects.",
        "inputSchema": { "type": "object", "properties": {} }
    })
}

fn features_def() -> Value {
    json!({
        "name": "cn_features",
        "description": "Return the cross-session feature inventory — named deliverables \
    shipped per session, with files, refs, layer, and replay recipe (how_to_test). Use \
    to answer 'what shipped recently' or 'which session built X'. Pass `format: \
    \"markdown\"` to get a paste-ready Markdown digest instead of JSON.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "since": { "type": "string", "description": "Age window (default 24h, e.g. 7d, 90m)." },
                "layer": { "type": "string", "description": "Optional layer filter (frontend|backend|infra|docs|tests|other)." },
                "project": { "type": "string", "description": "Substring match on project_cwd." },
                "format": { "type": "string", "description": "`json` (default) or `markdown` for a paste-ready prose body." }
            }
        }
    })
}

fn prompt_context_atoms_def() -> Value {
    json!({
        "name": "cn_prompt_context_atoms",
        "description": "Return cross-session trajectory atoms — decisions, failures, \
    verifications, evidence refs, risks, etc. — filterable by kind/project/session/age. \
    The deterministic L1 read layer that feeds prompt-context compilation.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "kind": { "type": "string", "description": "One trajectory kind (decision_made, failure, verification, read_context, evidence_ref, prompt_directive, assumption, artifact, memory_candidate, risk_flag)." },
                "project": { "type": "string", "description": "Substring match on project_cwd." },
                "session_id": { "type": "string", "description": "Exact src_session match." },
                "since": { "type": "string", "description": "Age window (default 30d)." },
                "limit": { "type": "integer", "description": "Max atoms returned (default 200, cap 1000)." }
            }
        }
    })
}

fn attention_def() -> Value {
    json!({
        "name": "cn_attention",
        "description": "Return sessions that need user attention RIGHT NOW — sessions \
    carrying open todos, user_action items, or decisions awaiting input — ranked by \
    recency of their attention-eligible work. Per-session preview shows up to 5 items. \
    Use this as the first call of a session: 'what's blocked across all my projects?'",
        "inputSchema": {
            "type": "object",
            "properties": {
                "project": { "type": "string", "description": "Substring match on project_cwd." },
                "since": { "type": "string", "description": "Age window (default 30d)." },
                "limit": { "type": "integer", "description": "Max sessions returned (default 20, cap 200)." }
            }
        }
    })
}

fn session_get_def() -> Value {
    json!({
        "name": "cn_session_get",
        "description": "Return one session's full grouped detail — every fragment the \
    session produced, grouped by kind in actionable-first order (user_action → todo → \
    decision → trajectory atoms → narrative). Use after `cn_attention` or \
    `cn_session_find` surfaces a candidate session you want to inspect end to end.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Session UUID (src_session)." }
            },
            "required": ["session_id"]
        }
    })
}

fn session_find_def() -> Value {
    json!({
        "name": "cn_session_find",
        "description": "Natural-language session search. Embeds the query, cosine-scores \
    every session's goal_phase + session_title fragments (the kinds that name a session's \
    intent), returns the top sessions ranked by max similarity. Use when you remember \
    WHAT you worked on but not WHEN.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Natural-language query (required)." },
                "project": { "type": "string", "description": "Substring match on project_cwd." },
                "since": { "type": "string", "description": "Age window (default 90d)." },
                "limit": { "type": "integer", "description": "Max sessions returned (default 10, cap 50)." }
            },
            "required": ["query"]
        }
    })
}

fn prompt_context_clusters_def() -> Value {
    json!({
        "name": "cn_prompt_context_clusters",
        "description": "Return cross-session trajectory atoms collapsed by normalized text \
    into clusters. Each cluster carries the unique sessions[] it appeared in, so a cluster \
    spanning multiple sessions is the deterministic 'promotion' signal — same lesson learned \
    twice is a real pattern. Sorted by cross-session reach desc. Set `semantic: true` to also \
    merge paraphrase clusters via embedding cosine (≥0.85); merged clusters carry a \
    `merged_from` array listing the absorbed normalized keys.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "kind": { "type": "string", "description": "One trajectory kind (e.g. decision_made, failure, verification, risk_flag)." },
                "project": { "type": "string", "description": "Substring match on project_cwd." },
                "session_id": { "type": "string", "description": "Exact src_session match." },
                "since": { "type": "string", "description": "Age window (default 30d)." },
                "min_count": { "type": "integer", "description": "Drop clusters below this count (default 2; 1 to include solo atoms)." },
                "limit": { "type": "integer", "description": "Max clusters returned (default 50, cap 500)." },
                "semantic": { "type": "boolean", "description": "When true, additionally merge clusters whose representative embeddings clear cosine ≥ 0.85. Gracefully degrades to deterministic-only when fragment embeddings aren't yet hydrated. Default false." }
            }
        }
    })
}

fn prompt_context_capsule_def() -> Value {
    json!({
        "name": "cn_prompt_context_capsule",
        "description": "Return a Markdown prompt-context capsule synthesising the top trajectory \
    clusters across all sessions, ordered by what a next agent most needs to know first \
    (Risks → Decisions → Failures → Verifications → Evidence → ...). The body is \
    paste-ready into another agent's prompt. Optional `query` is a deterministic substring \
    filter; optional `semantic: true` adds an embedding-based paraphrase merge pass and \
    annotates the body header with `· semantic merge ON`.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Case-insensitive substring filter on normalized cluster text. Omit to include all." },
                "project": { "type": "string", "description": "Substring match on project_cwd." },
                "session_id": { "type": "string", "description": "Exact src_session match." },
                "since": { "type": "string", "description": "Age window (default 30d)." },
                "min_count": { "type": "integer", "description": "Drop clusters below this count (default 2)." },
                "max_per_kind": { "type": "integer", "description": "Cap clusters listed per kind (default 5, cap 25)." },
                "semantic": { "type": "boolean", "description": "When true, additionally merge clusters whose representative embeddings clear cosine ≥ 0.85. Gracefully degrades when embeddings aren't yet hydrated. Default false." }
            }
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
        // Phase 1.
        "cn_store" => call_store(http, base_url, args).await,
        "cn_retrieve" => call_retrieve(http, base_url, args).await,
        "cn_summarize" => call_summarize(http, base_url, args).await,
        // Phase 2.
        "cn_sessions_list" => call_sessions_list(http, base_url).await,
        "cn_session_summary" => call_session_summary(http, base_url, args).await,
        "cn_session_trajectory" => call_session_trajectory(http, base_url, args).await,
        "cn_inbox" => call_inbox(http, base_url).await,
        "cn_features" => call_features(http, base_url, args).await,
        "cn_prompt_context_atoms" => call_prompt_context_atoms(http, base_url, args).await,
        // Phase 3.
        "cn_attention" => call_attention(http, base_url, args).await,
        "cn_session_get" => call_session_get(http, base_url, args).await,
        "cn_session_find" => call_session_find(http, base_url, args).await,
        // Phase 4.
        "cn_prompt_context_clusters" => call_prompt_context_clusters(http, base_url, args).await,
        "cn_prompt_context_capsule" => call_prompt_context_capsule(http, base_url, args).await,
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

async fn call_sessions_list(http: &Client, base: &str) -> Result<String, ToolError> {
    get(http, base, "/api/v1/sessions", &[]).await
}

async fn call_session_summary(http: &Client, base: &str, args: Value) -> Result<String, ToolError> {
    let session_id = require_str(&args, "session_id", "cn_session_summary")?;
    let path = format!("/api/v1/sessions/{}/summary", urlencode(&session_id));
    get(http, base, &path, &[]).await
}

async fn call_session_trajectory(
    http: &Client,
    base: &str,
    args: Value,
) -> Result<String, ToolError> {
    let session_id = require_str(&args, "session_id", "cn_session_trajectory")?;
    let path = format!("/api/v1/sessions/{}/trajectory", urlencode(&session_id));
    get(http, base, &path, &[]).await
}

async fn call_inbox(http: &Client, base: &str) -> Result<String, ToolError> {
    get(http, base, "/api/v1/inbox", &[]).await
}

async fn call_features(http: &Client, base: &str, args: Value) -> Result<String, ToolError> {
    // `format=markdown` returns text/markdown; the shared `get` helper's
    // JSON-or-raw-text fallback delivers the Markdown body verbatim
    // (same passthrough that `cn_prompt_context_capsule` relies on).
    let q = collect_query(&args, &["since", "layer", "project", "format"]);
    get(http, base, "/api/v1/features", &q).await
}

async fn call_prompt_context_atoms(
    http: &Client,
    base: &str,
    args: Value,
) -> Result<String, ToolError> {
    let q = collect_query(&args, &["kind", "project", "session_id", "since", "limit"]);
    get(http, base, "/api/v1/prompt-context/atoms", &q).await
}

async fn call_attention(http: &Client, base: &str, args: Value) -> Result<String, ToolError> {
    let q = collect_query(&args, &["project", "since", "limit"]);
    get(http, base, "/api/v1/sessions/attention", &q).await
}

async fn call_session_get(http: &Client, base: &str, args: Value) -> Result<String, ToolError> {
    let session_id = require_str(&args, "session_id", "cn_session_get")?;
    let path = format!("/api/v1/sessions/{}", urlencode(&session_id));
    get(http, base, &path, &[]).await
}

async fn call_session_find(http: &Client, base: &str, args: Value) -> Result<String, ToolError> {
    let query = require_str(&args, "query", "cn_session_find")?;
    let mut body = json!({ "query": query });
    forward(&mut body, &args, &["project", "since", "limit"]);
    post(http, base, "/api/v1/sessions/find", body).await
}

async fn call_prompt_context_clusters(
    http: &Client,
    base: &str,
    args: Value,
) -> Result<String, ToolError> {
    let q = collect_query(
        &args,
        &[
            "kind",
            "project",
            "session_id",
            "since",
            "min_count",
            "limit",
            "semantic",
        ],
    );
    get(http, base, "/api/v1/prompt-context/clusters", &q).await
}

async fn call_prompt_context_capsule(
    http: &Client,
    base: &str,
    args: Value,
) -> Result<String, ToolError> {
    // Capsule returns text/markdown; the shared `get` helper's "JSON parse,
    // else raw text" fallback delivers the Markdown body through verbatim.
    let q = collect_query(
        &args,
        &[
            "query",
            "project",
            "session_id",
            "since",
            "min_count",
            "max_per_kind",
            "semantic",
        ],
    );
    get(http, base, "/api/v1/prompt-context/capsule", &q).await
}

/// Encode a path segment so a stray `/` or `?` in a `session_id` can't
/// re-route the request. Minimal — only the chars that change Axum routing
/// or URL parsing. (`session_id` is a UUID in normal flow, but the MCP
/// agent can pass anything, so this is defence-in-depth.)
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Pluck the present keys from `args` and stringify their values as query
/// pairs. Numbers serialize via their JSON form (`5`, not `"5"`), strings
/// drop their quotes, anything else (arrays/objects) goes through
/// `to_string` and lands in the URL — the substrate will reject malformed
/// inputs with its own 400.
fn collect_query(args: &Value, keys: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for k in keys {
        if let Some(v) = args.get(*k) {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Null => continue,
                other => other.to_string(),
            };
            out.push((k.to_string(), s));
        }
    }
    out
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
    finalize(url, resp).await
}

/// GET `<base><path>?<query>`; same response handling as `post`. Empty
/// `query` means no `?` is appended.
async fn get(
    http: &Client,
    base: &str,
    path: &str,
    query: &[(String, String)],
) -> Result<String, ToolError> {
    let url = format!("{base}{path}");
    let mut req = http.get(&url);
    if !query.is_empty() {
        req = req.query(query);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| ToolError::Upstream(format!("request to {url} failed: {e}")))?;
    finalize(url, resp).await
}

async fn finalize(url: String, resp: reqwest::Response) -> Result<String, ToolError> {
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
    fn list_tools_advertises_all_phase_tools() {
        let listed = list_tools();
        let tools = listed["tools"].as_array().expect("tools array");
        assert_eq!(
            tools.len(),
            14,
            "3 Phase-1 + 6 Phase-2 + 3 Phase-3 + 2 Phase-4 = 14 tools"
        );
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        for expected in [
            "cn_store",
            "cn_retrieve",
            "cn_summarize",
            "cn_sessions_list",
            "cn_session_summary",
            "cn_session_trajectory",
            "cn_inbox",
            "cn_features",
            "cn_prompt_context_atoms",
            "cn_attention",
            "cn_session_get",
            "cn_session_find",
            "cn_prompt_context_clusters",
            "cn_prompt_context_capsule",
        ] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
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

    #[tokio::test]
    async fn session_summary_without_session_id_is_bad_arguments() {
        let http = Client::new();
        let err = call_tool(&http, "http://127.0.0.1:1", "cn_session_summary", json!({}))
            .await
            .expect_err("missing session_id must fail before any network call");
        assert!(matches!(err, ToolError::BadArguments(_)));
    }

    #[tokio::test]
    async fn session_trajectory_without_session_id_is_bad_arguments() {
        let http = Client::new();
        let err = call_tool(
            &http,
            "http://127.0.0.1:1",
            "cn_session_trajectory",
            json!({}),
        )
        .await
        .expect_err("missing session_id must fail before any network call");
        assert!(matches!(err, ToolError::BadArguments(_)));
    }

    #[test]
    fn collect_query_flattens_present_keys_only() {
        let args = json!({
            "kind": "decision_made",
            "limit": 50,
            "since": "7d",
            "project": null,           // explicit null is dropped
            "session_id": "s1"
        });
        let q = collect_query(
            &args,
            &["kind", "project", "session_id", "since", "limit", "missing"],
        );
        let map: std::collections::HashMap<_, _> = q.into_iter().collect();
        assert_eq!(map.get("kind").map(String::as_str), Some("decision_made"));
        assert_eq!(map.get("limit").map(String::as_str), Some("50"));
        assert_eq!(map.get("since").map(String::as_str), Some("7d"));
        assert_eq!(map.get("session_id").map(String::as_str), Some("s1"));
        assert!(!map.contains_key("project"));
        assert!(!map.contains_key("missing"));
    }

    #[test]
    fn urlencode_escapes_path_separators() {
        // Realistic uuid stays untouched.
        assert_eq!(
            urlencode("01HXYZ-abcd_1234.test~ok"),
            "01HXYZ-abcd_1234.test~ok"
        );
        // Slash and question mark must be escaped so an attacker-controlled
        // session_id can't rewrite the URL.
        assert_eq!(urlencode("a/b?c"), "a%2Fb%3Fc");
        // Spaces are escaped (not + because we are not encoding form data).
        assert_eq!(urlencode("a b"), "a%20b");
    }

    #[tokio::test]
    async fn sessions_list_takes_no_args() {
        // Tool exists and is dispatched (network call will fail at this host,
        // but reaching that point proves the no-arg call path is wired).
        let http = Client::new();
        let err = call_tool(&http, "http://127.0.0.1:1", "cn_sessions_list", json!({}))
            .await
            .expect_err("nothing is listening on 127.0.0.1:1, so upstream must fail");
        assert!(
            matches!(err, ToolError::Upstream(_)),
            "expected Upstream (network) error, got {err:?}"
        );
    }

    #[test]
    fn features_def_advertises_project_and_format_params() {
        // The inputSchema must list every key the handler forwards via
        // `collect_query`, otherwise an MCP agent inspecting the schema
        // can't discover the new options.
        let listed = list_tools();
        let tools = listed["tools"].as_array().expect("tools array");
        let features = tools
            .iter()
            .find(|t| t["name"] == "cn_features")
            .expect("cn_features advertised");
        let props = features["inputSchema"]["properties"]
            .as_object()
            .expect("properties object");
        for required in ["since", "layer", "project", "format"] {
            assert!(
                props.contains_key(required),
                "cn_features inputSchema missing `{required}`"
            );
        }
    }

    #[tokio::test]
    async fn features_optional_args_round_trip_through_dispatch() {
        let http = Client::new();
        // Pass all four optional args (since/layer/project/format) — they
        // must dispatch and only fail at the network step, proving
        // collect_query forwards every key the new schema advertises.
        let err = call_tool(
            &http,
            "http://127.0.0.1:1",
            "cn_features",
            json!({
                "since": "7d",
                "layer": "backend",
                "project": "ContextNest",
                "format": "markdown",
            }),
        )
        .await
        .expect_err("network must fail at 127.0.0.1:1");
        assert!(matches!(err, ToolError::Upstream(_)));
    }

    #[tokio::test]
    async fn session_get_without_session_id_is_bad_arguments() {
        let http = Client::new();
        let err = call_tool(&http, "http://127.0.0.1:1", "cn_session_get", json!({}))
            .await
            .expect_err("missing session_id must fail before any network call");
        assert!(matches!(err, ToolError::BadArguments(_)));
    }

    #[tokio::test]
    async fn session_find_without_query_is_bad_arguments() {
        let http = Client::new();
        let err = call_tool(
            &http,
            "http://127.0.0.1:1",
            "cn_session_find",
            json!({ "limit": 5 }),
        )
        .await
        .expect_err("missing query must fail before any network call");
        assert!(matches!(err, ToolError::BadArguments(_)));
    }

    #[tokio::test]
    async fn attention_takes_only_optional_args() {
        // No required args; with no args it dispatches and only fails at the
        // network step, proving the no-required path is wired.
        let http = Client::new();
        let err = call_tool(&http, "http://127.0.0.1:1", "cn_attention", json!({}))
            .await
            .expect_err("network must fail at 127.0.0.1:1");
        assert!(matches!(err, ToolError::Upstream(_)));
    }

    #[tokio::test]
    async fn prompt_context_clusters_dispatches_with_optional_args() {
        let http = Client::new();
        let err = call_tool(
            &http,
            "http://127.0.0.1:1",
            "cn_prompt_context_clusters",
            json!({ "kind": "decision_made", "min_count": 3, "limit": 10 }),
        )
        .await
        .expect_err("network must fail at 127.0.0.1:1");
        assert!(matches!(err, ToolError::Upstream(_)));
    }

    #[tokio::test]
    async fn prompt_context_capsule_dispatches_with_optional_args() {
        let http = Client::new();
        let err = call_tool(
            &http,
            "http://127.0.0.1:1",
            "cn_prompt_context_capsule",
            json!({ "query": "auth", "since": "7d" }),
        )
        .await
        .expect_err("network must fail at 127.0.0.1:1");
        assert!(matches!(err, ToolError::Upstream(_)));
    }
}
