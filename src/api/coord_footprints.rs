//! Concord P1 — per-principal read/write footprint capture
//! (`POST /api/v1/coord/footprints`) and PreToolUse advisory
//! (`POST /api/v1/coord/precheck`).
//!
//! ## Why two endpoints, not one
//!
//! Claude Code's PostToolUse is fire-and-forget (the parent shell
//! backgrounds the curl), so a `Write` followed immediately by an
//! `Edit` on the same file can race — the Edit's precheck fires
//! before the Write's footprint has been inserted. The P1 design
//! accepts that race; the brief is explicit. Folding precheck into
//! the same handler would not fix the ordering.
//!
//! ## Identity
//!
//! Both endpoints reuse [`coord_turn::parse_input`] and
//! [`coord_turn::resolve_principal`] for the X-Concord-* header
//! extraction. Resolution order is:
//!
//!   1. `X-Concord-Principal` header (if valid).
//!   2. Existing binding for `session_id` (if any).
//!   3. Derived lineage id (`session:<term>@<repo>` or
//!      `session:<sanitized session_id>`).
//!
//! The precheck is **advisory by default** and `permissionDecision` is
//! never `"allow"`: in Claude Code's hook contract `"allow"` skips the
//! user's permission prompt, so a globally installed hook answering
//! `allow` would auto-approve every Edit/Write. The field is inserted
//! ONLY for a hot-conflict on an env-configured shared-config path when
//! the operator set `CONTEXTNEST_CONCORD_HOT_MODE=ask|deny`, and even
//! then its value is `"ask"`/`"deny"` — never `"allow"`. Omitting the
//! field leaves the user's normal permission flow untouched while
//! `additionalContext` still reaches the model. When the substrate is
//! down, the synchronous curl's `|| true` keeps Claude Code on that same
//! prompt path.
//!
//! ## Scope
//!
//! All SQL lives in [`coord_store`] — this file is the HTTP and
//! helper layer only. No LLM or embedding work, no awaits while a
//! store lock is held. The metrics counter is bumped only after
//! every store call returns, then the metrics RwLock is dropped
//! before the handler returns.

use axum::{
    extract::State,
    http::HeaderMap,
    response::Json,
    routing::{get, post},
    Router,
};
use chrono::SecondsFormat;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tracing::error;

use crate::api::coord_turn::{self, TurnInputWithHeaders};
use crate::services::coord_store::{
    CoordStore, CoordStoreResult, HotClaim, HotClaimOutcome, OtherWriter, PrincipalUpsert,
    LINEAGE_MAX_HOPS,
};
use crate::services::ContextNestServices;

// ───────────────────────── pure helpers ─────────────────────────

/// Default hot-path globs used when `CONTEXTNEST_CONCORD_HOT_GLOBS` is
/// unset. Comma-separated; the same list the mini-ork concord brief
/// names as shared live config: `.mini-ork/config`, `secrets*.sh`,
/// `providers.yaml`, `agents.yaml`, `.claude/settings*.json`, `.env`
/// (and siblings), and `db/migrations`.
const DEFAULT_HOT_GLOBS: &str = "**/.mini-ork/config/**,**/secrets*.sh,**/providers.yaml,**/agents.yaml,**/.claude/settings*.json,**/.env,**/.env.*,**/db/migrations/**";

/// The hot-path globs, read fresh from env on every call so test
/// suites can flip them without restarting the binary. An explicitly
/// set value is split on ',', trimmed, and stripped of empty entries —
/// so an empty string disables the hot set. Unset → [`DEFAULT_HOT_GLOBS`].
pub fn hot_globs() -> Vec<String> {
    parse_hot_globs(
        std::env::var("CONTEXTNEST_CONCORD_HOT_GLOBS")
            .ok()
            .as_deref(),
    )
}

/// Pure parser behind [`hot_globs`]: `None` (unset) → the defaults; a set
/// value is split on ',', trimmed, and stripped of empties (so "" disables).
/// Unit tests call this directly — mutating the process env from parallel
/// test threads raced (`cargo test` runs tests concurrently).
pub fn parse_hot_globs(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or(DEFAULT_HOT_GLOBS)
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// How a hot conflict should be surfaced on the precheck. `ask` and
/// `deny` add a `permissionDecision` to `hookSpecificOutput`; anything
/// else (including unset) is the advisory-only default [`HotMode::Warn`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotMode {
    Warn,
    Ask,
    Deny,
}

/// Read `CONTEXTNEST_CONCORD_HOT_MODE` fresh on every call: trim +
/// ascii-lowercase, `ask`/`deny` map to their modes, anything else
/// (bogus or unset) is [`HotMode::Warn`].
pub fn hot_mode() -> HotMode {
    parse_hot_mode(
        std::env::var("CONTEXTNEST_CONCORD_HOT_MODE")
            .ok()
            .as_deref(),
    )
}

/// Pure parser behind [`hot_mode`]; anything but `ask`/`deny` is `Warn`.
pub fn parse_hot_mode(raw: Option<&str>) -> HotMode {
    match raw.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("ask") => HotMode::Ask,
        Some("deny") => HotMode::Deny,
        _ => HotMode::Warn,
    }
}

/// Claim TTL in seconds from `CONTEXTNEST_CONCORD_HOT_TTL_SECS`,
/// read fresh per call. Unparseable or non-positive falls back to 600.
pub fn hot_ttl_secs() -> i64 {
    parse_hot_ttl_secs(
        std::env::var("CONTEXTNEST_CONCORD_HOT_TTL_SECS")
            .ok()
            .as_deref(),
    )
}

/// Pure parser behind [`hot_ttl_secs`]; unparseable or non-positive → 600.
pub fn parse_hot_ttl_secs(raw: Option<&str>) -> i64 {
    raw.and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(600)
}

/// Match `path` against a component glob `pattern`. Both are split on
/// '/': a `**` segment matches zero or more whole components; within a
/// segment `*` matches any run of chars and `?` exactly one; everything
/// else is a case-sensitive literal. No `glob`/`globset` dependency —
/// this is the hand-written matcher the brief calls for.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let segs: Vec<&str> = path.split('/').collect();
    components_match(&pat, &segs)
}

fn components_match(pat: &[&str], segs: &[&str]) -> bool {
    if pat.is_empty() {
        return segs.is_empty();
    }
    match pat[0] {
        "**" => (0..=segs.len()).any(|skip| components_match(&pat[1..], &segs[skip..])),
        p => {
            if segs.is_empty() {
                return false;
            }
            segment_match(p, segs[0]) && components_match(&pat[1..], &segs[1..])
        }
    }
}

fn segment_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    // dp[i][j] == "pattern[..i] matches text[..j]" under the glob
    // semantics above. `*` is zero-or-more, `?` exactly one.
    let mut dp = vec![vec![false; t.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 1..=p.len() {
        dp[i][0] = p[i - 1] == '*' && dp[i - 1][0];
    }
    for i in 1..=p.len() {
        for j in 1..=t.len() {
            dp[i][j] = match p[i - 1] {
                '*' => dp[i - 1][j] || dp[i][j - 1],
                '?' => dp[i - 1][j - 1],
                c => dp[i - 1][j - 1] && c == t[j - 1],
            };
        }
    }
    dp[p.len()][t.len()]
}

/// True iff `path` matches any of the configured hot globs. Reads the
/// glob list fresh per call (see [`hot_globs`]).
pub fn is_hot_path(path: &str) -> bool {
    is_hot_path_with(path, &hot_globs())
}

/// [`is_hot_path`] against an explicit glob list (env-free, for tests).
pub fn is_hot_path_with(path: &str, globs: &[String]) -> bool {
    globs.iter().any(|g| glob_match(g, path))
}

/// Map a Claude Code tool name to the footprint `op` it represents.
/// `Read` → `"read"`; the Edit-class tools → `"write"`. Anything else
/// (Bash, WebFetch, …) returns `None` so the handler records no
/// footprint and the precheck is a no-op.
pub fn file_tool_op(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        "Read" => Some("read"),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => Some("write"),
        _ => None,
    }
}

/// Resolve the path the tool touched. Reads `tool_input.file_path`
/// for the Edit-class tools and `tool_input.notebook_path` for
/// `NotebookEdit` (CC's notebook editor stores its target under
/// `notebook_path`, not `file_path`). A relative path is joined to
/// `cwd`. `std::fs::canonicalize` is applied when it succeeds, so
/// macOS tempdirs (`/var/folders/...` → `/private/var/folders/...`)
/// and symlinks resolve to the same string on both the read and the
/// write side of a precheck pair.
///
/// Missing or unparseable input returns `None` so the handler
/// records no footprint and the precheck is a no-op.
pub fn resolve_tool_path(tool_name: &str, extra: &Value, cwd: &Path) -> Option<PathBuf> {
    let tool_input = extra.get("tool_input").and_then(Value::as_object);
    let raw: Option<String> = if tool_name == "NotebookEdit" {
        tool_input
            .and_then(|ti| ti.get("notebook_path"))
            .and_then(Value::as_str)
            .map(|s| s.to_string())
    } else {
        tool_input
            .and_then(|ti| ti.get("file_path"))
            .and_then(Value::as_str)
            .map(|s| s.to_string())
    };
    let raw = raw?;
    if raw.is_empty() {
        return None;
    }
    let p = PathBuf::from(&raw);
    let joined = if p.is_absolute() { p } else { cwd.join(&p) };
    Some(match std::fs::canonicalize(&joined) {
        Ok(c) => c,
        Err(_) => joined,
    })
}

/// Resolve a principal id for the hook call without mutating store
/// state: a valid explicit header, else this session's existing binding,
/// else the derived lineage id (P0b `resolve_principal` order). Callers
/// decide whether to upsert on a miss (footprints does; precheck never does).
pub fn resolve_hook_principal(
    store: &CoordStore,
    input: &TurnInputWithHeaders,
) -> CoordStoreResult<Option<String>> {
    let cwd_path = Path::new(input.inner.cwd.as_deref().unwrap_or(""));
    let (resolved, explicit) = match coord_turn::resolve_principal(
        input.explicit_principal.as_deref(),
        input.tmux_pane.as_deref(),
        input.tty.as_deref(),
        cwd_path,
        &input.inner.session_id,
    ) {
        Some(t) => t,
        None => return Ok(None),
    };
    if explicit {
        return Ok(Some(resolved));
    }
    // Non-explicit path: prefer an existing binding for this session
    // (the P0b turn hook may have already bound one). An empty
    // session_id never reaches here — we returned at `resolve_principal`.
    if !input.inner.session_id.is_empty() {
        if let Some(b) = store.get_binding(&input.inner.session_id)? {
            return Ok(Some(b.principal_id));
        }
    }
    Ok(Some(resolved))
}

/// `mtime_ns` (saturating to `i64`) and `size` from `std::fs::metadata`.
/// `None` for both when the file vanished (ephemeral /tmp, deleted
/// before the stat, etc).
pub fn stat_file(path: &Path) -> (Option<i64>, Option<i64>) {
    match std::fs::metadata(path) {
        Ok(m) => {
            let mtime_ns = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| {
                    // i64 nanos saturates around year 2262; mktemp
                    // files are nowhere near that, but stay defensive.
                    let n = d.as_nanos();
                    if n > i64::MAX as u128 {
                        i64::MAX
                    } else {
                        n as i64
                    }
                });
            let size = Some(m.len() as i64);
            (mtime_ns, size)
        }
        Err(_) => (None, None),
    }
}

/// Render the `additionalContext` text Claude Code surfaces verbatim
/// to the model. Empty list → empty string (no-op). The newest
/// writer is named with the `mini-ork concord send` suggestion, so
/// Claude has a ready-made handoff command to ask for the latest
/// diff. `<harness>` and `<cwd>` are `unknown` when the writer's
/// principal row doesn't carry them. `<ts>` is RFC3339 seconds with
/// a Z suffix (matches `coord_turn::render_context`).
pub fn render_precheck_context(path: &Path, others: &[OtherWriter]) -> String {
    if others.is_empty() {
        return String::new();
    }
    // Newest first is the order `writes_after` returns; first entry
    // is the freshest writer.
    let newest = &others[0];
    let harness = newest.harness.as_deref().unwrap_or("unknown");
    let cwd = newest
        .cwd
        .as_deref()
        .and_then(|c| {
            let p = Path::new(c);
            p.file_name().and_then(|n| n.to_str())
        })
        .unwrap_or("unknown");
    let ts = newest.ts.to_rfc3339_opts(SecondsFormat::Secs, true);
    let mut out = format!(
        "[concord] {} wrote `{}` after you last read it.\n\
         Newest: {} ({}, {}) at {}\n\
         Run `mini-ork concord send {}` to ask for the current state before editing.",
        newest.principal_id,
        path.display(),
        newest.principal_id,
        harness,
        cwd,
        ts,
        newest.principal_id,
    );
    for w in others.iter().skip(1) {
        let h = w.harness.as_deref().unwrap_or("unknown");
        let c = w
            .cwd
            .as_deref()
            .and_then(|p| Path::new(p).file_name().and_then(|n| n.to_str()))
            .unwrap_or("unknown");
        let t = w.ts.to_rfc3339_opts(SecondsFormat::Secs, true);
        out.push_str(&format!(
            "\nAlso: {} ({}, {}) at {}",
            w.principal_id, h, c, t
        ));
    }
    out
}

/// Render the hot-conflict advisory for a shared-config path. Exact
/// shape the brief specifies: the 🔒 glyph, the canonical path, the
/// holder, the last-write `<ts>` (RFC3339 seconds Z, `unknown` when the
/// footprint row was pruned), and the claim's expiry. Ends with the
/// ready-made `mini-ork concord send` handoff.
pub fn render_hot_context(path: &Path, claim: &HotClaim) -> String {
    let ts = claim
        .last_write_ts
        .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        "[concord] \u{1F512} {} is shared live config, claimed by {} (last write {}, claim until {}). Concurrent edits clobber running agents. Coordinate first: mini-ork concord send {} \"...\"",
        path.display(),
        claim.principal_id,
        ts,
        claim.expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        claim.principal_id,
    )
}

// ───────────────────────── handlers ─────────────────────────

/// `POST /api/v1/coord/footprints` — PostToolUse hook. Records a
/// read or write footprint for the resolved principal, upserting a
/// minimal principal row on first sight (no `labels` so a future
/// `labels.parent` set by the operator is preserved) and binding
/// `session_id` → principal on a fresh session.
///
/// **Always answers 200.** A store error is logged and the response
/// is `{"recorded": false}` so Claude Code's hook protocol never
/// sees a 4xx.
pub async fn coord_footprints(
    State(services): State<ContextNestServices>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Json<Value> {
    let input = coord_turn::parse_input(&headers, &body);
    let tool_name = input
        .inner
        .extra
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let op = match file_tool_op(tool_name) {
        Some(o) => o,
        None => return Json(json!({ "recorded": false })),
    };
    let cwd_str = input.inner.cwd.clone().unwrap_or_default();
    let cwd_path = Path::new(&cwd_str);
    let path = match resolve_tool_path(tool_name, &input.inner.extra, cwd_path) {
        Some(p) => p,
        None => return Json(json!({ "recorded": false })),
    };
    let principal_id = match resolve_hook_principal(&services.coord_store, &input) {
        Ok(Some(p)) => p,
        Ok(None) => return Json(json!({ "recorded": false })),
        Err(e) => {
            error!(error = %e, "coord_footprints: principal resolution failed");
            return Json(json!({ "recorded": false }));
        }
    };

    // Side effects under the store lock. Each is best-effort — a
    // failure logs and returns recorded=false; Claude Code never sees
    // a non-200.
    let store = services.coord_store.clone();
    let path_str = path.to_string_lossy().to_string();
    let session_id = input.inner.session_id.clone();

    let upsert_result: CoordStoreResult<()> = (|| {
        if store.get_principal(&principal_id)?.is_none() {
            // Minimal upsert: harness + cwd, NO labels. This is the
            // critical branch — copying the P0b derived-principal path
            // would clobber a future labels.parent and silently break
            // lineage (DoD 4).
            store.upsert_principal(
                &principal_id,
                PrincipalUpsert {
                    harness: Some("claude-code".into()),
                    cwd: Some(cwd_str.clone()),
                    ..Default::default()
                },
            )?;
        }
        if !session_id.is_empty() {
            let current = store.get_binding(&session_id)?;
            let needs_bind = match current {
                Some(b) => b.principal_id != principal_id,
                None => true,
            };
            if needs_bind {
                store.bind(&session_id, &principal_id, input.pid)?;
            }
        }
        Ok(())
    })();

    if let Err(e) = upsert_result {
        error!(error = %e, principal_id = %principal_id, "coord_footprints: upsert/bind failed");
        return Json(json!({ "recorded": false }));
    }

    let (mtime_ns, size) = stat_file(&path);
    let worker_id = if session_id.is_empty() {
        principal_id.clone()
    } else {
        session_id.clone()
    };
    let seq = match store.record_footprint(&principal_id, &worker_id, op, &path_str, mtime_ns, size)
    {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, principal_id = %principal_id, "coord_footprints: record_footprint failed");
            return Json(json!({ "recorded": false }));
        }
    };

    // Behaviour 1 — writes to a hot shared-config path take or renew a
    // TTL claim. The footprint has already landed, so a store error or
    // contention never changes the response (the hook must keep
    // answering 200). The claim call releases the store lock before we
    // take the metrics lock, matching the module's "no lock across
    // await / bump metrics only after the store call returns" rule.
    if op == "write" && is_hot_path(&path_str) {
        match store.claim_hot(&path_str, &principal_id, seq, hot_ttl_secs()) {
            Ok(HotClaimOutcome::Contended { .. }) => {
                let mut m = services.coord_metrics.write().await;
                m.coord_hot_contended_total = m.coord_hot_contended_total.saturating_add(1);
            }
            Ok(_) => {}
            Err(e) => {
                error!(error = %e, principal_id = %principal_id, path = %path_str, "coord_footprints: claim_hot failed");
            }
        }
    }

    Json(json!({
        "recorded": true,
        "seq": seq,
        "principal_id": principal_id,
    }))
}

/// `POST /api/v1/coord/precheck` — PreToolUse hook. Resolves the
/// caller (read-only — no upsert, no bind), finds the newest writes to
/// `path` from any non-lineage principal since the caller's last
/// footprint on it (P1), and layers on the hot-conflict check for
/// shared-config paths (P2).
///
/// Always answers 200. `permissionDecision` is set — to `"ask"` or
/// `"deny"`, never `"allow"` — only when `hot` is some AND
/// `CONTEXTNEST_CONCORD_HOT_MODE` is `ask`/`deny`; otherwise the key is
/// absent (see the module doc). Every request — no-op tools, unresolved
/// paths or principals, and store errors included — counts toward
/// `coord_precheck_total`; `coord_precheck_warn` counts only the P1
/// stale-premise warnings; `coord_hot_conflicts_total` counts only the
/// hot conflicts.
pub async fn coord_precheck(
    State(services): State<ContextNestServices>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Json<Value> {
    let input = coord_turn::parse_input(&headers, &body);
    let outcome = precheck_decision(&services.coord_store, &input);
    {
        let mut m = services.coord_metrics.write().await;
        m.coord_precheck_total = m.coord_precheck_total.saturating_add(1);
        if outcome.warn {
            m.coord_precheck_warn = m.coord_precheck_warn.saturating_add(1);
        }
        if outcome.hot.is_some() {
            m.coord_hot_conflicts_total = m.coord_hot_conflicts_total.saturating_add(1);
        }
    }
    let p1_text = match (&outcome.path, outcome.warn) {
        (Some(p), true) => render_precheck_context(p, &outcome.others),
        _ => String::new(),
    };
    let hot_text = match (&outcome.path, &outcome.hot) {
        (Some(p), Some(claim)) => render_hot_context(p, claim),
        _ => String::new(),
    };
    let additional_context = match (p1_text.is_empty(), hot_text.is_empty()) {
        (false, false) => format!("{p1_text}\n\n{hot_text}"),
        (false, true) => p1_text,
        (true, false) => hot_text.clone(),
        (true, true) => String::new(),
    };
    let others_json: Vec<Value> = outcome
        .others
        .iter()
        .map(|w| {
            json!({
                "principal_id": w.principal_id,
                "seq": w.seq,
                "ts": w.ts.to_rfc3339_opts(SecondsFormat::Secs, true),
                "harness": w.harness,
                "cwd": w.cwd,
            })
        })
        .collect();

    // Build hookSpecificOutput as a Map so `permissionDecision` can be
    // inserted conditionally — absent (not null, not "allow") everywhere
    // except the hot-conflict + ask/deny branch.
    let mut hook = serde_json::Map::new();
    hook.insert("hookEventName".to_string(), json!("PreToolUse"));
    hook.insert("additionalContext".to_string(), json!(additional_context));
    if outcome.hot.is_some() {
        match hot_mode() {
            HotMode::Ask => {
                hook.insert("permissionDecision".to_string(), json!("ask"));
                hook.insert("permissionDecisionReason".to_string(), json!(hot_text));
            }
            HotMode::Deny => {
                hook.insert("permissionDecision".to_string(), json!("deny"));
                hook.insert("permissionDecisionReason".to_string(), json!(hot_text));
            }
            HotMode::Warn => {}
        }
    }

    Json(json!({
        "warn": outcome.warn,
        "others": others_json,
        "hot_conflict": outcome.hot.is_some(),
        "hookSpecificOutput": hook,
    }))
}

/// The pure decision behind `coord_precheck`. `warn`/`others` carry the
/// P1 stale-premise signal; `hot` carries the P2 hot-conflict claim.
/// The hot check runs right after principal resolution and BEFORE the
/// P1 no-prior-footprint early return, so a caller who has never touched
/// the path still sees a live claim held by an outsider. Store errors are
/// logged and treated as no-ops so the hook can never block Claude Code.
struct PrecheckOutcome {
    warn: bool,
    others: Vec<OtherWriter>,
    path: Option<PathBuf>,
    hot: Option<HotClaim>,
}

fn precheck_decision(store: &CoordStore, input: &TurnInputWithHeaders) -> PrecheckOutcome {
    let noop = PrecheckOutcome {
        warn: false,
        others: Vec::new(),
        path: None,
        hot: None,
    };
    let tool_name = input
        .inner
        .extra
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    if file_tool_op(tool_name) != Some("write") {
        return noop;
    }
    let cwd_str = input.inner.cwd.clone().unwrap_or_default();
    let path = match resolve_tool_path(tool_name, &input.inner.extra, Path::new(&cwd_str)) {
        Some(p) => p,
        None => return noop,
    };
    let principal_id = match resolve_hook_principal(store, input) {
        Ok(Some(p)) => p,
        _ => {
            return PrecheckOutcome {
                warn: false,
                others: Vec::new(),
                path: Some(path),
                hot: None,
            }
        }
    };
    let path_str = path.to_string_lossy().to_string();

    // Hot-conflict check. Must run even when the caller has no prior
    // footprint (the P1 early return below), because a caller that has
    // never touched a hot file still needs the 🔒 advisory.
    let hot: Option<HotClaim> = if is_hot_path(&path_str) {
        match store.live_hot_claim(&path_str) {
            Ok(Some(claim)) => match store.lineage(&principal_id, LINEAGE_MAX_HOPS) {
                Ok(lin) if lin.contains(&claim.principal_id) => None,
                Ok(_) => Some(claim),
                Err(e) => {
                    error!(error = %e, principal_id = %principal_id, "coord_precheck: lineage lookup failed");
                    None
                }
            },
            Ok(None) => None,
            Err(e) => {
                error!(error = %e, principal_id = %principal_id, "coord_precheck: live_hot_claim failed");
                None
            }
        }
    } else {
        None
    };

    let outcome: CoordStoreResult<(bool, Vec<OtherWriter>)> = (|| {
        let last = match store.last_footprint_seq(&principal_id, &path_str)? {
            Some(s) => s,
            None => return Ok((false, Vec::new())),
        };
        let excl: HashSet<String> = store.lineage(&principal_id, LINEAGE_MAX_HOPS)?;
        let others = store.writes_after(&path_str, last, &excl, 3)?;
        Ok((!others.is_empty(), others))
    })();
    let (warn, others) = match outcome {
        Ok((warn, others)) => (warn, others),
        Err(e) => {
            error!(error = %e, principal_id = %principal_id, "coord_precheck: store query failed");
            (false, Vec::new())
        }
    };
    PrecheckOutcome {
        warn,
        others,
        path: Some(path),
        hot,
    }
}

/// `GET /api/v1/coord/hot-claims` — live hot claims, each shaped as
/// exactly `{path, principal_id, expires_at (RFC3339), last_write_seq}`
/// under `{"claims": [...]}`. The read-side sweep of expired rows happens
/// inside `list_live_hot_claims`. A store error logs and returns an
/// empty list with 200 — the endpoint is observability, not a gate.
pub async fn coord_hot_claims(State(services): State<ContextNestServices>) -> Json<Value> {
    match services.coord_store.list_live_hot_claims() {
        Ok(claims) => {
            let arr: Vec<Value> = claims
                .iter()
                .map(|c| {
                    json!({
                        "path": c.path,
                        "principal_id": c.principal_id,
                        "expires_at": c.expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
                        "last_write_seq": c.last_write_seq,
                    })
                })
                .collect();
            Json(json!({ "claims": arr }))
        }
        Err(e) => {
            error!(error = %e, "coord_hot_claims: list_live_hot_claims failed");
            Json(json!({ "claims": [] }))
        }
    }
}

/// Mount the P1 + P2 endpoints. Merged into `base_router` by `simple.rs`.
pub fn create_coord_footprints_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/api/v1/coord/footprints", post(coord_footprints))
        .route("/api/v1/coord/precheck", post(coord_precheck))
        .route("/api/v1/coord/hot-claims", get(coord_hot_claims))
}

// ───────────────────────── tests ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use serde_json::json;

    #[test]
    fn file_tool_op_maps_known_tools() {
        assert_eq!(file_tool_op("Read"), Some("read"));
        assert_eq!(file_tool_op("Edit"), Some("write"));
        assert_eq!(file_tool_op("Write"), Some("write"));
        assert_eq!(file_tool_op("MultiEdit"), Some("write"));
        assert_eq!(file_tool_op("NotebookEdit"), Some("write"));
        assert_eq!(file_tool_op("Bash"), None);
        assert_eq!(file_tool_op("WebFetch"), None);
        assert_eq!(file_tool_op(""), None);
    }

    #[test]
    fn resolve_tool_path_handles_relative_and_notebook() {
        let cwd = Path::new("/work");
        // Relative + cwd join.
        let extra = json!({"tool_input": {"file_path": "src/foo.rs"}});
        let p = resolve_tool_path("Edit", &extra, cwd).unwrap();
        assert_eq!(p, PathBuf::from("/work/src/foo.rs"));
        // Absolute path canonicalizes if possible. On macOS,
        // /etc/hosts → /private/etc/hosts; on Linux, it stays put.
        // We test the canonicalized form so the test is portable.
        let extra = json!({"tool_input": {"file_path": "/etc/hosts"}});
        let p = resolve_tool_path("Read", &extra, cwd).unwrap();
        let expected = std::fs::canonicalize("/etc/hosts").unwrap();
        assert_eq!(p, expected);
        // NotebookEdit reads notebook_path.
        let extra = json!({"tool_input": {"notebook_path": "nb.ipynb"}});
        let p = resolve_tool_path("NotebookEdit", &extra, cwd).unwrap();
        assert_eq!(p, PathBuf::from("/work/nb.ipynb"));
        // Missing field → None.
        let extra = json!({"tool_input": {}});
        assert!(resolve_tool_path("Edit", &extra, cwd).is_none());
    }

    #[test]
    fn stat_file_returns_metadata_or_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let f = dir.path().join("f.txt");
        std::fs::write(&f, b"hi").expect("write");
        let (mtime, size) = stat_file(&f);
        assert!(mtime.is_some());
        assert_eq!(size, Some(2));
        // Missing file → (None, None).
        let (mtime, size) = stat_file(&dir.path().join("missing"));
        assert!(mtime.is_none());
        assert!(size.is_none());
    }

    #[test]
    fn render_precheck_context_empty_for_no_others() {
        assert_eq!(render_precheck_context(Path::new("/x.rs"), &[]), "");
    }

    #[test]
    fn render_precheck_context_single_writer_named() {
        let now: DateTime<Utc> = "2026-10-02T12:34:56Z".parse().unwrap();
        let w = OtherWriter {
            principal_id: "loop:b".into(),
            seq: 7,
            ts: now,
            harness: Some("claude-code".into()),
            cwd: Some("/work/repo".into()),
        };
        let out = render_precheck_context(Path::new("/work/repo/src/foo.rs"), &[w]);
        assert!(out.contains("[concord] loop:b wrote"));
        assert!(out.contains("after you last read it"));
        assert!(out.contains("(claude-code, repo)"));
        assert!(out.contains("at 2026-10-02T12:34:56Z"));
        assert!(out.contains("mini-ork concord send loop:b"));
    }

    #[test]
    fn render_precheck_context_multiple_writers_lists_also() {
        let now: DateTime<Utc> = "2026-10-02T12:34:56Z".parse().unwrap();
        let w1 = OtherWriter {
            principal_id: "loop:b".into(),
            seq: 9,
            ts: now,
            harness: Some("claude-code".into()),
            cwd: Some("/work/repo".into()),
        };
        let w2 = OtherWriter {
            principal_id: "loop:c".into(),
            seq: 7,
            ts: now,
            harness: None,
            cwd: None,
        };
        let out = render_precheck_context(Path::new("/x.rs"), &[w1, w2]);
        assert!(out.contains("Newest: loop:b"));
        assert!(out.contains("Also: loop:c (unknown, unknown)"));
    }

    #[test]
    fn render_precheck_context_unknown_harness_does_not_panic() {
        let now: DateTime<Utc> = "2026-10-02T12:34:56Z".parse().unwrap();
        let w = OtherWriter {
            principal_id: "loop:b".into(),
            seq: 1,
            ts: now,
            harness: None,
            cwd: Some("/work/repo".into()),
        };
        let out = render_precheck_context(Path::new("/x.rs"), &[w]);
        assert!(out.contains("(unknown, repo)"));
    }

    #[test]
    fn render_precheck_context_lists_every_extra_writer_as_also() {
        // Defensive: writes_after is called with limit=3, so the
        // render function never sees more than 3 rows in practice.
        // Asserting the render doesn't blow up if it ever does.
        let now: DateTime<Utc> = "2026-10-02T12:34:56Z".parse().unwrap();
        let rows: Vec<OtherWriter> = (0..5)
            .map(|i| OtherWriter {
                principal_id: format!("loop:{i}"),
                seq: i,
                ts: now,
                harness: Some("h".into()),
                cwd: Some("/c".into()),
            })
            .collect();
        let out = render_precheck_context(Path::new("/x.rs"), &rows);
        // 1 "Newest" line + 4 "Also:" lines.
        assert!(out.matches("Also:").count() == 4);
    }

    // ────────────── hot-path matcher + config (Concord P2) ──────────────

    #[test]
    fn glob_match_segment_star_and_question() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", ".rs"), "star matches empty run");
        assert!(!glob_match("*.rs", "main.rs.bak"));
        assert!(glob_match("secrets*.sh", "secrets-prod.sh"));
        assert!(!glob_match("secrets*.sh", "mysecrets.sh"));
        assert!(glob_match("file?.txt", "file1.txt"));
        assert!(!glob_match("file?.txt", "file10.txt"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
    }

    #[test]
    fn glob_match_double_star_whole_components() {
        assert!(glob_match("**/.env", "/r/.env"));
        assert!(glob_match("**/.env", ".env"));
        assert!(glob_match("**/.env.*", "/r/.env.local"));
        assert!(glob_match(
            "**/.mini-ork/config/**",
            "/private/var/x/.mini-ork/config/agents.yaml"
        ));
        assert!(glob_match(
            "**/db/migrations/**",
            "/r/db/migrations/001.sql"
        ));
        // `**` must match whole components — `configs` is not `config`.
        assert!(!glob_match(
            "**/.mini-ork/config/**",
            "/a/.mini-ork/configs/x"
        ));
        // Case-sensitive literals.
        assert!(!glob_match("**/.env", "/r/.ENV"));
    }

    #[test]
    fn default_globs_match_positives_and_reject_near_misses() {
        // Env-free: test the default list directly (no process-env races).
        let defaults = parse_hot_globs(None);
        let positives = [
            "/private/var/x/.mini-ork/config/agents.yaml",
            "/r/.env",
            "/r/.env.local",
            "/Users/u/.claude/settings.local.json",
            "/r/db/migrations/001.sql",
            "/x/secrets-prod.sh",
        ];
        for p in positives {
            assert!(is_hot_path_with(p, &defaults), "expected hot: {p}");
        }
        let negatives = [
            "/r/.envrc",
            "/x/mysecrets.sh",
            "/a/.mini-ork/configs/x",
            "/w/src/main.rs",
        ];
        for p in negatives {
            assert!(!is_hot_path_with(p, &defaults), "expected NOT hot: {p}");
        }
    }

    #[test]
    fn hot_mode_parses_ask_deny_and_falls_back_to_warn() {
        assert_eq!(parse_hot_mode(Some("ASK")), HotMode::Ask);
        assert_eq!(parse_hot_mode(Some("deny")), HotMode::Deny);
        assert_eq!(parse_hot_mode(Some("bogus")), HotMode::Warn);
        assert_eq!(parse_hot_mode(None), HotMode::Warn);
    }

    #[test]
    fn hot_ttl_secs_parses_and_falls_back() {
        assert_eq!(parse_hot_ttl_secs(Some("42")), 42);
        assert_eq!(parse_hot_ttl_secs(Some("0")), 600);
        assert_eq!(parse_hot_ttl_secs(Some("-3")), 600);
        assert_eq!(parse_hot_ttl_secs(Some("garbage")), 600);
        assert_eq!(parse_hot_ttl_secs(None), 600);
    }

    #[test]
    fn hot_globs_empty_string_disables_and_splits() {
        assert!(parse_hot_globs(Some("")).is_empty());
        assert_eq!(
            parse_hot_globs(Some(" **/a.yaml , , **/b.yaml ")),
            vec!["**/a.yaml".to_string(), "**/b.yaml".to_string()]
        );
        assert!(!parse_hot_globs(None).is_empty(), "defaults when unset");
    }

    #[test]
    fn render_hot_context_names_holder_and_unknown_ts() {
        let claim = HotClaim {
            path: "/r/.env".into(),
            principal_id: "loop:a".into(),
            expires_at: "2026-10-02T12:34:56Z".parse().unwrap(),
            last_write_seq: 1,
            last_write_ts: None,
        };
        let out = render_hot_context(Path::new("/r/.env"), &claim);
        assert!(out.contains("\u{1F512}"));
        assert!(out.contains("claimed by loop:a"));
        assert!(out.contains("last write unknown"));
        assert!(out.contains("claim until 2026-10-02T12:34:56Z"));
        assert!(out.contains("mini-ork concord send loop:a"));
    }
}
