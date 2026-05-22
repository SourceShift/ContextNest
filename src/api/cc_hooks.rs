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

/// What we remember about a session between hook fires: the byte offset
/// we've already ingested up to, plus the last-known transcript path
/// and project cwd.
///
/// Storing the path lets the [`spawn_sweeper`] background task re-tail
/// a session even when no hook fires — defence in depth against
/// dropped/lost hook calls (e.g. Claude killed mid-`curl`, substrate
/// restarting through the curl-retry window, sessions abandoned after
/// the last successful `tail_and_ingest`).
#[derive(Clone, Debug, Default)]
struct TrackedSession {
    offset: u64,
    transcript_path: Option<PathBuf>,
    cwd: Option<String>,
}

/// Per-session byte offset tracker for incremental `.jsonl` tailing.
///
/// Wrap with `Arc` and install via `.layer(Extension(Arc::new(...)))`.
/// All access is async (RwLock) — hook handlers hold the lock for
/// microseconds only, never across a `.jsonl` read.
#[derive(Default)]
pub struct SessionTracker {
    inner: tokio::sync::RwLock<HashMap<String, TrackedSession>>,
}

impl SessionTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self, session_id: &str) -> u64 {
        self.inner
            .read()
            .await
            .get(session_id)
            .map(|t| t.offset)
            .unwrap_or(0)
    }

    /// Set the byte offset for a session without touching the cached
    /// transcript_path / cwd. Used by `session_start` (which doesn't
    /// carry a transcript path) and by the truncation-recovery path.
    pub async fn set(&self, session_id: &str, offset: u64) {
        let mut guard = self.inner.write().await;
        let entry = guard.entry(session_id.to_string()).or_default();
        entry.offset = offset;
    }

    /// Record offset + transcript path + cwd in one write. Called by
    /// `tail_and_ingest` on every successful run so the sweeper has
    /// everything it needs to re-tail without a hook.
    pub async fn record(
        &self,
        session_id: &str,
        offset: u64,
        transcript_path: Option<PathBuf>,
        cwd: Option<String>,
    ) {
        let mut guard = self.inner.write().await;
        let entry = guard.entry(session_id.to_string()).or_default();
        entry.offset = offset;
        if transcript_path.is_some() {
            entry.transcript_path = transcript_path;
        }
        if cwd.is_some() {
            entry.cwd = cwd;
        }
    }

    /// Snapshot every session that has a known transcript path.
    /// Returned as `(session_id, transcript_path, cwd)` tuples so the
    /// sweeper can drive `tail_and_ingest` over an unchanging copy and
    /// release the lock immediately.
    pub async fn sweepable(&self) -> Vec<(String, PathBuf, Option<String>)> {
        self.inner
            .read()
            .await
            .iter()
            .filter_map(|(sid, t)| {
                t.transcript_path
                    .as_ref()
                    .map(|p| (sid.clone(), p.clone(), t.cwd.clone()))
            })
            .collect()
    }

    /// Best-effort introspection for tests / debug.
    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
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

    // Two interesting cases when `total_len < last_offset`:
    //
    // 1. **Truncation / rotation.** Some workflows (notably Claude
    //    Code's `/clear` in versions that re-use the same session_id,
    //    or a manual file rotation) shrink the transcript out from
    //    under us. If we silently early-return here we will never
    //    catch up; the offset is permanently past the file end. The
    //    only safe move is to reset the offset to 0 and re-read.
    //
    //    We accept the cost: any z-insight blocks that survived the
    //    truncation may re-ingest (duplicating the substrate side),
    //    but the dashboard's inbox dedup at view time absorbs that.
    //    A duplicate fragment is far less harmful than a permanently-
    //    silent session.
    //
    // 2. **No new content** (total_len == last_offset). File hasn't
    //    grown; nothing to do.
    if total_len < last_offset {
        tracing::warn!(
            session_id = %payload.session_id,
            path = %transcript_path.display(),
            old_offset = last_offset,
            new_len = total_len,
            "cc_hooks: transcript shrank (likely truncation or /clear); resetting offset to 0",
        );
        tracker.set(&payload.session_id, 0).await;
        // Fall through with last_offset effectively reset; the
        // alignment block below treats 0 as "start of file".
    } else if total_len == last_offset {
        // No new content — fast no-op.
        return Ok(());
    }
    let last_offset = if total_len < last_offset {
        0
    } else {
        last_offset
    };

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

    // Stash the transcript path + cwd alongside the new offset so the
    // sweeper can re-tail this session even if the next hook never fires
    // (Claude killed mid-curl, session abandoned, server restart, etc).
    tracker
        .record(
            &payload.session_id,
            total_len,
            Some(transcript_path.clone()),
            payload
                .cwd
                .clone()
                .or_else(|| metadata.cwd.clone())
                .or_else(|| {
                    if project_cwd.is_empty() {
                        None
                    } else {
                        Some(project_cwd.to_string())
                    }
                }),
        )
        .await;
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
    .with_meta("src_session", json!(session_uuid))
    // Stamp a timestamp so this accomplishment sorts alongside z-insight
    // accomplishments instead of falling to the bottom of any ts-ordered
    // view. The hook payload doesn't carry an event time, so the
    // server's clock is the best we have — accuracy is bounded by hook
    // delivery latency, which is sub-second in practice.
    .with_meta(
        "ts",
        json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    );
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

/// Periodic background sweep: re-tail every tracked session's transcript.
///
/// This is the "pull" half of a deliberate push+pull delivery design.
/// The hook protocol ([`dispatch`]) is the push path — fast, real-time,
/// usually reliable. But it can drop a payload (Claude killed mid-curl
/// before the retries complete, substrate down for longer than the curl
/// retry window, session abandoned before a Stop fires). The sweeper
/// recovers from any such drop on the next tick.
///
/// Cost is bounded by [`tail_and_ingest`]'s own short-circuit at
/// `total_len == last_offset` — a session with no new transcript bytes
/// since the last successful run pays one stat + one read of the file
/// header and nothing else.
///
/// Spawn via [`spawn_sweeper`] at server bootstrap. Lives for the
/// lifetime of the runtime.
pub async fn sweep_once(services: &ContextNestServices, tracker: &SessionTracker) {
    let snapshot = tracker.sweepable().await;
    if snapshot.is_empty() {
        return;
    }
    tracing::debug!(
        sessions = snapshot.len(),
        "cc_hooks: sweeper running tail-and-ingest"
    );
    for (session_id, transcript_path, cwd) in snapshot {
        let payload = HookPayload {
            session_id,
            cwd,
            transcript_path: Some(transcript_path),
            hook_event_name: Some("Sweep".to_string()),
            extra: HashMap::new(),
        };
        if let Err(e) = tail_and_ingest(services, tracker, &payload).await {
            // Sweeper errors are debug-level: they're already covered
            // by the regular hook path; a transient sweep failure just
            // means the next tick (or next real hook) will pick up.
            tracing::debug!(
                session_id = %payload.session_id,
                error = %e,
                "cc_hooks: sweeper tail_and_ingest skipped",
            );
        }
    }
}

/// Spawn the periodic sweeper. Idempotent only in the sense that calling
/// twice doubles the work — bootstrap should call this once.
pub fn spawn_sweeper(
    services: ContextNestServices,
    tracker: Arc<SessionTracker>,
    interval: std::time::Duration,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // Skip missed ticks (don't burst-catch-up). The sweep is
        // already a cheap no-op when nothing changed, but bursting
        // after a long pause adds no value over the next normal tick.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // First tick fires immediately; consume it so the first sweep
        // happens `interval` after spawn, giving normal hook traffic
        // a chance to be the source of truth on warm startups.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            sweep_once(&services, &tracker).await;
        }
    });
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
    async fn tracker_record_keeps_path_and_cwd_for_sweeper() {
        let t = SessionTracker::new();
        t.record(
            "sid-a",
            42,
            Some(PathBuf::from("/tmp/sid-a.jsonl")),
            Some("/work/repo".to_string()),
        )
        .await;
        assert_eq!(t.get("sid-a").await, 42);
        let sweepable = t.sweepable().await;
        assert_eq!(sweepable.len(), 1);
        assert_eq!(sweepable[0].0, "sid-a");
        assert_eq!(sweepable[0].1, PathBuf::from("/tmp/sid-a.jsonl"));
        assert_eq!(sweepable[0].2.as_deref(), Some("/work/repo"));
    }

    #[tokio::test]
    async fn tracker_set_does_not_clobber_path() {
        // Regression guard: `session_start` uses `set` to register a
        // session at offset 0 before any transcript path is known.
        // A subsequent `tail_and_ingest` call records the path via
        // `record`. If a later `set` (e.g. truncation reset) wiped the
        // path, the sweeper would lose the session.
        let t = SessionTracker::new();
        t.record("sid-a", 100, Some(PathBuf::from("/tmp/sid-a.jsonl")), None)
            .await;
        t.set("sid-a", 0).await; // simulate truncation reset
        let sweepable = t.sweepable().await;
        assert_eq!(sweepable.len(), 1, "path must survive a bare set()");
        assert_eq!(sweepable[0].1, PathBuf::from("/tmp/sid-a.jsonl"));
        assert_eq!(t.get("sid-a").await, 0);
    }

    #[tokio::test]
    async fn tracker_sweepable_skips_sessions_with_no_path() {
        // session_start fires before any tail_and_ingest; that session
        // is in the tracker but has no transcript path yet, so the
        // sweeper cannot act on it. It MUST be excluded from
        // `sweepable` rather than crash a sweep tick.
        let t = SessionTracker::new();
        t.set("session-start-only", 0).await;
        t.record("fully-known", 10, Some(PathBuf::from("/tmp/x.jsonl")), None)
            .await;
        let sweepable = t.sweepable().await;
        assert_eq!(sweepable.len(), 1);
        assert_eq!(sweepable[0].0, "fully-known");
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
