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
//!
//! ## Disk truth (P1b)
//!
//! The P1 precheck warns when a **recorded** footprint shows another
//! principal wrote the file after the caller last read it. That misses
//! writers Concord never sees — shell redirects, `sed -i`, editors,
//! formatters, `git checkout`, humans. The P1b disk check fills that
//! hole: when the P1 path comes back clean (the caller has a prior
//! footprint and no foreign writer), load the newest footprint on the
//! path from any principal, stat the file now, and report an advisory
//! paragraph if the on-disk state diverges. Advisory only — never
//! feeds `permissionDecision`. Enabled by default; toggled by
//! `CONTEXTNEST_CONCORD_DISK_CHECK` and tuned with
//! `CONTEXTNEST_CONCORD_DISK_GRACE_MS`.

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::Json,
    routing::{get, post},
    Router,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::error;

use crate::api::coord_turn::{self, TurnInputWithHeaders};
use crate::services::coord_store::{
    CoordStore, CoordStoreResult, Freeze, HotClaim, HotClaimOutcome, OtherWriter, PrincipalUpsert,
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

/// How an owns-violation should be surfaced on the precheck (Concord P2d).
/// `Ask` and `Deny` contribute to `permissionDecision` (composed
/// strictest-wins with the hot claim); anything else (including unset)
/// is the audit-only default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnsMode {
    Audit,
    Ask,
    Deny,
}

/// Read `CONTEXTNEST_CONCORD_OWNS_MODE` fresh on every call: trim +
/// ascii-lowercase, `ask`/`deny` map to their modes, anything else
/// (bogus or unset) is [`OwnsMode::Audit`].
pub fn owns_mode() -> OwnsMode {
    parse_owns_mode(
        std::env::var("CONTEXTNEST_CONCORD_OWNS_MODE")
            .ok()
            .as_deref(),
    )
}

/// Pure parser behind [`owns_mode`]; anything but `ask`/`deny` is
/// `Audit`.
pub fn parse_owns_mode(raw: Option<&str>) -> OwnsMode {
    match raw.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("ask") => OwnsMode::Ask,
        Some("deny") => OwnsMode::Deny,
        _ => OwnsMode::Audit,
    }
}

/// Default implicit-scope globs used when
/// `CONTEXTNEST_CONCORD_OWNS_IMPLICIT_GLOBS` is unset. A worktree's
/// edits to paths matching this list are treated as covered by the
/// owns check, no audit row, no hand glyph, no metric bump, and no
/// permissionDecision (Concord P2e).
const DEFAULT_OWNS_IMPLICIT_GLOBS: &str = ".mini-ork/**";

/// The implicit-scope globs, read fresh from env on every call so test
/// suites can flip them without restarting the binary. An explicitly
/// set value is split on ',', trimmed, and stripped of empty entries —
/// so an empty string disables the implicit set. Unset →
/// [`DEFAULT_OWNS_IMPLICIT_GLOBS`].
pub fn owns_implicit_globs() -> Vec<String> {
    parse_owns_implicit_globs(
        std::env::var("CONTEXTNEST_CONCORD_OWNS_IMPLICIT_GLOBS")
            .ok()
            .as_deref(),
    )
}

/// Pure parser behind [`owns_implicit_globs`]: `None` (unset) → the
/// default `.mini-ork/**`; a set value is split on ',', trimmed, and
/// stripped of empties (so "" disables). Unit tests call this directly
/// — mutating the process env from parallel test threads raced
/// (`cargo test` runs tests concurrently).
pub fn parse_owns_implicit_globs(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or(DEFAULT_OWNS_IMPLICIT_GLOBS)
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Pure parser behind [`owns_own_kickoff_enabled`]. Only an exact `"0"`
/// (trimmed) disables the rule; unset or any other value enables it.
/// Mirrors [`parse_disk_check_enabled`].
pub fn parse_owns_own_kickoff_enabled(raw: Option<&str>) -> bool {
    !matches!(raw.map(|s| s.trim()), Some("0"))
}

/// Read `CONTEXTNEST_CONCORD_OWNS_OWN_KICKOFF` fresh on every call.
/// `=0` disables the own-kickoff implicit rule; anything else (or
/// unset) enables it.
pub fn owns_own_kickoff_enabled() -> bool {
    parse_owns_own_kickoff_enabled(
        std::env::var("CONTEXTNEST_CONCORD_OWNS_OWN_KICKOFF")
            .ok()
            .as_deref(),
    )
}

/// Pure matcher for the own-kickoff implicit rule (Concord P2e).
/// True iff `principal_id` starts with `agent:wt-<slug>` with a
/// non-empty slug, `rel` is under `kickoffs/`, ends in `.md`, and its
/// file name starts with `<slug>`. Matching only on the file name
/// (a plain `starts_with`) lets slug `vt1-mr-relations` cover
/// `kickoffs/vt1-mr-relations-r2.md` while rejecting
/// `kickoffs/auto/xvt1-mr-relations.md`.
pub fn own_kickoff_covers(principal_id: &str, rel: &str) -> bool {
    let slug = match principal_id.strip_prefix("agent:wt-") {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    if !rel.starts_with("kickoffs/") || !rel.ends_with(".md") {
        return false;
    }
    let file_name = match rel.rsplit_once('/') {
        Some((_, name)) => name,
        None => return false,
    };
    file_name.starts_with(slug)
}

/// Pure combinator that decides whether a target is implicitly covered
/// by the P2e implicit-scope rules. Reads `rel` against `implicit_globs`
/// first via [`glob_match`], then falls back to the own-kickoff matcher
/// when `own_kickoff` is enabled. Env I/O lives in the readers above;
/// this function stays pure so unit tests can drive it without
/// touching the process env.
pub fn owns_implicitly_covered(
    principal_id: &str,
    rel: &str,
    implicit_globs: &[String],
    own_kickoff: bool,
) -> bool {
    implicit_globs.iter().any(|g| glob_match(g, rel))
        || (own_kickoff && own_kickoff_covers(principal_id, rel))
}

/// True iff `entry` covers `rel` — either they are equal, `rel` is a
/// file/descendant of `entry` (next char in `rel` after `entry` is
/// `/`), or `entry` is a glob (re-uses the P2a [`glob_match`]).
/// A trailing `/` on `entry` is stripped so `"docs"` and `"docs/"`
/// behave identically.
pub fn owns_covers(entry: &str, rel: &str) -> bool {
    let entry = entry.trim_end_matches('/');
    let rel = rel.trim_end_matches('/');
    if entry == rel {
        return true;
    }
    if rel.len() > entry.len() && rel.starts_with(entry) && rel.as_bytes()[entry.len()] == b'/' {
        return true;
    }
    glob_match(entry, rel)
}

/// Render the audit/ask/deny text for an owns violation. Exact shape
/// the brief specifies: `[concord]` ascii header, the ✋ `\u{270B}`
/// hand, the worktree-relative path, the comma-joined claimed scope,
/// and the operator-facing instruction to re-scope the worktree
/// before editing.
pub fn render_owns_context(rel: &str, owns: &[String]) -> String {
    format!(
        "[concord] \u{270B} {} is outside this worktree's claimed scope ({}). Stay inside the claim, or re-scope the worktree before editing.",
        rel,
        owns.join(", "),
    )
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

// ─────────────────────── disk-truth (P1b) config ───────────────────────

/// Default grace window for the disk-truth check. Absorbs the lag of
/// the caller's own async PostToolUse footprint: Edit #1's write
/// footprint may not be stored yet when Edit #2's precheck fires, so a
/// just-recorded reference stat that matches the current stat up to
/// `DEFAULT_DISK_GRACE_MS` old is treated as "still our write".
const DEFAULT_DISK_GRACE_MS: u64 = 2000;

/// Pure parser behind [`disk_check_enabled`]. Only an exact `"0"`
/// (trimmed) disables the check; unset or any other value enables it.
pub fn parse_disk_check_enabled(raw: Option<&str>) -> bool {
    !matches!(raw.map(|s| s.trim()), Some("0"))
}

/// Pure parser behind [`disk_grace_ms`]. Non-negative integer; any
/// failure (missing, non-numeric, negative, fractional) falls back to
/// [`DEFAULT_DISK_GRACE_MS`].
pub fn parse_disk_grace_ms(raw: Option<&str>) -> u64 {
    raw.and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_DISK_GRACE_MS)
}

/// Read `CONTEXTNEST_CONCORD_DISK_CHECK` fresh on every call. `=0`
/// disables the disk-truth check; anything else (or unset) enables it.
pub fn disk_check_enabled() -> bool {
    parse_disk_check_enabled(
        std::env::var("CONTEXTNEST_CONCORD_DISK_CHECK")
            .ok()
            .as_deref(),
    )
}

/// Read `CONTEXTNEST_CONCORD_DISK_GRACE_MS` fresh on every call.
/// Invalid values fall back to [`DEFAULT_DISK_GRACE_MS`].
pub fn disk_grace_ms() -> u64 {
    parse_disk_grace_ms(
        std::env::var("CONTEXTNEST_CONCORD_DISK_GRACE_MS")
            .ok()
            .as_deref(),
    )
}

/// What kind of on-disk drift the precheck saw (Concord P1b). The
/// renderer maps each variant to a slightly different paragraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskDrift {
    /// File still exists but `(mtime, size)` differs from the
    /// reference and the change is older than the grace window.
    Changed,
    /// File is gone entirely. No grace window: a deleted file is
    /// always a deletion, regardless of how fresh the last footprint
    /// was — `mktemp` cleanup races don't help a future Edit.
    Deleted,
}

/// Decide whether `current` is a drift from `ref_mtime_ns`/`ref_size`.
/// Returns `None` for "no drift", `Some(DiskDrift::Changed)` for a
/// stale change past `grace_ms`, and `Some(DiskDrift::Deleted)` for a
/// file gone missing. The grace window is symmetric around `now_ns`:
/// a recent edit by the caller (whose PostToolUse footprint hasn't
/// landed yet) is suppressed, but so is a tiny clock skew on a network
/// filesystem. `i128` math prevents overflow even with extreme values
/// (mtime ~ year 2262 in ns).
pub fn classify_disk_drift(
    ref_mtime_ns: Option<i64>,
    ref_size: Option<i64>,
    current: (Option<i64>, Option<i64>),
    now_ns: i64,
    grace_ms: u64,
) -> Option<DiskDrift> {
    // No recorded stat → nothing to compare against.
    let ref_mtime = ref_mtime_ns?;
    let (cur_mtime, cur_size) = current;
    // File is gone. Always a deletion — a deleted file is gone, and
    // the grace window does not apply (a vanished file is a vanished
    // file regardless of how fresh the last footprint was).
    let cur_mtime = match cur_mtime {
        Some(m) => m,
        None => return Some(DiskDrift::Deleted),
    };
    // Identical stat → no drift.
    if cur_mtime == ref_mtime && cur_size == ref_size {
        return None;
    }
    // Symmetric grace window around `now_ns`. Suppresses a just-made
    // change (recent past) AND a tiny clock skew (recent future).
    let grace_ns = grace_ms as i128 * 1_000_000;
    let diff_ns = (now_ns as i128 - cur_mtime as i128).abs();
    if diff_ns < grace_ns {
        return None;
    }
    Some(DiskDrift::Changed)
}

/// Render the unrecorded-change advisory. Exact shape the brief
/// specifies: `[concord]` header, the backticked path, the
/// `"<ts RFC3339 Z>"` of the latest recorded footprint, the writer
/// attribution line, and the U+2014 em dash before "Re-read it before
/// editing." For `Deleted`, "changed on disk" becomes "was deleted on
/// disk" — the rest of the sentence is identical so the model has the
/// same instruction regardless of variant.
///
/// When `attr` is `Some` (Concord P1d), one attribution sentence is
/// appended: `Probably <principal> (<harness>, <cwd basename>) ran
/// \`<command, first 120 chars>\` at <ts>.` — with "your own shell
/// command" substituted for the `<principal> (<harness>, <cwd
/// basename>) ran` part when the exec principal is in the caller's
/// lineage.
fn render_unrecorded_context(
    path: &Path,
    drift: DiskDrift,
    last_ts: DateTime<Utc>,
    attr: Option<&ExecAttribution>,
) -> String {
    let ts = last_ts.to_rfc3339_opts(SecondsFormat::Secs, true);
    let verb = match drift {
        DiskDrift::Changed => "changed on disk",
        DiskDrift::Deleted => "was deleted on disk",
    };
    let mut out = format!(
        "[concord] `{}` {} after the last change any agent recorded ({}), and no agent recorded this write \u{2014} a shell command (possibly your own), an editor, a formatter, or a git operation. Re-read it before editing.",
        path.display(),
        verb,
        ts,
    );
    if let Some(a) = attr {
        let harness = a.harness.as_deref().unwrap_or("unknown");
        let cwd_base = Path::new(&a.cwd)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let detail: String = a.detail.chars().take(120).collect();
        let ats = a.ts.to_rfc3339_opts(SecondsFormat::Secs, true);
        let sentence = if a.is_self {
            format!("Probably your own shell command `{}` at {}.", detail, ats)
        } else {
            format!(
                "Probably {} ({}, {}) ran `{}` at {}.",
                a.principal_id, harness, cwd_base, detail, ats
            )
        };
        out.push_str("\n\n");
        out.push_str(&sentence);
    }
    out
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

/// Canonicalize a Bash cwd the same way [`resolve_tool_path`]
/// canonicalizes file paths: `std::fs::canonicalize` when it succeeds
/// (macOS `/var` → `/private/var`, symlink resolution), else the raw
/// path. The exec-footprint branch stores this as the footprint `path`
/// so the P1d ancestor check and the read/write file paths share one
/// canonicalization rule — if they diverged, the ancestor match would
/// silently never fire on macOS.
pub fn canonicalize_cwd(cwd: &str) -> PathBuf {
    let p = PathBuf::from(cwd);
    match std::fs::canonicalize(&p) {
        Ok(c) => c,
        Err(_) => p,
    }
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
    let cwd_str = input.inner.cwd.clone().unwrap_or_default();
    let cwd_path = Path::new(&cwd_str);

    // A Bash call records an exec footprint (command + cwd, never
    // stat'ed, never a write). The file tools record read/write
    // footprints on the resolved file path. Both share the
    // resolve/upsert/bind/record tail below.
    let (op, path, detail): (&'static str, PathBuf, Option<String>) = if tool_name == "Bash" {
        let command = input
            .inner
            .extra
            .get("tool_input")
            .and_then(Value::as_object)
            .and_then(|ti| ti.get("command"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let detail = match command {
            Some(cmd) if !cmd.trim().is_empty() => cmd,
            _ => return Json(json!({ "recorded": false })),
        };
        ("exec", canonicalize_cwd(&cwd_str), Some(detail))
    } else {
        let op = match file_tool_op(tool_name) {
            Some(o) => o,
            None => return Json(json!({ "recorded": false })),
        };
        let path = match resolve_tool_path(tool_name, &input.inner.extra, cwd_path) {
            Some(p) => p,
            None => return Json(json!({ "recorded": false })),
        };
        (op, path, None)
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

    // Exec rows skip stat_file entirely (there is no file to stat) —
    // the (mtime_ns, size) tuple is forced to (None, None).
    let (mtime_ns, size) = if op == "exec" {
        (None, None)
    } else {
        stat_file(&path)
    };
    let worker_id = if session_id.is_empty() {
        principal_id.clone()
    } else {
        session_id.clone()
    };
    let seq = match &detail {
        Some(d) => store.record_exec_footprint(&principal_id, &worker_id, &path_str, d),
        None => store.record_footprint(&principal_id, &worker_id, op, &path_str, mtime_ns, size),
    };
    let seq = match seq {
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
/// shared-config paths (P2). When the P1 path is clean (caller has a
/// premise and no recorded foreign writer), the P1b disk check
/// compares the newest recorded footprint on the path with the
/// current stat and appends an advisory paragraph when an unrecorded
/// writer changed the file (Concord P1b).
///
/// Always answers 200. `permissionDecision` is set — to `"ask"` or
/// `"deny"`, never `"allow"` — only when `hot` is some AND
/// `CONTEXTNEST_CONCORD_HOT_MODE` is `ask`/`deny`; otherwise the key is
/// absent (see the module doc). The unrecorded advisory is
/// strictly informational and never feeds `permissionDecision`. Every
/// request — no-op tools, unresolved paths or principals, and store
/// errors included — counts toward `coord_precheck_total`;
/// `coord_precheck_warn` counts only the P1 stale-premise warnings;
/// `coord_hot_conflicts_total` counts only the hot conflicts;
/// `coord_precheck_unrecorded_total` counts only the P1b unrecorded
/// disk hits.
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
        if outcome.owns.is_some() {
            m.coord_owns_violations_total = m.coord_owns_violations_total.saturating_add(1);
        }
        if outcome.unrecorded.is_some() {
            m.coord_precheck_unrecorded_total = m.coord_precheck_unrecorded_total.saturating_add(1);
        }
    }
    let now = Utc::now();
    let path_str = outcome
        .path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let caller = outcome.principal_id.as_deref();
    let mut any_escalated = false;

    // Each fired signal records one overlap row (Concord P4) and carries
    // its own ` (overlap O-<id>)` suffix on its rendered line — the
    // suffix never lands on the joined blob, so every signal keeps its
    // own id.
    let p1_text = match (&outcome.path, outcome.warn) {
        (Some(p), true) => {
            let mut text = render_precheck_context(p, &outcome.others);
            if let (Some(pid), Some(subject), Some(other)) =
                (caller, path_str.as_deref(), outcome.others.first())
            {
                text.push_str(&overlap_suffix(
                    &services.coord_store,
                    "stale",
                    subject,
                    pid,
                    Some(&other.principal_id),
                    now,
                    &mut any_escalated,
                ));
            }
            text
        }
        _ => String::new(),
    };
    let unrecorded_text = match (&outcome.path, &outcome.unrecorded) {
        (Some(p), Some((drift, ts, attr))) => {
            let mut text = render_unrecorded_context(p, *drift, *ts, attr.as_ref());
            if let (Some(pid), Some(subject)) = (caller, path_str.as_deref()) {
                text.push_str(&overlap_suffix(
                    &services.coord_store,
                    "unrecorded",
                    subject,
                    pid,
                    None,
                    now,
                    &mut any_escalated,
                ));
            }
            text
        }
        _ => String::new(),
    };
    let hot_text = match (&outcome.path, &outcome.hot) {
        (Some(p), Some(claim)) => {
            let mut text = render_hot_context(p, claim);
            if let (Some(pid), Some(subject)) = (caller, path_str.as_deref()) {
                text.push_str(&overlap_suffix(
                    &services.coord_store,
                    "hot",
                    subject,
                    pid,
                    Some(&claim.principal_id),
                    now,
                    &mut any_escalated,
                ));
            }
            text
        }
        _ => String::new(),
    };
    let owns_text = match &outcome.owns {
        Some(hit) => {
            let mut text = render_owns_context(&hit.rel, &hit.owns);
            if let (Some(pid), Some(subject)) = (caller, path_str.as_deref()) {
                text.push_str(&overlap_suffix(
                    &services.coord_store,
                    "owns",
                    subject,
                    pid,
                    None,
                    now,
                    &mut any_escalated,
                ));
            }
            text
        }
        None => String::new(),
    };
    // Compose the four advisory strings in [p1, unrecorded, hot, owns]
    // order so existing hot/p1 reasons (and tests that read them) stay
    // byte-identical when only one branch fires.
    let additional_context = join_context(&[&p1_text, &unrecorded_text, &hot_text, &owns_text]);

    // Escalation is counted once per request, after every store call has
    // returned and the metrics lock is free to take.
    if any_escalated {
        let mut m = services.coord_metrics.write().await;
        m.coord_overlaps_escalated_total = m.coord_overlaps_escalated_total.saturating_add(1);
    }

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

    // Compose the hot + owns decisions strictest-wins, then overlay the
    // freeze decision (Concord P4), which is unconditional — a matching
    // freeze always Denies, regardless of hot/owns mode. Hot/owns
    // contribute ONLY when their check fired; Audit / Warn map to None.
    // The unrecorded signal is advisory only — never participates in the
    // permissionDecision composition.
    #[derive(PartialOrd, Ord, Eq, PartialEq, Clone, Copy)]
    enum Decision {
        None,
        Ask,
        Deny,
    }
    let hot_decision = if outcome.hot.is_some() {
        match hot_mode() {
            HotMode::Ask => Decision::Ask,
            HotMode::Deny => Decision::Deny,
            HotMode::Warn => Decision::None,
        }
    } else {
        Decision::None
    };
    let owns_decision = if outcome.owns.is_some() {
        match owns_mode() {
            OwnsMode::Ask => Decision::Ask,
            OwnsMode::Deny => Decision::Deny,
            OwnsMode::Audit => Decision::None,
        }
    } else {
        Decision::None
    };
    let freeze_decision = if outcome.freeze.is_some() {
        Decision::Deny
    } else {
        Decision::None
    };
    let freeze_text = match (&outcome.freeze, path_str.as_deref()) {
        (Some(f), Some(p)) => render_freeze_context(p, f),
        _ => String::new(),
    };
    let decision = std::cmp::max(std::cmp::max(hot_decision, owns_decision), freeze_decision);

    // Build hookSpecificOutput as a Map so `permissionDecision` can be
    // inserted conditionally — absent (not null, not "allow") for
    // audit/warn or when neither signal fired. There is NO Allow path.
    let mut hook = serde_json::Map::new();
    hook.insert("hookEventName".to_string(), json!("PreToolUse"));
    hook.insert("additionalContext".to_string(), json!(additional_context));
    if decision != Decision::None {
        let value = match decision {
            Decision::Ask => "ask",
            Decision::Deny => "deny",
            Decision::None => unreachable!(),
        };
        let mut reason_parts: Vec<String> = Vec::new();
        if hot_decision != Decision::None && !hot_text.is_empty() {
            reason_parts.push(hot_text.clone());
        }
        if owns_decision != Decision::None && !owns_text.is_empty() {
            reason_parts.push(owns_text.clone());
        }
        if freeze_decision != Decision::None && !freeze_text.is_empty() {
            reason_parts.push(freeze_text.clone());
        }
        hook.insert("permissionDecision".to_string(), json!(value));
        hook.insert(
            "permissionDecisionReason".to_string(),
            json!(reason_parts.join("\n\n")),
        );
    }

    Json(json!({
        "warn": outcome.warn,
        "others": others_json,
        "hot_conflict": outcome.hot.is_some(),
        "owns_violation": outcome.owns.is_some(),
        "unrecorded_change": outcome.unrecorded.is_some(),
        "hookSpecificOutput": hook,
    }))
}

/// Join non-empty strings in `parts` with two newlines between them.
/// Empty parts are dropped so callers can pass `&[&str; 3]` (or any
/// slice of `&str`) directly without a 4-arm match.
fn join_context(parts: &[&str]) -> String {
    let mut out = String::new();
    for p in parts.iter() {
        if p.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(p);
    }
    out
}

/// Record one overlap notice and return the ` (overlap O-<id>)` suffix to
/// append to the matching signal's rendered line. On a store error the
/// suffix is empty (the signal still renders, just without a tracked id)
/// and the error is logged — the hook must never block.
fn overlap_suffix(
    store: &CoordStore,
    kind: &str,
    subject: &str,
    a: &str,
    b: Option<&str>,
    now: DateTime<Utc>,
    any_escalated: &mut bool,
) -> String {
    match store.upsert_overlap(kind, subject, a, b, now) {
        Ok(item) => {
            if item.just_escalated {
                *any_escalated = true;
            }
            format!(" (overlap O-{})", item.id)
        }
        Err(e) => {
            error!(error = %e, kind = %kind, "coord_precheck: upsert_overlap failed");
            String::new()
        }
    }
}

/// Render the P4 freeze deny reason: `⛔ <path> is frozen by <by>:
/// <reason> (until <expires_at>)`.
fn render_freeze_context(path: &str, f: &Freeze) -> String {
    format!(
        "⛔ {path} is frozen by {}: {} (until {})",
        f.by,
        f.reason,
        f.expires_at.to_rfc3339_opts(SecondsFormat::Secs, true)
    )
}

/// The pure decision behind `coord_precheck`. `warn`/`others` carry the
/// P1 stale-premise signal; `hot` carries the P2 hot-conflict claim;
/// `owns` carries the P2d owns-scope audit hit (some when the target
/// is outside the worktree's claim, none otherwise); `unrecorded`
/// carries the P1b disk-truth drift (when the file on disk no longer
/// matches the newest recorded footprint on the path). The P2e
/// implicit scope — a worktree's own kickoff and the implicit-glob set
/// (default `.mini-ork/**`) — short-circuits the owns arm to `None`
/// before the audit row is written.
///
/// The hot and owns checks both run BEFORE the P1 no-prior-footprint
/// early return so a caller that has never touched the path still sees
/// live claims / owns violations held by an outsider. The owns check
/// runs even when the caller is unresolvable — the audit row records
/// the unbound caller as `NULL`. Store errors are logged and treated
/// as no-ops so the hook can never block Claude Code.
struct PrecheckOutcome {
    warn: bool,
    others: Vec<OtherWriter>,
    path: Option<PathBuf>,
    hot: Option<HotClaim>,
    owns: Option<OwnsHit>,
    /// `(drift, ts, attribution)` when the P1b disk check fired:
    /// on-disk state diverged from the latest recorded footprint on
    /// the path, past the grace window (or the file is gone).
    /// `attribution` is the most plausible exec footprint that caused
    /// the change (Concord P1d), `None` when no candidate matched.
    /// `None` otherwise (no drift).
    unrecorded: Option<(DiskDrift, DateTime<Utc>, Option<ExecAttribution>)>,
    /// The resolved caller principal, `None` when unresolvable (an
    /// unbound session). Carried so the handler can key each overlap
    /// notice to its caller without re-resolving.
    principal_id: Option<String>,
    /// The live, non-exempt freeze that matched the target path (Concord
    /// P4), `None` when no freeze applies. Its presence forces the
    /// composed `permissionDecision` to `"deny"`.
    freeze: Option<Freeze>,
}

/// The most plausible shell command (an exec footprint) that caused an
/// unrecorded disk change (Concord P1d). `cwd` is the command's
/// canonicalized working directory, `detail` the command string, `ts`
/// the exec row's timestamp. `is_self` is true when the exec row's
/// principal is in the caller's lineage (so the renderer can say "your
/// own shell command" instead of naming a principal).
struct ExecAttribution {
    principal_id: String,
    harness: Option<String>,
    cwd: String,
    detail: String,
    ts: DateTime<Utc>,
    is_self: bool,
}

/// The P1 stale-premise query result: whether a foreign writer exists,
/// the deduped writers, the caller's last footprint seq on the path,
/// and the caller's lineage (reused by the P1d attribution self-check).
struct P1Lookup {
    warn: bool,
    others: Vec<OtherWriter>,
    last: Option<i64>,
    excl: HashSet<String>,
}

/// One owns-scope audit hit (Concord P2d). `worktree_principal` is the
/// resolved root; `rel` is the target stripped of that root with
/// forward-slash separators; `owns` is the normalized Vec the
/// principal claimed.
///
/// `worktree_principal` is already passed to
/// `CoordStore::record_owns_violation` during the decision, so the
/// field on `OwnsHit` is not read by the handler — kept for parity
/// with the audit row and to make log/debug output self-describing.
#[derive(Debug, Clone)]
struct OwnsHit {
    #[allow(dead_code)]
    worktree_principal: String,
    rel: String,
    owns: Vec<String>,
}

/// Scan exec footprints newer than `after_seq` for the most plausible
/// shell command that changed `path` (Concord P1d). A candidate matches
/// when its cwd is `path`'s parent (or an ancestor of it) AND its
/// command mentions the file — either by bare file name or by the
/// path's cwd-relative form. Newest match wins. Store errors log and
/// degrade to `None` (advisory only). `is_self` is decided against the
/// caller's already-computed lineage set.
fn attribute_exec_footprint(
    store: &CoordStore,
    after_seq: i64,
    path: &Path,
    excl: &HashSet<String>,
) -> Option<ExecAttribution> {
    let candidates = match store.exec_footprints_after(after_seq, 32) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "coord_precheck: exec_footprints_after failed");
            return None;
        }
    };
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let parent = path.parent().unwrap_or(path);
    for c in candidates {
        let cwd = Path::new(&c.path);
        if !parent.starts_with(cwd) {
            continue;
        }
        let detail = c.detail.as_deref().unwrap_or("");
        if detail.is_empty() {
            continue;
        }
        // Mention the file: bare name, or the cwd-relative path form
        // (for commands run in an ancestor directory).
        let rel = path.strip_prefix(cwd).ok().and_then(|r| r.to_str());
        let mentions = (!file_name.is_empty() && detail.contains(file_name))
            || rel.is_some_and(|r| !r.is_empty() && detail.contains(r));
        if !mentions {
            continue;
        }
        return Some(ExecAttribution {
            is_self: excl.contains(&c.principal_id),
            principal_id: c.principal_id,
            harness: c.harness,
            cwd: c.path,
            detail: c.detail.unwrap_or_default(),
            ts: c.ts,
        });
    }
    None
}

fn precheck_decision(store: &CoordStore, input: &TurnInputWithHeaders) -> PrecheckOutcome {
    let noop = PrecheckOutcome {
        warn: false,
        others: Vec::new(),
        path: None,
        hot: None,
        owns: None,
        unrecorded: None,
        principal_id: None,
        freeze: None,
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

    // Resolve caller as Option<String> (Some on success, None on
    // Ok(None) or Err) so the owns check still runs when the principal
    // resolution fails. An audit row with `caller_principal = NULL` is
    // the documented shape for an unbound session.
    let caller_principal: Option<String> = match resolve_hook_principal(store, input) {
        Ok(Some(p)) => Some(p),
        Ok(None) => None,
        Err(e) => {
            error!(error = %e, "coord_precheck: principal resolution failed");
            None
        }
    };

    // Owns-scope check (Concord P2d). Runs BEFORE the caller-unresolved
    // early return so an unbound session still gets the audit row.
    let owns: Option<OwnsHit> = match store.owning_worktree_principal(&path) {
        Ok(Some(wt)) if !wt.owns.is_empty() => {
            // The store computed `rel` from the canonicalized target it
            // matched against; re-deriving it from the raw path broke for
            // new files under a symlinked root (macOS /var → /private/var).
            let rel_str = wt.rel.clone();
            // P2e implicit-scope: a worktree's own kickoff and the
            // implicit-glob set (default `.mini-ork/**`) are covered,
            // so the arm short-circuits to None BEFORE the audit row is
            // written — that suppresses the glyph, the row, the metric
            // bump, and the permissionDecision in one shot.
            let implicit = owns_implicitly_covered(
                &wt.principal_id,
                &rel_str,
                &owns_implicit_globs(),
                owns_own_kickoff_enabled(),
            );
            let covered = wt.owns.iter().any(|e| owns_covers(e, &rel_str));
            if implicit || covered {
                None
            } else {
                if let Err(e) = store.record_owns_violation(
                    &wt.principal_id,
                    &rel_str,
                    caller_principal.as_deref(),
                ) {
                    error!(error = %e, worktree_principal = %wt.principal_id, "coord_precheck: record_owns_violation failed");
                }
                Some(OwnsHit {
                    worktree_principal: wt.principal_id,
                    rel: rel_str,
                    owns: wt.owns,
                })
            }
        }
        Ok(_) => None,
        Err(e) => {
            error!(error = %e, "coord_precheck: owning_worktree_principal failed");
            None
        }
    };

    // Freeze check (Concord P4). A live, non-exempt freeze forces the
    // precheck decision to Deny unconditionally. It runs BEFORE the
    // unbound-caller early return: an unidentified session is an outsider
    // (empty lineage), so a freeze must stop it too. The lookup stays
    // fail-safe: a store error (list or lineage) logs and degrades to
    // `None` — never to an empty-lineage default for a KNOWN caller, which
    // would wrongly deny the freezer's own lineage.
    let freeze_path = path.to_string_lossy().to_string();
    let freeze: Option<Freeze> = (|| -> CoordStoreResult<Option<Freeze>> {
        let freezes = store.list_freezes(Utc::now())?;
        if freezes.is_empty() {
            return Ok(None);
        }
        let lineage: HashSet<String> = match caller_principal.as_deref() {
            Some(p) => store.lineage(p, LINEAGE_MAX_HOPS)?,
            None => HashSet::new(),
        };
        Ok(freezes
            .into_iter()
            .find(|f| glob_match(&f.glob, &freeze_path) && !lineage.contains(&f.by)))
    })()
    .unwrap_or_else(|e| {
        error!(error = %e, caller = ?caller_principal, "coord_precheck: freeze lookup failed");
        None
    });

    let principal_id = match caller_principal {
        Some(p) => p,
        None => {
            return PrecheckOutcome {
                warn: false,
                others: Vec::new(),
                path: Some(path),
                hot: None,
                owns,
                unrecorded: None,
                principal_id: None,
                freeze,
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

    let outcome: CoordStoreResult<P1Lookup> = (|| {
        let last = match store.last_footprint_seq(&principal_id, &path_str)? {
            Some(s) => s,
            None => {
                return Ok(P1Lookup {
                    warn: false,
                    others: Vec::new(),
                    last: None,
                    excl: HashSet::new(),
                })
            }
        };
        let excl: HashSet<String> = store.lineage(&principal_id, LINEAGE_MAX_HOPS)?;
        let others = store.writes_after(&path_str, last, &excl, 3)?;
        Ok(P1Lookup {
            warn: !others.is_empty(),
            others,
            last: Some(last),
            excl,
        })
    })();
    let P1Lookup {
        warn,
        others,
        last,
        excl,
    } = match outcome {
        Ok(l) => l,
        Err(e) => {
            error!(error = %e, principal_id = %principal_id, "coord_precheck: store query failed");
            P1Lookup {
                warn: false,
                others: Vec::new(),
                last: None,
                excl: HashSet::new(),
            }
        }
    };

    // Disk-truth check (Concord P1b). Only fires when the P1 path was
    // clean: caller has a prior footprint on the path AND no recorded
    // foreign writer. Compares the newest recorded footprint on the
    // path (any principal) with the current stat — past the grace
    // window, this is an unrecorded write. Disabled by setting
    // CONTEXTNEST_CONCORD_DISK_CHECK=0. Errors log and degrade to
    // "no unrecorded change" so the hook can never block.
    //
    // A `Deleted` drift on a `Write` tool is silently dropped: a
    // Write replaces the whole file, so a prior deletion (worktree
    // rollback, then re-create) breaks no premise — there is no
    // partial update to lose. `Changed` stays reportable for every
    // tool (a real lost update), and `Deleted` still fires for Edit,
    // MultiEdit, and NotebookEdit.
    let unrecorded: Option<(DiskDrift, DateTime<Utc>, Option<ExecAttribution>)> = if !others
        .is_empty()
        || last.is_none()
        || !disk_check_enabled()
    {
        None
    } else {
        match store.latest_footprint(&path_str) {
            Ok(Some(fp)) => {
                let now_ns: i64 = match SystemTime::now().duration_since(UNIX_EPOCH) {
                    Ok(d) => {
                        let n = d.as_nanos();
                        if n > i64::MAX as u128 {
                            i64::MAX
                        } else {
                            n as i64
                        }
                    }
                    Err(_) => 0,
                };
                let current = stat_file(&path);
                match classify_disk_drift(fp.mtime_ns, fp.size, current, now_ns, disk_grace_ms()) {
                    Some(drift) if drift == DiskDrift::Deleted && tool_name == "Write" => None,
                    Some(drift) => {
                        let attribution = attribute_exec_footprint(store, fp.seq, &path, &excl);
                        Some((drift, fp.ts, attribution))
                    }
                    None => None,
                }
            }
            Ok(None) => None,
            Err(e) => {
                error!(error = %e, principal_id = %principal_id, "coord_precheck: latest_footprint failed");
                None
            }
        }
    };

    PrecheckOutcome {
        warn,
        others,
        path: Some(path),
        hot,
        owns,
        unrecorded,
        principal_id: Some(principal_id),
        freeze,
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

/// `GET /api/v1/coord/owns-violations?since=<seq>` — Concord P2d
/// observability endpoint. Returns rows with `seq > since` (default 0)
/// in ascending order (newest last), capped at 200. `next_seq` is the
/// last returned row's seq, or `since` when the result is empty — a
/// forward cursor. A missing, unparseable, or negative `since` clamps
/// to 0. Always responds 200 even on store errors (logged via `error!`).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OwnsViolationsQuery {
    #[serde(default)]
    since: Option<String>,
}

pub async fn coord_owns_violations(
    State(services): State<ContextNestServices>,
    Query(q): Query<OwnsViolationsQuery>,
) -> Json<Value> {
    let since: i64 = q
        .since
        .as_deref()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .map(|n| n.max(0))
        .unwrap_or(0);
    match services.coord_store.list_owns_violations(since, 200) {
        Ok(rows) => {
            let next_seq = rows.last().map(|r| r.seq).unwrap_or(since);
            let arr: Vec<Value> = rows
                .iter()
                .map(|r| {
                    json!({
                        "seq": r.seq,
                        "worktree_principal": r.worktree_principal,
                        "path": r.path,
                        "caller_principal": r.caller_principal,
                        "ts": r.ts.to_rfc3339_opts(SecondsFormat::Secs, true),
                    })
                })
                .collect();
            Json(json!({ "violations": arr, "next_seq": next_seq }))
        }
        Err(e) => {
            error!(error = %e, "coord_owns_violations: list_owns_violations failed");
            Json(json!({ "violations": [], "next_seq": since }))
        }
    }
}

/// Mount the P1 + P2 endpoints. Merged into `base_router` by `simple.rs`.
pub fn create_coord_footprints_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/api/v1/coord/footprints", post(coord_footprints))
        .route("/api/v1/coord/precheck", post(coord_precheck))
        .route("/api/v1/coord/hot-claims", get(coord_hot_claims))
        .route("/api/v1/coord/owns-violations", get(coord_owns_violations))
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

    // ────────────── owns-scope helpers (Concord P2d) ──────────────

    #[test]
    fn parse_owns_mode_falls_back_to_audit() {
        assert_eq!(parse_owns_mode(Some("ASK")), OwnsMode::Ask);
        assert_eq!(parse_owns_mode(Some("deny")), OwnsMode::Deny);
        assert_eq!(parse_owns_mode(Some("  ask  ")), OwnsMode::Ask);
        assert_eq!(parse_owns_mode(Some("bogus")), OwnsMode::Audit);
        assert_eq!(parse_owns_mode(None), OwnsMode::Audit);
    }

    #[test]
    fn owns_covers_supports_equality_dir_prefix_and_glob() {
        // Equality.
        assert!(owns_covers("src/a.rs", "src/a.rs"));
        // Dir-prefix: `docs` covers `docs/x/y.md` because the next
        // char after "docs" is '/'. Trailing slash is also OK — the
        // matcher strips it before comparing.
        assert!(owns_covers("docs", "docs/x/y.md"));
        assert!(owns_covers("docs/", "docs/x/y.md"));
        // Boundary near-miss: `docs` must NOT cover `docsx/y.md`.
        assert!(!owns_covers("docs", "docsx/y.md"));
        // Glob (re-uses glob_match from P2a).
        assert!(owns_covers("tests/coord_*.rs", "tests/coord_owns_test.rs"));
        assert!(!owns_covers("tests/coord_*.rs", "tests/foo.rs"));
    }

    #[test]
    fn render_owns_context_lists_scope_and_instructs() {
        let out = render_owns_context("src/b.rs", &["src/a.rs".to_string(), "docs".to_string()]);
        assert!(out.contains("\u{270B}"));
        assert!(out.contains("src/b.rs"));
        assert!(out.contains("src/a.rs, docs"));
        assert!(out.contains("Stay inside the claim"));
        assert!(out.contains("re-scope the worktree"));
    }

    // ────────────── owns implicit scope (Concord P2e) ──────────────

    #[test]
    fn parse_owns_implicit_globs_none_default_empty_and_trim() {
        // Unset → default.
        assert_eq!(
            parse_owns_implicit_globs(None),
            vec![".mini-ork/**".to_string()]
        );
        // Empty string disables the implicit set.
        assert!(parse_owns_implicit_globs(Some("")).is_empty());
        // Trim + drop empties.
        assert_eq!(
            parse_owns_implicit_globs(Some(" a/** , , b/*.log ")),
            vec!["a/**".to_string(), "b/*.log".to_string()]
        );
    }

    #[test]
    fn parse_owns_own_kickoff_enabled_only_exact_zero_disables() {
        assert!(parse_owns_own_kickoff_enabled(None));
        assert!(!parse_owns_own_kickoff_enabled(Some("0")));
        assert!(!parse_owns_own_kickoff_enabled(Some(" 0 ")));
        assert!(parse_owns_own_kickoff_enabled(Some("1")));
        assert!(parse_owns_own_kickoff_enabled(Some("false")));
    }

    #[test]
    fn own_kickoff_covers_positive_and_negative_cases() {
        // Positives: file name starts with the slug.
        assert!(own_kickoff_covers(
            "agent:wt-vt1-mr-relations",
            "kickoffs/auto/vt1-mr-relations.md"
        ));
        assert!(own_kickoff_covers(
            "agent:wt-vt1-mr-relations",
            "kickoffs/vt1-mr-relations-r2.md"
        ));

        // Negatives: wrong dir / wrong slug / wrong extension / wrong id form.
        assert!(!own_kickoff_covers(
            "agent:wt-vt1-mr-relations",
            "kickoffs/auto/other.md"
        ));
        assert!(!own_kickoff_covers(
            "agent:wt-vt1-mr-relations",
            "kickoffs/auto/xvt1-mr-relations.md"
        ));
        assert!(!own_kickoff_covers(
            "agent:wt-vt1-mr-relations",
            "docs/vt1-mr-relations.md"
        ));
        assert!(!own_kickoff_covers(
            "agent:wt-vt1-mr-relations",
            "kickoffs/auto/vt1-mr-relations.txt"
        ));
        // Non-`agent:wt-` principal id → no own-kickoff coverage.
        assert!(!own_kickoff_covers(
            "loop:vt1-mr-relations",
            "kickoffs/auto/vt1-mr-relations.md"
        ));
        // Empty slug (bare `agent:wt-`) → no own-kickoff coverage.
        assert!(!own_kickoff_covers("agent:wt-", "kickoffs/auto/.md"));
    }

    #[test]
    fn owns_implicitly_covered_matches_glob_or_own_kickoff() {
        // Default implicit globs cover a run-artifact path.
        let defaults = parse_owns_implicit_globs(None);
        assert!(owns_implicitly_covered(
            "agent:wt-vt1",
            ".mini-ork/runs/r1/impl.log",
            &defaults,
            false
        ));
        // Empty globs + own_kickoff=false cover neither the run-artifact
        // nor a same-named kickoff.
        let empty: Vec<String> = vec![];
        assert!(!owns_implicitly_covered(
            "agent:wt-vt1",
            ".mini-ork/runs/r1/impl.log",
            &empty,
            false
        ));
        assert!(!owns_implicitly_covered(
            "agent:wt-vt1",
            "kickoffs/auto/vt1.md",
            &empty,
            false
        ));
        // Empty globs + own_kickoff=true still covers the matching kickoff.
        assert!(owns_implicitly_covered(
            "agent:wt-vt1",
            "kickoffs/auto/vt1.md",
            &empty,
            true
        ));
    }

    // ────────────── disk-truth (P1b) ──────────────

    #[test]
    fn parse_disk_check_enabled_treats_only_exact_zero_as_off() {
        assert!(!parse_disk_check_enabled(Some("0")));
        assert!(!parse_disk_check_enabled(Some(" 0 ")));
        assert!(parse_disk_check_enabled(None));
        assert!(parse_disk_check_enabled(Some("")));
        assert!(parse_disk_check_enabled(Some("1")));
        assert!(parse_disk_check_enabled(Some("false")));
    }

    #[test]
    fn parse_disk_grace_ms_falls_back_on_garbage() {
        assert_eq!(parse_disk_grace_ms(None), 2000);
        assert_eq!(parse_disk_grace_ms(Some("abc")), 2000);
        assert_eq!(parse_disk_grace_ms(Some("-5")), 2000);
        assert_eq!(parse_disk_grace_ms(Some("1.5")), 2000);
        assert_eq!(parse_disk_grace_ms(Some("0")), 0);
        assert_eq!(parse_disk_grace_ms(Some(" 250 ")), 250);
        assert_eq!(parse_disk_grace_ms(Some("600000")), 600000);
    }

    #[test]
    fn classify_disk_drift_none_for_no_reference() {
        assert!(classify_disk_drift(None, None, (Some(1), Some(2)), 1_000_000_000, 2000).is_none());
        assert!(
            classify_disk_drift(None, Some(7), (Some(1), Some(2)), 1_000_000_000, 2000).is_none(),
            "ref_size alone is not enough — ref_mtime is the gate"
        );
    }

    #[test]
    fn classify_disk_drift_none_when_stats_match() {
        let now = 10_000_000_000_i64;
        assert_eq!(
            classify_disk_drift(Some(7), Some(42), (Some(7), Some(42)), now, 2000),
            None
        );
    }

    #[test]
    fn classify_disk_drift_deleted_ignores_grace() {
        let now = 10_000_000_000_i64;
        // File vanished — Deleted even with a 1-hour grace.
        assert_eq!(
            classify_disk_drift(Some(7), Some(42), (None, None), now, 3_600_000),
            Some(DiskDrift::Deleted)
        );
        assert_eq!(
            classify_disk_drift(Some(7), Some(42), (None, None), now, 0),
            Some(DiskDrift::Deleted)
        );
    }

    #[test]
    fn classify_disk_drift_size_change_past_grace_is_changed() {
        let now = 10_000_000_000_i64;
        assert_eq!(
            classify_disk_drift(Some(7), Some(42), (Some(8), Some(43)), now, 2000),
            Some(DiskDrift::Changed)
        );
    }

    #[test]
    fn classify_disk_drift_suppresses_within_window_in_both_directions() {
        let now = 10_000_000_000_i64;
        // Grace 0 — any tiny diff reports.
        assert_eq!(
            classify_disk_drift(Some(7), Some(42), (Some(now), Some(42)), now, 0),
            Some(DiskDrift::Changed)
        );
        // Grace 2000ms — an mtime 1ms in the past is within the window.
        assert_eq!(
            classify_disk_drift(
                Some(7),
                Some(42),
                (Some(now - 1_000_000), Some(42)),
                now,
                2000
            ),
            None
        );
        // Future 10s — way past grace 0.
        assert_eq!(
            classify_disk_drift(
                Some(7),
                Some(42),
                (Some(now + 10 * 1_000_000_000), Some(42)),
                now,
                0
            ),
            Some(DiskDrift::Changed)
        );
        // Future 10s — past grace 2000ms too.
        assert_eq!(
            classify_disk_drift(
                Some(7),
                Some(42),
                (Some(now + 10 * 1_000_000_000), Some(42)),
                now,
                2000
            ),
            Some(DiskDrift::Changed)
        );
        // Future 1s — within grace 2000ms.
        assert_eq!(
            classify_disk_drift(
                Some(7),
                Some(42),
                (Some(now + 1_000_000_000), Some(42)),
                now,
                2000
            ),
            None
        );
    }

    #[test]
    fn render_unrecorded_context_changed_uses_present_tense() {
        let ts: DateTime<Utc> = "2026-10-03T12:34:56Z".parse().unwrap();
        let out = render_unrecorded_context(Path::new("/work/f.txt"), DiskDrift::Changed, ts, None);
        assert!(out.contains("[concord]"));
        assert!(out.contains("`/work/f.txt`"));
        assert!(out.contains("changed on disk"));
        assert!(out.contains("2026-10-03T12:34:56Z"));
        assert!(out.contains("Re-read it before editing."));
        assert!(!out.contains("was deleted on disk"));
    }

    #[test]
    fn render_unrecorded_context_deleted_uses_past_tense() {
        let ts: DateTime<Utc> = "2026-10-03T12:34:56Z".parse().unwrap();
        let out = render_unrecorded_context(Path::new("/work/f.txt"), DiskDrift::Deleted, ts, None);
        assert!(out.contains("[concord]"));
        assert!(out.contains("`/work/f.txt`"));
        assert!(out.contains("was deleted on disk"));
        assert!(!out.contains("changed on disk"));
        assert!(out.contains("Re-read it before editing."));
    }
}
