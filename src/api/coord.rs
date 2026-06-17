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
//! (readers-writers, Courtois 1971). Phase 1 is advisory only; priority
//! inheritance, deadlock detection, and deny-mode enforcement are later
//! phases (see `docs/roadmap/epics/agent-coordination.md`).

use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::{delete, get, post, put},
    Router,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

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
        /// Number of conflicting holders at or above the requester's
        /// priority — a coarse "how far back am I" signal for Phase 1.
        position: usize,
    },
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
/// orders the returned queue. Preemption/inheritance is Phase 2.
fn decide(leases: &mut Vec<Lease>, req: AcquireRequest, now: DateTime<Utc>) -> AcquireResponse {
    let ttl = req
        .ttl_secs
        .unwrap_or(DEFAULT_TTL_SECS)
        .clamp(1, MAX_TTL_SECS);

    let conflicting: Vec<&Lease> = leases
        .iter()
        .filter(|l| l.agent_id != req.agent_id && conflicts(l, &req.paths, req.mode))
        .collect();

    if conflicting.is_empty() {
        drop(conflicting);
        let lease = Lease {
            lease_id: uuid::Uuid::new_v4().to_string(),
            agent_id: req.agent_id,
            fleet_id: req.fleet_id,
            paths: req.paths,
            mode: req.mode,
            priority: req.priority,
            reason: req.reason,
            granted_at: now,
            expires_at: now + Duration::seconds(ttl as i64),
        };
        let resp = AcquireResponse::Granted {
            lease_id: lease.lease_id.clone(),
            expires_at: lease.expires_at,
        };
        leases.push(lease);
        resp
    } else {
        let retry_after_secs = conflicting
            .iter()
            .map(|l| (l.expires_at - now).num_seconds().max(0))
            .min()
            .unwrap_or(0);
        let position = conflicting
            .iter()
            .filter(|l| l.priority >= req.priority)
            .count();
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

#[derive(Debug, Serialize)]
struct ListResponse {
    count: usize,
    leases: Vec<Lease>,
}

/// GET /api/v1/coord/leases[?path=] — inspect live contention. With
/// `path`, returns only leases whose scope overlaps it.
async fn list(
    State(services): State<ContextNestServices>,
    Query(q): Query<ListQuery>,
) -> Json<ListResponse> {
    let now = Utc::now();
    let mut leases = services.coord_leases.write().await;
    let mut metrics = services.coord_metrics.write().await;
    let mut audit = services.coord_audit.write().await;
    let _swept = sweep_and_audit(&mut leases, &mut audit, &mut metrics, now);
    let filtered: Vec<Lease> = match q.path.as_deref() {
        Some(p) => leases
            .iter()
            .filter(|l| l.paths.iter().any(|lp| path_overlap(lp, p)))
            .cloned()
            .collect(),
        None => leases.clone(),
    };
    Json(ListResponse {
        count: filtered.len(),
        leases: filtered,
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
    let blockers: Vec<&Lease> = leases
        .iter()
        .filter(|l| {
            l.agent_id != agent_id
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
