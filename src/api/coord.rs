//! Cross-fleet agent coordination — advisory lease plane (Phase 1 MVP).
//!
//! Multiple master Claude sessions spawn mini-ork teams of heterogeneous
//! agents (Claude, Codex) that edit the same codebase. Without a shared
//! arbiter two agents can read-then-write the same file and clobber each
//! other (lost update). `arXiv:2606.07845` shows LLMs cannot reliably
//! self-negotiate shared-resource access and RL doesn't fix it — so the
//! arbiter must be external and authoritative. ContextNest is the one
//! substrate every agent already shares, so it hosts a central lock
//! manager (the Chubby/ZooKeeper role) without the distributed-consensus
//! tax of Ricart–Agrawala / Maekawa.
//!
//! This module is the registry: scoped, priority-aware, TTL'd **leases**
//! (Gray & Cheriton 1989 — a crashed holder's lease expires rather than
//! deadlocking the fleet). Conflict = path-set overlap AND ≥1 writer
//! (readers-writers, Courtois 1971).
//!
//! Phase 2 adds two fairness mechanisms on top of the Phase-1 registry,
//! both implemented purely over the `Vec<Lease>` so they stay unit-testable
//! without booting services:
//!
//! - **Priority inheritance** (Sha/Rajkumar/Lehoczky 1990). While a
//!   high-priority agent waits on a lower-priority holder, the holder's
//!   *effective* priority is raised to the waiter's so a medium-priority
//!   third agent can't jump the queue and starve the high-priority waiter.
//!   We never store the inherited value on the holder lease — it is derived
//!   on read from the recorded waiters (see `effective_priority`) and surfaced
//!   as `effective_priority` in `GET /coord/leases`.
//! - **Deadlock detection via wound-wait** (Rosenkrantz 1978). Queued
//!   requests are recorded as waiter rows (a `Lease` whose `lease_id` carries
//!   the `waiter:` prefix). A wait-for graph is derived from waiters → held
//!   leases; if a new `queued` decision would close a cycle, the
//!   lower-priority requester in the cycle loses and its acquire returns
//!   `{ status: "abort", reason: "deadlock" }`. Wound-wait only ever cancels
//!   a *requester's wait*; an established holder is never preempted.
//!
//! Deny-mode enforcement and persistence are later phases (see
//! `docs/roadmap/epics/agent-coordination.md`).

use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::{delete, get, post, put},
    Router,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::services::ContextNestServices;

/// Default lease TTL when the caller omits `ttl_secs`. Long enough for a
/// multi-file edit, short enough that an abandoned lease self-heals fast.
const DEFAULT_TTL_SECS: u64 = 120;

/// Upper bound on a single lease's TTL. Caps how long a crashed holder can
/// stall the fleet before lazy expiry frees its scope.
const MAX_TTL_SECS: u64 = 3600;

/// Cap on the in-memory contention audit log. Oldest entries are evicted
/// (FIFO) when the ring overflows so memory stays bounded for long-lived
/// servers. 512 is enough headroom for a busy agent fleet's worth of
/// recent contention without forcing the dashboard panel to scan more
/// than the tail.
pub const AUDIT_RING_CAP: usize = 512;

/// Access intent for a lease. `read/read` is compatible; any `write`
/// against an overlapping scope conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeaseMode {
    Read,
    Write,
}

/// A granted lease — one agent's claim on a set of paths for a bounded
/// window. `expires_at` is the self-healing property: a crashed agent's
/// lease is swept the next time the registry is read.
#[derive(Debug, Clone, Serialize)]
pub struct Lease {
    pub lease_id: String,
    pub agent_id: String,
    pub fleet_id: Option<String>,
    pub paths: Vec<String>,
    pub mode: LeaseMode,
    /// Higher integer = higher priority. Only orders the queue in Phase 1
    /// (non-preemptive); a held lease is never interrupted.
    pub priority: i64,
    pub reason: Option<String>,
    /// Opt-in strict mode: while held, another agent's PreToolUse on an
    /// overlapping path is hard-blocked (`permissionDecision: "deny"`)
    /// instead of warned. Default false keeps Phase 1 warn-only.
    pub strict: bool,
    pub granted_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// `POST /api/v1/coord/lease` body.
#[derive(Debug, Clone, Deserialize)]
pub struct AcquireRequest {
    pub agent_id: String,
    #[serde(default)]
    pub fleet_id: Option<String>,
    pub paths: Vec<String>,
    pub mode: LeaseMode,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub ttl_secs: Option<u64>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub strict: bool,
}

/// A lease standing between the requester and its scope.
#[derive(Debug, Clone, Serialize)]
pub struct Blocker {
    pub lease_id: String,
    pub agent_id: String,
    pub priority: i64,
    pub reason: Option<String>,
}

/// `POST /api/v1/coord/lease` response — granted immediately, or queued
/// behind the conflicting holders with a suggested re-poll time.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum AcquireResponse {
    Granted {
        lease_id: String,
        expires_at: DateTime<Utc>,
    },
    Queued {
        blocked_by: Vec<Blocker>,
        /// Min TTL-remaining among blockers — the "query at a proper time"
        /// hint. The waiter sleeps roughly this long (e.g. via
        /// `ScheduleWakeup`) then re-requests, instead of busy-spinning.
        retry_after_secs: i64,
        /// How far back in line the requester is: conflicting holders whose
        /// **effective** priority is at or above the requester, plus other
        /// already-waiting agents that outrank it (higher priority, or equal
        /// priority recorded earlier). Priority inheritance feeds the holder
        /// term; the waiter term is what keeps a medium-priority agent behind
        /// a high-priority waiter.
        position: usize,
    },
    /// Wound-wait deadlock break (Phase 2). Returned when granting/queueing
    /// this request would close a wait-for cycle and the requester is the
    /// lower-priority party in that cycle. The requester must abandon this
    /// acquire (and typically retry later); the higher-priority party keeps
    /// its place. After an abort no wait-for cycle remains.
    Abort { reason: String },
}

/// Phase 4 observability counters — snapshotted by
/// `GET /api/v1/coord/metrics`. All fields are in-memory and ephemeral;
/// restart resets them to zero, matching the lease registry itself.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CoordMetrics {
    /// Current count of non-expired leases in the registry (gauge).
    pub coord_leases_held: usize,
    /// Peak `position` observed across all `queued` responses since
    /// process start — the worst-case "how many higher-or-equal-priority
    /// blockers ahead of any waiter" the fleet has seen. Coarse proxy for
    /// fleet-wide contention since Phase 1 doesn't keep a persistent
    /// waiter list.
    pub coord_queue_depth: usize,
    /// Cumulative count of leases broken by deadlock-cycle detection.
    /// Always `0` in Phase 1 — cycle detection is Phase 2. Field exists so
    /// the dashboard wiring doesn't need a schema break later.
    pub coord_deadlocks_broken: u64,
    /// Cumulative count of leases swept out by the lazy TTL sweeper.
    /// Counts every lease whose `expires_at <= now` was found at sweep
    /// time, regardless of which handler triggered the sweep.
    pub coord_ttl_expirations: u64,
    /// Cumulative count of `granted` responses (successful acquisitions).
    pub coord_leases_granted: u64,
    /// Cumulative count of `queued` responses (conflicts at acquire time).
    pub coord_queued_total: u64,
}

/// Outcome label for a row in the contention audit log. Serialised as the
/// snake_case form requested by the Phase 4 spec:
/// `granted|queued|released|ttl_expired|deadlock_abort`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Granted,
    Queued,
    Released,
    TtlExpired,
    DeadlockAbort,
}

/// One row in the bounded contention audit log. Shape mirrors the spec:
/// `ts` always set; `waiter`/`holder` are agent ids (one or both may be
/// `None` depending on outcome); `scope` is the contended path(s);
/// `waited_secs` is only meaningful for `queued` events.
#[derive(Debug, Clone, Serialize)]
pub struct AuditRecord {
    pub ts: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,
    pub scope: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waited_secs: Option<f64>,
    pub outcome: AuditOutcome,
}

/// Two scopes overlap if one is a path-prefix of the other at a segment
/// boundary, or they're equal. MVP granularity is whole files / dir
/// prefixes; symbol/glob scopes are Phase 5.
fn path_overlap(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    long.starts_with(short)
        && (short.ends_with('/') || long.as_bytes().get(short.len()) == Some(&b'/'))
}

/// Any path in `a` overlaps any path in `b`.
fn sets_overlap(a: &[String], b: &[String]) -> bool {
    a.iter().any(|pa| b.iter().any(|pb| path_overlap(pa, pb)))
}

/// A held lease conflicts with an incoming request iff their scopes
/// overlap **and** at least one side wants to write. read/read coexists.
fn conflicts(held: &Lease, req_paths: &[String], req_mode: LeaseMode) -> bool {
    let write_involved = held.mode == LeaseMode::Write || req_mode == LeaseMode::Write;
    write_involved && sets_overlap(&held.paths, req_paths)
}

/// Lazy TTL — drop every lease whose window has closed and return the
/// swept leases so the caller can append `ttl_expired` rows to the audit
/// log + bump `coord_ttl_expirations`. Called at the top of every
/// registry read so an abandoned lease never blocks past its TTL, with
/// no background sweeper needed for the MVP.
fn sweep_expired(leases: &mut Vec<Lease>, now: DateTime<Utc>) -> Vec<Lease> {
    let mut kept = Vec::with_capacity(leases.len());
    let mut expired = Vec::new();
    for l in leases.drain(..) {
        if l.expires_at > now {
            kept.push(l);
        } else {
            expired.push(l);
        }
    }
    *leases = kept;
    expired
}

/// Push a record onto the audit ring buffer. Drops the oldest entry when
/// at capacity so memory stays bounded.
fn push_audit(audit: &mut VecDeque<AuditRecord>, rec: AuditRecord) {
    if audit.len() >= AUDIT_RING_CAP {
        audit.pop_front();
    }
    audit.push_back(rec);
}

/// Sweep expired leases and append one `ttl_expired` audit row per swept
/// lease, bumping `coord_ttl_expirations` accordingly. Pure helper —
/// operates on the mutable refs the caller already holds under their
/// respective `RwLock` guards.
fn sweep_and_audit(
    leases: &mut Vec<Lease>,
    audit: &mut VecDeque<AuditRecord>,
    metrics: &mut CoordMetrics,
    now: DateTime<Utc>,
) -> Vec<Lease> {
    let swept = sweep_expired(leases, now);
    if swept.is_empty() {
        return swept;
    }
    metrics.coord_ttl_expirations = metrics
        .coord_ttl_expirations
        .saturating_add(swept.len() as u64);
    metrics.coord_leases_held = leases.len();
    for l in &swept {
        push_audit(
            audit,
            AuditRecord {
                ts: now,
                waiter: None,
                holder: Some(l.agent_id.clone()),
                scope: l.paths.clone(),
                waited_secs: None,
                outcome: AuditOutcome::TtlExpired,
            },
        );
    }
    swept
}

/// Parse a `since` lookback string of the form `<n><suffix?>` where
/// suffix is one of `s`/`m`/`h`/`d` (defaulting to seconds). Returns
/// `None` when the input is missing or unparseable — the caller treats
/// `None` as "no filter, return everything". Lenient on purpose so a
/// typo on the dashboard URL doesn't silently 4xx.
pub(crate) fn parse_since(s: Option<&str>) -> Option<i64> {
    let s = s?.trim();
    if s.is_empty() {
        return None;
    }
    let (num_part, suffix) = match s.find(|c: char| !c.is_ascii_digit()) {
        Some(idx) => (&s[..idx], s[idx..].trim()),
        None => (s, ""),
    };
    let n: i64 = num_part.parse().ok()?;
    let mult: i64 = match suffix {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return None,
    };
    Some(n.saturating_mul(mult))
}

// ── Phase 2: waiter records, priority inheritance, wound-wait deadlock ──────
//
// Phase 2 needs state that outlives a single HTTP call: a queued requester's
// priority (so a holder can inherit it) and the "who waits for whom" edges (so
// a cycle is detectable on the *next* request). The registry's only storage is
// the `Vec<Lease>`, so a queued request is recorded as a *waiter row* — a
// normal `Lease` whose `lease_id` carries this prefix. Waiter rows never count
// as held leases (they don't block, don't appear in `GET /coord/leases`, and
// don't drive the advisory); they only feed inheritance and cycle detection,
// and they self-heal via the same lazy TTL sweep as held leases.

/// Marks a `Lease` row as a queued *waiter*, not a held lease. Keeping waiters
/// in the same `Vec` (rather than a second field on the services struct) lets
/// every Phase-2 rule stay a pure function over `&mut Vec<Lease>`.
const WAITER_PREFIX: &str = "waiter:";

fn is_waiter(l: &Lease) -> bool {
    l.lease_id.starts_with(WAITER_PREFIX)
}

fn is_held(l: &Lease) -> bool {
    !is_waiter(l)
}

/// A held lease's *effective* priority = its own priority, raised to the
/// highest priority among waiters currently blocked on it (priority
/// inheritance). Derived on read so it's always consistent with the live
/// waiter set — there is no stored, drift-prone copy.
fn effective_priority(leases: &[Lease], held: &Lease) -> i64 {
    let mut eff = held.priority;
    for w in leases.iter().filter(|w| is_waiter(w)) {
        if w.agent_id != held.agent_id && conflicts(held, &w.paths, w.mode) && w.priority > eff {
            eff = w.priority;
        }
    }
    eff
}

/// Wait-for graph: an edge `waiter_agent → holder_agent` for every held lease
/// a waiter is blocked by. Agents are the nodes; a cycle here is a deadlock.
fn waiter_adjacency(leases: &[Lease]) -> HashMap<String, HashSet<String>> {
    let mut adj: HashMap<String, HashSet<String>> = HashMap::new();
    for w in leases.iter().filter(|w| is_waiter(w)) {
        let edges = adj.entry(w.agent_id.clone()).or_default();
        for h in leases.iter().filter(|h| is_held(h)) {
            if h.agent_id != w.agent_id && conflicts(h, &w.paths, w.mode) {
                edges.insert(h.agent_id.clone());
            }
        }
    }
    adj
}

/// DFS for a path `start → … → target` in the wait-for graph. Returns the node
/// sequence (inclusive) or `None` if `target` is unreachable from `start`.
fn find_path(
    adj: &HashMap<String, HashSet<String>>,
    start: &str,
    target: &str,
) -> Option<Vec<String>> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut parent: HashMap<String, String> = HashMap::new();
    let mut stack = vec![start.to_string()];
    visited.insert(start.to_string());
    while let Some(node) = stack.pop() {
        if node == target {
            let mut path = vec![node.clone()];
            let mut cur = node;
            while let Some(p) = parent.get(&cur) {
                path.push(p.clone());
                cur = p.clone();
            }
            path.reverse();
            return Some(path);
        }
        if let Some(neigh) = adj.get(&node) {
            for n in neigh {
                if visited.insert(n.clone()) {
                    parent.insert(n.clone(), node.clone());
                    stack.push(n.clone());
                }
            }
        }
    }
    None
}

/// If queueing `req` (which would wait on the held leases it conflicts with)
/// closes a wait-for cycle, return the agents on that cycle (including
/// `req.agent_id`). The proposed edges are `req.agent → each conflicting
/// holder`; a cycle exists iff one of those holders can already reach
/// `req.agent` through the existing waiter graph.
fn find_deadlock_cycle(leases: &[Lease], req: &AcquireRequest) -> Option<Vec<String>> {
    let adj = waiter_adjacency(leases);
    for h in leases.iter().filter(|h| is_held(h)) {
        if h.agent_id != req.agent_id && conflicts(h, &req.paths, req.mode) {
            if let Some(path) = find_path(&adj, &h.agent_id, &req.agent_id) {
                return Some(path);
            }
        }
    }
    None
}

/// The cycle member that loses the wound-wait break: the lowest-priority
/// requester, ties broken deterministically by the lexicographically greater
/// `agent_id`. `req`'s priority is taken from the request itself (it isn't a
/// waiter row yet); every other member's from its recorded waiter row.
fn pick_loser(leases: &[Lease], members: &[String], req: &AcquireRequest) -> String {
    let prio_of = |agent: &str| -> i64 {
        if agent == req.agent_id {
            req.priority
        } else {
            leases
                .iter()
                .filter(|w| is_waiter(w) && w.agent_id == agent)
                .map(|w| w.priority)
                .max()
                .unwrap_or(i64::MAX)
        }
    };
    let mut best: Option<(i64, &String)> = None;
    for m in members {
        let p = prio_of(m);
        let replace = match best {
            None => true,
            // Lower priority loses; ties broken by the greater agent_id so the
            // choice is independent of which agent happened to call.
            Some((bp, bm)) => p < bp || (p == bp && m > bm),
        };
        if replace {
            best = Some((p, m));
        }
    }
    best.map(|(_, m)| m.clone()).unwrap_or_default()
}

/// Drop a queued waiter's record entirely (all of that agent's waiter rows).
/// Used to break a cycle: the loser's *wait* is cancelled — never its held
/// lease, so wound-wait stays non-preemptive for established holders.
fn cancel_waiter(leases: &mut Vec<Lease>, agent: &str) {
    leases.retain(|l| !(is_waiter(l) && l.agent_id == agent));
}

/// Record (or refresh) `req` as a waiter on its scope. Re-polls keep their
/// original `granted_at` so FIFO position among equal-priority waiters is
/// preserved; only priority/scope/TTL are updated.
fn upsert_waiter(leases: &mut Vec<Lease>, req: &AcquireRequest, now: DateTime<Utc>, ttl: u64) {
    let expires_at = now + Duration::seconds(ttl as i64);
    if let Some(w) = leases
        .iter_mut()
        .find(|w| is_waiter(w) && w.agent_id == req.agent_id && sets_overlap(&w.paths, &req.paths))
    {
        w.priority = req.priority;
        w.paths = req.paths.clone();
        w.mode = req.mode;
        w.reason = req.reason.clone();
        w.expires_at = expires_at;
    } else {
        leases.push(Lease {
            lease_id: format!("{WAITER_PREFIX}{}", uuid::Uuid::new_v4()),
            agent_id: req.agent_id.clone(),
            fleet_id: req.fleet_id.clone(),
            paths: req.paths.clone(),
            mode: req.mode,
            priority: req.priority,
            reason: req.reason.clone(),
            strict: req.strict,
            granted_at: now,
            expires_at,
        });
    }
}

/// Drop any waiter rows the granted agent had on this scope — once it holds the
/// lease it is no longer waiting, so its stale wait edges must disappear.
fn clear_waiters_for_scope(leases: &mut Vec<Lease>, agent: &str, paths: &[String]) {
    leases.retain(|l| !(is_waiter(l) && l.agent_id == agent && sets_overlap(&l.paths, paths)));
}

/// Pure grant/queue decision over a lease set — the functional core. The
/// async handler is a thin lock-acquire wrapper around this so the
/// conflict/priority/TTL rules are testable without booting services.
///
/// Caller is responsible for sweeping expired leases first — `decide`
/// operates on a clean registry so the audit log can capture exactly
/// which leases the sweeper evicted.
///
/// A request from the **same** agent never blocks itself (re-entrant /
/// re-acquire is allowed). Grant is non-preemptive: a held conflicting
/// lease always wins regardless of the requester's priority; priority only
/// orders the returned queue.
///
/// Phase 2 layers two rules onto the queue branch. Before recording the
/// requester as a waiter it runs wound-wait deadlock detection: if queueing
/// would close a wait-for cycle, the lowest-priority agent in the cycle loses
/// — if that's the requester it `Abort`s here, otherwise the loser's wait is
/// cancelled (its held lease untouched) and the requester proceeds to queue.
/// The recorded waiter then powers priority inheritance, which is computed on
/// read in `effective_priority` rather than stored on the holder.
fn decide(leases: &mut Vec<Lease>, req: AcquireRequest, now: DateTime<Utc>) -> AcquireResponse {
    let ttl = req
        .ttl_secs
        .unwrap_or(DEFAULT_TTL_SECS)
        .clamp(1, MAX_TTL_SECS);

    // Only *held* leases block; waiter rows are bookkeeping, never holders.
    let conflicting: Vec<Lease> = leases
        .iter()
        .filter(|l| is_held(l) && l.agent_id != req.agent_id && conflicts(l, &req.paths, req.mode))
        .cloned()
        .collect();

    if conflicting.is_empty() {
        clear_waiters_for_scope(leases, &req.agent_id, &req.paths);
        let lease = Lease {
            lease_id: uuid::Uuid::new_v4().to_string(),
            agent_id: req.agent_id,
            fleet_id: req.fleet_id,
            paths: req.paths,
            mode: req.mode,
            priority: req.priority,
            reason: req.reason,
            strict: req.strict,
            granted_at: now,
            expires_at: now + Duration::seconds(ttl as i64),
        };
        let resp = AcquireResponse::Granted {
            lease_id: lease.lease_id.clone(),
            expires_at: lease.expires_at,
        };
        leases.push(lease);
        return resp;
    }

    // Wound-wait: break any wait-for cycle this request would close. Loop so a
    // chain of breaks settles before we commit the requester to the queue;
    // each pass drops ≥1 waiter, so it terminates.
    while let Some(cycle) = find_deadlock_cycle(leases, &req) {
        let loser = pick_loser(leases, &cycle, &req);
        if loser == req.agent_id {
            return AcquireResponse::Abort {
                reason: "deadlock".to_string(),
            };
        }
        cancel_waiter(leases, &loser);
    }

    // Commit the requester as a waiter so it can inherit priority to its
    // holders and be visible to future cycle checks.
    upsert_waiter(leases, &req, now, ttl);

    let retry_after_secs = conflicting
        .iter()
        .map(|l| (l.expires_at - now).num_seconds().max(0))
        .min()
        .unwrap_or(0);
    // Conflicting holders that (with inheritance) outrank-or-tie the requester…
    let holders_ahead = conflicting
        .iter()
        .filter(|l| effective_priority(leases, l) >= req.priority)
        .count();
    // …plus other waiters that legitimately sit ahead in line.
    let waiters_ahead = leases
        .iter()
        .filter(|w| {
            is_waiter(w)
                && w.agent_id != req.agent_id
                && conflicts(w, &req.paths, req.mode)
                && (w.priority > req.priority || (w.priority == req.priority && w.granted_at < now))
        })
        .count();
    let position = holders_ahead + waiters_ahead;
    let blocked_by = conflicting
        .iter()
        .map(|l| Blocker {
            lease_id: l.lease_id.clone(),
            agent_id: l.agent_id.clone(),
            priority: l.priority,
            reason: l.reason.clone(),
        })
        .collect();
    AcquireResponse::Queued {
        blocked_by,
        retry_after_secs,
        position,
    }
}

/// POST /api/v1/coord/lease — acquire (or queue for) a scoped lease.
async fn acquire(
    State(services): State<ContextNestServices>,
    Json(req): Json<AcquireRequest>,
) -> Json<AcquireResponse> {
    let now = Utc::now();
    let mut leases = services.coord_leases.write().await;
    let mut metrics = services.coord_metrics.write().await;
    let mut audit = services.coord_audit.write().await;
    let _swept = sweep_and_audit(&mut leases, &mut audit, &mut metrics, now);
    let resp = decide(&mut leases, req.clone(), now);
    match &resp {
        AcquireResponse::Granted { .. } => {
            metrics.coord_leases_granted = metrics.coord_leases_granted.saturating_add(1);
            metrics.coord_leases_held = leases.len();
            push_audit(
                &mut audit,
                AuditRecord {
                    ts: now,
                    waiter: Some(req.agent_id.clone()),
                    holder: None,
                    scope: req.paths.clone(),
                    waited_secs: None,
                    outcome: AuditOutcome::Granted,
                },
            );
        }
        AcquireResponse::Queued {
            blocked_by,
            position,
            ..
        } => {
            metrics.coord_queued_total = metrics.coord_queued_total.saturating_add(1);
            if *position > metrics.coord_queue_depth {
                metrics.coord_queue_depth = *position;
            }
            push_audit(
                &mut audit,
                AuditRecord {
                    ts: now,
                    waiter: Some(req.agent_id.clone()),
                    holder: blocked_by.first().map(|b| b.agent_id.clone()),
                    scope: req.paths.clone(),
                    waited_secs: Some(0.0),
                    outcome: AuditOutcome::Queued,
                },
            );
        }
        AcquireResponse::Abort { .. } => {
            metrics.coord_deadlocks_broken = metrics.coord_deadlocks_broken.saturating_add(1);
            push_audit(
                &mut audit,
                AuditRecord {
                    ts: now,
                    waiter: Some(req.agent_id.clone()),
                    holder: None,
                    scope: req.paths.clone(),
                    waited_secs: None,
                    outcome: AuditOutcome::DeadlockAbort,
                },
            );
        }
    }
    Json(resp)
}

#[derive(Debug, Serialize)]
struct ReleaseResponse {
    status: &'static str,
    released: bool,
}

/// DELETE /api/v1/coord/lease/{id} — release a held lease, freeing its
/// scope for the next queued requester.
async fn release(
    State(services): State<ContextNestServices>,
    Path(id): Path<String>,
) -> Json<ReleaseResponse> {
    let now = Utc::now();
    let mut leases = services.coord_leases.write().await;
    let removed: Option<Lease> = leases.iter().find(|l| l.lease_id == id).cloned();
    let before = leases.len();
    leases.retain(|l| l.lease_id != id);
    let released = leases.len() < before;
    if released {
        if let Some(l) = removed {
            let mut metrics = services.coord_metrics.write().await;
            let mut audit = services.coord_audit.write().await;
            metrics.coord_leases_held = leases.len();
            push_audit(
                &mut audit,
                AuditRecord {
                    ts: now,
                    waiter: None,
                    holder: Some(l.agent_id),
                    scope: l.paths,
                    waited_secs: None,
                    outcome: AuditOutcome::Released,
                },
            );
        }
    }
    Json(ReleaseResponse {
        status: if released { "released" } else { "not_found" },
        released,
    })
}

#[derive(Debug, Default, Deserialize)]
struct RenewRequest {
    #[serde(default)]
    ttl_secs: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum RenewResponse {
    Renewed {
        lease_id: String,
        expires_at: DateTime<Utc>,
    },
    NotFound,
}

/// PUT /api/v1/coord/lease/{id}/renew — heartbeat. Extends the lease's TTL
/// so a long edit keeps its scope without re-acquiring. Body is optional;
/// omit it to extend by the default TTL.
async fn renew(
    State(services): State<ContextNestServices>,
    Path(id): Path<String>,
    body: Option<Json<RenewRequest>>,
) -> Json<RenewResponse> {
    let now = Utc::now();
    let ttl = body
        .and_then(|b| b.0.ttl_secs)
        .unwrap_or(DEFAULT_TTL_SECS)
        .clamp(1, MAX_TTL_SECS);
    let mut leases = services.coord_leases.write().await;
    let mut metrics = services.coord_metrics.write().await;
    let mut audit = services.coord_audit.write().await;
    let _swept = sweep_and_audit(&mut leases, &mut audit, &mut metrics, now);
    if let Some(lease) = leases.iter_mut().find(|l| l.lease_id == id) {
        lease.expires_at = now + Duration::seconds(ttl as i64);
        return Json(RenewResponse::Renewed {
            lease_id: lease.lease_id.clone(),
            expires_at: lease.expires_at,
        });
    }
    Json(RenewResponse::NotFound)
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    path: Option<String>,
}

/// A held lease as rendered in the listing — all `Lease` fields plus the
/// Phase-2 `effective_priority` (the static `priority` raised by any inherited
/// waiter). The two are distinct: `priority` is the agent's own, while
/// `effective_priority` reflects who is currently blocked behind it.
#[derive(Debug, Serialize)]
struct LeaseView {
    #[serde(flatten)]
    lease: Lease,
    effective_priority: i64,
}

#[derive(Debug, Serialize)]
struct ListResponse {
    count: usize,
    leases: Vec<LeaseView>,
}

/// GET /api/v1/coord/leases[?path=] — inspect live contention. With `path`,
/// returns only held leases whose scope overlaps it. Waiter bookkeeping rows
/// are never listed; each held lease carries its `effective_priority`.
async fn list(
    State(services): State<ContextNestServices>,
    Query(q): Query<ListQuery>,
) -> Json<ListResponse> {
    let now = Utc::now();
    let mut leases = services.coord_leases.write().await;
    let mut metrics = services.coord_metrics.write().await;
    let mut audit = services.coord_audit.write().await;
    let _swept = sweep_and_audit(&mut leases, &mut audit, &mut metrics, now);
    let snapshot = leases.clone();
    let views: Vec<LeaseView> = snapshot
        .iter()
        .filter(|l| is_held(l))
        .filter(|l| match q.path.as_deref() {
            Some(p) => l.paths.iter().any(|lp| path_overlap(lp, p)),
            None => true,
        })
        .map(|l| LeaseView {
            effective_priority: effective_priority(&snapshot, l),
            lease: l.clone(),
        })
        .collect();
    Json(ListResponse {
        count: views.len(),
        leases: views,
    })
}

/// Build the advisory string for the PreToolUse gate, or `None` when no
/// other agent holds a conflicting write lease on `path`. A write lease by
/// another agent blocks both readers and writers of the same scope, so the
/// gate (which fires on Edit/Write) only needs to surface write holders.
fn advisory_for(
    leases: &mut Vec<Lease>,
    audit: &mut VecDeque<AuditRecord>,
    metrics: &mut CoordMetrics,
    agent_id: &str,
    path: &str,
    now: DateTime<Utc>,
) -> Option<String> {
    let _swept = sweep_and_audit(leases, audit, metrics, now);
    build_wait_advisory(leases, agent_id, path, now)
}

/// Pure advisory builder — no sweeping, no audit/metrics side effects. Given
/// an already-swept lease set, returns the WAIT string when another agent
/// holds a conflicting write lease on `path`. Callers that need the audited
/// sweep go through [`advisory_for`]; callers that have already swept (e.g.
/// [`decision_for`]) call this directly to avoid double-sweeping.
fn build_wait_advisory(
    leases: &[Lease],
    agent_id: &str,
    path: &str,
    now: DateTime<Utc>,
) -> Option<String> {
    let blockers: Vec<&Lease> = leases
        .iter()
        .filter(|l| {
            is_held(l)
                && l.agent_id != agent_id
                && l.mode == LeaseMode::Write
                && l.paths.iter().any(|lp| path_overlap(lp, path))
        })
        .collect();
    if blockers.is_empty() {
        return None;
    }
    let mut out = format!(
        "ContextNest coordination: {} active write-lease(s) overlap `{}`, held by another agent. WAIT before editing:",
        blockers.len(),
        path
    );
    for b in &blockers {
        let eta = (b.expires_at - now).num_seconds().max(0);
        let reason = b
            .reason
            .as_ref()
            .map(|r| format!(" — {r}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "\n- agent `{}` (prio {}) holds it ~{}s more{}",
            b.agent_id, b.priority, eta, reason
        ));
    }
    Some(out)
}

/// Decision returned to the PreToolUse gate. A strict lease held by another
/// agent on an overlapping path forces a hard deny; otherwise the gate is
/// allowed, optionally carrying the Phase-1 WAIT advisory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseGateDecision {
    Deny { reason: String },
    Allow { warning: Option<String> },
}

/// Pure PreToolUse gate decision over a lease set. Mirrors `advisory_for`
/// for the warn path, but adds a hard-deny branch for strict leases held by
/// another agent on an overlapping path.
fn decision_for(
    leases: &mut Vec<Lease>,
    agent_id: &str,
    path: &str,
    now: DateTime<Utc>,
) -> LeaseGateDecision {
    sweep_expired(leases, now);
    let blockers: Vec<&Lease> = leases
        .iter()
        .filter(|l| {
            l.agent_id != agent_id
                && l.mode == LeaseMode::Write
                && l.strict
                && l.paths.iter().any(|lp| path_overlap(lp, path))
        })
        .collect();

    if !blockers.is_empty() {
        let holders: Vec<&str> = blockers.iter().map(|l| l.agent_id.as_str()).collect();
        let reason = format!(
            "ContextNest coordination: hard block — {} strict write-lease(s) overlap `{}`, held by agent(s) {}.",
            blockers.len(),
            path,
            holders.join(", ")
        );
        return LeaseGateDecision::Deny { reason };
    }

    match build_wait_advisory(leases, agent_id, path, now) {
        Some(warning) => LeaseGateDecision::Allow {
            warning: Some(warning),
        },
        None => LeaseGateDecision::Allow { warning: None },
    }
}

/// Consulted by the PreToolUse gate. Returns a `WAIT` advisory when
/// another agent holds a conflicting write lease on `path`, else `None`.
pub async fn lease_advisory(
    services: &ContextNestServices,
    agent_id: &str,
    path: &str,
) -> Option<String> {
    let now = Utc::now();
    let mut leases = services.coord_leases.write().await;
    let mut metrics = services.coord_metrics.write().await;
    let mut audit = services.coord_audit.write().await;
    advisory_for(&mut leases, &mut audit, &mut metrics, agent_id, path, now)
}

#[derive(Debug, Default, Deserialize)]
struct AuditQuery {
    /// Lookback window. Accepts `<n>` (seconds) or `<n><s|m|h|d>` —
    /// e.g. `10m`, `1h`, `2d`. Missing or unparseable → return every
    /// record. See [`parse_since`].
    #[serde(default)]
    since: Option<String>,
}

/// `GET /api/v1/coord/metrics` — snapshot of the in-memory counter
/// struct. Cheap (single RwLock read + clone); safe to poll.
async fn metrics_handler(State(services): State<ContextNestServices>) -> Json<CoordMetrics> {
    let metrics = services.coord_metrics.read().await;
    Json(metrics.clone())
}

/// `GET /api/v1/coord/audit[?since=<dur>]` — newest-first slice of the
/// bounded audit ring, optionally filtered to a lookback window. The
/// dashboard panel polls this on the same cadence as `/metrics`.
async fn audit_handler(
    State(services): State<ContextNestServices>,
    Query(q): Query<AuditQuery>,
) -> Json<Vec<AuditRecord>> {
    let audit = services.coord_audit.read().await;
    let cutoff = parse_since(q.since.as_deref()).map(|secs| Utc::now() - Duration::seconds(secs));
    let out: Vec<AuditRecord> = match cutoff {
        Some(c) => audit.iter().rev().filter(|r| r.ts >= c).cloned().collect(),
        None => audit.iter().rev().cloned().collect(),
    };
    Json(out)
}

/// PreToolUse gate decision helper. Returns deny when another agent holds a
/// strict overlapping write lease, otherwise allow with an optional warning.
pub async fn lease_decision(
    services: &ContextNestServices,
    agent_id: &str,
    path: &str,
) -> LeaseGateDecision {
    let now = Utc::now();
    let mut leases = services.coord_leases.write().await;
    decision_for(&mut leases, agent_id, path, now)
}

/// Register the coordination sub-router.
pub fn create_coord_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/api/v1/coord/lease", post(acquire))
        .route("/api/v1/coord/lease/:id", delete(release))
        .route("/api/v1/coord/lease/:id/renew", put(renew))
        .route("/api/v1/coord/leases", get(list))
        .route("/api/v1/coord/metrics", get(metrics_handler))
        .route("/api/v1/coord/audit", get(audit_handler))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(agent: &str, paths: &[&str], mode: LeaseMode, prio: i64) -> AcquireRequest {
        AcquireRequest {
            agent_id: agent.to_string(),
            fleet_id: None,
            paths: paths.iter().map(|s| s.to_string()).collect(),
            mode,
            priority: prio,
            ttl_secs: Some(120),
            reason: Some(format!("{agent} work")),
            strict: false,
        }
    }

    fn strict_req(agent: &str, paths: &[&str], mode: LeaseMode, prio: i64) -> AcquireRequest {
        AcquireRequest {
            strict: true,
            ..req(agent, paths, mode, prio)
        }
    }

    #[test]
    fn path_overlap_equality_prefix_and_boundary() {
        assert!(path_overlap("src/foo.rs", "src/foo.rs"));
        // Directory prefix overlaps a file beneath it.
        assert!(path_overlap("src/", "src/foo.rs"));
        assert!(path_overlap("src", "src/foo.rs"));
        // Prefix that isn't a segment boundary must NOT overlap.
        assert!(!path_overlap("src/foo", "src/foobar.rs"));
        // Disjoint files don't overlap.
        assert!(!path_overlap("src/foo.rs", "src/bar.rs"));
    }

    #[test]
    fn conflicts_only_when_write_involved_and_overlapping() {
        let held_read = Lease {
            lease_id: "x".into(),
            agent_id: "A".into(),
            fleet_id: None,
            paths: vec!["src/foo.rs".into()],
            mode: LeaseMode::Read,
            priority: 1,
            reason: None,
            strict: false,
            granted_at: Utc::now(),
            expires_at: Utc::now() + Duration::seconds(60),
        };
        // read vs read on same path: compatible.
        assert!(!conflicts(
            &held_read,
            &["src/foo.rs".into()],
            LeaseMode::Read
        ));
        // read held vs write request: conflict.
        assert!(conflicts(
            &held_read,
            &["src/foo.rs".into()],
            LeaseMode::Write
        ));
        // write vs write but disjoint paths: no conflict.
        let held_write = Lease {
            mode: LeaseMode::Write,
            ..held_read.clone()
        };
        assert!(!conflicts(
            &held_write,
            &["src/bar.rs".into()],
            LeaseMode::Write
        ));
    }

    #[test]
    fn grant_then_queue_by_overlap() {
        let now = Utc::now();
        let mut leases = Vec::new();

        // A (prio 10) takes foo.rs → granted.
        let a = decide(
            &mut leases,
            req("A", &["src/foo.rs"], LeaseMode::Write, 10),
            now,
        );
        assert!(matches!(a, AcquireResponse::Granted { .. }));

        // B (prio 5) wants foo.rs → queued behind A.
        let b = decide(
            &mut leases,
            req("B", &["src/foo.rs"], LeaseMode::Write, 5),
            now,
        );
        match b {
            AcquireResponse::Queued {
                blocked_by,
                position,
                ..
            } => {
                assert_eq!(blocked_by.len(), 1);
                assert_eq!(blocked_by[0].agent_id, "A");
                // One higher-priority holder ahead of B.
                assert_eq!(position, 1);
            }
            _ => panic!("B should be queued"),
        }

        // C wants a different file → granted, no contention.
        let c = decide(
            &mut leases,
            req("C", &["src/bar.rs"], LeaseMode::Write, 1),
            now,
        );
        assert!(matches!(c, AcquireResponse::Granted { .. }));
    }

    #[test]
    fn read_read_compatible() {
        let now = Utc::now();
        let mut leases = Vec::new();
        let r1 = decide(
            &mut leases,
            req("R1", &["src/baz.rs"], LeaseMode::Read, 1),
            now,
        );
        let r2 = decide(
            &mut leases,
            req("R2", &["src/baz.rs"], LeaseMode::Read, 1),
            now,
        );
        assert!(matches!(r1, AcquireResponse::Granted { .. }));
        assert!(matches!(r2, AcquireResponse::Granted { .. }));
        assert_eq!(leases.len(), 2);
    }

    #[test]
    fn release_frees_the_queue() {
        let now = Utc::now();
        let mut leases = Vec::new();
        let a = decide(
            &mut leases,
            req("A", &["src/foo.rs"], LeaseMode::Write, 10),
            now,
        );
        let a_id = match a {
            AcquireResponse::Granted { lease_id, .. } => lease_id,
            _ => panic!("A granted"),
        };
        // B blocked.
        let b = decide(
            &mut leases,
            req("B", &["src/foo.rs"], LeaseMode::Write, 5),
            now,
        );
        assert!(matches!(b, AcquireResponse::Queued { .. }));
        // Release A.
        leases.retain(|l| l.lease_id != a_id);
        // B retries → now granted.
        let b2 = decide(
            &mut leases,
            req("B", &["src/foo.rs"], LeaseMode::Write, 5),
            now,
        );
        assert!(matches!(b2, AcquireResponse::Granted { .. }));
    }

    #[test]
    fn expired_lease_self_heals() {
        let now = Utc::now();
        let mut leases = Vec::new();
        // A's lease already expired (granted in the past, 1s TTL).
        leases.push(Lease {
            lease_id: "stale".into(),
            agent_id: "A".into(),
            fleet_id: None,
            paths: vec!["src/foo.rs".into()],
            mode: LeaseMode::Write,
            priority: 10,
            reason: None,
            strict: false,
            granted_at: now - Duration::seconds(10),
            expires_at: now - Duration::seconds(1),
        });
        // Caller is responsible for sweeping expired leases before decide().
        let swept = sweep_expired(&mut leases, now);
        assert_eq!(swept.len(), 1);
        assert_eq!(swept[0].lease_id, "stale");
        // B requests the same path on a now-clean registry → granted.
        let b = decide(
            &mut leases,
            req("B", &["src/foo.rs"], LeaseMode::Write, 1),
            now,
        );
        assert!(matches!(b, AcquireResponse::Granted { .. }));
        assert!(leases.iter().all(|l| l.lease_id != "stale"));
    }

    #[test]
    fn strict_lease_denies_other_agent_on_overlap() {
        let now = Utc::now();
        let mut leases = Vec::new();
        let a = decide(
            &mut leases,
            strict_req("A", &["src/migrations/"], LeaseMode::Write, 10),
            now,
        );
        assert!(matches!(a, AcquireResponse::Granted { .. }));

        // B is hard-blocked on the exact path.
        let dec = decision_for(&mut leases, "B", "src/migrations/0007.sql", now);
        if let LeaseGateDecision::Deny { reason } = &dec {
            assert!(
                reason.contains("hard block"),
                "deny reason mentions hard block: {reason}"
            );
            assert!(reason.contains('A'), "deny reason names holder A: {reason}");
        } else {
            panic!("strict overlap must deny: {dec:?}");
        }

        // B is hard-blocked on a path under the strict directory scope.
        let dec = decision_for(&mut leases, "B", "src/migrations/0008.sql", now);
        assert!(
            matches!(dec, LeaseGateDecision::Deny { .. }),
            "directory-scoped strict overlap must deny: {dec:?}"
        );

        // Same agent is never blocked by its own strict lease.
        let dec = decision_for(&mut leases, "A", "src/migrations/0007.sql", now);
        assert_eq!(dec, LeaseGateDecision::Allow { warning: None });

        // Disjoint path is allowed even for another agent.
        let dec = decision_for(&mut leases, "B", "src/other.rs", now);
        assert_eq!(dec, LeaseGateDecision::Allow { warning: None });
    }

    #[test]
    fn non_strict_lease_still_warns_and_allows() {
        let now = Utc::now();
        let mut leases = Vec::new();
        decide(
            &mut leases,
            req("A", &["src/foo.rs"], LeaseMode::Write, 10),
            now,
        );

        let dec = decision_for(&mut leases, "B", "src/foo.rs", now);
        match dec {
            LeaseGateDecision::Allow { warning: Some(w) } => {
                assert!(w.contains("WAIT"), "non-strict overlap must warn: {w}");
            }
            other => panic!("expected allow+warn for non-strict overlap: {other:?}"),
        }
    }

    #[test]
    fn strict_read_lease_does_not_deny() {
        let now = Utc::now();
        let mut leases = Vec::new();
        let a = decide(
            &mut leases,
            strict_req("A", &["src/foo.rs"], LeaseMode::Read, 10),
            now,
        );
        assert!(matches!(a, AcquireResponse::Granted { .. }));

        // Strict only applies to write leases; read/read stays compatible.
        let dec = decision_for(&mut leases, "B", "src/foo.rs", now);
        assert_eq!(dec, LeaseGateDecision::Allow { warning: None });
    }

    // ── Phase 2 ────────────────────────────────────────────────────────────

    /// A high-priority waiter bumps a low-priority holder's *effective*
    /// priority (priority inheritance), while the holder's static priority is
    /// left untouched.
    #[test]
    fn priority_inheritance_bumps_holder_effective() {
        let now = Utc::now();
        let mut leases = Vec::new();

        // L (prio 1) holds foo.rs.
        let l = decide(
            &mut leases,
            req("L", &["src/foo.rs"], LeaseMode::Write, 1),
            now,
        );
        assert!(matches!(l, AcquireResponse::Granted { .. }));

        // H (prio 10) waits on foo.rs → queued behind L.
        let h = decide(
            &mut leases,
            req("H", &["src/foo.rs"], LeaseMode::Write, 10),
            now,
        );
        assert!(matches!(h, AcquireResponse::Queued { .. }));

        // L's static priority is unchanged, but its effective priority is the
        // inherited 10 — so a third agent can't slip in ahead of H.
        let l_held = leases
            .iter()
            .find(|x| is_held(x) && x.agent_id == "L")
            .expect("L still holds the lease");
        assert_eq!(l_held.priority, 1, "static priority is untouched");
        assert_eq!(
            effective_priority(&leases, l_held),
            10,
            "holder inherits the waiter's priority"
        );
    }

    /// A medium-priority requester must queue *behind* a high-priority waiter
    /// that is already blocked on the same holder (no starvation of the
    /// high-priority waiter). Position is the observable ordering signal.
    #[test]
    fn medium_prio_queues_behind_high_prio_waiter() {
        let now = Utc::now();
        let mut leases = Vec::new();

        // L (prio 1) holds foo.rs.
        decide(
            &mut leases,
            req("L", &["src/foo.rs"], LeaseMode::Write, 1),
            now,
        );

        // H (prio 10) waits first.
        let h = decide(
            &mut leases,
            req("H", &["src/foo.rs"], LeaseMode::Write, 10),
            now,
        );
        let h_pos = match h {
            AcquireResponse::Queued { position, .. } => position,
            _ => panic!("H should be queued"),
        };

        // M (prio 5) waits next → must land behind H, not ahead of it.
        let m = decide(
            &mut leases,
            req("M", &["src/foo.rs"], LeaseMode::Write, 5),
            now,
        );
        let m_pos = match m {
            AcquireResponse::Queued { position, .. } => position,
            _ => panic!("M should be queued"),
        };

        assert!(
            m_pos > h_pos,
            "medium-prio M (pos {m_pos}) must queue behind high-prio H (pos {h_pos})"
        );
    }

    /// Wound-wait: A holds X & waits on Y; B holds Y & requests X, closing the
    /// cycle. The lower-priority requester (B) aborts; the higher-priority
    /// party (A) keeps its place, and no wait-for cycle remains.
    #[test]
    fn deadlock_detected_lower_priority_aborts() {
        let now = Utc::now();
        let mut leases = Vec::new();

        // A (prio 10) holds X; B (prio 5) holds Y.
        decide(
            &mut leases,
            req("A", &["src/x.rs"], LeaseMode::Write, 10),
            now,
        );
        decide(
            &mut leases,
            req("B", &["src/y.rs"], LeaseMode::Write, 5),
            now,
        );

        // A requests Y → queued behind B (records A's wait edge A→B).
        let a2 = decide(
            &mut leases,
            req("A", &["src/y.rs"], LeaseMode::Write, 10),
            now,
        );
        assert!(matches!(a2, AcquireResponse::Queued { .. }));

        // B requests X → would close the cycle B→A→B. B is lower-priority, so
        // B is the wound-wait loser and aborts.
        let b2 = decide(
            &mut leases,
            req("B", &["src/x.rs"], LeaseMode::Write, 5),
            now,
        );
        match b2 {
            AcquireResponse::Abort { reason } => assert_eq!(reason, "deadlock"),
            other => panic!("B should abort on deadlock, got {other:?}"),
        }

        // The cycle is gone: B was never recorded as a waiter, A's wait stands,
        // and both original holders are intact (no holder was preempted).
        assert!(
            !leases.iter().any(|w| is_waiter(w) && w.agent_id == "B"),
            "the aborted requester leaves no waiter row"
        );
        assert!(
            leases.iter().any(|w| is_waiter(w) && w.agent_id == "A"),
            "the surviving party keeps its place in line"
        );
        assert!(leases
            .iter()
            .any(|l| is_held(l) && l.agent_id == "A" && l.paths == vec!["src/x.rs".to_string()]));
        assert!(leases
            .iter()
            .any(|l| is_held(l) && l.agent_id == "B" && l.paths == vec!["src/y.rs".to_string()]));
        // The *committed* wait-for graph is acyclic: A still waits on B, but B
        // only holds (no committed wait edge of its own), so nothing loops.
        let adj = waiter_adjacency(&leases);
        for (g, neigh) in &adj {
            for n in neigh {
                assert!(
                    find_path(&adj, n, g).is_none(),
                    "committed waiter graph must be acyclic ({g}->{n} must not return)"
                );
            }
        }
    }

    /// The symmetric break: when the requester closing the cycle is the
    /// *higher*-priority party, it keeps moving — the lower-priority waiter's
    /// wait is cancelled (its held lease untouched) and the requester queues.
    #[test]
    fn deadlock_break_cancels_lower_priority_waiter_not_requester() {
        let now = Utc::now();
        let mut leases = Vec::new();

        // A (prio 5) holds X; B (prio 10) holds Y.
        decide(
            &mut leases,
            req("A", &["src/x.rs"], LeaseMode::Write, 5),
            now,
        );
        decide(
            &mut leases,
            req("B", &["src/y.rs"], LeaseMode::Write, 10),
            now,
        );

        // A (low prio) requests Y → queued, recording A→B.
        let a2 = decide(
            &mut leases,
            req("A", &["src/y.rs"], LeaseMode::Write, 5),
            now,
        );
        assert!(matches!(a2, AcquireResponse::Queued { .. }));

        // B (high prio) requests X → cycle; A is the loser, so A's wait is
        // cancelled and B queues normally (B does NOT abort).
        let b2 = decide(
            &mut leases,
            req("B", &["src/x.rs"], LeaseMode::Write, 10),
            now,
        );
        assert!(
            matches!(b2, AcquireResponse::Queued { .. }),
            "the higher-priority requester keeps its position"
        );

        assert!(
            !leases.iter().any(|w| is_waiter(w) && w.agent_id == "A"),
            "the lower-priority waiter loses its wait"
        );
        assert!(
            leases.iter().any(|w| is_waiter(w) && w.agent_id == "B"),
            "the surviving requester is now queued"
        );
        // A's held lease on X is preserved — wound-wait never preempts a holder.
        assert!(leases
            .iter()
            .any(|l| is_held(l) && l.agent_id == "A" && l.paths == vec!["src/x.rs".to_string()]));
    }

    /// The PreToolUse advisory names the blocking holder and only the
    /// contended scope — audited helper over `advisory_for`.
    #[test]
    fn advisory_names_the_blocking_holder() {
        let now = Utc::now();
        let mut leases = Vec::new();
        let mut audit = VecDeque::new();
        let mut metrics = CoordMetrics::default();
        decide(
            &mut leases,
            req("A", &["src/foo.rs"], LeaseMode::Write, 10),
            now,
        );

        // B's gate check on the contended file gets a WAIT advisory.
        let adv = advisory_for(
            &mut leases,
            &mut audit,
            &mut metrics,
            "B",
            "src/foo.rs",
            now,
        )
        .expect("advisory present");
        assert!(adv.contains("WAIT"));
        assert!(adv.contains('A'));

        // A's own check on its own lease: no self-advisory.
        assert!(advisory_for(
            &mut leases,
            &mut audit,
            &mut metrics,
            "A",
            "src/foo.rs",
            now
        )
        .is_none());

        // An unrelated file: no advisory.
        assert!(advisory_for(
            &mut leases,
            &mut audit,
            &mut metrics,
            "B",
            "src/other.rs",
            now
        )
        .is_none());
    }

    /// Ring buffer must drop the oldest entry when at capacity so a
    /// long-running server's memory stays bounded. Pure helper, no axum.
    #[test]
    fn audit_ring_evicts_oldest_past_cap() {
        let mut audit: VecDeque<AuditRecord> = VecDeque::with_capacity(AUDIT_RING_CAP);
        let base = Utc::now();
        // Push cap + 1 records; oldest (id "0") must be evicted, the
        // remaining IDs must be `1..=cap` in arrival order.
        for i in 0..=(AUDIT_RING_CAP as i64) {
            push_audit(
                &mut audit,
                AuditRecord {
                    ts: base + Duration::seconds(i),
                    waiter: Some(format!("w{i}")),
                    holder: None,
                    scope: vec![format!("src/file{i}.rs")],
                    waited_secs: None,
                    outcome: AuditOutcome::Granted,
                },
            );
        }
        assert_eq!(audit.len(), AUDIT_RING_CAP);
        // The oldest entry should now be id 1, not 0.
        assert_eq!(audit.front().unwrap().waiter.as_deref(), Some("w1"));
        // The newest should be id `cap`.
        assert_eq!(
            audit.back().unwrap().waiter.as_deref(),
            Some(format!("w{AUDIT_RING_CAP}").as_str())
        );
        // Verify the entire sequence is contiguous 1..=cap (FIFO).
        for (idx, rec) in audit.iter().enumerate() {
            assert_eq!(
                rec.waiter.as_deref(),
                Some(format!("w{}", idx + 1).as_str())
            );
        }
    }

    /// `since` parser: suffix multiplication + missing/invalid → None.
    /// Pure helper, no axum.
    #[test]
    fn parse_since_basic_units() {
        assert_eq!(parse_since(None), None);
        assert_eq!(parse_since(Some("")), None);
        assert_eq!(parse_since(Some("   ")), None);
        assert_eq!(parse_since(Some("30s")), Some(30));
        assert_eq!(parse_since(Some("10m")), Some(600));
        assert_eq!(parse_since(Some("2h")), Some(7200));
        assert_eq!(parse_since(Some("1d")), Some(86_400));
        // Bare number defaults to seconds.
        assert_eq!(parse_since(Some("45")), Some(45));
        // Whitespace around the suffix is trimmed.
        assert_eq!(parse_since(Some(" 5m ")), Some(300));
        // Unparseable: lenient → None (treated as "no filter").
        assert_eq!(parse_since(Some("abc")), None);
        assert_eq!(parse_since(Some("10x")), None);
        assert_eq!(parse_since(Some("-5m")), None);
    }
}
