//! Synchronous `/api/v1/coord/turn` endpoint — Claude Code's
//! per-turn principal hook.
//!
//! ## Wire contract
//!
//! Every Claude Code `SessionStart` / `UserPromptSubmit` fires one
//! synchronous `curl` at this endpoint. The endpoint binds the session
//! to a stable Concord principal (explicit header, else lineage
//! `session:<term>@<repo>`, else `session:<session_id>`), then delivers
//! that principal's undelivered mailbox at most once through
//! `hookSpecificOutput.additionalContext`. The endpoint ALWAYS answers
//! `200 OK`, even on malformed bodies or store errors — a hook that
//! returns a non-200 would inject garbage into Claude's context window
//! (the JSON body would be surfaced verbatim by Claude Code), and a
//! 4xx/timeout would block the user's prompt.
//!
//! ## Identity
//!
//! Three sources of identity, in priority order:
//!
//! 1. `X-Concord-Principal: <id>` header — must pass
//!    `validate_principal_id`. Invalid values fall through silently so a
//!    typo in one hook call doesn't unbind the session for the rest of
//!    the run.
//! 2. Lineage id `session:<term>@<repo_slug>` derived from the tmux
//!    pane (e.g. `%94` → `p94`) and the cwd's nearest `.git` ancestor.
//!    Either pane or tty can produce a term; pane wins.
//!    Tty alone is fragile on headless loop workers (`ps -o tty=` prints
//!    `??` on macOS, `?` on Linux) so the helper rejects those values
//!    and falls through.
//! 3. `session:<sanitized session_id>` when the lineage id cannot be built.
//!
//! With an empty session_id the handler binds nothing and returns 200
//! with `bound=false` and empty context — never an error.
//!
//! ## Pretool gate
//!
//! `pretool_gate` in `cc_hooks.rs` resolves the same binding via
//! `CoordStore::get_binding(&req.session_id)` and passes the bound
//! `principal_id` as the lease agent. An unbound session falls back to
//! `req.session_id` so the existing behaviour is preserved for sessions
//! that haven't fired a turn yet.
//!
//! ## Scope
//!
//! `coord.rs` (the lease plane) is NOT touched. The identity swap
//! happens at the `cc_hooks.rs` call site, by passing a different
//! `agent_id` string to `lease_decision`.

use axum::{extract::State, http::HeaderMap, response::Json, routing::post, Router};
use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use tracing::{error, warn};

use crate::api::coord_footprints::glob_match;
use crate::api::coord_topics;
use crate::services::coord_store::{
    validate_principal_id, CoordStore, CoordStoreResult, Message, PrincipalUpsert, TurnDigest,
};
use crate::services::ContextNestServices;

/// Maximum number of distinct paths surfaced in one turn digest. The
/// brief caps per-path lines at 5; when more qualify, the renderer
/// appends a `(+(N) more)` summary line.
pub const DIGEST_MAX_PATHS: usize = 5;

// ───────────────────────── pure resolution helpers ─────────────────────────

/// Strip any character outside `[A-Za-z0-9._-]` to `-`, then cap at
/// `max_chars` characters (no Unicode char-boundary panic).
pub fn sanitize_slug(s: &str, max: usize) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.chars().count() <= max {
        cleaned
    } else {
        cleaned.chars().take(max).collect()
    }
}

/// `TMUX_PANE` looks like `%94` (digits after `%`). Anything else
/// (empty, missing `%`, non-digit trailing chars) is rejected so a
/// future tmux format change degrades to the session-id fallback
/// rather than minting colliding ids.
pub fn pane_term(pane: &str) -> Option<String> {
    let trimmed = pane.trim();
    if !trimmed.starts_with('%') {
        return None;
    }
    let digits = &trimmed[1..];
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("p{digits}"))
}

/// `ps -o tty= -p $PPID` prints `??` (macOS) or `?` (Linux) for a
/// process with no controlling tty — the typical `claude --print`
/// headless loop worker. Without the explicit `?`/`-` rejection, every
/// headless worker would collapse onto one principal,
/// `session:tty---@<repo>`, and share one mailbox. After taking the
/// basename, an empty result or a result made only of `?`/`-` chars is
/// rejected.
pub fn tty_term(tty: &str) -> Option<String> {
    let trimmed = tty.trim();
    if trimmed.is_empty() {
        return None;
    }
    let base = trimmed.rsplit('/').next().unwrap_or(trimmed);
    if base.is_empty() {
        return None;
    }
    // Every char must be either `?` or `-` to be rejected. Any
    // alphanumeric/symbol char makes it usable.
    let all_dash_or_q = base.chars().all(|c| c == '?' || c == '-');
    if all_dash_or_q {
        return None;
    }
    Some(format!("tty-{}", sanitize_slug(base, 32)))
}

/// Walk `cwd.ancestors()` looking for the first directory that contains
/// a `.git` (file OR dir, so worktrees count). Fall back to cwd's own
/// basename. Sanitize to at most 64 chars.
///
/// An empty cwd or a cwd whose ancestors all lack `.git` (e.g. `"/"`)
/// collapses to a sanitized empty/basename string — never an error,
/// because the helper's caller will fall through to the session-id
/// branch on a validate failure.
pub fn repo_slug(cwd: &Path) -> String {
    let found = cwd.ancestors().find(|dir| dir.join(".git").exists());
    let base = match found {
        Some(dir) => dir,
        None => cwd,
    };
    let raw = base.file_name().and_then(|n| n.to_str()).unwrap_or("");
    sanitize_slug(raw, 64)
}

/// Resolve a stable principal id for one hook call.
///
/// Priority:
///   1. `explicit` header value, if `validate_principal_id` accepts it.
///   2. Lineage `session:<term>@<repo_slug>` using pane first, then
///      tty. Accepted only if the composed id passes
///      `validate_principal_id` (≤128 char limit).
///   3. `session:<sanitized session_id>` (truncated to 64 chars).
///
/// Returns `None` only when `session_id` is empty (so the handler can
/// return 200 with `bound=false` without ever touching the store).
pub fn resolve_principal(
    explicit: Option<&str>,
    pane: Option<&str>,
    tty: Option<&str>,
    cwd: &Path,
    session_id: &str,
) -> Option<(String, bool)> {
    if let Some(explicit) = explicit {
        if !explicit.is_empty() && validate_principal_id(explicit).is_ok() {
            return Some((explicit.to_string(), true));
        }
    }
    let slug = repo_slug(cwd);
    let term = pane.and_then(pane_term).or_else(|| tty.and_then(tty_term));
    if let Some(term) = term {
        let candidate = if slug.is_empty() {
            format!("session:{term}")
        } else {
            format!("session:{term}@{slug}")
        };
        if validate_principal_id(&candidate).is_ok() {
            return Some((candidate, false));
        }
    }
    if session_id.is_empty() {
        return None;
    }
    let fallback = format!("session:{}", sanitize_slug(session_id, 64));
    if validate_principal_id(&fallback).is_ok() {
        Some((fallback, false))
    } else {
        None
    }
}

/// Render the `additionalContext` body Claude Code surfaces verbatim
/// to the model. Empty mailbox → empty string (a no-op the hook passes
/// through without injecting anything into context).
///
/// Format, byte-for-byte to match the brief:
///   - 0 messages: ""
///   - N messages:
///     - `[concord] N message(s) for <pid>`     ("message" when N==1)
///     - `- <M-id> from <from> (<RFC3339 Z>): <body>`
///     - `Ack: mini-ork concord ack <pid> <M-id>`     (per message)
pub fn render_context(principal_id: &str, messages: &[Message]) -> String {
    if messages.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "[concord] {} {} for {}",
        messages.len(),
        if messages.len() == 1 {
            "message"
        } else {
            "messages"
        },
        principal_id,
    );
    for m in messages {
        let ts = m.created_at.to_rfc3339_opts(SecondsFormat::Secs, true);
        out.push_str(&format!(
            "\n- {} from {} ({}): {}",
            m.msg_id, m.from, ts, m.body
        ));
        out.push_str(&format!(
            "\nAck: mini-ork concord ack {} {}",
            principal_id, m.msg_id
        ));
    }
    out
}

/// Pure parser behind [`digest_enabled`]. `0`/`false`/`off` (trimmed,
/// case-insensitive) disable the digest; `None` and every other value
/// enable it. Mirrors the `parse_hot_mode`/`hot_mode` split in
/// `coord_footprints.rs:113-140`.
pub fn parse_digest_enabled(raw: Option<&str>) -> bool {
    match raw {
        Some(v) => {
            let t = v.trim().to_ascii_lowercase();
            !matches!(t.as_str(), "0" | "false" | "off")
        }
        None => true,
    }
}

/// Read `CONTEXTNEST_CONCORD_DIGEST` fresh on every call. Defaults to
/// enabled when unset.
pub fn digest_enabled() -> bool {
    parse_digest_enabled(std::env::var("CONTEXTNEST_CONCORD_DIGEST").ok().as_deref())
}

/// Default glob set for paths the per-turn digest drops before the
/// cap and the count. Auto-memory writes (`MEMORY.md` and friends) are
/// the loudest contributor in practice — they accumulate per principal
/// and never affect another agent's plan. The cursor still advances
/// past them, so dropping them is once-only behaviour.
const DEFAULT_DIGEST_IGNORE_GLOBS: &str = "**/.claude/projects/*/memory/**";

/// Pure parser behind [`digest_ignore_globs`]: `None` (unset) → the
/// defaults; a set value is split on ',', trimmed, and stripped of
/// empties (so "" disables ignores). Unit tests call this directly —
/// mutating the process env from parallel test threads raced.
pub fn parse_digest_ignore_globs(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or(DEFAULT_DIGEST_IGNORE_GLOBS)
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// The per-turn digest ignore-glob list, read fresh from
/// `CONTEXTNEST_CONCORD_DIGEST_IGNORE_GLOBS` on every call so test
/// suites can flip it without restarting the binary. Defaults to
/// [`DEFAULT_DIGEST_IGNORE_GLOBS`] when unset; an explicit empty string
/// disables ignores.
pub fn digest_ignore_globs() -> Vec<String> {
    parse_digest_ignore_globs(
        std::env::var("CONTEXTNEST_CONCORD_DIGEST_IGNORE_GLOBS")
            .ok()
            .as_deref(),
    )
}

/// Pure renderer for the turn digest. Returns `(text, path_lines)`:
/// `text` is the body to splice after the mailbox (empty when there
/// are no changes), and `path_lines` is the number of per-path lines
/// emitted — the metric counter excludes the header and the
/// `(+(N) more)` line.
///
/// Exact wire format (byte-for-byte):
///   header: `[concord] changed by other agents since your last turn \
///            (judge whether it affects your plan):`
///   line:   `\n- <path> — <principal_id> (<harness|unknown>, \
///            <cwd basename|unknown>) at <RFC3339 Z>` (U+2014 EM DASH)
///   suffix: ` (+K other writer)` or ` (+K other writers)` when K >= 1
///   tail:   `\n(+N more)` when total_paths > changes.len()
pub fn render_digest(d: &TurnDigest) -> (String, usize) {
    if d.changes.is_empty() {
        return (String::new(), 0);
    }
    let mut out = String::from(
        "[concord] changed by other agents since your last turn (judge whether it affects your plan):",
    );
    for change in &d.changes {
        let harness = change.harness.as_deref().unwrap_or("unknown");
        let cwd = change
            .cwd
            .as_deref()
            .and_then(|c| {
                let p = Path::new(c);
                p.file_name().and_then(|n| n.to_str())
            })
            .unwrap_or("unknown");
        let ts = change.ts.to_rfc3339_opts(SecondsFormat::Secs, true);
        out.push_str(&format!(
            "\n- {} — {} ({}, {}) at {}",
            change.path, change.principal_id, harness, cwd, ts
        ));
        if change.writer_count > 1 {
            let k = change.writer_count - 1;
            let noun = if k == 1 { "writer" } else { "writers" };
            out.push_str(&format!(" (+{k} other {noun})"));
        }
    }
    if d.total_paths > d.changes.len() {
        let more = d.total_paths - d.changes.len();
        out.push_str(&format!("\n(+{more} more)"));
    }
    (out, d.changes.len())
}

// ───────────────────────── turn pipeline ─────────────────────────

/// Parsed hook body. All optional so malformed bodies don't error —
/// every field defaults, the handler still runs.
#[derive(Debug, Default, Deserialize)]
pub struct TurnInput {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub hook_event_name: Option<String>,
    // Catch-all so future-Claude fields don't break us.
    #[serde(flatten, default)]
    pub extra: Value,
}

/// Outcome of one turn pipeline. The handler projects this into JSON.
#[derive(Debug)]
pub struct TurnOutcome {
    pub principal_id: Option<String>,
    pub bound: bool,
    pub delivered: Vec<String>,
    pub hook_event_name: String,
    pub additional_context: String,
    /// Number of per-path digest lines emitted by `run_turn`. The
    /// async handler bumps `coord_digest_lines_total` by this value
    /// after `run_turn` returns (the metric lives behind a tokio
    /// RwLock that `run_turn` — sync — can't acquire). Zero when the
    /// digest is disabled, empty, or this turn isn't a
    /// UserPromptSubmit.
    pub digest_lines: usize,
}

/// Run one turn pipeline synchronously against `store`.
///
/// Steps:
///   1. Resolve principal id.
///   2. Upsert + bind.
///   3. Claim undelivered messages.
///   4. Render context.
///   5. On UserPromptSubmit only: take + render the turn digest, and
///      append it after the mailbox block when non-empty and the
///      `CONTEXTNEST_CONCORD_DIGEST` env var isn't disabled. The
///      cursor ALWAYS advances, even when disabled or empty, so once-only
///      semantics survive a turn that opts out.
///
/// Errors from the store bubble up so the handler can choose its
/// fallback path. `Err(_)` here is mapped to `bound=false`,
/// `delivered=[]`, empty context by `coord_turn`. A digest error
/// (logged at `warn`) degrades to mailbox-only — it does NOT
/// propagate, because `claim_undelivered` has already stamped the
/// messages delivered and a propagated Err would discard them through
/// the handler's `Err → empty context` path.
pub fn run_turn(store: &CoordStore, input: &TurnInputWithHeaders) -> CoordStoreResult<TurnOutcome> {
    let cwd_path = Path::new(input.inner.cwd.as_deref().unwrap_or(""));
    let (principal_id, explicit) = match resolve_principal(
        input.explicit_principal.as_deref(),
        input.tmux_pane.as_deref(),
        input.tty.as_deref(),
        cwd_path,
        &input.inner.session_id,
    ) {
        Some(t) => t,
        None => {
            return Ok(TurnOutcome {
                principal_id: None,
                bound: false,
                delivered: Vec::new(),
                hook_event_name: input.inner.hook_event_name.clone().unwrap_or_default(),
                additional_context: String::new(),
                digest_lines: 0,
            });
        }
    };

    if explicit {
        // Heartbeat: only update `pids` so a stale-session bind doesn't
        // wipe the stored harness/cwd/etc.
        let prior = store.get_principal(&principal_id)?;
        let mut upsert = PrincipalUpsert::default();
        if let Some(pid) = input.pid {
            let existing: Vec<i64> = prior
                .as_ref()
                .and_then(|p| p.pids.clone())
                .unwrap_or_default();
            let mut combined = existing;
            if !combined.contains(&pid) {
                combined.push(pid);
            }
            upsert.pids = Some(combined);
        }
        store.upsert_principal(&principal_id, upsert)?;
    } else {
        // Derived principal: stamp the fields the brief specifies. No
        // `host` (the brief lists exactly which fields to set), so the
        // status computation will report these as `stale` past the
        // TTL — accepted by the brief for P0b.
        let cwd_str = input.inner.cwd.clone().unwrap_or_default();
        let pane_str = input.tmux_pane.clone().unwrap_or_default();
        let pids = input.pid.map(|p| vec![p]);
        store.upsert_principal(
            &principal_id,
            PrincipalUpsert {
                harness: Some("claude-code".into()),
                cwd: Some(cwd_str),
                tmux_pane: Some(pane_str),
                pids,
                labels: Some(json!({"auto": "true"})),
                ..Default::default()
            },
        )?;
    }

    let pid_opt = input.pid;
    store.bind(&input.inner.session_id, &principal_id, pid_opt)?;

    // 10 is a pragmatic cap — keeps additionalContext under Claude's
    // soft limit (80 KiB worst-case at 8 KiB bodies) and matches the
    // brief's "deliver at most once through hookSpecificOutput".
    let messages = store.claim_undelivered(&principal_id, &input.inner.session_id, 10)?;
    let delivered: Vec<String> = messages.iter().map(|m| m.msg_id.clone()).collect();
    let mut additional_context = render_context(&principal_id, &messages);

    let mut digest_lines: usize = 0;
    if input.inner.hook_event_name.as_deref() == Some("UserPromptSubmit") {
        // Always advance the cursor (once-only), even when disabled
        // or empty. Take the digest first; if it fails, log and
        // degrade to mailbox-only — do NOT propagate.
        let principal_id_for_digest = principal_id.clone();
        let ignore_globs = digest_ignore_globs();
        match store.take_turn_digest(&principal_id_for_digest, DIGEST_MAX_PATHS, &|p: &str| {
            ignore_globs.iter().any(|g| glob_match(g, p))
        }) {
            Ok(digest) => {
                if digest_enabled() {
                    let (text, lines) = render_digest(&digest);
                    if !text.is_empty() {
                        if additional_context.is_empty() {
                            additional_context = text;
                        } else {
                            additional_context = format!("{additional_context}\n\n{text}");
                        }
                        digest_lines = lines;
                    }
                }
            }
            Err(e) => {
                warn!(
                    error = %e,
                    principal_id = %principal_id_for_digest,
                    "coord_turn: digest failed"
                );
            }
        }
    }

    Ok(TurnOutcome {
        principal_id: Some(principal_id),
        bound: true,
        delivered,
        hook_event_name: input.inner.hook_event_name.clone().unwrap_or_default(),
        additional_context,
        digest_lines,
    })
}

/// Internal: parsed headers + body in one place so `run_turn` doesn't
/// need to know about axum. The `explicit_principal`, `tmux_pane`,
/// `tty`, `pid` fields are populated from headers; the rest from the
/// JSON body.
#[derive(Debug, Default)]
pub struct TurnInputWithHeaders {
    pub inner: TurnInput,
    pub explicit_principal: Option<String>,
    pub tmux_pane: Option<String>,
    pub tty: Option<String>,
    pub pid: Option<i64>,
}

impl std::ops::Deref for TurnInputWithHeaders {
    type Target = TurnInput;
    fn deref(&self) -> &TurnInput {
        &self.inner
    }
}

// ───────────────────────── handler ─────────────────────────

/// `POST /api/v1/coord/turn` — synchronous Claude Code per-turn hook.
///
/// **Always answers 200.** On a malformed body, a store error, or a
/// `Json<T>` extractor failure, we log at `warn`/`error` and return a
/// structured empty response so Claude Code never sees a 4xx.
pub async fn coord_turn(
    State(services): State<ContextNestServices>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Json<Value> {
    let input = parse_input(&headers, &body);
    let store = services.coord_store.clone();
    let outcome = match run_turn(&store, &input) {
        Ok(o) => o,
        Err(e) => {
            error!(error = %e, "coord_turn: run_turn failed");
            TurnOutcome {
                // Best-effort id for the response only (the binding itself
                // never happened). Resolve it the same way as the success
                // path, so an invalid header is never echoed back.
                principal_id: {
                    resolve_principal(
                        input.explicit_principal.as_deref(),
                        input.tmux_pane.as_deref(),
                        input.tty.as_deref(),
                        Path::new(input.inner.cwd.as_deref().unwrap_or("")),
                        &input.inner.session_id,
                    )
                    .map(|(s, _)| s)
                },
                bound: false,
                delivered: Vec::new(),
                hook_event_name: input.inner.hook_event_name.clone().unwrap_or_default(),
                additional_context: String::new(),
                digest_lines: 0,
            }
        }
    };

    // Concord P3 — topic overlap. Proceed only on a bound
    // UserPromptSubmit. Capture runs even when notices are off so
    // `/api/v1/coord/topic-pairs` has data to calibrate against.
    let mut topic_noticed = false;
    let mut additional_context = outcome.additional_context.clone();
    let mut captured: Option<String> = None;
    let mut compare_pid: Option<String> = None;
    if outcome.bound && outcome.hook_event_name == "UserPromptSubmit" {
        captured =
            coord_topics::capture_text(input.inner.extra.get("prompt").and_then(Value::as_str));
        if coord_topics::topic_enabled() {
            compare_pid = outcome.principal_id.clone();
        }
    }

    if let Some(pid) = compare_pid.as_deref() {
        let now = Utc::now();
        let window = coord_topics::topic_window_secs();
        let threshold = coord_topics::topic_threshold();
        let dedup = coord_topics::topic_dedup_secs();
        let sim_fn = |a: &[f32], b: &[f32]| services.embedding.calculate_similarity(a, b);
        let match_out = services
            .coord_store
            .best_topic_match(pid, window, now, sim_fn);
        match match_out {
            Ok(Some(m)) if m.similarity >= threshold => {
                match services
                    .coord_store
                    .claim_topic_notice(pid, &m.other, dedup, now)
                {
                    Ok(true) => {
                        let line = coord_topics::render_topic_notice(
                            &m.other,
                            &m.other_text,
                            m.similarity,
                        );
                        if additional_context.is_empty() {
                            additional_context = line;
                        } else {
                            additional_context = format!("{additional_context}\n\n{line}");
                        }
                        topic_noticed = true;
                    }
                    Ok(false) => {
                        // Suppressed by the dedup window — no line, no bump.
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            principal_id = %pid,
                            "coord_turn: claim_topic_notice failed"
                        );
                    }
                }
            }
            Ok(Some(_)) => {
                // Below threshold — no notice, no metric bump.
            }
            Ok(None) => {
                // Caller has no stored intent yet — peer scan is empty.
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    principal_id = %pid,
                    "coord_turn: best_topic_match failed"
                );
            }
        }
    }

    // Bump the metric AFTER the store call returns and the lock is
    // released. run_turn is sync (it holds std::sync::Mutex, not the
    // tokio RwLock the metric lives behind); doing the bump here
    // matches coord_footprints' "bump metrics only after the store
    // call returns" convention. Topic-notices bump rides on the same
    // write guard so the metric snapshot is one consistent view.
    // Only take the write guard when there is something to count, so
    // SessionStart and quiet turns stay off the metrics lock.
    if outcome.digest_lines > 0 || topic_noticed {
        let mut m = services.coord_metrics.write().await;
        if outcome.digest_lines > 0 {
            m.coord_digest_lines_total = m
                .coord_digest_lines_total
                .saturating_add(outcome.digest_lines as u64);
        }
        if topic_noticed {
            m.coord_topic_notices_total = m.coord_topic_notices_total.saturating_add(1);
        }
    }

    // Spawn the async embedder + store write OFF the critical path so
    // the hook stays well under the 50 ms budget. The JoinHandle is
    // dropped — DoD-1's "does not wait" contract.
    if let (Some(pid), Some(text)) = (outcome.principal_id.as_ref(), captured.as_ref()) {
        if outcome.bound {
            let embedding = services.embedding.clone();
            let store = services.coord_store.clone();
            let pid_owned = pid.clone();
            let text_owned = text.clone();
            tokio::spawn(async move {
                match embedding.generate_embedding(&text_owned).await {
                    Ok(v) if !v.is_empty() => {
                        if let Err(e) = store.upsert_intent(&pid_owned, &text_owned, &v, Utc::now())
                        {
                            tracing::warn!(
                                error = %e,
                                principal_id = %pid_owned,
                                "coord_turn: upsert_intent failed"
                            );
                        }
                    }
                    Ok(_) => {
                        tracing::warn!(
                            principal_id = %pid_owned,
                            "coord_turn: empty embedding returned"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            principal_id = %pid_owned,
                            "coord_turn: generate_embedding failed"
                        );
                    }
                }
            });
        }
    }

    let hook_event = if outcome.hook_event_name.is_empty() {
        "Unknown".to_string()
    } else {
        outcome.hook_event_name.clone()
    };

    Json(json!({
        "principal_id": outcome.principal_id,
        "bound": outcome.bound,
        "delivered": outcome.delivered,
        "hookSpecificOutput": {
            "hookEventName": hook_event,
            "additionalContext": additional_context,
        }
    }))
}

/// `pub fn create_coord_turn_router()` — mount point.
pub fn create_coord_turn_router() -> Router<ContextNestServices> {
    Router::new().route("/api/v1/coord/turn", post(coord_turn))
}

/// Parse the raw body bytes + headers into a `TurnInputWithHeaders`.
/// NEVER errors — a malformed body just becomes `TurnInput::default()`.
///
/// `pub` so the P1 `coord_footprints` module can reuse the same
/// header/body parsing without duplicating the X-Concord-* extractor
/// logic. Callers (handlers) must NEVER use this to do anything other
/// than feed `resolve_principal` / lineage resolution.
pub fn parse_input(headers: &HeaderMap, body: &[u8]) -> TurnInputWithHeaders {
    let inner: TurnInput = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "coord_turn: body parse failed; using defaults");
            TurnInput::default()
        }
    };

    let header_string = |name: &str| -> Option<String> {
        let raw = headers.get(name)?.to_str().ok()?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    let pid = header_string("X-Concord-Pid").and_then(|s| s.parse::<i64>().ok());

    TurnInputWithHeaders {
        inner,
        explicit_principal: header_string("X-Concord-Principal"),
        tmux_pane: header_string("X-Concord-Pane"),
        tty: header_string("X-Concord-Tty"),
        pid,
    }
}

// ───────────────────────── tests ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::coord_store::{CoordStore, DigestChange, PrincipalUpsert};
    use chrono::Utc;

    #[test]
    fn sanitize_slug_replaces_unsafe_chars_and_truncates() {
        assert_eq!(sanitize_slug("hello/world?x", 32), "hello-world-x");
        assert_eq!(sanitize_slug("a.b_c-1", 32), "a.b_c-1");
        let long: String = "a".repeat(100);
        assert_eq!(sanitize_slug(&long, 10).chars().count(), 10);
        // Multi-byte passthrough — never slice a codepoint.
        assert_eq!(sanitize_slug("αβγ", 32), "---");
    }

    #[test]
    fn pane_term_accepts_percent_digits_only() {
        assert_eq!(pane_term("%94"), Some("p94".into()));
        assert_eq!(pane_term("  %7  "), Some("p7".into()));
        assert_eq!(pane_term("%"), None);
        assert_eq!(pane_term("%94a"), None);
        assert_eq!(pane_term("ttys003"), None);
        assert_eq!(pane_term(""), None);
    }

    #[test]
    fn tty_term_rejects_question_and_dash_only_values() {
        assert_eq!(tty_term("ttys003"), Some("tty-ttys003".into()));
        assert_eq!(tty_term("/dev/ttys003"), Some("tty-ttys003".into()));
        assert_eq!(tty_term("??"), None, "macOS no-tty marker");
        assert_eq!(tty_term("?"), None, "Linux no-tty marker");
        assert_eq!(tty_term("---"), None);
        assert_eq!(tty_term(""), None);
        assert_eq!(tty_term("/"), None);
    }

    #[test]
    fn repo_slug_finds_git_ancestor() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).expect("mkdir");
        std::fs::write(repo.join(".git"), b"gitdir: /tmp/x\n").expect("write .git");
        let sub = repo.join("sub");
        std::fs::create_dir(&sub).expect("mkdir sub");
        assert_eq!(repo_slug(&sub), "repo");
        assert_eq!(repo_slug(&repo), "repo");
        // No .git anywhere — fall back to basename.
        let other = tmp.path().join("scratch");
        std::fs::create_dir(&other).expect("mkdir");
        assert_eq!(repo_slug(&other), "scratch");
        // Root with no git falls back to empty sanitized.
        assert_eq!(repo_slug(Path::new("/")), "");
    }

    #[test]
    fn resolve_principal_prefers_explicit_over_lineage() {
        let cwd = Path::new("/tmp/repo");
        let (id, explicit) = resolve_principal(
            Some("loop:abc"),
            Some("%7"),
            Some("ttys001"),
            cwd,
            "session-xyz",
        )
        .expect("explicit id");
        assert_eq!(id, "loop:abc");
        assert!(explicit);
    }

    #[test]
    fn resolve_principal_falls_through_on_invalid_explicit() {
        let cwd = Path::new("/tmp/repo");
        let (id, explicit) =
            resolve_principal(Some("bogus"), Some("%7"), Some("ttys001"), cwd, "s1")
                .expect("lineage id");
        assert_eq!(id, "session:p7@repo");
        assert!(!explicit);
    }

    #[test]
    fn resolve_principal_pane_wins_over_tty() {
        let cwd = Path::new("/tmp/repo");
        let (id, _) =
            resolve_principal(None, Some("%94"), Some("ttys001"), cwd, "s1").expect("lineage id");
        assert_eq!(id, "session:p94@repo");
    }

    #[test]
    fn resolve_principal_tty_fallback_works() {
        let cwd = Path::new("/tmp/repo");
        let (id, _) =
            resolve_principal(None, None, Some("ttys001"), cwd, "s1").expect("lineage id");
        assert_eq!(id, "session:tty-ttys001@repo");
    }

    #[test]
    fn resolve_principal_tty_question_only_falls_through_to_session() {
        let cwd = Path::new("/tmp/repo");
        let (id, _) = resolve_principal(None, None, Some("??"), cwd, "s1").expect("session id");
        assert_eq!(id, "session:s1");
    }

    #[test]
    fn resolve_principal_empty_session_id_returns_none() {
        let cwd = Path::new("/tmp/repo");
        let out = resolve_principal(None, None, None, cwd, "");
        assert!(out.is_none());
    }

    #[test]
    fn resolve_principal_session_fallback_when_no_lineage() {
        let cwd = Path::new("/no/such/repo/.git");
        let (id, _) = resolve_principal(None, None, None, cwd, "abc-def").expect("session id");
        assert_eq!(id, "session:abc-def");
    }

    #[test]
    fn render_context_empty_for_no_messages() {
        assert_eq!(render_context("loop:x", &[]), "");
    }

    #[test]
    fn render_context_one_message_uses_singular() {
        let m = Message {
            msg_id: "M-1".into(),
            principal_id: "loop:x".into(),
            from: "alice".into(),
            body: "hi".into(),
            created_at: chrono::DateTime::parse_from_rfc3339("2026-10-02T12:34:56Z")
                .unwrap()
                .with_timezone(&Utc),
            delivered_at: None,
            delivered_to: None,
            acked_at: None,
            acked_by: None,
        };
        let out = render_context("loop:x", std::slice::from_ref(&m));
        assert!(out.starts_with("[concord] 1 message for loop:x"));
        assert!(out.contains("- M-1 from alice (2026-10-02T12:34:56Z): hi"));
        assert!(out.contains("Ack: mini-ork concord ack loop:x M-1"));
    }

    #[test]
    fn render_context_multiple_messages_uses_plural() {
        let mk = |id: &str, body: &str| Message {
            msg_id: id.into(),
            principal_id: "loop:x".into(),
            from: "alice".into(),
            body: body.into(),
            created_at: chrono::DateTime::parse_from_rfc3339("2026-10-02T12:34:56Z")
                .unwrap()
                .with_timezone(&Utc),
            delivered_at: None,
            delivered_to: None,
            acked_at: None,
            acked_by: None,
        };
        let out = render_context("loop:x", &[mk("M-1", "a"), mk("M-2", "b")]);
        assert!(out.starts_with("[concord] 2 messages for loop:x"));
        assert!(out.contains("Ack: mini-ork concord ack loop:x M-1"));
        assert!(out.contains("Ack: mini-ork concord ack loop:x M-2"));
    }

    #[test]
    fn run_turn_returns_empty_outcome_when_session_id_empty() {
        let s = CoordStore::open_in_memory().expect("in-memory store");
        let input = TurnInputWithHeaders {
            inner: TurnInput {
                session_id: String::new(),
                ..Default::default()
            },
            ..Default::default()
        };
        let out = run_turn(&s, &input).expect("run_turn");
        assert!(!out.bound);
        assert!(out.delivered.is_empty());
        assert!(out.additional_context.is_empty());
        assert_eq!(out.digest_lines, 0);
    }

    #[test]
    fn parse_digest_enabled_handles_disabling_values() {
        assert!(parse_digest_enabled(None));
        assert!(parse_digest_enabled(Some("")));
        assert!(parse_digest_enabled(Some("anything")));
        assert!(!parse_digest_enabled(Some("0")));
        assert!(!parse_digest_enabled(Some("false")));
        assert!(!parse_digest_enabled(Some("off")));
        assert!(!parse_digest_enabled(Some("  FALSE  ")));
        assert!(!parse_digest_enabled(Some("Off")));
    }

    fn mk_change(path: &str, pid: &str, harness: Option<&str>, cwd: Option<&str>) -> DigestChange {
        DigestChange {
            path: path.into(),
            principal_id: pid.into(),
            seq: 1,
            ts: chrono::DateTime::parse_from_rfc3339("2026-10-02T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            harness: harness.map(str::to_string),
            cwd: cwd.map(str::to_string),
            writer_count: 1,
        }
    }

    #[test]
    fn render_digest_empty_for_no_changes() {
        let d = TurnDigest {
            changes: Vec::new(),
            total_paths: 0,
            cursor_initialized: false,
        };
        let (text, lines) = render_digest(&d);
        assert_eq!(text, "");
        assert_eq!(lines, 0);
    }

    #[test]
    fn render_digest_single_line_exact_format() {
        let mut c = mk_change("/r/f.rs", "loop:b", Some("codex"), Some("/w/repo-b"));
        c.writer_count = 1;
        let d = TurnDigest {
            changes: vec![c],
            total_paths: 1,
            cursor_initialized: false,
        };
        let (text, lines) = render_digest(&d);
        assert_eq!(lines, 1);
        assert!(
            text.starts_with(
                "[concord] changed by other agents since your last turn \
                 (judge whether it affects your plan):"
            ),
            "header must be byte-exact; got {text:?}"
        );
        assert!(
            text.contains("- /r/f.rs — loop:b (codex, repo-b) at 2026-10-02T12:00:00Z"),
            "exact line format; got {text:?}"
        );
    }

    #[test]
    fn render_digest_appends_more_when_total_exceeds_limit() {
        let mut changes = Vec::new();
        for i in 0..3 {
            let mut c = mk_change(
                &format!("/r/f{i}.rs"),
                "loop:b",
                Some("codex"),
                Some("/w/repo-b"),
            );
            c.writer_count = 1;
            changes.push(c);
        }
        let d = TurnDigest {
            changes,
            total_paths: 7,
            cursor_initialized: false,
        };
        let (text, lines) = render_digest(&d);
        assert_eq!(lines, 3);
        assert!(text.contains("\n(+4 more)"), "more line; got {text:?}");
        // No 'other writer' suffix when count==1.
        assert!(!text.contains("other writer"));
    }

    #[test]
    fn render_digest_other_writer_suffix_uses_singular_for_k_equals_1() {
        let mut c = mk_change("/r/f.rs", "loop:b", Some("codex"), Some("/w/repo-b"));
        c.writer_count = 2;
        let d = TurnDigest {
            changes: vec![c],
            total_paths: 1,
            cursor_initialized: false,
        };
        let (text, _) = render_digest(&d);
        assert!(
            text.contains("(+1 other writer)"),
            "K=1 must use singular; got {text:?}"
        );
    }

    #[test]
    fn render_digest_other_writer_suffix_uses_plural_for_k_greater_than_1() {
        let mut c = mk_change("/r/f.rs", "loop:b", Some("codex"), Some("/w/repo-b"));
        c.writer_count = 5;
        let d = TurnDigest {
            changes: vec![c],
            total_paths: 1,
            cursor_initialized: false,
        };
        let (text, _) = render_digest(&d);
        assert!(
            text.contains("(+4 other writers)"),
            "K=4 must use plural; got {text:?}"
        );
    }

    #[test]
    fn run_turn_store_error_propagates() {
        // Open a file-backed store, then drop its tables out from under
        // it through a SECOND rusqlite::Connection. run_turn must error
        // out (the handler maps Err → empty 200 context).
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("coord.db");
        let s = CoordStore::open(&path).expect("open");
        // Seed a principal so the binding step would otherwise succeed.
        s.upsert_principal(
            "session:abc",
            PrincipalUpsert {
                harness: Some("h".into()),
                ..Default::default()
            },
        )
        .unwrap();
        {
            // Open a second connection, drop the tables. The first
            // connection's prepared statements will see
            // SQLITE_ERROR/SQLITE_SCHEMA on the next query.
            let conn = rusqlite::Connection::open(&path).expect("second conn");
            conn.execute_batch("DROP TABLE messages; DROP TABLE bindings; DROP TABLE principals;")
                .expect("drop tables");
        }
        let input = TurnInputWithHeaders {
            inner: TurnInput {
                session_id: "abc".into(),
                cwd: Some("/tmp".into()),
                hook_event_name: Some("SessionStart".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let result = run_turn(&s, &input);
        assert!(result.is_err(), "store error must propagate");
    }
}
