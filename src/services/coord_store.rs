//! Concord P0 layer — durable SQLite-backed principal registry, worker
//! bindings, and per-principal mailbox.
//!
//! This sits *next to* the existing `api::coord` lease plane. Leases are
//! ephemeral in-memory coordination state; principals/bindings/mailbox
//! records are durable across restarts and (in the default config) live
//! on disk at the path derived in `bin/contextnest.rs::serve` from
//! `CONTEXTNEST_COORD_DB` or the WAL path's sibling.
//!
//! The wire contract for the routes served by `api::coord_principals.rs`
//! is the mini-ork `concord-protocol.md` document — every method here
//! maps to one PUT/GET/POST/DELETE path there.
//!
//! Concurrency: a `std::sync::Mutex<Connection>` serializes every call.
//! Each public method takes the lock *inside* itself so the caller can
//! never hold it across an `.await`. Status computation is the
//! exception that proves the rule — `nix::sys::signal::kill` is a
//! syscall, not an awaitable, so it's safe to call under the lock; we
//! only release the lock *before* returning because nothing inside this
//! module ever awaits.

use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// Default TTL for the live→idle transition. Read on every status
/// computation from `CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS` so test
/// suites can shrink the window without restarting the binary.
const DEFAULT_PRINCIPAL_TTL_SECS: i64 = 90;

/// Hard cap on a message body (bytes). 8 KiB matches the mini-ork
/// protocol contract; oversized messages get a 400 instead of being
/// silently truncated.
const MAX_MESSAGE_BODY_BYTES: usize = 8192;

/// Default retention window (days) for `footprints` rows. Read once
/// at `open()` time from `CONTEXTNEST_COORD_FOOTPRINT_DAYS` so
/// parallel test threads don't race on the process env.
const DEFAULT_FOOTPRINT_RETENTION_DAYS: i64 = 7;

/// Per-direction hop cap used by [`CoordStore::lineage`]. "Direction"
/// here means the labels.parent chain, walked either upward (towards
/// an ancestor) or downward (towards a descendant).
pub const LINEAGE_MAX_HOPS: usize = 3;

#[derive(Debug, thiserror::Error)]
pub enum CoordStoreError {
    #[error("not found")]
    NotFound,
    #[error("invalid principal id: {0}")]
    InvalidId(String),
    #[error("invalid message body: {0}")]
    InvalidBody(String),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type CoordStoreResult<T> = std::result::Result<T, CoordStoreError>;

/// Map store errors into the project-wide error type so the service
/// container constructor can use `?` without bespoke handling.
impl From<CoordStoreError> for crate::error::ContextNestError {
    fn from(e: CoordStoreError) -> Self {
        match e {
            CoordStoreError::NotFound => crate::error::ContextNestError::NotFound("coord".into()),
            CoordStoreError::InvalidId(_) => {
                crate::error::ContextNestError::Validation("coord invalid id".into())
            }
            CoordStoreError::InvalidBody(_) => {
                crate::error::ContextNestError::Validation("coord invalid message body".into())
            }
            CoordStoreError::Db(e) => crate::error::ContextNestError::Database(e.to_string()),
            CoordStoreError::Json(e) => {
                crate::error::ContextNestError::Serialization(e.to_string())
            }
            CoordStoreError::Io(e) => crate::error::ContextNestError::Io(e.to_string()),
        }
    }
}

/// One upsert body. Every field is optional so the second PUT of an
/// existing principal can advance only `last_seen` (via presence, not
/// the field — see [`CoordStore::upsert_principal`]) while leaving the
/// stored shape intact.
///
/// `labels` is a JSON object stored as TEXT — replaced as a whole when
/// present (no deep merge), absent when JSON `null` or omitted.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PrincipalUpsert {
    #[serde(default)]
    pub harness: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub pgid: Option<i64>,
    #[serde(default)]
    pub pids: Option<Vec<i64>>,
    #[serde(default)]
    pub tmux_pane: Option<String>,
    #[serde(default)]
    pub kill_recipe: Option<Vec<String>>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub labels: Option<serde_json::Value>,
}

/// Persisted principal record returned by GET and PUT.
///
/// `kind` is derived from the id prefix (`loop`/`run`/`session`/
/// `human`/`agent`); it's stored explicitly so list endpoints don't
/// have to re-parse.
///
/// Every `Option` field serializes as JSON `null` when absent (no
/// `skip_serializing_if` — clients should be able to see which fields
/// the caller chose not to send).
#[derive(Debug, Clone, Serialize)]
pub struct Principal {
    pub principal_id: String,
    pub kind: String,
    pub started_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: String,
    pub harness: Option<String>,
    pub host: Option<String>,
    pub cwd: Option<String>,
    pub worktree: Option<String>,
    pub pgid: Option<i64>,
    pub pids: Option<Vec<i64>>,
    pub tmux_pane: Option<String>,
    pub kill_recipe: Option<Vec<String>>,
    pub priority: Option<i64>,
    pub labels: Option<serde_json::Value>,
}

/// Worker → principal binding. A worker has at most one current
/// binding; rebinding replaces it (the `(worker_id)` primary key on
/// the bindings table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub worker_id: String,
    pub principal_id: String,
    pub pid: Option<i64>,
    pub bound_at: DateTime<Utc>,
}

/// One message on a principal's mailbox. `M-<n>` is the public id;
/// the underlying autoincrement integer is hidden from the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub msg_id: String,
    pub principal_id: String,
    pub from: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub delivered_to: Option<String>,
    pub acked_at: Option<DateTime<Utc>>,
    pub acked_by: Option<String>,
}

/// One read or write footprint recorded by the PostToolUse Concord
/// hook. `mtime_ns` and `size` come from `std::fs::metadata` and may be
/// `None` when the file vanished between the tool call and our stat
/// (common for ephemeral /tmp paths). `detail` carries the shell
/// command string for `op == "exec"` rows and is `None` for read/write
/// rows.
#[derive(Debug, Clone, Serialize)]
pub struct Footprint {
    pub seq: i64,
    pub principal_id: String,
    pub worker_id: String,
    pub op: String,
    pub path: String,
    pub mtime_ns: Option<i64>,
    pub size: Option<i64>,
    pub detail: Option<String>,
    pub ts: DateTime<Utc>,
}

/// One exec footprint (a Bash shell command recorded by the
/// PostToolUse hook). `path` is the canonicalized cwd the command ran
/// in; `detail` is the command string (truncated to 300 chars on
/// write); `harness` rides along via LEFT JOIN for the advisory.
#[derive(Debug, Clone, Serialize)]
pub struct ExecFootprint {
    pub principal_id: String,
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub path: String,
    pub detail: Option<String>,
    pub harness: Option<String>,
}

/// Summarised view of "another principal wrote this path after seq N",
/// used by the PreToolUse precheck to render the WAIT advisory.
#[derive(Debug, Clone, Serialize)]
pub struct OtherWriter {
    pub principal_id: String,
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub harness: Option<String>,
    pub cwd: Option<String>,
}

/// One row in the UserPromptSubmit turn digest (Concord P2c).
/// Distinct from `OtherWriter` in two ways: it's grouped per-path (one
/// row per touched path, not per writer), it carries the path itself
/// (the precheck already has it injected), and it counts distinct
/// non-lineage writers so the renderer can append a "+(K other
/// writers)" suffix when K >= 1.
#[derive(Debug, Clone, Serialize)]
pub struct DigestChange {
    pub path: String,
    pub principal_id: String,
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub harness: Option<String>,
    pub cwd: Option<String>,
    /// Number of distinct non-lineage writer principals that touched
    /// `path` in the examined window. `1` means only `principal_id`
    /// wrote it; the renderer omits the suffix in that case.
    pub writer_count: usize,
}

/// Result of [`CoordStore::take_turn_digest`]. `changes` is already
/// newest-first and already truncated to the caller-supplied `limit`.
/// `total_paths` is the count of non-ignored grouped paths, so the
/// renderer can append a `(+(total-changes) more)` line when needed.
/// `cursor_initialized` is `true` when this call promoted a NULL
/// `digest_seq` to the current MAX(seq) — the caller renders nothing
/// for that first call (the brief scopes all digest behaviour to
/// UserPromptSubmit, not SessionStart).
#[derive(Debug, Clone, Serialize)]
pub struct TurnDigest {
    pub changes: Vec<DigestChange>,
    pub total_paths: usize,
    pub cursor_initialized: bool,
}

/// A live TTL-bound claim on a hot shared-config path (Concord P2).
/// One row per path in `hot_claims`; `last_write_ts` is hydrated from
/// the `footprints` row whose `seq == last_write_seq` (rendered as
/// `unknown` when retention pruned that row).
#[derive(Debug, Clone, Serialize)]
pub struct HotClaim {
    pub path: String,
    pub principal_id: String,
    pub expires_at: DateTime<Utc>,
    pub last_write_seq: i64,
    pub last_write_ts: Option<DateTime<Utc>>,
}

/// Result of a `claim_hot` attempt against a hot path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotClaimOutcome {
    /// No live claim existed (or it expired) — this writer took it.
    Taken,
    /// The caller is the holder or in the holder's lineage — claim
    /// refreshed, holder unchanged.
    Renewed,
    /// A principal outside the caller's lineage holds a live claim.
    Contended { holder: String },
}

/// One audit row recorded when a write-class PreToolUse precheck landed
/// outside its worktree principal's claimed `labels.owns` scope
/// (Concord P2d). `worktree_principal` identifies the root; `path` is
/// stored worktree-relative (matches the owns vocabulary and the ✋
/// advisory). `caller_principal` is the hook caller's resolved principal
/// id, or NULL for an unbound / hard-to-resolve session.
#[derive(Debug, Clone, Serialize)]
pub struct OwnsViolation {
    pub seq: i64,
    pub worktree_principal: String,
    pub path: String,
    pub caller_principal: Option<String>,
    pub ts: DateTime<Utc>,
}

/// Concord P3 — one captured UserPromptSubmit prompt + its intent
/// embedding. `dim` is stored separately from the BLOB length so a
/// malformed blob (caught by `decode_embedding`) doesn't poison
/// similarity comparisons with zero-dim neighbours. `samples` (P3b)
/// counts blended prompts feeding the stored vector: `1` on every
/// replace-path write, `>= 2` after the blended path has mixed in
/// prior rows.
#[derive(Debug, Clone)]
pub struct Intent {
    pub principal_id: String,
    pub text: String,
    pub embedding: Vec<f32>,
    pub dim: usize,
    pub updated_at: DateTime<Utc>,
    pub samples: i64,
}

/// Concord P3 — best match across non-lineage live intents (used by the
/// per-turn hook to decide whether to emit a notice).
#[derive(Debug, Clone, Serialize)]
pub struct TopicMatch {
    pub other: String,
    pub other_text: String,
    pub similarity: f32,
}

/// Concord P3 — one pairing surfaced by `GET /api/v1/coord/topic-pairs`
/// for threshold calibration. `a_text`/`b_text` ride along so the
/// operator can tell WHY a near-pair fires.
#[derive(Debug, Clone, Serialize)]
pub struct TopicPair {
    pub a: String,
    pub b: String,
    pub similarity: f32,
    pub a_text: String,
    pub b_text: String,
}

/// Encode an `f32` slice to little-endian bytes. The on-disk shape is
/// `Vec<u8>` packed as `f32::to_le_bytes()` per element; the matching
/// decoder [`decode_embedding`] round-trips every defined float bit-for-bit
/// (including `-0.0`, subnormals, `f32::MIN_POSITIVE`, `f32::MAX`).
pub fn encode_embedding(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Inverse of [`encode_embedding`]. Returns `Err(InvalidBody)` when the
/// blob length isn't a multiple of 4 bytes — there's no other
/// reasonable encoding failure mode, and the brief forbids new error
/// variants, so we reuse the existing one.
pub fn decode_embedding(b: &[u8]) -> CoordStoreResult<Vec<f32>> {
    if b.len() % 4 != 0 {
        return Err(CoordStoreError::InvalidBody(format!(
            "embedding blob length {} is not a multiple of 4 bytes",
            b.len()
        )));
    }
    let mut out = Vec::with_capacity(b.len() / 4);
    for chunk in b.chunks_exact(4) {
        let mut arr = [0u8; 4];
        arr.copy_from_slice(chunk);
        out.push(f32::from_le_bytes(arr));
    }
    Ok(out)
}

/// Cosine floor below which the OLD text is kept on a blend (an
/// off-topic turn such as a status question). At or above it the new
/// prompt is on-topic and its text replaces the stored one, so the
/// notice quotes the principal's current work. 0.5 is the P3b spec value.
const INTENT_TEXT_KEEP_COS: f32 = 0.5;

/// L2 norm of a vector, accumulated in f64 for stability. Returns 0
/// when every component is zero; returns `f32::NAN` if any component
/// is non-finite.
fn l2_norm(v: &[f32]) -> f32 {
    let mut sum = 0.0_f64;
    for x in v {
        if !x.is_finite() {
            return f32::NAN;
        }
        sum += f64::from(*x) * f64::from(*x);
    }
    sum.sqrt() as f32
}

/// Return a unit vector in the direction of `v`. Empty input or any
/// non-finite component returns an empty / non-finite vec so callers
/// can branch on `norm > 0` and `result.is_finite()`. A zero vector
/// returns an empty Vec to keep the `norm > 0` short-circuit honest
/// (every component would be 0/0 = NaN otherwise).
fn normalized(v: &[f32]) -> Vec<f32> {
    let n = l2_norm(v);
    if !n.is_finite() || n <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(v.len());
    let inv = 1.0_f64 / f64::from(n);
    for x in v {
        out.push((f64::from(*x) * inv) as f32);
    }
    out
}

/// Free helper behind [`CoordStore::upsert_intent`] and
/// [`CoordStore::upsert_intent_blended`]. Writes one row through the
/// canonical INSERT … ON CONFLICT SQL so the two write paths share
/// one statement. `samples` is supplied by the caller — `1` on a
/// replace, `old_samples + 1` on a blend. Takes `&Connection`
/// directly so the caller can hold the store's `std::Mutex` and avoid
/// re-entrancy.
fn write_intent_row(
    conn: &Connection,
    principal_id: &str,
    text: &str,
    embedding: &[f32],
    samples: i64,
    at: DateTime<Utc>,
) -> rusqlite::Result<()> {
    let blob = encode_embedding(embedding);
    let now_str = fmt_ts(at);
    let dim = embedding.len() as i64;
    conn.execute(
        "INSERT INTO intents (principal_id, text, embedding, dim, updated_at, samples)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(principal_id) DO UPDATE SET
            text       = excluded.text,
            embedding  = excluded.embedding,
            dim        = excluded.dim,
            updated_at = excluded.updated_at,
            samples    = excluded.samples",
        params![principal_id, text, blob, dim, now_str, samples],
    )?;
    Ok(())
}

/// One active worktree principal surfaced by [`CoordStore::owning_worktree_principal`].
/// `worktree` is canonicalized; `owns` is normalized (trimmed, leading
/// `./` stripped, trailing `/` stripped, empties dropped). An empty Vec
/// means "no claim" — callers should treat that as no check.
#[derive(Debug, Clone)]
pub struct WorktreePrincipal {
    pub principal_id: String,
    pub worktree: std::path::PathBuf,
    pub owns: Vec<String>,
    /// The target's path relative to `worktree`, '/'-separated, computed
    /// from the SAME canonicalized target used for the prefix match — so a
    /// brand-new file under a symlinked root (macOS /var → /private/var)
    /// yields the right relative path. Callers must use this, never
    /// re-derive it from the raw path.
    pub rel: String,
}

/// Computed status. Persisted as lowercase text per the wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalStatus {
    Live,
    Idle,
    Stale,
    Ended,
}

impl PrincipalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PrincipalStatus::Live => "live",
            PrincipalStatus::Idle => "idle",
            PrincipalStatus::Stale => "stale",
            PrincipalStatus::Ended => "ended",
        }
    }
}

/// Compiled regex for principal-id validation. Built lazily on first
/// use so module load doesn't fail on exotic regex engines.
fn id_regex() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Loop / run / session / human / agent prefix; then 1..=128
        // chars from a deliberately-conservative alphabet
        // (alphanumerics, dot, underscore, @, /, -). The / lets
        // `human:amir/feat-foo` style ids pass; callers must URL-encode
        // the / as %2F when it appears in a path segment because
        // matchit routes on the raw path.
        regex::Regex::new(r"^(loop|run|session|human|agent):[A-Za-z0-9._@/-]{1,128}$")
            .expect("principal id regex is a compile-time constant")
    })
}

/// Validate a principal id. Returns the parsed `kind` on success.
pub fn validate_principal_id(id: &str) -> CoordStoreResult<&'static str> {
    if id_regex().is_match(id) {
        // After a successful match, the prefix is one of these five
        // literals — compare against them to recover a `'static str`
        // without re-parsing the captures.
        if id.starts_with("loop:") {
            Ok("loop")
        } else if id.starts_with("run:") {
            Ok("run")
        } else if id.starts_with("session:") {
            Ok("session")
        } else if id.starts_with("human:") {
            Ok("human")
        } else if id.starts_with("agent:") {
            Ok("agent")
        } else {
            // Unreachable given the regex, but let the compiler prove
            // we always produce a value.
            Err(CoordStoreError::InvalidId(id.to_string()))
        }
    } else {
        Err(CoordStoreError::InvalidId(id.to_string()))
    }
}

/// Server hostname. `None` when `nix::unistd::gethostname()` fails —
/// we treat that as "no idle on this host, every local principal goes
/// stale" which is the safe degradation.
pub fn local_hostname() -> Option<String> {
    nix::unistd::gethostname()
        .ok()
        .and_then(|os| os.into_string().ok())
}

/// True iff `pid` is alive *and* visible to this process.
///
/// Rejection rules (in order):
///   - `pid <= 0` → false. `kill(0, None)` probes the caller's
///     process group and `kill(-1, None)` probes every process the
///     caller can signal — both would wrongly say "alive". A pid ≤ 0
///     is meaningless as a principal's child.
///   - `pid > i32::MAX` → false. `libc::pid_t` is `i32`; a value
///     larger than that would silently wrap if passed to nix.
///   - Otherwise: `kill(pid, None)` succeeds (running) OR returns
///     `EPERM` (running but owned by another user). Both are "alive".
///     ESRCH (no such process) means reaped.
pub fn pid_alive(pid: i64) -> bool {
    if pid <= 0 || pid > i32::MAX as i64 {
        return false;
    }
    let raw_pid = nix::unistd::Pid::from_raw(pid as i32);
    match nix::sys::signal::kill(raw_pid, None) {
        Ok(()) => true,
        Err(nix::errno::Errno::EPERM) => true,
        Err(_) => false,
    }
}

/// TTL read fresh on every call so test environments can flip
/// `CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS` without bouncing the binary.
fn principal_ttl_secs() -> i64 {
    std::env::var("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| *n >= 0)
        .unwrap_or(DEFAULT_PRINCIPAL_TTL_SECS)
}

/// Footprint retention in days. A value <= 0 or unparseable falls back
/// to the default. Read once at `open()` so test threads don't race
/// on the process env while they exercise the substrate.
fn footprint_retention_days() -> i64 {
    std::env::var("CONTEXTNEST_COORD_FOOTPRINT_DAYS")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_FOOTPRINT_RETENTION_DAYS)
}

/// Prune every footprint row older than `days` days from `now`. The
/// `ts` column is fixed-width RFC3339 microseconds, so `ts < cutoff`
/// compares lexicographically the same way it compares
/// chronologically — no ISO parser required.
fn prune_old_footprints(conn: &Connection, days: i64) -> rusqlite::Result<usize> {
    let cutoff = fmt_ts(Utc::now() - ChronoDuration::days(days));
    conn.execute("DELETE FROM footprints WHERE ts < ?1", params![cutoff])
}

/// Idempotent migration for the `principals.digest_seq` column
/// (Concord P2c). Reads `PRAGMA table_info(principals)` and runs the
/// `ALTER TABLE … ADD COLUMN` only when the column is absent, so a
/// fresh DB and an upgraded DB share one code path and a second
/// `open()` is a no-op instead of "duplicate column name".
///
/// The column is intentionally nullable: pre-existing principal rows
/// from before the upgrade have no digest cursor, and `take_turn_digest`
/// treats a NULL cursor as "initialize on first turn".
fn ensure_digest_seq_column(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(principals)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    if !cols.iter().any(|c| c == "digest_seq") {
        conn.execute("ALTER TABLE principals ADD COLUMN digest_seq INTEGER", [])?;
    }
    Ok(())
}

/// Idempotent migration for the `intents.samples` column (Concord
/// P3b). Same shape as `ensure_digest_seq_column`: a fresh DB builds
/// the column via SCHEMA, an upgraded DB gets it from `ALTER TABLE …
/// ADD COLUMN`, and a second `open()` is a no-op. `NOT NULL DEFAULT
/// 1` is legal for `ALTER TABLE ADD COLUMN` in SQLite, so pre-P3b
/// rows read as samples=1 once the upgrade has run.
fn ensure_intents_samples_column(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(intents)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    if !cols.iter().any(|c| c == "samples") {
        conn.execute(
            "ALTER TABLE intents ADD COLUMN samples INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }
    Ok(())
}

/// Idempotent migration for the P1d footprints schema (Bash exec
/// footprints): the `detail` column AND the widened `op` CHECK. A fresh
/// DB builds both via SCHEMA; a pre-P1d DB has `CHECK(op IN
/// ('read','write'))` and no `detail`, and because SQLite cannot ALTER a
/// CHECK constraint, the table is rebuilt in one transaction — new table
/// with the widened CHECK + `detail`, every row copied over (old rows
/// read `detail` as NULL), old table dropped, renamed, indexes
/// recreated. A second `open()` is a no-op.
fn ensure_footprint_detail_column(conn: &Connection) -> rusqlite::Result<()> {
    let create_sql: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'footprints'",
        [],
        |r| r.get(0),
    )?;

    // The widened CHECK and the `detail` column ship together in SCHEMA,
    // so a table whose CREATE SQL already names 'exec' has the new
    // schema. Keep the ADD COLUMN path as a defensive no-op for any
    // intermediate build that widened the CHECK without the column.
    if create_sql.contains("'exec'") {
        let mut stmt = conn.prepare("PRAGMA table_info(footprints)")?;
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        if !cols.iter().any(|c| c == "detail") {
            conn.execute("ALTER TABLE footprints ADD COLUMN detail TEXT", [])?;
        }
        return Ok(());
    }

    // Pre-P1d schema. Rebuild with the widened CHECK and `detail`. The
    // `unchecked_transaction` rolls back on drop if any statement fails,
    // so a half-migrated table can never be committed.
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE footprints_new (
            seq          INTEGER PRIMARY KEY AUTOINCREMENT,
            principal_id TEXT NOT NULL,
            worker_id    TEXT NOT NULL,
            op           TEXT NOT NULL CHECK(op IN ('read','write','exec')),
            path         TEXT NOT NULL,
            mtime_ns     INTEGER,
            size         INTEGER,
            detail       TEXT,
            ts           TEXT NOT NULL
         );
         INSERT INTO footprints_new
            (seq, principal_id, worker_id, op, path, mtime_ns, size, detail, ts)
         SELECT seq, principal_id, worker_id, op, path, mtime_ns, size, NULL, ts
         FROM footprints;
         DROP TABLE footprints;
         ALTER TABLE footprints_new RENAME TO footprints;
         CREATE INDEX IF NOT EXISTS idx_footprints_path ON footprints(path, seq);
         CREATE INDEX IF NOT EXISTS idx_footprints_principal_path
             ON footprints(principal_id, path, seq);",
    )?;
    tx.commit()?;
    Ok(())
}

/// Compute status from a stored principal at `now`.
pub fn status_at(p: &Principal, now: DateTime<Utc>) -> PrincipalStatus {
    if p.ended_at.is_some() {
        return PrincipalStatus::Ended;
    }
    let ttl_ms = principal_ttl_secs() * 1000;
    let since_ms = (now - p.last_seen).num_milliseconds();
    if since_ms <= ttl_ms {
        return PrincipalStatus::Live;
    }
    // Past the TTL. Distinguish idle (host alive, pids alive) from
    // stale (no signs of life). Host check requires an exact match —
    // a different hostname always means "stale, not idle".
    let host_match = match (p.host.as_deref(), local_hostname().as_deref()) {
        (Some(h), Some(local)) => h == local,
        _ => false,
    };
    if host_match {
        if let Some(pids) = p.pids.as_ref() {
            if pids.iter().any(|pid| pid_alive(*pid)) {
                return PrincipalStatus::Idle;
            }
        }
    }
    PrincipalStatus::Stale
}

/// Format a `DateTime<Utc>` as fixed-width RFC3339 microseconds so
/// SQLite TEXT columns sort lexicographically the same way they sort
/// chronologically.
fn fmt_ts(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Truncate `s` to at most `max` chars on a UTF-8 char boundary. Never
/// slices a multibyte codepoint in half — `s.chars().take(max)` walks
/// whole chars, not bytes.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

fn parse_ts(s: &str) -> CoordStoreResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            CoordStoreError::Db(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(e),
            ))
        })
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS principals (
        principal_id TEXT PRIMARY KEY,
        kind         TEXT NOT NULL,
        harness      TEXT,
        host         TEXT,
        cwd          TEXT,
        worktree     TEXT,
        pgid         INTEGER,
        pids         TEXT,
        tmux_pane    TEXT,
        kill_recipe  TEXT,
        priority     INTEGER,
        labels       TEXT,
        started_at   TEXT NOT NULL,
        last_seen    TEXT NOT NULL,
        ended_at     TEXT
    );
    CREATE TABLE IF NOT EXISTS bindings (
        worker_id    TEXT PRIMARY KEY,
        principal_id TEXT NOT NULL,
        pid          INTEGER,
        bound_at     TEXT NOT NULL,
        FOREIGN KEY(principal_id) REFERENCES principals(principal_id) ON DELETE CASCADE
    );
    CREATE TABLE IF NOT EXISTS messages (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        principal_id   TEXT NOT NULL,
        from_actor     TEXT NOT NULL,
        body           TEXT NOT NULL,
        created_at     TEXT NOT NULL,
        delivered_at   TEXT,
        delivered_to   TEXT,
        acked_at       TEXT,
        acked_by       TEXT,
        FOREIGN KEY(principal_id) REFERENCES principals(principal_id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS idx_messages_principal ON messages(principal_id, id);
    CREATE INDEX IF NOT EXISTS idx_bindings_principal ON bindings(principal_id);
    CREATE TABLE IF NOT EXISTS footprints (
        seq          INTEGER PRIMARY KEY AUTOINCREMENT,
        principal_id TEXT NOT NULL,
        worker_id    TEXT NOT NULL,
        op           TEXT NOT NULL CHECK(op IN ('read','write','exec')),
        path         TEXT NOT NULL,
        mtime_ns     INTEGER,
        size         INTEGER,
        detail       TEXT,
        ts           TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_footprints_path ON footprints(path, seq);
    CREATE INDEX IF NOT EXISTS idx_footprints_principal_path ON footprints(principal_id, path, seq);
    CREATE TABLE IF NOT EXISTS hot_claims (
        path           TEXT PRIMARY KEY,
        principal_id   TEXT NOT NULL,
        expires_at     TEXT NOT NULL,
        last_write_seq INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_hot_claims_expires ON hot_claims(expires_at);
    CREATE TABLE IF NOT EXISTS owns_violations (
        seq               INTEGER PRIMARY KEY AUTOINCREMENT,
        worktree_principal TEXT NOT NULL,
        path              TEXT NOT NULL,
        caller_principal  TEXT,
        ts                TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_owns_violations_wt ON owns_violations(worktree_principal, seq);
    CREATE TABLE IF NOT EXISTS intents (
        principal_id TEXT PRIMARY KEY,
        text         TEXT NOT NULL,
        embedding    BLOB NOT NULL,
        dim          INTEGER NOT NULL,
        updated_at   TEXT NOT NULL,
        samples      INTEGER NOT NULL DEFAULT 1
    );
    CREATE INDEX IF NOT EXISTS idx_intents_updated ON intents(updated_at);
    CREATE TABLE IF NOT EXISTS topic_notices (
        pair_key    TEXT PRIMARY KEY,
        notified_at TEXT NOT NULL
    );
";

pub struct CoordStore {
    conn: Mutex<Connection>,
}

impl CoordStore {
    /// Take the connection lock, recovering from poisoning. A panic while
    /// one request held the lock must not take every later Concord
    /// endpoint down with it until the process restarts.
    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Open (or create) a file-backed store at `path`. Creates the
    /// parent directory if missing. WAL + busy_timeout so concurrent
    /// servers (different worktrees against the same ~/.contextnest)
    /// don't fail with SQLITE_BUSY.
    pub fn open(path: &Path) -> CoordStoreResult<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
        )?;
        conn.execute_batch(SCHEMA)?;
        ensure_digest_seq_column(&conn)?;
        ensure_intents_samples_column(&conn)?;
        ensure_footprint_detail_column(&conn)?;
        let days = footprint_retention_days();
        prune_old_footprints(&conn, days)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// In-memory store for tests. Same schema, no on-disk artifact.
    pub fn open_in_memory() -> CoordStoreResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        ensure_digest_seq_column(&conn)?;
        ensure_intents_samples_column(&conn)?;
        ensure_footprint_detail_column(&conn)?;
        let days = footprint_retention_days();
        prune_old_footprints(&conn, days)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Count of unacked messages on `principal_id`. Used by the upsert
    /// response so the caller can drain a queue that accumulated
    /// while it was offline.
    pub fn unacked_count(&self, principal_id: &str) -> CoordStoreResult<usize> {
        let conn = self.lock();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE principal_id = ?1 AND acked_at IS NULL",
            params![principal_id],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Upsert a principal. On first write the row is created with
    /// `started_at = last_seen = now` and `ended_at = NULL`. On a
    /// re-write only the `Some` fields overwrite the stored value;
    /// `null` and absent fields preserve the prior shape. `last_seen`
    /// always advances to `now`. If `ended_at` was set on the prior
    /// row, clear it and reset `started_at = now` so the principal
    /// looks freshly reborn to the next GET.
    pub fn upsert_principal(
        &self,
        id: &str,
        upsert: PrincipalUpsert,
    ) -> CoordStoreResult<Principal> {
        let kind = validate_principal_id(id)?.to_string();
        let now = Utc::now();
        let now_str = fmt_ts(now);

        let pids_json = match &upsert.pids {
            Some(pids) => Some(serde_json::to_string(pids)?),
            None => None,
        };
        let kill_json = match &upsert.kill_recipe {
            Some(r) => Some(serde_json::to_string(r)?),
            None => None,
        };
        let labels_json = match &upsert.labels {
            Some(v) => Some(serde_json::to_string(v)?),
            None => None,
        };

        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;

        // Pull the existing row, if any, to drive the "was ended?"
        // branch. `Option<String>` because ended_at is NULL for
        // fresh rows — `r.get(0)` on a NULL column is a hard error
        // in rusqlite.
        let existed: Option<Option<String>> = tx
            .query_row(
                "SELECT ended_at FROM principals WHERE principal_id = ?1",
                params![id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?;
        let was_ended = matches!(existed, Some(Some(_)));

        if existed.is_none() {
            // INSERT path — every field present-or-NULL with started_at
            // == last_seen == now.
            tx.execute(
                "INSERT INTO principals (
                    principal_id, kind, harness, host, cwd, worktree,
                    pgid, pids, tmux_pane, kill_recipe, priority, labels,
                    started_at, last_seen, ended_at
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6,
                    ?7, ?8, ?9, ?10, ?11, ?12,
                    ?13, ?13, NULL
                )",
                params![
                    id,
                    kind,
                    upsert.harness,
                    upsert.host,
                    upsert.cwd,
                    upsert.worktree,
                    upsert.pgid,
                    pids_json,
                    upsert.tmux_pane,
                    kill_json,
                    upsert.priority,
                    labels_json,
                    now_str,
                ],
            )?;
        } else {
            // UPDATE path — merge only the Some fields. We coalesce
            // NULL placeholders against the current value via a SELECT,
            // except for the JSON columns where absent means "leave
            // alone" (NULL placeholder would overwrite with SQL NULL).
            let current: PrincipalRow = tx.query_row(
                "SELECT principal_id, kind, harness, host, cwd, worktree,
                        pgid, pids, tmux_pane, kill_recipe, priority, labels,
                        started_at, last_seen, ended_at
                 FROM principals WHERE principal_id = ?1",
                params![id],
                PrincipalRow::from_row,
            )?;
            let new_harness = upsert.harness.or(current.harness);
            let new_host = upsert.host.or(current.host);
            let new_cwd = upsert.cwd.or(current.cwd);
            let new_worktree = upsert.worktree.or(current.worktree);
            let new_pgid = upsert.pgid.or(current.pgid);
            let new_pids = pids_json.or(current.pids);
            let new_tmux = upsert.tmux_pane.or(current.tmux_pane);
            let new_kill = kill_json.or(current.kill_recipe);
            let new_priority = upsert.priority.or(current.priority);
            let new_labels = labels_json.or(current.labels);

            // Rebirth handling: a prior ended_at that gets cleared
            // resets started_at so downstream consumers see a fresh
            // session, not a zombie resurrection.
            let reset_started = was_ended;
            if reset_started {
                tx.execute(
                    "UPDATE principals SET
                        harness      = ?2,
                        host         = ?3,
                        cwd          = ?4,
                        worktree     = ?5,
                        pgid         = ?6,
                        pids         = ?7,
                        tmux_pane    = ?8,
                        kill_recipe  = ?9,
                        priority     = ?10,
                        labels       = ?11,
                        started_at   = ?13,
                        last_seen    = ?13,
                        ended_at     = NULL
                     WHERE principal_id = ?1",
                    params![
                        id,
                        new_harness,
                        new_host,
                        new_cwd,
                        new_worktree,
                        new_pgid,
                        new_pids,
                        new_tmux,
                        new_kill,
                        new_priority,
                        new_labels,
                        // placeholder for last_seen position below
                        now_str,
                        now_str,
                    ],
                )?;
            } else {
                tx.execute(
                    "UPDATE principals SET
                        harness      = ?2,
                        host         = ?3,
                        cwd          = ?4,
                        worktree     = ?5,
                        pgid         = ?6,
                        pids         = ?7,
                        tmux_pane    = ?8,
                        kill_recipe  = ?9,
                        priority     = ?10,
                        labels       = ?11,
                        last_seen    = ?12
                     WHERE principal_id = ?1",
                    params![
                        id,
                        new_harness,
                        new_host,
                        new_cwd,
                        new_worktree,
                        new_pgid,
                        new_pids,
                        new_tmux,
                        new_kill,
                        new_priority,
                        new_labels,
                        now_str,
                    ],
                )?;
            }
        }
        tx.commit()?;

        // Read the row back so we can return the freshly-computed
        // status alongside the merge result. Two reads under one lock
        // — the function is sync so the lock is held end-to-end.
        drop(conn);
        let mut p = self.get_principal(id)?.ok_or(CoordStoreError::NotFound)?;
        p.status = status_at(&p, Utc::now()).as_str().to_string();
        Ok(p)
    }

    pub fn get_principal(&self, id: &str) -> CoordStoreResult<Option<Principal>> {
        // A malformed id cannot name a stored principal: report it as
        // absent (→ 404) rather than as a bad request. Only the PUT upsert
        // answers 400 for a malformed id.
        if validate_principal_id(id).is_err() {
            return Ok(None);
        }
        let conn = self.lock();
        let row: Option<PrincipalRow> = conn
            .query_row(
                "SELECT principal_id, kind, harness, host, cwd, worktree,
                        pgid, pids, tmux_pane, kill_recipe, priority, labels,
                        started_at, last_seen, ended_at
                 FROM principals WHERE principal_id = ?1",
                params![id],
                PrincipalRow::from_row,
            )
            .optional()?;
        row.map(PrincipalRow::into_principal).transpose()
    }

    /// All principals, ordered by `last_seen DESC`. When
    /// `include_inactive` is false, `ended` and `stale` are dropped.
    pub fn list_principals(&self, include_inactive: bool) -> CoordStoreResult<Vec<Principal>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT principal_id, kind, harness, host, cwd, worktree,
                    pgid, pids, tmux_pane, kill_recipe, priority, labels,
                    started_at, last_seen, ended_at
             FROM principals ORDER BY last_seen DESC",
        )?;
        let rows = stmt
            .query_map([], PrincipalRow::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        drop(conn);
        let now = Utc::now();
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let mut p = r.into_principal()?;
            p.status = status_at(&p, now).as_str().to_string();
            if include_inactive
                || matches!(
                    status_str(&p.status),
                    PrincipalStatus::Live | PrincipalStatus::Idle
                )
            {
                out.push(p);
            }
        }
        Ok(out)
    }

    pub fn end_principal(&self, id: &str) -> CoordStoreResult<Principal> {
        if validate_principal_id(id).is_err() {
            return Err(CoordStoreError::NotFound);
        }
        let now_str = fmt_ts(Utc::now());
        let conn = self.lock();
        let updated = conn.execute(
            "UPDATE principals SET ended_at = ?2 WHERE principal_id = ?1",
            params![id, now_str],
        )?;
        if updated == 0 {
            return Err(CoordStoreError::NotFound);
        }
        drop(conn);
        let mut p = self.get_principal(id)?.ok_or(CoordStoreError::NotFound)?;
        p.status = PrincipalStatus::Ended.as_str().to_string();
        Ok(p)
    }

    pub fn bind(
        &self,
        worker_id: &str,
        principal_id: &str,
        pid: Option<i64>,
    ) -> CoordStoreResult<Binding> {
        if validate_principal_id(principal_id).is_err() {
            return Err(CoordStoreError::NotFound);
        }
        let now_str = fmt_ts(Utc::now());
        let conn = self.lock();
        // Foreign-key style pre-check — a binding to a missing
        // principal returns NotFound instead of leaving an orphan row.
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM principals WHERE principal_id = ?1",
            params![principal_id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(CoordStoreError::NotFound);
        }
        conn.execute(
            "INSERT INTO bindings (worker_id, principal_id, pid, bound_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(worker_id) DO UPDATE SET
                principal_id = excluded.principal_id,
                pid          = excluded.pid,
                bound_at     = excluded.bound_at",
            params![worker_id, principal_id, pid, now_str],
        )?;
        Ok(Binding {
            worker_id: worker_id.to_string(),
            principal_id: principal_id.to_string(),
            pid,
            bound_at: parse_ts(&now_str)?,
        })
    }

    pub fn get_binding(&self, worker_id: &str) -> CoordStoreResult<Option<Binding>> {
        let conn = self.lock();
        let row: Option<(String, Option<i64>, String)> = conn
            .query_row(
                "SELECT principal_id, pid, bound_at FROM bindings WHERE worker_id = ?1",
                params![worker_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        row.map(|(principal_id, pid, bound_at)| {
            Ok(Binding {
                worker_id: worker_id.to_string(),
                principal_id,
                pid,
                bound_at: parse_ts(&bound_at)?,
            })
        })
        .transpose()
    }

    pub fn post_message(
        &self,
        principal_id: &str,
        from: &str,
        body: &str,
    ) -> CoordStoreResult<Message> {
        if validate_principal_id(principal_id).is_err() {
            return Err(CoordStoreError::NotFound);
        }
        if body.is_empty() || body.len() > MAX_MESSAGE_BODY_BYTES {
            return Err(CoordStoreError::InvalidBody(format!(
                "message body must be 1..={} bytes",
                MAX_MESSAGE_BODY_BYTES
            )));
        }
        let now_str = fmt_ts(Utc::now());
        let conn = self.lock();
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM principals WHERE principal_id = ?1",
            params![principal_id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(CoordStoreError::NotFound);
        }
        conn.execute(
            "INSERT INTO messages (principal_id, from_actor, body, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![principal_id, from, body, now_str],
        )?;
        let id: i64 = conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0))?;
        Ok(Message {
            msg_id: format!("M-{id}"),
            principal_id: principal_id.to_string(),
            from: from.to_string(),
            body: body.to_string(),
            created_at: parse_ts(&now_str)?,
            delivered_at: None,
            delivered_to: None,
            acked_at: None,
            acked_by: None,
        })
    }

    pub fn list_messages(
        &self,
        principal_id: &str,
        unacked_only: bool,
    ) -> CoordStoreResult<Vec<Message>> {
        if validate_principal_id(principal_id).is_err() {
            return Err(CoordStoreError::NotFound);
        }
        let conn = self.lock();
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM principals WHERE principal_id = ?1",
            params![principal_id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(CoordStoreError::NotFound);
        }
        let sql = if unacked_only {
            "SELECT id, principal_id, from_actor, body, created_at,
                    delivered_at, delivered_to, acked_at, acked_by
             FROM messages WHERE principal_id = ?1 AND acked_at IS NULL
             ORDER BY id ASC"
        } else {
            "SELECT id, principal_id, from_actor, body, created_at,
                    delivered_at, delivered_to, acked_at, acked_by
             FROM messages WHERE principal_id = ?1
             ORDER BY id ASC"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map(params![principal_id], MessageRow::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(MessageRow::into_message).collect()
    }

    /// Idempotent ack. First call writes `acked_at` and `acked_by`;
    /// subsequent calls return the row unchanged (same `acked_at`,
    /// same `acked_by`).
    pub fn ack_message(
        &self,
        principal_id: &str,
        msg_id: &str,
        by: &str,
    ) -> CoordStoreResult<Message> {
        if validate_principal_id(principal_id).is_err() {
            return Err(CoordStoreError::NotFound);
        }
        let id_num = msg_id
            .strip_prefix("M-")
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| CoordStoreError::NotFound)?;
        let now_str = fmt_ts(Utc::now());
        let conn = self.lock();
        // Cross-check principal — ack against another principal's
        // message is NotFound, not a leak.
        let found: Option<(String, Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT principal_id, acked_at, acked_by FROM messages WHERE id = ?1",
                params![id_num],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (row_principal, _existing_at, _existing_by) = match found {
            Some(row) => row,
            None => return Err(CoordStoreError::NotFound),
        };
        if row_principal != principal_id {
            return Err(CoordStoreError::NotFound);
        }
        // Idempotent: only stamp when acked_at is currently NULL.
        conn.execute(
            "UPDATE messages SET acked_at = ?1, acked_by = ?2
             WHERE id = ?3 AND acked_at IS NULL",
            params![now_str, by, id_num],
        )?;
        // Re-select so the response always carries the canonical
        // acked_at/acked_by — first ack writes them, subsequent
        // acks return the original.
        let row: MessageRow = conn.query_row(
            "SELECT id, principal_id, from_actor, body, created_at,
                    delivered_at, delivered_to, acked_at, acked_by
             FROM messages WHERE id = ?1",
            params![id_num],
            MessageRow::from_row,
        )?;
        row.into_message()
    }

    /// Claim and stamp up to `limit` undelivered messages for
    /// `principal_id`, returning only the rows THIS call stamped.
    ///
    /// The atomicity contract: even when two callers race (e.g. two
    /// Claude Code sessions bound to the same principal, or two server
    /// processes sharing the SQLite file), each message is returned by
    /// AT MOST one caller. Per-row UPDATE with `delivered_at IS NULL`
    /// predicate plus a `changes()==1` check gives that guarantee
    /// without needing `BEGIN IMMEDIATE`.
    ///
    /// An unknown or invalid `principal_id` returns `Ok(vec![])` rather
    /// than `NotFound`, so a first-turn session that just bound a fresh
    /// principal and the principal row hasn't propagated yet is not an
    /// error.
    pub fn claim_undelivered(
        &self,
        principal_id: &str,
        worker_id: &str,
        limit: usize,
    ) -> CoordStoreResult<Vec<Message>> {
        if validate_principal_id(principal_id).is_err() {
            return Ok(Vec::new());
        }
        if limit == 0 {
            return Ok(Vec::new());
        }
        let now_str = fmt_ts(Utc::now());
        let conn = self.lock();

        // Candidate selection: same projection as `list_messages` but
        // filtered to undelivered rows. We order by id ASC so the
        // caller's delivery order matches the insertion order — old
        // messages first, so a principal that ignored its inbox for a
        // long time reads it FIFO.
        let mut stmt = conn.prepare(
            "SELECT id, principal_id, from_actor, body, created_at,
                    delivered_at, delivered_to, acked_at, acked_by
             FROM messages
             WHERE principal_id = ?1 AND delivered_at IS NULL
             ORDER BY id ASC
             LIMIT ?2",
        )?;
        let candidates: Vec<i64> = stmt
            .query_map(params![principal_id, limit as i64], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);

        let mut out: Vec<Message> = Vec::with_capacity(candidates.len());
        for id in candidates {
            // Race-safe claim: only the call whose UPDATE affects 1 row
            // sees this id. Another process that won gets `Ok(())` with
            // 0 affected rows and skips us.
            let affected = conn.execute(
                "UPDATE messages SET delivered_at = ?1, delivered_to = ?2
                 WHERE id = ?3 AND delivered_at IS NULL",
                params![now_str, worker_id, id],
            )?;
            if affected != 1 {
                continue;
            }
            let row: MessageRow = conn.query_row(
                "SELECT id, principal_id, from_actor, body, created_at,
                        delivered_at, delivered_to, acked_at, acked_by
                 FROM messages WHERE id = ?1",
                params![id],
                MessageRow::from_row,
            )?;
            out.push(row.into_message()?);
        }
        Ok(out)
    }

    /// Mark a message as delivered to a worker. Currently unused by
    /// the API surface but kept on the store so the worker side of the
    /// Concord protocol can wire it in without a schema change.
    pub fn mark_delivered(&self, msg_id: &str, worker_id: &str) -> CoordStoreResult<Message> {
        let id_num = msg_id
            .strip_prefix("M-")
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| CoordStoreError::NotFound)?;
        let now_str = fmt_ts(Utc::now());
        let conn = self.lock();
        let updated = conn.execute(
            "UPDATE messages SET delivered_at = ?1, delivered_to = ?2
             WHERE id = ?3 AND delivered_at IS NULL",
            params![now_str, worker_id, id_num],
        )?;
        if updated == 0 {
            // Already delivered, or row missing — re-select to decide.
            let row: Option<MessageRow> = conn
                .query_row(
                    "SELECT id, principal_id, from_actor, body, created_at,
                            delivered_at, delivered_to, acked_at, acked_by
                     FROM messages WHERE id = ?1",
                    params![id_num],
                    MessageRow::from_row,
                )
                .optional()?;
            return match row {
                Some(r) => r.into_message(),
                None => Err(CoordStoreError::NotFound),
            };
        }
        let row: MessageRow = conn.query_row(
            "SELECT id, principal_id, from_actor, body, created_at,
                    delivered_at, delivered_to, acked_at, acked_by
             FROM messages WHERE id = ?1",
            params![id_num],
            MessageRow::from_row,
        )?;
        row.into_message()
    }

    // ────────────── footprints (Concord P1) ──────────────

    /// Record one read/write footprint for `(principal_id, worker_id)`
    /// against `path`. Returns the new autoincrement `seq`. The
    /// `op` arg must be `"read"` or `"write"`; anything else is a
    /// programmer error and is reported as `InvalidBody` so the
    /// PostToolUse hook fails loudly rather than silently mis-classifying.
    pub fn record_footprint(
        &self,
        principal_id: &str,
        worker_id: &str,
        op: &str,
        path: &str,
        mtime_ns: Option<i64>,
        size: Option<i64>,
    ) -> CoordStoreResult<i64> {
        if op != "read" && op != "write" {
            return Err(CoordStoreError::InvalidBody(format!(
                "footprint op must be 'read' or 'write', got {op:?}"
            )));
        }
        let now_str = fmt_ts(Utc::now());
        let conn = self.lock();
        conn.execute(
            "INSERT INTO footprints
                (principal_id, worker_id, op, path, mtime_ns, size, ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![principal_id, worker_id, op, path, mtime_ns, size, now_str],
        )?;
        let seq: i64 = conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0))?;
        Ok(seq)
    }

    /// Record one Bash exec footprint for `(principal_id, worker_id)`.
    /// `path` is the canonicalized cwd the shell ran in; `detail` is
    /// the command string, truncated to 300 chars on a char boundary.
    /// No `mtime_ns`/`size` — exec rows are never stat'ed. Returns the
    /// new autoincrement `seq`.
    pub fn record_exec_footprint(
        &self,
        principal_id: &str,
        worker_id: &str,
        path: &str,
        detail: &str,
    ) -> CoordStoreResult<i64> {
        let detail = truncate_chars(detail, 300);
        let now_str = fmt_ts(Utc::now());
        let conn = self.lock();
        conn.execute(
            "INSERT INTO footprints
                (principal_id, worker_id, op, path, mtime_ns, size, detail, ts)
             VALUES (?1, ?2, 'exec', ?3, NULL, NULL, ?4, ?5)",
            params![principal_id, worker_id, path, detail, now_str],
        )?;
        let seq: i64 = conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0))?;
        Ok(seq)
    }

    /// Look up a single footprint by its autoincrement seq. Returns
    /// `None` when the row was pruned out of the retention window.
    pub fn get_footprint(&self, seq: i64) -> CoordStoreResult<Option<Footprint>> {
        let conn = self.lock();
        let row: Option<FootprintRow> = conn
            .query_row(
                "SELECT seq, principal_id, worker_id, op, path, mtime_ns, size, detail, ts
                 FROM footprints WHERE seq = ?1",
                params![seq],
                FootprintRow::from_row,
            )
            .optional()?;
        row.map(FootprintRow::into_footprint).transpose()
    }

    /// Newest `seq` ever recorded against `(principal_id, path)`,
    /// either op. Used by the precheck to find "what's the last
    /// footprint this principal left on this file?".
    pub fn last_footprint_seq(
        &self,
        principal_id: &str,
        path: &str,
    ) -> CoordStoreResult<Option<i64>> {
        let conn = self.lock();
        // MAX() over an empty set is NULL; we read it as `Option<i64>`
        // so the no-rows case maps to `None`.
        let max: Option<i64> = conn.query_row(
            "SELECT MAX(seq) FROM footprints
                 WHERE principal_id = ?1 AND path = ?2",
            params![principal_id, path],
            |r| r.get::<_, Option<i64>>(0),
        )?;
        Ok(max)
    }

    /// Newest footprint on `path` by ANY principal, either read or
    /// write op. Used by the precheck's disk-truth check (Concord P1b)
    /// to find the last `(mtime_ns, size)` Concord knows about for the
    /// file — so the handler can compare it with the current
    /// `stat_file` result and warn when a writer outside the
    /// footprinter changed the file. Exec rows are excluded: they are
    /// keyed on a directory cwd and carry no stat, so they must never
    /// be the P1b reference (Concord P1d).
    /// The existing `idx_footprints_path(path, seq)` covers the query.
    pub fn latest_footprint(&self, path: &str) -> CoordStoreResult<Option<Footprint>> {
        let conn = self.lock();
        let row: Option<FootprintRow> = conn
            .query_row(
                "SELECT seq, principal_id, worker_id, op, path, mtime_ns, size, detail, ts
                 FROM footprints WHERE path = ?1 AND op != 'exec' ORDER BY seq DESC LIMIT 1",
                params![path],
                FootprintRow::from_row,
            )
            .optional()?;
        row.map(FootprintRow::into_footprint).transpose()
    }

    /// The set of principals in `principal_id`'s lineage, capped at
    /// `max_hops` per direction. Self is always included. See
    /// [`lineage_in`] for the traversal; this is a thin lock-then-delegate
    /// wrapper so callers that already hold the lock (e.g. `claim_hot`)
    /// can run the same query without re-entering the non-reentrant
    /// `std::sync::Mutex`.
    pub fn lineage(
        &self,
        principal_id: &str,
        max_hops: usize,
    ) -> CoordStoreResult<HashSet<String>> {
        let conn = self.lock();
        lineage_in(&conn, principal_id, max_hops)
    }

    /// Distinct write footprints on `path` with `seq > after_seq`,
    /// excluding every principal in `exclude`, deduped to one row per
    /// principal (the newest seq/ts for that principal). Carries the
    /// writer's `harness` and `cwd` via LEFT JOIN so the precheck
    /// advisory can render the `<harness>, <cwd basename>` tuple.
    /// Ordered newest seq first, capped at `limit`.
    pub fn writes_after(
        &self,
        path: &str,
        after_seq: i64,
        exclude: &HashSet<String>,
        limit: usize,
    ) -> CoordStoreResult<Vec<OtherWriter>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        // Build a JSON array of excluded ids and pass it through
        // `json_each` so the IN list doesn't have to be a dynamic
        // number of `?` placeholders. Empty set → json_each over
        // '[]' returns no rows, which is the right behavior.
        let exclude_json = serde_json::to_string(&exclude.iter().cloned().collect::<Vec<_>>())?;
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT f.principal_id, MAX(f.seq), MAX(f.ts), p.harness, p.cwd
             FROM footprints f
             LEFT JOIN principals p ON p.principal_id = f.principal_id
             WHERE f.op = 'write' AND f.path = ?1 AND f.seq > ?2
               AND NOT EXISTS (
                   SELECT 1 FROM json_each(?3) AS ex
                   WHERE ex.value = f.principal_id
               )
             GROUP BY f.principal_id
             ORDER BY MAX(f.seq) DESC
             LIMIT ?4",
        )?;
        let rows = stmt
            .query_map(params![path, after_seq, exclude_json, limit as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        let mut out = Vec::with_capacity(rows.len());
        for (pid, seq, ts, harness, cwd) in rows {
            out.push(OtherWriter {
                principal_id: pid,
                seq,
                ts: parse_ts(&ts)?,
                harness,
                cwd,
            });
        }
        Ok(out)
    }

    /// Exec footprints (Bash commands, Concord P1d) with
    /// `seq > after_seq`, newest first, capped at `limit`. Each row
    /// carries the command's cwd (`path`), the command string
    /// (`detail`), and the principal's `harness` via LEFT JOIN — the
    /// precheck's disk-drift attribution scans this for the most
    /// plausible shell command that caused an unrecorded change.
    pub fn exec_footprints_after(
        &self,
        after_seq: i64,
        limit: usize,
    ) -> CoordStoreResult<Vec<ExecFootprint>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT f.principal_id, f.seq, f.ts, f.path, f.detail, p.harness
             FROM footprints f
             LEFT JOIN principals p ON p.principal_id = f.principal_id
             WHERE f.op = 'exec' AND f.seq > ?1
             ORDER BY f.seq DESC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![after_seq, limit as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        let mut out = Vec::with_capacity(rows.len());
        for (pid, seq, ts, path, detail, harness) in rows {
            out.push(ExecFootprint {
                principal_id: pid,
                seq,
                ts: parse_ts(&ts)?,
                path,
                detail,
                harness,
            });
        }
        Ok(out)
    }

    // ────────────── turn digest (Concord P2c) ──────────────

    /// Current `digest_seq` for `principal_id`. `None` when the
    /// principal row is missing OR when the cursor was never
    /// initialised (pre-upgrade row or fresh principal). Tests use
    /// this to assert cursor advancement after a turn.
    pub fn digest_cursor(&self, principal_id: &str) -> CoordStoreResult<Option<i64>> {
        if validate_principal_id(principal_id).is_err() {
            return Ok(None);
        }
        let conn = self.lock();
        let cursor: Option<Option<i64>> = conn
            .query_row(
                "SELECT digest_seq FROM principals WHERE principal_id = ?1",
                params![principal_id],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?;
        Ok(cursor.flatten())
    }

    /// Compute the once-only turn digest for `principal_id`: writes
    /// on paths the caller has any footprint on, by non-lineage
    /// principals, with `seq` strictly greater than the caller's
    /// `digest_seq` cursor and at most `limit` paths returned
    /// (newest first). Always advances the cursor to MAX(footprints.seq)
    /// before returning, even when the digest is empty — that's what
    /// makes subsequent calls skip already-digested writes.
    ///
    /// The whole method runs under one `self.lock()` acquisition
    /// (`std::sync::Mutex` is non-reentrant, so the inner lineage
    /// walk uses the free `lineage_in(&conn, …)` helper, not
    /// `self.lineage()`).
    pub fn take_turn_digest(
        &self,
        principal_id: &str,
        limit: usize,
        ignore: &dyn Fn(&str) -> bool,
    ) -> CoordStoreResult<TurnDigest> {
        if validate_principal_id(principal_id).is_err() {
            return Ok(TurnDigest {
                changes: Vec::new(),
                total_paths: 0,
                cursor_initialized: false,
            });
        }
        let conn = self.lock();

        // Missing principal → empty digest (matches claim_undelivered's
        // "unknown principal returns Ok(vec![])" contract for a
        // first-turn session that hasn't yet propagated its row).
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM principals WHERE principal_id = ?1",
            params![principal_id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Ok(TurnDigest {
                changes: Vec::new(),
                total_paths: 0,
                cursor_initialized: false,
            });
        }

        let cursor: Option<i64> = conn.query_row(
            "SELECT digest_seq FROM principals WHERE principal_id = ?1",
            params![principal_id],
            |r| r.get::<_, Option<i64>>(0),
        )?;

        // MAX() over an empty footprints table is NULL → COALESCE to 0
        // so the bounded query `(cursor, hi]` stays a valid range even
        // when no writes exist yet.
        let hi: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM footprints", [], |r| {
            r.get(0)
        })?;

        if cursor.is_none() {
            // Lazy-init cursor on first turn; nothing to render.
            conn.execute(
                "UPDATE principals SET digest_seq = ?1 WHERE principal_id = ?2",
                params![hi, principal_id],
            )?;
            return Ok(TurnDigest {
                changes: Vec::new(),
                total_paths: 0,
                cursor_initialized: true,
            });
        }
        let cursor = cursor.expect("checked Some above");

        // Lineage exclude set. Free fn, not `self.lineage()` —
        // re-entering the std Mutex would deadlock.
        let lineage = lineage_in(&conn, principal_id, LINEAGE_MAX_HOPS)?;
        let exclude_json = serde_json::to_string(&lineage.iter().cloned().collect::<Vec<_>>())?;

        // Per-path aggregation: pick the newest qualifying seq, count
        // distinct non-lineage writers on that path, and join the
        // writer's harness + cwd back from principals.
        //
        // CTE shape so the WHERE in the outer scan can reference the
        // precomputed MAX(seq) without a self-join:
        //   eligible: every write in (cursor, hi] by a non-lineage
        //   principal that the caller has touched at any earlier seq
        //   top:     per-path MAX(seq) from eligible
        let mut stmt = conn.prepare(
            "WITH eligible AS (
                 SELECT f.seq, f.principal_id, f.path, f.ts
                 FROM footprints f
                 WHERE f.op = 'write'
                   AND f.seq > ?1 AND f.seq <= ?2
                   AND NOT EXISTS (
                       SELECT 1 FROM json_each(?3) AS ex
                       WHERE ex.value = f.principal_id
                   )
                   AND EXISTS (
                       SELECT 1 FROM footprints m
                       WHERE m.principal_id = ?4
                         AND m.path      = f.path
                         AND m.seq       < f.seq
                   )
             ),
             top AS (
                 SELECT path, MAX(seq) AS max_seq, COUNT(DISTINCT principal_id) AS writers
                 FROM eligible
                 GROUP BY path
             )
             SELECT t.path, t.max_seq, t.writers, e.principal_id, e.ts,
                    p.harness, p.cwd
             FROM top t
             JOIN eligible e ON e.path = t.path AND e.seq = t.max_seq
             LEFT JOIN principals p ON p.principal_id = e.principal_id
             ORDER BY t.max_seq DESC",
        )?;
        let rows = stmt
            .query_map(params![cursor, hi, exclude_json, principal_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);

        // Filter ignored paths in Rust (after SQL has reduced to one
        // row per path), then set the cap + total to reflect only the
        // survivors. The candidate set is writes since the last turn
        // per principal, so it's small; filtering here keeps the count
        // and the cap consistent and lets the final `UPDATE` advance
        // the cursor past ignored rows for once-only semantics.
        type DigestRow = (
            String,
            i64,
            i64,
            String,
            String,
            Option<String>,
            Option<String>,
        );
        let filtered: Vec<DigestRow> = rows.into_iter().filter(|row| !ignore(&row.0)).collect();
        let total_paths = filtered.len();

        let mut changes = Vec::with_capacity(limit.min(filtered.len()));
        for (path, seq, writers, pid, ts, harness, cwd) in filtered.into_iter().take(limit) {
            changes.push(DigestChange {
                path,
                principal_id: pid,
                seq,
                ts: parse_ts(&ts)?,
                harness,
                cwd,
                writer_count: writers.max(0) as usize,
            });
        }

        // Advance the cursor to MAX(seq), even when `changes` is
        // empty — that's how once-only works across subsequent calls.
        conn.execute(
            "UPDATE principals SET digest_seq = ?1 WHERE principal_id = ?2",
            params![hi, principal_id],
        )?;

        Ok(TurnDigest {
            changes,
            total_paths,
            cursor_initialized: false,
        })
    }

    // ────────────── hot claims (Concord P2) ──────────────

    /// Claim (or refresh) the hot claim on `path` for a write of
    /// `write_seq` by `principal_id`, expiring `ttl_secs` from now.
    ///
    /// The whole decision runs under one connection lock so two
    /// writers can't both take an expired claim:
    ///   (a) no row, or `expires_at <= now` → take (`Taken`),
    ///   (b) holder == caller or holder ∈ caller's lineage → refresh
    ///       (`Renewed`, holder unchanged),
    ///   (c) otherwise → `Contended { holder }`, row untouched.
    ///
    /// `expires_at` is stored through `fmt_ts` (fixed-width RFC3339
    /// micros) so the `<= now` liveness test and the read-side sweep
    /// compare strings lexicographically, exactly like
    /// `prune_old_footprints`.
    pub fn claim_hot(
        &self,
        path: &str,
        principal_id: &str,
        write_seq: i64,
        ttl_secs: i64,
    ) -> CoordStoreResult<HotClaimOutcome> {
        let now = Utc::now();
        let now_str = fmt_ts(now);
        let expires_str = fmt_ts(now + ChronoDuration::seconds(ttl_secs));
        let conn = self.lock();
        let existing: Option<(String, String)> = conn
            .query_row(
                "SELECT principal_id, expires_at FROM hot_claims WHERE path = ?1",
                params![path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let holder = match existing {
            None => {
                conn.execute(
                    "INSERT OR REPLACE INTO hot_claims
                        (path, principal_id, expires_at, last_write_seq)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![path, principal_id, expires_str, write_seq],
                )?;
                return Ok(HotClaimOutcome::Taken);
            }
            Some((holder, expires_at)) => {
                if expires_at <= now_str {
                    conn.execute(
                        "INSERT OR REPLACE INTO hot_claims
                            (path, principal_id, expires_at, last_write_seq)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![path, principal_id, expires_str, write_seq],
                    )?;
                    return Ok(HotClaimOutcome::Taken);
                }
                holder
            }
        };
        if holder == principal_id
            || lineage_in(&conn, principal_id, LINEAGE_MAX_HOPS)?.contains(&holder)
        {
            conn.execute(
                "UPDATE hot_claims SET expires_at = ?1, last_write_seq = ?2 WHERE path = ?3",
                params![expires_str, write_seq, path],
            )?;
            Ok(HotClaimOutcome::Renewed)
        } else {
            Ok(HotClaimOutcome::Contended { holder })
        }
    }

    /// The live claim on `path`, if any (`expires_at > now`), hydrated
    /// with `last_write_ts` from the matching footprint row. `None`
    /// when there is no claim or it has expired.
    pub fn live_hot_claim(&self, path: &str) -> CoordStoreResult<Option<HotClaim>> {
        let now_str = fmt_ts(Utc::now());
        let conn = self.lock();
        let row: Option<(String, String, i64, Option<String>)> = conn
            .query_row(
                "SELECT h.principal_id, h.expires_at, h.last_write_seq, f.ts
                 FROM hot_claims h
                 LEFT JOIN footprints f ON f.seq = h.last_write_seq
                 WHERE h.path = ?1 AND h.expires_at > ?2",
                params![path, now_str],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        row.map(
            |(principal_id, expires_at, last_write_seq, last_write_ts)| {
                Ok(HotClaim {
                    path: path.to_string(),
                    principal_id,
                    expires_at: parse_ts(&expires_at)?,
                    last_write_seq,
                    last_write_ts: last_write_ts.as_deref().map(parse_ts).transpose()?,
                })
            },
        )
        .transpose()
    }

    /// All live claims, ordered by path. Sweeps expired rows first
    /// (read-side TTL), then selects the survivors with the same
    /// footprint JOIN as [`CoordStore::live_hot_claim`].
    pub fn list_live_hot_claims(&self) -> CoordStoreResult<Vec<HotClaim>> {
        let now_str = fmt_ts(Utc::now());
        let conn = self.lock();
        conn.execute(
            "DELETE FROM hot_claims WHERE expires_at <= ?1",
            params![now_str],
        )?;
        let mut stmt = conn.prepare(
            "SELECT h.path, h.principal_id, h.expires_at, h.last_write_seq, f.ts
             FROM hot_claims h
             LEFT JOIN footprints f ON f.seq = h.last_write_seq
             ORDER BY h.path",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        let mut out = Vec::with_capacity(rows.len());
        for (path, principal_id, expires_at, last_write_seq, last_write_ts) in rows {
            out.push(HotClaim {
                path,
                principal_id,
                expires_at: parse_ts(&expires_at)?,
                last_write_seq,
                last_write_ts: last_write_ts.as_deref().map(parse_ts).transpose()?,
            });
        }
        Ok(out)
    }

    // ────────────── owns audit (Concord P2d) ──────────────

    /// Look up the active worktree principal whose `worktree` dir is
    /// the longest component-aligned prefix of `target`. `ended_at IS
    /// NULL` is the only liveness filter — `status_at` and
    /// `list_principals(false)` drop 'stale' rows, and worktree
    /// principals have no pids so they always go stale after the TTL.
    /// Filtering on `ended_at` keeps enforcement in place across the
    /// staleness cliff (DoD 5).
    ///
    /// Selects only `labels.kind='worktree'` so an `agent:` row
    /// without the worktree label doesn't shadow a real worktree
    /// principal. Owns is parsed from `labels.owns`; a missing or
    /// non-array field returns an empty Vec (treated as "no claim").
    pub fn owning_worktree_principal(
        &self,
        target: &std::path::Path,
    ) -> CoordStoreResult<Option<WorktreePrincipal>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT principal_id, worktree, labels FROM principals
             WHERE ended_at IS NULL
               AND worktree IS NOT NULL AND worktree <> ''
               AND json_valid(labels) = 1
               AND json_extract(labels, '$.kind') = 'worktree'",
        )?;
        let rows: Vec<(String, String, Option<String>)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        // canonicalize_lossy makes filesystem syscalls per row: never hold
        // the connection lock across them.
        drop(conn);

        let canonical_target = canonicalize_lossy(target);

        let mut best: Option<WorktreePrincipal> = None;
        for (pid, worktree_raw, labels_raw) in rows {
            let wt = canonicalize_lossy(std::path::Path::new(&worktree_raw));
            if !canonical_target.starts_with(&wt) {
                continue;
            }
            // Component-aligned prefix: strip the worktree root and
            // require a non-empty relative path. `Path::strip_prefix`
            // returns Err when the prefix isn't component-aligned, so
            // `/wt/a` cannot match `/wt/ab`.
            let rel = match canonical_target.strip_prefix(&wt) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if rel.as_os_str().is_empty() {
                continue;
            }
            let owns = parse_owns_field(labels_raw.as_deref());
            let candidate_components = wt.components().count();
            let best_components = best
                .as_ref()
                .map(|b| b.worktree.components().count())
                .unwrap_or(0);
            if candidate_components > best_components {
                best = Some(WorktreePrincipal {
                    principal_id: pid,
                    worktree: wt,
                    owns,
                    rel: rel.to_string_lossy().replace('\\', "/"),
                });
            }
        }
        Ok(best)
    }

    /// Insert one audit row recording that the precheck saw a write
    /// to `path` (worktree-relative) that fell outside the worktree
    /// principal's claimed `owns` scope. Returns the new autoincrement
    /// `seq` so the caller can echo it back in the response.
    pub fn record_owns_violation(
        &self,
        worktree_principal: &str,
        path: &str,
        caller_principal: Option<&str>,
    ) -> CoordStoreResult<i64> {
        let now_str = fmt_ts(Utc::now());
        let conn = self.lock();
        conn.execute(
            "INSERT INTO owns_violations
                (worktree_principal, path, caller_principal, ts)
             VALUES (?1, ?2, ?3, ?4)",
            params![worktree_principal, path, caller_principal, now_str],
        )?;
        let seq: i64 = conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0))?;
        Ok(seq)
    }

    /// List audit rows with `seq > since` in ascending order
    /// (newest-last), capped at `limit`. `since <= 0` returns from the
    /// start. The endpoint surfaces the result under `{"violations":
    /// [...], "next_seq": <last seq or since>}`.
    pub fn list_owns_violations(
        &self,
        since: i64,
        limit: usize,
    ) -> CoordStoreResult<Vec<OwnsViolation>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT seq, worktree_principal, path, caller_principal, ts
             FROM owns_violations
             WHERE seq > ?1
             ORDER BY seq ASC
             LIMIT ?2",
        )?;
        let rows: Vec<(i64, String, String, Option<String>, String)> = stmt
            .query_map(params![since, limit as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        let mut out = Vec::with_capacity(rows.len());
        for (seq, worktree_principal, path, caller_principal, ts) in rows {
            out.push(OwnsViolation {
                seq,
                worktree_principal,
                path,
                caller_principal,
                ts: parse_ts(&ts)?,
            });
        }
        Ok(out)
    }

    // ────────────── intents (Concord P3) ──────────────

    /// Upsert one captured prompt + its embedding for `principal_id`.
    /// This is the REPLACE path: it stores the raw embedder vector
    /// verbatim (no normalization), overwrites the text, and resets
    /// `samples` to 1. Concord P3b's blended path uses a different
    /// method (`upsert_intent_blended`) that mixes the new vector into
    /// the prior one and increments `samples` — both paths are
    /// intentionally distinct so existing tests keep byte-identical
    /// behaviour. Embedding is encoded as little-endian bytes via
    /// [`encode_embedding`] so a fixed-width round-trip survives schema
    /// dumps. `at` is the `updated_at` timestamp so tests can plant
    /// stale rows without sleeping. An empty embedding is rejected
    /// with `InvalidBody` — the hook's spawned task would otherwise
    /// write a 0-vector that matches every other 0-vector by
    /// definition.
    pub fn upsert_intent(
        &self,
        principal_id: &str,
        text: &str,
        embedding: &[f32],
        at: DateTime<Utc>,
    ) -> CoordStoreResult<()> {
        validate_principal_id(principal_id)?;
        if embedding.is_empty() {
            return Err(CoordStoreError::InvalidBody(
                "intent embedding must be non-empty".into(),
            ));
        }
        let conn = self.lock();
        write_intent_row(&conn, principal_id, text, embedding, 1, at)?;
        Ok(())
    }

    /// Concord P3b — exponentially-weighted blend of a principal's
    /// stored intent with a freshly-captured prompt embedding.
    ///
    /// On every call, the stored vector becomes
    /// `normalize((1-α)·normalize(e_old) + α·normalize(e_new))`, the
    /// stored text becomes `e_new`'s text when the new prompt is
    /// on-topic with the old one (cosine ≥ [`INTENT_TEXT_KEEP_COS`])
    /// and `e_old`'s text otherwise, and `samples` increments by 1.
    ///
    /// The blended path is short-circuited to the REPLACE path on any
    /// of the following: no stored row, `alpha >= 1.0`, a dim mismatch,
    /// `old.updated_at < at - window_secs` (the row is too old to be a
    /// useful prior), or any of the three vectors (old / new / mixed)
    /// failing the L2-norm gate (zero or non-finite). The replace
    /// branch stores the RAW embedder vector verbatim — exactly what
    /// the original `upsert_intent` does — so callers that always
    /// pass `alpha = 1.0` reproduce today's behaviour byte-for-byte.
    ///
    /// The whole read-modify-write runs under one
    /// `std::sync::Mutex` acquisition so two spawned capture tasks
    /// targeting the same principal cannot lose a blended sample.
    /// Re-entering `self.lock()` would deadlock the non-reentrant
    /// mutex, so the inner SELECT and write go through free helpers
    /// that take `&Connection`.
    pub fn upsert_intent_blended(
        &self,
        principal_id: &str,
        text: &str,
        embedding: &[f32],
        at: DateTime<Utc>,
        alpha: f32,
        window_secs: i64,
    ) -> CoordStoreResult<()> {
        validate_principal_id(principal_id)?;
        if embedding.is_empty() {
            return Err(CoordStoreError::InvalidBody(
                "intent embedding must be non-empty".into(),
            ));
        }
        if !alpha.is_finite() || alpha <= 0.0 || alpha > 1.0 {
            return Err(CoordStoreError::InvalidBody(format!(
                "intent alpha must be finite and in (0,1]; got {alpha}"
            )));
        }
        let conn = self.lock();
        let existing: Option<(String, Vec<u8>, i64, String, i64)> = conn
            .query_row(
                "SELECT text, embedding, dim, updated_at, samples
                 FROM intents WHERE principal_id = ?1",
                params![principal_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()?;
        let (existing_text, existing_blob, existing_dim, existing_ts, existing_samples) =
            match existing {
                Some(t) => t,
                None => {
                    write_intent_row(&conn, principal_id, text, embedding, 1, at)?;
                    return Ok(());
                }
            };

        let new_dim = embedding.len() as i64;
        let replace_dim = existing_dim != new_dim;
        let stale = match parse_ts(&existing_ts) {
            Ok(t) => t < at - ChronoDuration::seconds(window_secs),
            Err(_) => true,
        };
        let replace_alpha = alpha >= 1.0;
        let new_norm = l2_norm(embedding);
        let new_norm_ok = new_norm.is_finite() && new_norm > 0.0;
        if replace_dim || stale || replace_alpha || !new_norm_ok {
            write_intent_row(&conn, principal_id, text, embedding, 1, at)?;
            return Ok(());
        }

        // Stored blob was written by either the replace path (raw) or
        // a previous blend (unit-norm). Decode and gate on its L2
        // norm; a degenerate old vector forces a replace.
        let old_vec = decode_embedding(&existing_blob)?;
        let old_norm = l2_norm(&old_vec);
        if !old_norm.is_finite() || old_norm <= 0.0 {
            write_intent_row(&conn, principal_id, text, embedding, 1, at)?;
            return Ok(());
        }

        // Normalize both inputs, then mix.
        let n_old = normalized(&old_vec);
        let n_new = normalized(embedding);
        if n_old.len() != n_new.len() {
            // Length mismatch is already guarded by the dim check above,
            // but defense-in-depth: replace if normalization collapsed a
            // dim.
            write_intent_row(&conn, principal_id, text, embedding, 1, at)?;
            return Ok(());
        }
        let alpha_f = f64::from(alpha);
        let mut mixed: Vec<f32> = Vec::with_capacity(n_new.len());
        for (a, b) in n_old.iter().zip(n_new.iter()) {
            mixed.push((((1.0 - alpha_f) * f64::from(*a)) + (alpha_f * f64::from(*b))) as f32);
        }
        let mixed_norm = l2_norm(&mixed);
        if !mixed_norm.is_finite() || mixed_norm <= f32::EPSILON {
            write_intent_row(&conn, principal_id, text, embedding, 1, at)?;
            return Ok(());
        }
        let stored = normalized(&mixed);
        if stored.is_empty() {
            write_intent_row(&conn, principal_id, text, embedding, 1, at)?;
            return Ok(());
        }

        // Cosine of the two unit vectors = their dot product.
        let cos: f32 = n_old.iter().zip(n_new.iter()).map(|(a, b)| *a * *b).sum();
        let stored_text = if cos.is_finite() && cos >= INTENT_TEXT_KEEP_COS {
            text
        } else {
            existing_text.as_str()
        };

        let samples = existing_samples.saturating_add(1);
        write_intent_row(&conn, principal_id, stored_text, &stored, samples, at)?;
        Ok(())
    }

    /// Read a single intent. `None` when the principal has never had a
    /// prompt captured (or when `principal_id` is malformed, to match
    /// `get_principal`'s 404-vs-400 contract).
    pub fn get_intent(&self, principal_id: &str) -> CoordStoreResult<Option<Intent>> {
        if validate_principal_id(principal_id).is_err() {
            return Ok(None);
        }
        let conn = self.lock();
        let row: Option<(String, String, Vec<u8>, i64, String, i64)> = conn
            .query_row(
                "SELECT principal_id, text, embedding, dim, updated_at, samples
                 FROM intents WHERE principal_id = ?1",
                params![principal_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((pid, text, blob, dim, updated_at, samples)) => {
                let embedding = decode_embedding(&blob)?;
                Ok(Some(Intent {
                    principal_id: pid,
                    text,
                    embedding,
                    dim: dim.max(0) as usize,
                    updated_at: parse_ts(&updated_at)?,
                    samples,
                }))
            }
        }
    }

    /// All intents whose principal row is still live (`ended_at IS NULL`)
    /// and whose `updated_at >= now - window_secs`. Newest first. The
    /// caller passes `now` so tests can simulate staleness without
    /// sleeping. Thin wrapper over [`live_intents_in`] so callers that
    /// already hold the lock (e.g. `best_topic_match`) can run the same
    /// query without re-entering the non-reentrant `std::sync::Mutex`.
    pub fn list_live_intents(
        &self,
        window_secs: i64,
        now: DateTime<Utc>,
    ) -> CoordStoreResult<Vec<Intent>> {
        let conn = self.lock();
        live_intents_in(&conn, window_secs, now)
    }

    /// Upsert one (a, b) notice if (a, b) has not been noticed in the
    /// last `dedup_secs`. `pair_key` is the two ids sorted
    /// lexicographically and joined by '|' — a separator outside the
    /// principal-id alphabet `[A-Za-z0-9._@/-:]`. Returns `true` when
    /// this call took the notice (caller should emit the line and bump
    /// the metric); `false` when the dedup window suppressed it.
    ///
    /// Atomicity: `INSERT ... ON CONFLICT DO UPDATE ... WHERE
    /// notified_at < cutoff` then `conn.changes() > 0`. Two concurrent
    /// hook calls cannot both notice the same pair.
    pub fn claim_topic_notice(
        &self,
        a: &str,
        b: &str,
        dedup_secs: i64,
        now: DateTime<Utc>,
    ) -> CoordStoreResult<bool> {
        let mut ids = [a.to_string(), b.to_string()];
        ids.sort();
        let pair_key = format!("{}|{}", ids[0], ids[1]);
        let cutoff = fmt_ts(now - ChronoDuration::seconds(dedup_secs));
        let now_str = fmt_ts(now);
        let conn = self.lock();
        let changes = conn.execute(
            "INSERT INTO topic_notices (pair_key, notified_at)
             VALUES (?1, ?2)
             ON CONFLICT(pair_key) DO UPDATE SET
                notified_at = excluded.notified_at
             WHERE topic_notices.notified_at < ?3",
            params![pair_key, now_str, cutoff],
        )?;
        Ok(changes > 0)
    }

    // ────────────── topic pairs (Concord P3) ──────────────

    /// Best-similarity match against non-lineage live intents. `None`
    /// when (a) the caller has no stored intent, or (b) every other live
    /// intent is self / lineage / dim-mismatched. The similarity is
    /// computed via the caller's closure — keeps `EmbeddingService` out
    /// of `CoordStore`'s dependency graph (the API layer passes
    /// `services.embedding.calculate_similarity`).
    ///
    /// Concurrency: takes the lock once. `lineage_in(&conn, ...)` is
    /// the free helper; calling `self.lineage()` here would deadlock
    /// the non-reentrant `std::sync::Mutex`.
    pub fn best_topic_match<F>(
        &self,
        caller: &str,
        window_secs: i64,
        now: DateTime<Utc>,
        sim: F,
    ) -> CoordStoreResult<Option<TopicMatch>>
    where
        F: Fn(&[f32], &[f32]) -> f32,
    {
        let conn = self.lock();
        let caller_row: Option<(Vec<u8>, i64)> = conn
            .query_row(
                "SELECT embedding, dim FROM intents WHERE principal_id = ?1",
                params![caller],
                |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?;
        let (caller_blob, caller_dim) = match caller_row {
            Some(t) => t,
            None => return Ok(None),
        };
        if caller_dim <= 0 {
            return Ok(None);
        }
        let caller_vec = decode_embedding(&caller_blob)?;
        let lineage = lineage_in(&conn, caller, LINEAGE_MAX_HOPS)?;
        let others = live_intents_in(&conn, window_secs, now)?;
        let mut best: Option<TopicMatch> = None;
        for o in others {
            if o.principal_id == caller || lineage.contains(&o.principal_id) {
                continue;
            }
            if o.dim != caller_dim as usize {
                continue;
            }
            let s = sim(&caller_vec, &o.embedding);
            if !s.is_finite() {
                continue;
            }
            let dominated = match &best {
                Some(prev) => s <= prev.similarity,
                None => false,
            };
            if dominated {
                continue;
            }
            best = Some(TopicMatch {
                other: o.principal_id,
                other_text: o.text,
                similarity: s,
            });
        }
        Ok(best)
    }

    /// All i<j pairs of live intents with equal dim where neither side
    /// is in the other's lineage, filtered by `min`, sorted descending
    /// (NaN treated as less-than), truncated to `limit`. Computes each
    /// lineage exactly once per principal under one lock acquisition.
    pub fn topic_pairs<F>(
        &self,
        window_secs: i64,
        now: DateTime<Utc>,
        min: f32,
        limit: usize,
        sim: F,
    ) -> CoordStoreResult<Vec<TopicPair>>
    where
        F: Fn(&[f32], &[f32]) -> f32,
    {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.lock();
        let intents = live_intents_in(&conn, window_secs, now)?;
        // live_intents_in already decoded the blob into Vec<f32>, so
        // here we just project the struct fields.
        let mut decoded: Vec<(String, String, Vec<f32>, usize)> = Vec::with_capacity(intents.len());
        for i in intents {
            decoded.push((i.principal_id, i.text, i.embedding, i.dim));
        }
        // Group by dim so we only compare same-dim pairs.
        let mut by_dim: std::collections::BTreeMap<usize, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (idx, (_, _, _, d)) in decoded.iter().enumerate() {
            by_dim.entry(*d).or_default().push(idx);
        }
        // Cache lineage sets keyed by principal_id.
        let mut lineage_cache: std::collections::HashMap<String, HashSet<String>> =
            std::collections::HashMap::new();
        let mut pairs: Vec<TopicPair> = Vec::new();
        for indices in by_dim.values() {
            for (pos, &i) in indices.iter().enumerate() {
                let (ref pid_i, ref text_i, ref emb_i, _) = decoded[i];
                // A lineage lookup error propagates, as in best_topic_match:
                // an empty fallback would surface lineage pairs as overlaps.
                if !lineage_cache.contains_key(pid_i) {
                    let l = lineage_in(&conn, pid_i, LINEAGE_MAX_HOPS)?;
                    lineage_cache.insert(pid_i.clone(), l);
                }
                let lin_i = &lineage_cache[pid_i];
                for &j in &indices[pos + 1..] {
                    let (ref pid_j, ref text_j, ref emb_j, _) = decoded[j];
                    if lin_i.contains(pid_j) {
                        continue;
                    }
                    let s = sim(emb_i, emb_j);
                    if !s.is_finite() || s < min {
                        continue;
                    }
                    pairs.push(TopicPair {
                        a: pid_i.clone(),
                        b: pid_j.clone(),
                        similarity: s,
                        a_text: text_i.clone(),
                        b_text: text_j.clone(),
                    });
                }
            }
        }
        // Descending by similarity; NaN treated as less so a stray NaN
        // never crowds the top of the list.
        pairs.sort_by(|x, y| {
            y.similarity
                .partial_cmp(&x.similarity)
                .unwrap_or(std::cmp::Ordering::Less)
        });
        pairs.truncate(limit);
        Ok(pairs)
    }
}

/// Free helper behind [`CoordStore::list_live_intents`] — same
/// INNER JOIN `principals WHERE ended_at IS NULL` + `updated_at >=
/// cutoff`, with the connection already locked by the caller. Newest
/// first.
fn live_intents_in(
    conn: &Connection,
    window_secs: i64,
    now: DateTime<Utc>,
) -> CoordStoreResult<Vec<Intent>> {
    let cutoff = fmt_ts(now - ChronoDuration::seconds(window_secs));
    let mut stmt = conn.prepare(
        "SELECT i.principal_id, i.text, i.embedding, i.dim, i.updated_at, i.samples
         FROM intents i
         INNER JOIN principals p ON p.principal_id = i.principal_id
         WHERE p.ended_at IS NULL AND i.updated_at >= ?1
         ORDER BY i.updated_at DESC",
    )?;
    let rows: Vec<(String, String, Vec<u8>, i64, String, i64)> = stmt
        .query_map(params![cutoff], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let mut out = Vec::with_capacity(rows.len());
    for (pid, text, blob, dim, updated_at, samples) in rows {
        let embedding = decode_embedding(&blob)?;
        out.push(Intent {
            principal_id: pid,
            text,
            embedding,
            dim: dim.max(0) as usize,
            updated_at: parse_ts(&updated_at)?,
            samples,
        });
    }
    Ok(out)
}

/// Canonicalize a path with the deepest-existing-ancestor strategy
/// used by [`CoordStore::owning_worktree_principal`]. Walks toward the
/// root until `std::fs::canonicalize` succeeds, then re-appends the
/// missing tail. Falls back to the raw path when nothing on the way
/// up canonicalizes.
///
/// On macOS `/var/folders/...` resolves to `/private/var/folders/...`
/// via `canonicalize`; without this helper a brand-new target file
/// would not canonicalize at all and the prefix match against the
/// stored worktree would silently fail (every DoD test passes
/// vacuously as "no violation").
fn canonicalize_lossy(path: &std::path::Path) -> std::path::PathBuf {
    let mut cursor = path.to_path_buf();
    let mut missing: Vec<std::path::PathBuf> = Vec::new();
    loop {
        match std::fs::canonicalize(&cursor) {
            Ok(canon) => {
                // Re-append the missing tail.
                let mut out = canon;
                for piece in missing.iter().rev() {
                    out.push(piece);
                }
                return out;
            }
            Err(_) => {
                let Some(parent) = cursor.parent() else {
                    return path.to_path_buf();
                };
                let name = cursor
                    .file_name()
                    .map(|n| n.to_os_string())
                    .unwrap_or_default();
                if name.is_empty() {
                    return path.to_path_buf();
                }
                missing.push(std::path::PathBuf::from(name));
                cursor = parent.to_path_buf();
                if cursor.as_os_str().is_empty() {
                    return path.to_path_buf();
                }
            }
        }
    }
}

/// Parse `labels.owns` into a normalized Vec. Trim each element, strip
/// a leading `./`, strip a trailing `/`, drop empties. A missing
/// field, a non-string element, a non-array, or invalid JSON returns
/// an empty Vec (treated as "no claim"). Errors are swallowed because
/// the helper runs in a hot path; corrupt JSON is reported by the
/// row-reader path elsewhere.
fn parse_owns_field(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let v: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(arr) = v.get("owns").and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for item in arr {
        let Some(s) = item.as_str() else {
            continue;
        };
        let trimmed = s.trim();
        let stripped = trimmed
            .strip_prefix("./")
            .unwrap_or(trimmed)
            .trim_end_matches('/');
        if stripped.is_empty() {
            continue;
        }
        out.push(stripped.to_string());
    }
    out
}

/// Lineage traversal on an already-locked `Connection`. Split out of
/// [`CoordStore::lineage`] so `claim_hot` can test "holder ∈ lineage"
/// under the SAME lock it uses for the read/upsert — `std::sync::Mutex`
/// is not reentrant, so calling the public `self.lineage()` while
/// holding `self.lock()` would deadlock.
///
/// Lineage = ancestors (walk `labels.parent` upward) ∪ descendants
/// (principals whose `labels.parent` equals a lineage member, walked
/// downward). Self is always included. Siblings and cousins are NOT
/// lineage. BFS-like with a visited set so cycles (a→b→a) terminate.
fn lineage_in(
    conn: &Connection,
    principal_id: &str,
    max_hops: usize,
) -> CoordStoreResult<HashSet<String>> {
    // Two parallel queues for the BFS. The visited set grows as
    // we pull a principal off either queue so a cycle that
    // touches the same node from both sides still terminates.
    let mut visited: HashSet<String> = HashSet::new();
    let mut up: VecDeque<(String, usize)> = VecDeque::new();
    let mut down: VecDeque<(String, usize)> = VecDeque::new();
    visited.insert(principal_id.to_string());
    up.push_back((principal_id.to_string(), 0));
    down.push_back((principal_id.to_string(), 0));

    while let Some((pid, depth)) = up.pop_front() {
        if depth > 0 {
            // depth=0 is self — already in `visited`.
            visited.insert(pid.clone());
        }
        if depth >= max_hops {
            continue;
        }
        // Walk up via labels.parent (a JSON string).
        let parent: Option<String> = conn
            .query_row(
                "SELECT json_extract(labels, '$.parent') FROM principals
                 WHERE principal_id = ?1 AND json_valid(labels) = 1",
                params![&pid],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if let Some(p) = parent {
            if !p.is_empty() && visited.insert(p.clone()) {
                up.push_back((p, depth + 1));
            }
        }
    }
    while let Some((pid, depth)) = down.pop_front() {
        if depth > 0 {
            visited.insert(pid.clone());
        }
        if depth >= max_hops {
            continue;
        }
        // Walk down: every principal whose labels.parent == pid.
        let mut stmt = conn.prepare(
            "SELECT principal_id FROM principals
             WHERE json_valid(labels) = 1
               AND json_extract(labels, '$.parent') = ?1",
        )?;
        let children: Vec<String> = stmt
            .query_map(params![&pid], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for c in children {
            if visited.insert(c.clone()) {
                down.push_back((c, depth + 1));
            }
        }
    }
    Ok(visited)
}

fn status_str(s: &str) -> PrincipalStatus {
    match s {
        "live" => PrincipalStatus::Live,
        "idle" => PrincipalStatus::Idle,
        "stale" => PrincipalStatus::Stale,
        "ended" => PrincipalStatus::Ended,
        _ => PrincipalStatus::Stale,
    }
}

struct PrincipalRow {
    principal_id: String,
    kind: String,
    harness: Option<String>,
    host: Option<String>,
    cwd: Option<String>,
    worktree: Option<String>,
    pgid: Option<i64>,
    pids: Option<String>,
    tmux_pane: Option<String>,
    kill_recipe: Option<String>,
    priority: Option<i64>,
    labels: Option<String>,
    started_at: String,
    last_seen: String,
    ended_at: Option<String>,
}

impl PrincipalRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            principal_id: r.get(0)?,
            kind: r.get(1)?,
            harness: r.get(2)?,
            host: r.get(3)?,
            cwd: r.get(4)?,
            worktree: r.get(5)?,
            pgid: r.get(6)?,
            pids: r.get(7)?,
            tmux_pane: r.get(8)?,
            kill_recipe: r.get(9)?,
            priority: r.get(10)?,
            labels: r.get(11)?,
            started_at: r.get(12)?,
            last_seen: r.get(13)?,
            ended_at: r.get(14)?,
        })
    }

    /// Fallible on purpose: a corrupt stored timestamp or JSON column
    /// surfaces as an error (→ 500) instead of panicking under the lock
    /// or silently reading as an empty field.
    fn into_principal(self) -> CoordStoreResult<Principal> {
        let pids = self
            .pids
            .as_deref()
            .map(serde_json::from_str::<Vec<i64>>)
            .transpose()?;
        let kill = self
            .kill_recipe
            .as_deref()
            .map(serde_json::from_str::<Vec<String>>)
            .transpose()?;
        let labels = self
            .labels
            .as_deref()
            .map(serde_json::from_str::<serde_json::Value>)
            .transpose()?;
        Ok(Principal {
            principal_id: self.principal_id,
            kind: self.kind,
            started_at: parse_ts(&self.started_at)?,
            last_seen: parse_ts(&self.last_seen)?,
            ended_at: self.ended_at.as_deref().map(parse_ts).transpose()?,
            status: "live".to_string(), // overwritten by callers
            harness: self.harness,
            host: self.host,
            cwd: self.cwd,
            worktree: self.worktree,
            pgid: self.pgid,
            pids,
            tmux_pane: self.tmux_pane,
            kill_recipe: kill,
            priority: self.priority,
            labels,
        })
    }
}

struct MessageRow {
    id: i64,
    principal_id: String,
    from_actor: String,
    body: String,
    created_at: String,
    delivered_at: Option<String>,
    delivered_to: Option<String>,
    acked_at: Option<String>,
    acked_by: Option<String>,
}

impl MessageRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            principal_id: r.get(1)?,
            from_actor: r.get(2)?,
            body: r.get(3)?,
            created_at: r.get(4)?,
            delivered_at: r.get(5)?,
            delivered_to: r.get(6)?,
            acked_at: r.get(7)?,
            acked_by: r.get(8)?,
        })
    }

    fn into_message(self) -> CoordStoreResult<Message> {
        Ok(Message {
            msg_id: format!("M-{}", self.id),
            principal_id: self.principal_id,
            from: self.from_actor,
            body: self.body,
            created_at: parse_ts(&self.created_at)?,
            delivered_at: self.delivered_at.as_deref().map(parse_ts).transpose()?,
            delivered_to: self.delivered_to,
            acked_at: self.acked_at.as_deref().map(parse_ts).transpose()?,
            acked_by: self.acked_by,
        })
    }
}

struct FootprintRow {
    seq: i64,
    principal_id: String,
    worker_id: String,
    op: String,
    path: String,
    mtime_ns: Option<i64>,
    size: Option<i64>,
    detail: Option<String>,
    ts: String,
}

impl FootprintRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            seq: r.get(0)?,
            principal_id: r.get(1)?,
            worker_id: r.get(2)?,
            op: r.get(3)?,
            path: r.get(4)?,
            mtime_ns: r.get(5)?,
            size: r.get(6)?,
            detail: r.get(7)?,
            ts: r.get(8)?,
        })
    }

    fn into_footprint(self) -> CoordStoreResult<Footprint> {
        Ok(Footprint {
            seq: self.seq,
            principal_id: self.principal_id,
            worker_id: self.worker_id,
            op: self.op,
            path: self.path,
            mtime_ns: self.mtime_ns,
            size: self.size,
            detail: self.detail,
            ts: parse_ts(&self.ts)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store() -> CoordStore {
        CoordStore::open_in_memory().expect("in-memory store should init")
    }

    #[test]
    fn id_validator_accepts_good_shapes() {
        assert!(validate_principal_id("loop:a").is_ok());
        assert!(validate_principal_id("human:amir").is_ok());
        assert!(validate_principal_id("run:x/y@z").is_ok());
    }

    #[test]
    fn id_validator_rejects_bad_shapes() {
        assert!(validate_principal_id("foo").is_err());
        assert!(validate_principal_id("loop:").is_err());
        assert!(validate_principal_id("bad kind:x").is_err());
        let long = "loop:".to_string() + &"a".repeat(60);
        assert!(validate_principal_id(&long).is_ok(), "60 chars OK");
        let over: String = "loop:".to_string() + &"a".repeat(129);
        assert!(
            validate_principal_id(&over).is_err(),
            "129-char name must fail"
        );
    }

    #[test]
    fn upsert_merges_partial_fields_and_advances_last_seen() {
        let s = store();
        let first = s
            .upsert_principal(
                "loop:a",
                PrincipalUpsert {
                    harness: Some("claude-code".into()),
                    host: Some("box1".into()),
                    cwd: Some("/work".into()),
                    pids: Some(vec![42]),
                    ..Default::default()
                },
            )
            .expect("first upsert");
        assert_eq!(first.harness.as_deref(), Some("claude-code"));
        assert_eq!(first.started_at, first.last_seen);

        std::thread::sleep(std::time::Duration::from_millis(10));

        let second = s
            .upsert_principal(
                "loop:a",
                PrincipalUpsert {
                    cwd: Some("/other".into()),
                    ..Default::default()
                },
            )
            .expect("second upsert");
        // Merged: prior fields kept, only cwd replaced.
        assert_eq!(second.harness.as_deref(), Some("claude-code"));
        assert_eq!(second.host.as_deref(), Some("box1"));
        assert_eq!(second.cwd.as_deref(), Some("/other"));
        assert_eq!(second.pids.as_deref(), Some(&vec![42_i64][..]));
        // last_seen advanced; started_at preserved.
        assert!(second.last_seen > first.last_seen);
        assert_eq!(second.started_at, first.started_at);
    }

    #[test]
    fn ack_is_idempotent() {
        let s = store();
        s.upsert_principal(
            "loop:a",
            PrincipalUpsert {
                harness: Some("h".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let m = s
            .post_message("loop:a", "alice", "hello")
            .expect("post message");
        let first = s
            .ack_message("loop:a", &m.msg_id, "bob")
            .expect("ack works");
        assert!(first.acked_at.is_some());
        let second = s
            .ack_message("loop:a", &m.msg_id, "carol")
            .expect("second ack is a no-op success");
        assert_eq!(first.acked_at, second.acked_at, "acked_at unchanged");
        assert_eq!(first.acked_by.as_deref(), Some("bob"));
    }

    #[test]
    fn ack_for_other_principal_is_not_found() {
        let s = store();
        s.upsert_principal(
            "loop:a",
            PrincipalUpsert {
                harness: Some("h".into()),
                ..Default::default()
            },
        )
        .unwrap();
        s.upsert_principal(
            "loop:b",
            PrincipalUpsert {
                harness: Some("h".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let m = s.post_message("loop:a", "alice", "hi").unwrap();
        let err = s.ack_message("loop:b", &m.msg_id, "bob").unwrap_err();
        assert!(matches!(err, CoordStoreError::NotFound));
    }

    #[test]
    fn pid_alive_rejects_zero_and_negative() {
        assert!(!pid_alive(0));
        assert!(!pid_alive(-1));
        assert!(!pid_alive(i32::MAX as i64 + 1));
    }

    #[test]
    fn rebind_replaces_principal_id() {
        let s = store();
        s.upsert_principal(
            "loop:a",
            PrincipalUpsert {
                harness: Some("h".into()),
                ..Default::default()
            },
        )
        .unwrap();
        s.upsert_principal(
            "loop:b",
            PrincipalUpsert {
                harness: Some("h".into()),
                ..Default::default()
            },
        )
        .unwrap();
        s.bind("w1", "loop:a", Some(7)).unwrap();
        let second = s.bind("w1", "loop:b", Some(8)).unwrap();
        assert_eq!(second.principal_id, "loop:b");
        let fetched = s.get_binding("w1").unwrap().expect("present");
        assert_eq!(fetched.principal_id, "loop:b");
    }

    #[test]
    fn end_principal_keeps_mailbox_and_binding() {
        let s = store();
        s.upsert_principal(
            "loop:a",
            PrincipalUpsert {
                harness: Some("h".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let m = s.post_message("loop:a", "alice", "before end").unwrap();
        s.bind("w1", "loop:a", Some(1)).unwrap();
        let ended = s.end_principal("loop:a").unwrap();
        assert_eq!(ended.status, "ended");
        // Mailbox still readable.
        let msgs = s.list_messages("loop:a", false).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].msg_id, m.msg_id);
        // Binding still resolvable.
        assert!(s.get_binding("w1").unwrap().is_some());
    }

    #[test]
    fn reopen_persists_everything() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("coord.db");
        let pids = vec![std::process::id() as i64];
        {
            let s = CoordStore::open(&path).expect("open");
            s.upsert_principal(
                "loop:a",
                PrincipalUpsert {
                    harness: Some("h".into()),
                    host: local_hostname(),
                    pids: Some(pids),
                    labels: Some(json!({"k": "v"})),
                    ..Default::default()
                },
            )
            .unwrap();
            s.bind("w1", "loop:a", Some(42)).unwrap();
            s.post_message("loop:a", "alice", "hi").unwrap();
        }
        let s2 = CoordStore::open(&path).expect("reopen");
        let p = s2.get_principal("loop:a").unwrap().expect("present");
        assert_eq!(p.harness.as_deref(), Some("h"));
        assert_eq!(
            p.labels
                .as_ref()
                .and_then(|v| v.get("k"))
                .and_then(|v| v.as_str()),
            Some("v")
        );
        assert!(s2.get_binding("w1").unwrap().is_some());
        assert_eq!(s2.list_messages("loop:a", false).unwrap().len(), 1);
    }

    #[test]
    fn corrupt_rows_are_errors_not_panics_and_the_store_keeps_serving() {
        let s = store();
        s.upsert_principal("loop:bad", PrincipalUpsert::default())
            .unwrap();
        s.upsert_principal("loop:good", PrincipalUpsert::default())
            .unwrap();
        // Corrupt one row the way a hand-edited or truncated coord.db could.
        s.lock()
            .execute(
                "UPDATE principals SET started_at = 'not-a-timestamp', pids = '[broken'
                 WHERE principal_id = 'loop:bad'",
                [],
            )
            .unwrap();
        assert!(
            s.get_principal("loop:bad").is_err(),
            "corrupt row must be an error"
        );
        assert!(
            s.list_principals(true).is_err(),
            "listing a corrupt row must be an error, not a silent drop"
        );
        // The lock is not poisoned: unrelated reads and writes still work.
        assert!(s.get_principal("loop:good").unwrap().is_some());
        s.upsert_principal("loop:after", PrincipalUpsert::default())
            .unwrap();
        assert!(s.get_principal("loop:after").unwrap().is_some());
    }

    #[test]
    fn poisoned_lock_is_recovered() {
        let s = std::sync::Arc::new(store());
        let s2 = s.clone();
        let _ = std::thread::spawn(move || {
            let _g = s2.lock();
            panic!("simulated panic while holding the coord lock");
        })
        .join();
        assert!(s.conn.is_poisoned());
        s.upsert_principal("loop:x", PrincipalUpsert::default())
            .unwrap();
        assert!(s.get_principal("loop:x").unwrap().is_some());
    }

    // ────────────── footprint tests (Concord P1) ──────────────

    fn upsert_with_labels(s: &CoordStore, id: &str, parent: Option<&str>) {
        let mut upsert = PrincipalUpsert::default();
        upsert.harness = Some("claude-code".into());
        if let Some(p) = parent {
            upsert.labels = Some(json!({ "parent": p }));
        } else {
            upsert.labels = Some(json!({}));
        }
        s.upsert_principal(id, upsert).unwrap();
    }

    #[test]
    fn record_footprint_returns_seq_and_rejects_bad_op() {
        let s = store();
        let seq = s
            .record_footprint("loop:a", "w1", "read", "/x.rs", None, None)
            .unwrap();
        assert!(seq > 0);
        let got = s.get_footprint(seq).unwrap().expect("present");
        assert_eq!(got.principal_id, "loop:a");
        assert_eq!(got.worker_id, "w1");
        assert_eq!(got.op, "read");
        assert_eq!(got.path, "/x.rs");
        assert!(got.mtime_ns.is_none());
        assert!(got.size.is_none());
        // Bad op → InvalidBody (no row written).
        let err = s
            .record_footprint("loop:a", "w1", "delete", "/x.rs", None, None)
            .unwrap_err();
        assert!(matches!(err, CoordStoreError::InvalidBody(_)));
    }

    #[test]
    fn last_footprint_seq_returns_max_across_ops() {
        let s = store();
        let r1 = s
            .record_footprint("loop:a", "w1", "read", "/x.rs", None, None)
            .unwrap();
        let w1 = s
            .record_footprint("loop:a", "w1", "write", "/x.rs", Some(1), Some(2))
            .unwrap();
        let r2 = s
            .record_footprint("loop:a", "w1", "read", "/x.rs", None, None)
            .unwrap();
        assert_eq!(s.last_footprint_seq("loop:a", "/x.rs").unwrap(), Some(r2));
        assert!(r2 > w1 && w1 > r1);
        // Other path → None.
        assert!(s.last_footprint_seq("loop:a", "/y.rs").unwrap().is_none());
    }

    #[test]
    fn latest_footprint_returns_newest_across_principals_on_path() {
        let s = store();
        upsert_with_labels(&s, "loop:a", None);
        upsert_with_labels(&s, "loop:b", None);
        // loop:a read on P, loop:b write on P (distinct mtime/size),
        // and a write on Q — Q must NOT bleed into P's result.
        let _r = s
            .record_footprint("loop:a", "w1", "read", "/P", Some(100), Some(10))
            .unwrap();
        let w = s
            .record_footprint("loop:b", "w2", "write", "/P", Some(200), Some(20))
            .unwrap();
        let _q = s
            .record_footprint("loop:a", "w1", "write", "/Q", Some(300), Some(30))
            .unwrap();
        let got = s.latest_footprint("/P").unwrap().expect("present");
        assert_eq!(got.seq, w);
        assert_eq!(got.principal_id, "loop:b");
        assert_eq!(got.op, "write");
        assert_eq!(got.mtime_ns, Some(200));
        assert_eq!(got.size, Some(20));
        assert_eq!(got.path, "/P");
        // Unknown path → None.
        assert!(s.latest_footprint("/nope").unwrap().is_none());
    }

    #[test]
    fn lineage_walks_up_and_down_with_three_hop_cap() {
        let s = store();
        // Chain: a ← b ← c ← d (a is root, d is leaf-3).
        upsert_with_labels(&s, "loop:a", None);
        upsert_with_labels(&s, "loop:b", Some("loop:a"));
        upsert_with_labels(&s, "loop:c", Some("loop:b"));
        upsert_with_labels(&s, "loop:d", Some("loop:c"));
        // Self + 3 hops up = {a, b, c, d}.
        let lin = s.lineage("loop:d", LINEAGE_MAX_HOPS).unwrap();
        assert!(lin.contains("loop:d"));
        assert!(lin.contains("loop:c"));
        assert!(lin.contains("loop:b"));
        assert!(lin.contains("loop:a"));
        // 4th ancestor is past the cap.
        upsert_with_labels(&s, "loop:e", Some("loop:d"));
        let lin = s.lineage("loop:e", LINEAGE_MAX_HOPS).unwrap();
        assert!(lin.contains("loop:e"));
        assert!(lin.contains("loop:d"));
        assert!(lin.contains("loop:c"));
        assert!(lin.contains("loop:b"));
        assert!(!lin.contains("loop:a"), "4th ancestor must be excluded");
    }

    #[test]
    fn lineage_walks_downward_too() {
        let s = store();
        upsert_with_labels(&s, "loop:a", None);
        upsert_with_labels(&s, "loop:b", Some("loop:a"));
        upsert_with_labels(&s, "loop:c", Some("loop:b"));
        let lin = s.lineage("loop:a", LINEAGE_MAX_HOPS).unwrap();
        assert!(lin.contains("loop:a"));
        assert!(lin.contains("loop:b"));
        assert!(lin.contains("loop:c"));
    }

    #[test]
    fn lineage_handles_cycle_without_infinite_loop() {
        let s = store();
        // a → b → a (cycle). Visited set must break the loop.
        upsert_with_labels(&s, "loop:a", Some("loop:b"));
        upsert_with_labels(&s, "loop:b", Some("loop:a"));
        let lin = s.lineage("loop:a", LINEAGE_MAX_HOPS).unwrap();
        assert!(lin.contains("loop:a"));
        assert!(lin.contains("loop:b"));
        // Visits terminate — if the BFS looped, this test would never
        // return. A runtime cap on BFS step count would also be
        // defensible; for now correctness is what we assert.
    }

    #[test]
    fn writes_after_excludes_lineage_and_dedupes() {
        let s = store();
        upsert_with_labels(&s, "loop:a", None);
        upsert_with_labels(&s, "loop:b", None);
        upsert_with_labels(&s, "loop:c", None);
        // After the user "loop:a" read the file, "loop:b" and "loop:c"
        // each wrote twice. The query must:
        //   - exclude the caller's lineage (here, {a, b}? — only "a"
        //     is the caller's lineage; "b" is NOT, so it appears),
        //   - dedupe to one row per principal (the newest seq/ts),
        //   - order newest first.
        let after = 0;
        let _r1 = s
            .record_footprint("loop:a", "w0", "read", "/f.rs", None, None)
            .unwrap();
        let _w1 = s
            .record_footprint("loop:b", "w1", "write", "/f.rs", Some(1), Some(10))
            .unwrap();
        let _w2 = s
            .record_footprint("loop:b", "w1", "write", "/f.rs", Some(2), Some(20))
            .unwrap();
        let _w3 = s
            .record_footprint("loop:c", "w2", "write", "/f.rs", Some(3), Some(30))
            .unwrap();
        let _w4 = s
            .record_footprint("loop:c", "w2", "write", "/f.rs", Some(4), Some(40))
            .unwrap();
        // Exclude only self (loop:a).
        let mut excl = std::collections::HashSet::new();
        excl.insert("loop:a".to_string());
        let out = s.writes_after("/f.rs", after, &excl, 3).unwrap();
        assert_eq!(out.len(), 2, "two distinct writers, deduped");
        // Newest seq first.
        assert_eq!(out[0].principal_id, "loop:c");
        assert_eq!(out[0].seq, _w4);
        assert_eq!(out[1].principal_id, "loop:b");
        assert_eq!(out[1].seq, _w2);
        // Harness + cwd ride along via LEFT JOIN.
        assert_eq!(out[0].harness.as_deref(), Some("claude-code"));
    }

    #[test]
    fn writes_after_empty_exclude_returns_all_writers() {
        let s = store();
        upsert_with_labels(&s, "loop:a", None);
        upsert_with_labels(&s, "loop:b", None);
        s.record_footprint("loop:b", "w1", "write", "/f.rs", None, None)
            .unwrap();
        let excl = std::collections::HashSet::new();
        let out = s.writes_after("/f.rs", 0, &excl, 10).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].principal_id, "loop:b");
    }

    // ────────────── hot-claim tests (Concord P2) ──────────────

    #[test]
    fn hot_claim_take_renew_lineage_contend() {
        let s = store();
        upsert_with_labels(&s, "loop:a", None);
        upsert_with_labels(&s, "run:x", Some("loop:a"));
        upsert_with_labels(&s, "loop:b", None);

        // Take on first write.
        assert_eq!(
            s.claim_hot("/hot/x.yaml", "loop:a", 1, 60).unwrap(),
            HotClaimOutcome::Taken
        );
        // Renew by holder.
        assert_eq!(
            s.claim_hot("/hot/x.yaml", "loop:a", 2, 60).unwrap(),
            HotClaimOutcome::Renewed
        );
        // Renew by a lineage child (labels.parent → loop:a). Holder
        // stays loop:a.
        assert_eq!(
            s.claim_hot("/hot/x.yaml", "run:x", 3, 60).unwrap(),
            HotClaimOutcome::Renewed
        );
        let claim = s.live_hot_claim("/hot/x.yaml").unwrap().expect("live");
        assert_eq!(claim.principal_id, "loop:a");
        assert_eq!(claim.last_write_seq, 3);
        // Contend by an outsider keeps the holder and last_write_seq.
        assert_eq!(
            s.claim_hot("/hot/x.yaml", "loop:b", 4, 60).unwrap(),
            HotClaimOutcome::Contended {
                holder: "loop:a".to_string()
            }
        );
        let claim = s.live_hot_claim("/hot/x.yaml").unwrap().expect("live");
        assert_eq!(claim.principal_id, "loop:a");
        assert_eq!(claim.last_write_seq, 3);
    }

    #[test]
    fn hot_claim_ttl_expiry_lets_outsider_take() {
        let s = store();
        upsert_with_labels(&s, "loop:a", None);
        upsert_with_labels(&s, "loop:b", None);
        // Negative ttl → expires_at is already in the past.
        assert_eq!(
            s.claim_hot("/hot/y.yaml", "loop:a", 5, -1).unwrap(),
            HotClaimOutcome::Taken
        );
        // Expired claim is not "live".
        assert!(s.live_hot_claim("/hot/y.yaml").unwrap().is_none());
        // Outsider takes the expired claim.
        assert_eq!(
            s.claim_hot("/hot/y.yaml", "loop:b", 6, 60).unwrap(),
            HotClaimOutcome::Taken
        );
        let claim = s.live_hot_claim("/hot/y.yaml").unwrap().expect("live");
        assert_eq!(claim.principal_id, "loop:b");
    }

    #[test]
    fn list_live_hot_claims_sweeps_expired_rows() {
        let s = store();
        upsert_with_labels(&s, "loop:a", None);
        upsert_with_labels(&s, "loop:b", None);
        // Path A is expired on arrival; path B is live.
        s.claim_hot("/hot/a.yaml", "loop:a", 1, -1).unwrap();
        s.claim_hot("/hot/b.yaml", "loop:b", 2, 600).unwrap();
        let list = s.list_live_hot_claims().unwrap();
        assert_eq!(list.len(), 1, "sweep must drop the expired row");
        assert_eq!(list[0].path, "/hot/b.yaml");
        assert_eq!(list[0].principal_id, "loop:b");
        // The sweep deleted path A from the table.
        assert!(s.live_hot_claim("/hot/a.yaml").unwrap().is_none());
    }
}
