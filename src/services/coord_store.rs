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

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
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
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// In-memory store for tests. Same schema, no on-disk artifact.
    pub fn open_in_memory() -> CoordStoreResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
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
}
