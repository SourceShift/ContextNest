//! Per-tenant SQLite authority. Acknowledgement follows FULL-synchronous WAL
//! commit. A completion transaction updates the canonical snapshot and the
//! exact claimed revision together; RAM caches are disposable.
use super::{types::*, Error, Result};
use crate::memory::attractors::memory_attractor_manager::CanonicalSnapshot;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

pub struct Database {
    pub(crate) connection: Connection,
    // Immutable, generation-qualified canonical views. At most two active
    // sessions / 64 MiB per registered tenant; inactive sessions stay on disk.
    cache: VecDeque<(String, i64, i64, Arc<CanonicalSnapshot>, usize)>,
}

#[derive(Clone)]
pub struct Job {
    pub scope: MemoryScope,
    pub lease: String,
    pub canonical_version: i64,
    pub record: StoredRecord,
    pub snapshot: CanonicalSnapshot,
    pub live_ids: Vec<String>,
    pub attempts: u32,
    pub embedding: Option<Vec<f32>>,
    pub queue_wait_ms: u64,
    pub embedding_ms: u64,
}

impl Database {
    pub fn open(path: &Path, policy: &TenantPolicy) -> Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_millis(100))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS configuration(id INTEGER PRIMARY KEY CHECK(id=1), policy TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS sessions(
                id TEXT PRIMARY KEY, generation INTEGER NOT NULL, status TEXT NOT NULL,
                expires_at INTEGER NOT NULL, canonical_version INTEGER NOT NULL DEFAULT 0,
                snapshot TEXT NOT NULL, lease TEXT, last_scheduled INTEGER NOT NULL DEFAULT 0,
                candidates INTEGER NOT NULL DEFAULT 0, processing_ms INTEGER NOT NULL DEFAULT 0,
                basin_count INTEGER NOT NULL DEFAULT 0, edge_count INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS records(
                session TEXT NOT NULL REFERENCES sessions(id), event TEXT NOT NULL,
                revision INTEGER NOT NULL, payload TEXT NOT NULL, fingerprint TEXT NOT NULL,
                state TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
                next_attempt INTEGER NOT NULL DEFAULT 0, error TEXT, embedding TEXT,
                PRIMARY KEY(session,event));
            CREATE TABLE IF NOT EXISTS job_metrics(session TEXT PRIMARY KEY REFERENCES sessions(id),
                queue_wait_ms INTEGER NOT NULL DEFAULT 0, embedding_ms INTEGER NOT NULL DEFAULT 0);
            CREATE INDEX IF NOT EXISTS pending_records ON records(state,next_attempt,session);
            CREATE INDEX IF NOT EXISTS session_state ON records(session,state);")?;
        let saved: Option<String> = connection
            .query_row("SELECT policy FROM configuration WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        let encoded = serde_json::to_string(policy)?;
        if let Some(saved) = saved {
            let previous: TenantPolicy = serde_json::from_str(&saved)?;
            if previous != *policy {
                if policy.version <= previous.version {
                    return Err(Error::Conflict(
                        "tenant policy change requires a higher version",
                    ));
                }
                let tx = connection.transaction()?;
                // A new space invalidates canonical vectors, never reuses old scores.
                tx.execute("UPDATE sessions SET snapshot=?1,canonical_version=canonical_version+1,generation=generation+1,lease=NULL,basin_count=0,edge_count=0",[empty_snapshot()?])?;
                tx.execute("UPDATE records SET state='pending',attempts=0,next_attempt=0 WHERE state!='deleted'",[])?;
                if previous.embedding_space != policy.embedding_space {
                    tx.execute("UPDATE records SET embedding=NULL", [])?;
                }
                tx.execute("UPDATE configuration SET policy=?1 WHERE id=1", [&encoded])?;
                tx.commit()?;
            }
        } else {
            connection.execute("INSERT INTO configuration VALUES(1,?1)", [&encoded])?;
        }
        // Only one process may own a tenant database (registry holds a file lock).
        connection.execute(
            "UPDATE records SET state='pending' WHERE state='processing'",
            [],
        )?;
        connection.execute("UPDATE sessions SET lease=NULL", [])?;
        Ok(Self {
            connection,
            cache: VecDeque::new(),
        })
    }

    pub fn open_session(
        &mut self,
        tenant: &str,
        id: &str,
        mode: &str,
        policy: &TenantPolicy,
    ) -> Result<MemoryScope> {
        if !valid_id(id) || !["create", "resume"].contains(&mode) {
            return Err(Error::Invalid("invalid session or mode"));
        }
        let tx = self.connection.transaction()?;
        let row: Option<(i64, String, i64)> = tx
            .query_row(
                "SELECT generation,status,expires_at FROM sessions WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let now = now();
        let generation = match row {
            None if mode == "create" => {
                let count: usize = tx.query_row(
                    "SELECT count(*) FROM sessions WHERE status!='deleted' AND expires_at>?1",
                    [now],
                    |r| r.get(0),
                )?;
                if count >= policy.max_sessions {
                    return Err(Error::Busy);
                }
                tx.execute("INSERT INTO sessions(id,generation,status,expires_at,snapshot) VALUES(?1,1,'active',?2,?3)",params![id,now+i64::from(policy.retention_days)*86400,empty_snapshot()?])?;
                1
            }
            None => return Err(Error::NotFound),
            Some((_, status, expires)) if status == "deleted" || expires <= now => {
                return Err(Error::NotFound)
            }
            Some((generation, status, _)) => {
                if mode == "create" {
                    return Err(Error::Conflict("session exists; use explicit resume"));
                }
                if status == "closed" {
                    tx.execute("UPDATE sessions SET status='active' WHERE id=?1", [id])?;
                }
                generation
            }
        };
        tx.commit()?;
        Ok(MemoryScope {
            tenant_id: tenant.into(),
            session_id: id.into(),
            generation,
        })
    }

    pub fn verify(&self, scope: &MemoryScope) -> Result<()> {
        let live: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND generation=?2 AND status='active' AND expires_at>?3)",params![scope.session_id,scope.generation,now()],|r|r.get(0))?;
        if live {
            Ok(())
        } else {
            Err(Error::Unauthorized)
        }
    }

    pub fn accept(
        &mut self,
        scope: &MemoryScope,
        input: StoreRequest,
        policy: &TenantPolicy,
    ) -> Result<Acceptance> {
        self.verify(scope)?;
        if !valid_id(&input.source_event_id)
            || input.revision < 1
            || input.content.trim().is_empty()
            || input.content.len() > policy.max_content_bytes
            || !input.importance.is_finite()
            || !(0.0..=1.0).contains(&input.importance)
            || input.metadata.keys().any(|k| {
                k.starts_with("_cn_")
                    || ["tenant_id", "session_id", "session_ids"].contains(&k.as_str())
            })
            || serde_json::to_vec(&input.metadata)?.len() > 8192
        {
            return Err(Error::Invalid("invalid record"));
        }
        let kind = input
            .metadata
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("conversation-turn");
        if !policy.kinds.iter().any(|k| k == kind) {
            return Err(Error::Invalid("memory kind is not permitted"));
        }
        let fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&input)?));
        let id = hex::encode(Sha256::digest(format!(
            "{}\0{}\0{}",
            scope.tenant_id, scope.session_id, input.source_event_id
        )));
        let tx = self.connection.transaction()?;
        let existing: Option<(i64, String, String)> = tx
            .query_row(
                "SELECT revision,fingerprint,state FROM records WHERE session=?1 AND event=?2",
                params![scope.session_id, input.source_event_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((revision, old_hash, state)) = &existing {
            if state == "deleted" {
                return Err(Error::Conflict("event was deleted; use a new event ID"));
            }
            if *revision == input.revision && *old_hash == fingerprint {
                return Ok(Acceptance {
                    fragment_id: id,
                    revision: *revision,
                    indexing_status: state.clone(),
                    duplicate: true,
                });
            }
            if input.revision != revision + 1 {
                return Err(Error::Conflict("event revision conflict"));
            }
        } else if input.revision != 1 {
            return Err(Error::Conflict("first revision must be 1"));
        }
        let backlog: usize = tx.query_row(
            "SELECT count(*) FROM records WHERE state IN ('pending','processing')",
            [],
            |r| r.get(0),
        )?;
        let count: usize = tx.query_row(
            "SELECT count(*) FROM records WHERE session=?1",
            [&scope.session_id],
            |r| r.get(0),
        )?;
        if backlog >= policy.max_pending
            || (existing.is_none() && count >= policy.max_records_per_session)
        {
            return Err(Error::Busy);
        }
        let record = StoredRecord {
            fragment_id: id.clone(),
            source_event_id: input.source_event_id,
            revision: input.revision,
            content: input.content,
            importance: input.importance,
            metadata: input.metadata,
            created_at: now(),
        };
        tx.execute("INSERT INTO records(session,event,revision,payload,fingerprint,state) VALUES(?1,?2,?3,?4,?5,'pending')
            ON CONFLICT(session,event) DO UPDATE SET revision=excluded.revision,payload=excluded.payload,
            fingerprint=excluded.fingerprint,state='pending',attempts=0,next_attempt=0,error=NULL,embedding=NULL",
            params![scope.session_id,record.source_event_id,record.revision,serde_json::to_string(&record)?,fingerprint])?;
        tx.commit()?;
        Ok(Acceptance {
            fragment_id: id,
            revision: record.revision,
            indexing_status: "pending".into(),
            duplicate: false,
        })
    }

    /// Round-robin sessions within a tenant. A lease serializes canonical
    /// mutations without holding a database transaction during provider work.
    pub fn claim(&mut self, tenant: &str) -> Result<Option<Job>> {
        let tx = self.connection.transaction()?;
        let row: Option<(String, i64, i64, String, String, u32)> = tx
            .query_row(
                "SELECT s.id,s.generation,s.canonical_version,s.snapshot,r.payload,r.attempts
             FROM sessions s JOIN records r ON r.session=s.id
             WHERE s.status='active' AND s.expires_at>?1 AND s.lease IS NULL
             AND r.state='pending' AND r.next_attempt<=?1
             ORDER BY s.last_scheduled,s.id,r.rowid LIMIT 1",
                [now()],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((session, generation, canonical_version, snapshot, payload, attempts)) = row
        else {
            return Ok(None);
        };
        let record: StoredRecord = serde_json::from_str(&payload)?;
        let (embedding, next_attempt): (Option<String>, i64) = tx.query_row(
            "SELECT embedding,next_attempt FROM records WHERE session=?1 AND event=?2",
            params![session, record.source_event_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let queue_wait_ms = now()
            .saturating_sub(record.created_at.max(next_attempt))
            .max(0) as u64
            * 1000;
        let embedding = embedding.map(|s| serde_json::from_str(&s)).transpose()?;
        let lease = uuid::Uuid::new_v4().to_string();
        // A monotonic sequence avoids ties when many jobs finish in one second.
        tx.execute("UPDATE sessions SET lease=?1,last_scheduled=(SELECT coalesce(max(last_scheduled),0)+1 FROM sessions) WHERE id=?2",params![lease,session])?;
        tx.execute("UPDATE records SET state='processing',attempts=attempts+1 WHERE session=?1 AND event=?2",params![session,record.source_event_id])?;
        let live_ids = {
            let mut q =
                tx.prepare("SELECT payload FROM records WHERE session=?1 AND state='ready'")?;
            let rows = q.query_map([&session], |r| r.get::<_, String>(0))?;
            let mut ids = Vec::new();
            for row in rows {
                ids.push(serde_json::from_str::<StoredRecord>(&row?)?.fragment_id);
            }
            ids
        };
        tx.commit()?;
        Ok(Some(Job {
            scope: MemoryScope {
                tenant_id: tenant.into(),
                session_id: session,
                generation,
            },
            lease,
            canonical_version,
            record,
            snapshot: serde_json::from_str(&snapshot)?,
            live_ids,
            attempts: attempts + 1,
            embedding,
            queue_wait_ms,
            embedding_ms: 0,
        }))
    }

    pub fn complete(
        &mut self,
        job: &Job,
        snapshot: CanonicalSnapshot,
        elapsed_ms: u64,
    ) -> Result<bool> {
        let tx = self.connection.transaction()?;
        let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sessions s JOIN records r ON r.session=s.id
            WHERE s.id=?1 AND s.generation=?2 AND s.canonical_version=?3 AND s.lease=?4
            AND s.status='active' AND s.expires_at>?5 AND r.event=?6 AND r.revision=?7 AND r.state='processing')",
            params![job.scope.session_id,job.scope.generation,job.canonical_version,job.lease,now(),job.record.source_event_id,job.record.revision],|r|r.get(0))?;
        if !valid {
            tx.execute("UPDATE records SET state='pending' WHERE session=?1 AND event=?2 AND revision=?3 AND state='processing'
                AND EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND generation=?4 AND lease=?5)",
                params![job.scope.session_id,job.record.source_event_id,job.record.revision,job.scope.generation,job.lease])?;
            tx.execute(
                "UPDATE sessions SET lease=NULL WHERE id=?1 AND lease=?2",
                params![job.scope.session_id, job.lease],
            )?;
            tx.commit()?;
            return Ok(false);
        }
        tx.execute("UPDATE sessions SET snapshot=?1,canonical_version=canonical_version+1,lease=NULL,
            candidates=candidates+?2,processing_ms=processing_ms+?3,basin_count=?5,edge_count=?6 WHERE id=?4",
            params![serde_json::to_string(&snapshot)?,job.live_ids.len() as i64,elapsed_ms as i64,job.scope.session_id,snapshot.basins.len(),snapshot.edges.len()])?;
        let vector = snapshot
            .fragments
            .iter()
            .find(|f| f.id == job.record.fragment_id)
            .ok_or(Error::Invalid("missing canonical fragment"))?;
        tx.execute("UPDATE records SET state='ready',error=NULL,embedding=?4 WHERE session=?1 AND event=?2 AND revision=?3",
            params![job.scope.session_id,job.record.source_event_id,job.record.revision,serde_json::to_string(&vector.content)?])?;
        tx.execute("INSERT INTO job_metrics VALUES(?1,?2,?3) ON CONFLICT(session) DO UPDATE SET
            queue_wait_ms=queue_wait_ms+excluded.queue_wait_ms,embedding_ms=embedding_ms+excluded.embedding_ms",
            params![job.scope.session_id,job.queue_wait_ms,job.embedding_ms])?;
        tx.commit()?;
        Ok(true)
    }

    pub fn fail(&mut self, job: &Job, transient: bool) -> Result<()> {
        let retry = transient && job.attempts < 5;
        let delay =
            (1i64 << job.attempts.min(5)) + i64::from(uuid::Uuid::new_v4().as_bytes()[0] % 3);
        let tx = self.connection.transaction()?;
        tx.execute("UPDATE records SET state=?1,next_attempt=?2,error=?3 WHERE session=?4 AND event=?5 AND revision=?6 AND state='processing' AND EXISTS(SELECT 1 FROM sessions WHERE id=?4 AND generation=?7 AND lease=?8)",
            params![if retry {"pending"} else {"failed"},now()+delay,if transient {"provider_unavailable"} else {"invalid_processing_result"},job.scope.session_id,job.record.source_event_id,job.record.revision,job.scope.generation,job.lease])?;
        tx.execute(
            "UPDATE sessions SET lease=NULL WHERE id=?1 AND lease=?2",
            params![job.scope.session_id, job.lease],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn read(
        &mut self,
        scope: &MemoryScope,
    ) -> Result<(i64, Arc<CanonicalSnapshot>, Vec<StoredRecord>)> {
        self.verify(scope)?;
        let version: i64 = self.connection.query_row(
            "SELECT canonical_version FROM sessions WHERE id=?1",
            [&scope.session_id],
            |r| r.get(0),
        )?;
        let cached = self.cache.iter().position(|(id, g, v, _, _)| {
            id == &scope.session_id && *g == scope.generation && *v == version
        });
        let snapshot = if let Some(index) = cached {
            let entry = self.cache.remove(index).unwrap();
            let snapshot = entry.3.clone();
            self.cache.push_back(entry);
            snapshot
        } else {
            let json: String = self.connection.query_row(
                "SELECT snapshot FROM sessions WHERE id=?1",
                [&scope.session_id],
                |r| r.get(0),
            )?;
            let snapshot: Arc<CanonicalSnapshot> = Arc::new(serde_json::from_str(&json)?);
            let weight = snapshot
                .fragments
                .iter()
                .map(|f| f.content.len() * 4)
                .sum::<usize>()
                + snapshot
                    .nodes
                    .iter()
                    .map(|n| n.content.len() * 4)
                    .sum::<usize>()
                + snapshot
                    .basins
                    .iter()
                    .map(|b| b.center.len() * 4 + b.associated_fragments.len() * 128)
                    .sum::<usize>()
                + snapshot.edges.len() * 512;
            self.cache
                .retain(|(id, _, _, _, _)| id != &scope.session_id);
            if weight <= 64 * 1024 * 1024 {
                while self.cache.len() >= 2
                    || self.cache.iter().map(|e| e.4).sum::<usize>() + weight > 64 * 1024 * 1024
                {
                    self.cache.pop_front();
                }
                self.cache.push_back((
                    scope.session_id.clone(),
                    scope.generation,
                    version,
                    snapshot.clone(),
                    weight,
                ));
            }
            snapshot
        };
        let mut q = self
            .connection
            .prepare("SELECT payload FROM records WHERE session=?1 AND state='ready'")?;
        let rows = q.query_map([&scope.session_id], |r| r.get::<_, String>(0))?;
        let mut records = Vec::new();
        for row in rows {
            records.push(serde_json::from_str(&row?)?);
        }
        Ok((version, snapshot, records))
    }

    pub fn transition(&mut self, scope: &MemoryScope, action: &str) -> Result<()> {
        self.verify(scope)?;
        if !["close", "reset", "delete"].contains(&action) {
            return Err(Error::Invalid("invalid session action"));
        }
        let tx = self.connection.transaction()?;
        let status = match action {
            "close" => "closed",
            "delete" => "deleted",
            _ => "active",
        };
        tx.execute(
            "UPDATE sessions SET status=?1,generation=generation+1,lease=NULL WHERE id=?2",
            params![status, scope.session_id],
        )?;
        if action != "close" {
            tx.execute("DELETE FROM records WHERE session=?1", [&scope.session_id])?;
            tx.execute(
                "DELETE FROM job_metrics WHERE session=?1",
                [&scope.session_id],
            )?;
            tx.execute("UPDATE sessions SET snapshot=?1,canonical_version=canonical_version+1,basin_count=0,edge_count=0,candidates=0,processing_ms=0 WHERE id=?2",params![empty_snapshot()?,scope.session_id])?;
        } else {
            tx.execute(
                "UPDATE records SET state='pending' WHERE session=?1 AND state='processing'",
                [&scope.session_id],
            )?;
        }
        tx.commit()?;
        self.cache.retain(|(id, ..)| id != &scope.session_id);
        Ok(())
    }

    pub fn discard(&mut self, scope: &MemoryScope, event: &str) -> Result<()> {
        self.verify(scope)?;
        let tx = self.connection.transaction()?;
        let payload: Option<String> = tx
            .query_row(
                "SELECT payload FROM records WHERE session=?1 AND event=?2 AND state!='deleted'",
                params![scope.session_id, event],
                |r| r.get(0),
            )
            .optional()?;
        let Some(payload) = payload else {
            return Err(Error::NotFound);
        };
        let record: StoredRecord = serde_json::from_str(&payload)?;
        let encoded: String = tx.query_row(
            "SELECT snapshot FROM sessions WHERE id=?1",
            [&scope.session_id],
            |r| r.get(0),
        )?;
        let mut snapshot: CanonicalSnapshot = serde_json::from_str(&encoded)?;
        snapshot.fragments.retain(|f| f.id != record.fragment_id);
        snapshot.nodes.retain(|n| n.id != record.fragment_id);
        snapshot
            .edges
            .retain(|e| e.source != record.fragment_id && e.target != record.fragment_id);
        snapshot.norms.remove(&record.fragment_id);
        snapshot.basins.retain_mut(|b| {
            b.associated_fragments.remove(&record.fragment_id);
            !b.associated_fragments.is_empty()
        });
        tx.execute("UPDATE records SET state='deleted',payload='{}',fingerprint='',embedding=NULL WHERE session=?1 AND event=?2",params![scope.session_id,event])?;
        tx.execute("UPDATE sessions SET canonical_version=canonical_version+1,snapshot=?2,basin_count=?3,edge_count=?4 WHERE id=?1",
            params![scope.session_id,serde_json::to_string(&snapshot)?,snapshot.basins.len(),snapshot.edges.len()])?;
        tx.commit()?;
        self.cache.retain(|(id, ..)| id != &scope.session_id);
        Ok(())
    }

    pub fn stats(&self, scope: &MemoryScope) -> Result<ScopeStats> {
        self.verify(scope)?;
        let mut counts = std::collections::HashMap::new();
        let mut query = self
            .connection
            .prepare("SELECT state,count(*) FROM records WHERE session=?1 GROUP BY state")?;
        for row in query.query_map([&scope.session_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, usize>(1)?))
        })? {
            let (state, n) = row?;
            counts.insert(state, n);
        }
        // Only scalar statistics cross the seam; no canonical JSON is parsed.
        let (basins, edges, candidates, processing_ms) = self.connection.query_row(
            "SELECT basin_count,edge_count,candidates,processing_ms FROM sessions WHERE id=?1",
            [&scope.session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let (queue_wait_ms, embedding_ms) = self.connection.query_row(
            "SELECT coalesce(m.queue_wait_ms,0),coalesce(m.embedding_ms,0) FROM sessions s LEFT JOIN job_metrics m ON m.session=s.id WHERE s.id=?1",
            [&scope.session_id], |r| Ok((r.get(0)?,r.get(1)?)))?;
        Ok(ScopeStats {
            collected_at: chrono::Utc::now().to_rfc3339(),
            ready: *counts.get("ready").unwrap_or(&0),
            pending: *counts.get("pending").unwrap_or(&0),
            processing: *counts.get("processing").unwrap_or(&0),
            failed: *counts.get("failed").unwrap_or(&0),
            basins,
            edges,
            candidates_scored: candidates,
            processing_ms,
            queue_wait_ms,
            embedding_ms,
            generation: scope.generation,
        })
    }

    pub fn expire(&mut self) -> Result<()> {
        self.cache.clear();
        let tx = self.connection.transaction()?;
        tx.execute(
            "DELETE FROM records WHERE session IN (SELECT id FROM sessions WHERE expires_at<=?1)",
            [now()],
        )?;
        tx.execute("UPDATE sessions SET status='deleted',generation=generation+1,lease=NULL,snapshot=?1,basin_count=0,edge_count=0 WHERE expires_at<=?2 AND status!='deleted'",params![empty_snapshot()?,now()])?;
        tx.commit()?;
        Ok(())
    }
}
fn empty_snapshot() -> Result<String> {
    Ok(serde_json::to_string(&CanonicalSnapshot::default())?)
}
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
