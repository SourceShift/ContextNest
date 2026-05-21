//! Real-time Claude Code hook receiver.
//!
//! Mounts five fire-and-forget HTTP endpoints under `/api/v1/cc/hook/`:
//!
//! | Path | Hook event | Behaviour |
//! |------|-----------|-----------|
//! | `/session_start` | `SessionStart` | Register session in [`SessionTracker`] at offset 0 |
//! | `/user_prompt_submit` | `UserPromptSubmit` | Tail the `.jsonl` from last byte offset, extract memories, push |
//! | `/stop` | `Stop` | Same tail-and-push behaviour. Final pass per turn-group. |
//! | `/task_completed` | `TaskCompleted` | Store one `accomplishment` memory directly (no `.jsonl` read) |
//! | `/subagent_stop` | `SubagentStop` | Logged only; reserved for follow-up |
//!
//! Every endpoint returns `204 No Content` within milliseconds. The
//! ingest work happens in a [`tokio::spawn`] task so a slow disk read or
//! a substrate hiccup never blocks Claude's hook loop. Claude's hook
//! framework already invokes the curl in the background (`... &`), so on
//! top of that this is "two layers of don't-block-the-user".
//!
//! ## Byte-offset tailing
//!
//! [`SessionTracker`] keeps an in-memory `session_id → byte_offset` map.
//! On each `user_prompt_submit`/`stop` we read the whole `.jsonl`, skip
//! the bytes already processed, parse only the rest, and bump the offset
//! to the new file length. Restarts wipe the tracker — the next hook
//! re-ingests one session from byte 0; downstream phase-clustering
//! re-emits any goal phase that crosses the gap. We accept that
//! over-write rather than persist tracker state to disk; the substrate's
//! own dedupe is the canonical correctness layer (currently a TODO; see
//! the `Out of scope` section in the PR body).
//!
//! ## Failure modes
//!
//! - Missing `transcript_path` on a tail event → silent skip (no write,
//!   no error). Some hook events legitimately don't carry one.
//! - Malformed `.jsonl` chunk → parser logs and skips the bad lines; the
//!   rest of the chunk still ingests.
//! - Substrate-side store error → logged at `warn`. The handler still
//!   returned 204 to Claude, so retries would be the caller's job — but
//!   the next hook fires anyway and the file gets re-tailed from the
//!   same offset (which got bumped to the post-attempt length), so a
//!   transient failure costs one batch of memories, not the whole
//!   session. Persistent failures (e.g. embedding service down) will
//!   reliably miss data — that's a substrate-level alarm, not a hook
//!   concern.

use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    routing::post,
    Extension, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::ingest::claude_code::{
    event::parse_session_string, extract_memories, MemoryKind, MemoryRecord, ServicesSink, Sink,
};
use crate::services::ContextNestServices;

/// Per-session byte offset tracker for incremental `.jsonl` tailing.
///
/// Wrap with `Arc` and install via `.layer(Extension(Arc::new(...)))`.
/// All access is async (RwLock) — hook handlers hold the lock for
/// microseconds only, never across a `.jsonl` read.
#[derive(Default)]
pub struct SessionTracker {
    offsets: tokio::sync::RwLock<HashMap<String, u64>>,
}

impl SessionTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self, session_id: &str) -> u64 {
        self.offsets
            .read()
            .await
            .get(session_id)
            .copied()
            .unwrap_or(0)
    }

    pub async fn set(&self, session_id: &str, offset: u64) {
        self.offsets
            .write()
            .await
            .insert(session_id.to_string(), offset);
    }

    /// Best-effort introspection for tests / debug.
    pub async fn len(&self) -> usize {
        self.offsets.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.offsets.read().await.is_empty()
    }
}

/// Minimum payload shape we use from a Claude Code hook event. All
/// fields are optional because the hook framework's payloads vary by
/// event type (e.g. `task_completed` doesn't carry a `transcript_path`).
/// `#[serde(flatten)] extra` catches any other fields so future-Claude
/// can add fields without breaking us.
#[derive(Debug, Clone, Deserialize)]
pub struct HookPayload {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<PathBuf>,
    #[serde(default)]
    pub hook_event_name: Option<String>,
    #[serde(flatten, default)]
    pub extra: HashMap<String, Value>,
}

/// Build the cc-hooks router. Expects the wrapping app to
/// `.layer(Extension(Arc::new(SessionTracker::new())))` so handlers can
/// pull the tracker via the `Extension` extractor, while keeping the
/// shared `State<ContextNestServices>` typed the same as the tools
/// router (no second state type, no router rewrite).
pub fn create_cc_hooks_router() -> Router<ContextNestServices> {
    Router::new().route("/api/v1/cc/hook/:event", post(dispatch))
}

/// Single dispatch entry — Claude Code's hook config points at this
/// path per event, with `<event>` being one of the five names below.
/// Doing one route + path-extraction (rather than five routes) keeps
/// the install-hooks command's surface tiny: one curl command template.
async fn dispatch(
    State(services): State<ContextNestServices>,
    Extension(tracker): Extension<Arc<SessionTracker>>,
    Path(event): Path<String>,
    Json(payload): Json<HookPayload>,
) -> StatusCode {
    match event.as_str() {
        "session_start" => {
            let sid = payload.session_id.clone();
            tokio::spawn(async move {
                tracker.set(&sid, 0).await;
            });
        }
        // user_prompt_submit, stop, and subagent_stop all share the same
        // ingest path: read whatever's new in the (sub)session's transcript
        // since the last fired event and extract memories from it.
        //
        // Why subagent_stop is now wired (was a no-op pre-fix): spawned
        // subagents (via the Task tool) have their own JSONL transcript
        // and their own z-insight blocks. Ignoring them meant inbox
        // never saw the actionable items they produced — exactly the
        // gap the researcher autopilot's todos[] currently work around
        // by emitting from the main session.
        //
        // The SessionTracker keys on `session_id`, which is the
        // subagent's own session id distinct from the parent's, so
        // offsets don't collide.
        "user_prompt_submit" | "stop" | "subagent_stop" => {
            let services = services.clone();
            tokio::spawn(async move {
                if let Err(e) = tail_and_ingest(&services, &tracker, &payload).await {
                    tracing::warn!(?event, error = %e, "cc_hooks: tail_and_ingest failed");
                }
            });
        }
        "task_completed" => {
            let services = services.clone();
            tokio::spawn(async move {
                if let Err(e) = store_task_completion(&services, &payload).await {
                    tracing::warn!(error = %e, "cc_hooks: store_task_completion failed");
                }
            });
        }
        other => {
            tracing::warn!(event = %other, "cc_hooks: unknown event, ignoring");
            return StatusCode::NOT_FOUND;
        }
    }
    StatusCode::NO_CONTENT
}

/// Read the transcript .jsonl from the session's last byte offset,
/// parse the new content, extract memories, push them to the substrate
/// via [`ServicesSink`]. Updates the tracker's offset to the new file
/// length on success.
async fn tail_and_ingest(
    services: &ContextNestServices,
    tracker: &SessionTracker,
    payload: &HookPayload,
) -> Result<(), String> {
    let Some(transcript_path) = payload.transcript_path.as_ref() else {
        // No transcript path — nothing to tail. Common for hook events
        // dispatched on session lifecycle changes that don't carry a path.
        return Ok(());
    };

    let bytes = match tokio::fs::read(transcript_path).await {
        Ok(b) => b,
        // Transcript file isn't there yet (or was rotated/deleted out from
        // under us). Claude Code occasionally fires `user_prompt_submit`
        // with a path that hasn't been flushed, or the session may have
        // been pruned by the user. Treat as no-op — same shape as the
        // "no transcript_path" early-return above. We deliberately do NOT
        // bump the offset tracker: if the file reappears later (rare but
        // possible on rename-into-place), we'll pick it up from byte 0.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!(
                path = %transcript_path.display(),
                "cc_hooks: transcript not found, skipping",
            );
            return Ok(());
        }
        Err(e) => return Err(format!("read {}: {}", transcript_path.display(), e)),
    };
    let total_len = bytes.len() as u64;

    let last_offset = tracker.get(&payload.session_id).await;
    if total_len <= last_offset {
        // File hasn't grown since last read. The hook may have fired on
        // a non-content event (rare but possible). Nothing to do.
        return Ok(());
    }

    // Slice the new portion. `last_offset` was always set to the full
    // post-read file length, so we should land at a line boundary —
    // but be defensive: if we somehow start mid-line (e.g. file got
    // truncated externally), skip to the first '\n' so the parser
    // doesn't choke on a partial JSON line.
    let start = last_offset as usize;
    let tail = &bytes[start..];
    let aligned: &[u8] = if last_offset == 0 {
        tail
    } else if !tail.starts_with(b"{") && !tail.starts_with(b"\n") {
        // Mid-line — skip to first newline.
        match tail.iter().position(|&b| b == b'\n') {
            Some(idx) => &tail[idx + 1..],
            None => &[], // no newline left — nothing parseable in this window
        }
    } else {
        tail
    };
    let chunk = std::str::from_utf8(aligned)
        .map_err(|e| format!("non-utf8 chunk at offset {}: {}", last_offset, e))?;

    let (events, metadata) = parse_session_string(chunk);
    if events.is_empty() {
        // Bump the offset so we don't keep re-reading the same chunk.
        tracker.set(&payload.session_id, total_len).await;
        return Ok(());
    }

    let session_uuid = payload.session_id.as_str();
    let project_cwd = payload
        .cwd
        .as_deref()
        .or(metadata.cwd.as_deref())
        .unwrap_or("");

    let records = extract_memories(&events, session_uuid, project_cwd);
    if !records.is_empty() {
        let sink = ServicesSink::new(services.clone());
        sink.store_batch(&records)
            .await
            .map_err(|e| format!("ServicesSink::store_batch: {e}"))?;
    }

    tracker.set(&payload.session_id, total_len).await;
    Ok(())
}

/// Store one `accomplishment` memory record built from the
/// `TaskCompleted` hook payload. The payload's exact shape varies — we
/// pull subject/id from a handful of common field names and degrade
/// gracefully if none are present.
async fn store_task_completion(
    services: &ContextNestServices,
    payload: &HookPayload,
) -> Result<(), String> {
    let subject = first_str(
        &payload.extra,
        &["subject", "task_subject", "title", "name"],
    )
    .or_else(|| {
        payload
            .extra
            .get("task")
            .and_then(|v| v.get("subject"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    });
    let task_id = first_str(&payload.extra, &["task_id", "id", "taskId"]);

    let Some(subject) = subject else {
        // No subject — nothing useful to remember. Don't pollute the
        // substrate with "(no subject)" rows.
        return Ok(());
    };

    let session_uuid = &payload.session_id;
    let cn_sid = if session_uuid.is_empty() {
        "cc-unknown".to_string()
    } else {
        format!("cc-{}", &session_uuid[..session_uuid.len().min(8)])
    };

    let mut record = MemoryRecord::new(
        MemoryKind::Accomplishment,
        format!("Completed: {subject}"),
        cn_sid,
    )
    .with_meta("kind", json!("accomplishment"))
    .with_meta("source", json!("TaskCompleted"))
    .with_meta("src_session", json!(session_uuid));
    if let Some(cwd) = payload.cwd.as_deref() {
        record = record.with_meta("project_cwd", json!(cwd));
    }
    if let Some(id) = task_id {
        record = record.with_meta("task_id", json!(id));
    }

    let sink = ServicesSink::new(services.clone());
    sink.store(&record)
        .await
        .map_err(|e| format!("ServicesSink::store: {e}"))
}

/// First string-valued entry from `extras` matching any of `keys`.
/// Helper for chasing Claude Code's drift between field-name variants.
fn first_str(extras: &HashMap<String, Value>, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = extras.get(*k).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tracker_round_trip() {
        let t = SessionTracker::new();
        assert!(t.is_empty().await);
        assert_eq!(t.get("sid-a").await, 0);
        t.set("sid-a", 12345).await;
        assert_eq!(t.get("sid-a").await, 12345);
        assert_eq!(t.get("sid-b").await, 0);
        assert_eq!(t.len().await, 1);
    }

    #[tokio::test]
    async fn first_str_finds_value_in_any_alias() {
        let mut extras: HashMap<String, Value> = HashMap::new();
        extras.insert("taskId".to_string(), json!("T-7"));
        assert_eq!(
            first_str(&extras, &["task_id", "id", "taskId"]),
            Some("T-7".to_string())
        );
        assert_eq!(first_str(&extras, &["nope", "still_nope"]), None);
    }

    #[tokio::test]
    async fn first_str_skips_empty_strings() {
        let mut extras: HashMap<String, Value> = HashMap::new();
        extras.insert("subject".to_string(), json!(""));
        extras.insert("title".to_string(), json!("Real title"));
        // Empty `subject` skipped, falls through to `title`.
        assert_eq!(
            first_str(&extras, &["subject", "title"]),
            Some("Real title".to_string())
        );
    }

    #[tokio::test]
    async fn payload_deserializes_with_unknown_fields() {
        let raw = json!({
            "session_id": "abc",
            "cwd": "/work",
            "transcript_path": "/tmp/abc.jsonl",
            "hook_event_name": "UserPromptSubmit",
            "future_unknown_field": {"nested": [1, 2, 3]},
            "tool_input": {"prompt": "..."}
        });
        let payload: HookPayload = serde_json::from_value(raw).expect("permissive deserialize");
        assert_eq!(payload.session_id, "abc");
        assert_eq!(payload.cwd.as_deref(), Some("/work"));
        assert!(payload.transcript_path.is_some());
        assert!(payload.extra.contains_key("future_unknown_field"));
        assert!(payload.extra.contains_key("tool_input"));
    }
}
