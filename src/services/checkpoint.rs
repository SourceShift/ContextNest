//! Canonical checkpoints for the existing operator WAL. Input replay remains
//! compatible; completed vectors/basins/edges restore without re-embedding.
//!
//! Storage format (disk-first substrate epic): object payloads are JSON with
//! their vector emptied; fragment and basin vectors live in the binary
//! `vectors` table (little-endian f32). Node payloads never carry a vector —
//! a node shares its fragment's. Rows written by older builds (vectors inline
//! as JSON arrays, ~11 KB per 1024-d vector) still load unchanged inside a
//! v0.2 file; [`compact`] rewrites a whole checkpoint into the binary form.
//!
//! A *whole-file* pre-v0.2 checkpoint (no `vectors` table at all) is refused
//! at [`CheckpointStore::open`] unless `CONTEXTNEST_ALLOW_LEGACY_CHECKPOINT=1`
//! — the reason is in [`is_legacy_checkpoint`].
use crate::memory::attractors::attractor_basin::AttractorBasin;
use crate::memory::attractors::connection_network::{ConnectionEdge, MemoryNode, MemoryNodeType};
use crate::memory::attractors::memory_attractor_manager::CanonicalSnapshot;
use crate::memory::attractors::MemoryFragment;
use crate::services::{compute, ContextNestServices};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Rows per restore batch handed to the attractor manager.
const RESTORE_BATCH: usize = 2048;

/// Set to `1` to let [`CheckpointStore::open`] adopt a pre-v0.2 checkpoint in
/// place. Off by default — see [`is_legacy_checkpoint`].
const ALLOW_LEGACY_ENV: &str = "CONTEXTNEST_ALLOW_LEGACY_CHECKPOINT";

const SCHEMA: &str = "PRAGMA journal_mode=WAL;PRAGMA synchronous=FULL;
    CREATE TABLE IF NOT EXISTS identity(space TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS objects(kind TEXT,id TEXT,payload TEXT NOT NULL,PRIMARY KEY(kind,id));
    CREATE TABLE IF NOT EXISTS vectors(kind TEXT,id TEXT,data BLOB NOT NULL,PRIMARY KEY(kind,id));
    CREATE TABLE IF NOT EXISTS completed(id TEXT PRIMARY KEY,metadata TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS tombstones(id TEXT PRIMARY KEY);
    CREATE TABLE IF NOT EXISTS retries(id TEXT PRIMARY KEY,payload TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS tails(id TEXT PRIMARY KEY,payload TEXT NOT NULL);";

pub struct CheckpointStore {
    _lock: std::fs::File,
    connection: Mutex<Connection>,
}

/// One ordered slice of a streaming restore. Fragments always precede the
/// nodes that share their vectors.
pub enum RestoreBatch {
    Fragments(Vec<MemoryFragment>),
    Basins(Vec<AttractorBasin>),
    Nodes(Vec<MemoryNode>),
    Edges(Vec<ConnectionEdge>),
}

pub struct RestoreSummary {
    pub fragments: usize,
    pub metadata: HashMap<String, HashMap<String, Value>>,
    pub deleted: HashSet<String>,
}

/// Node row without its (ignored) legacy `content` array: serde skips the
/// unknown field without allocating the vector.
#[derive(serde::Deserialize)]
struct NodeRow {
    id: String,
    node_type: MemoryNodeType,
    importance: f32,
    last_accessed: DateTime<Utc>,
    access_frequency: f32,
    metadata: HashMap<String, String>,
    created_at: DateTime<Utc>,
    fragment_ids: Vec<String>,
}

impl From<NodeRow> for MemoryNode {
    fn from(row: NodeRow) -> Self {
        MemoryNode {
            id: row.id,
            node_type: row.node_type,
            content: Vec::new(),
            importance: row.importance,
            last_accessed: row.last_accessed,
            access_frequency: row.access_frequency,
            metadata: row.metadata,
            created_at: row.created_at,
            fragment_ids: row.fragment_ids,
        }
    }
}

fn encode_vector(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn decode_vector(bytes: &[u8]) -> Option<Vec<f32>> {
    (bytes.len() % 4 == 0).then(|| {
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    })
}

fn upsert_object(
    tx: &rusqlite::Transaction<'_>,
    kind: &str,
    id: &str,
    payload: &str,
) -> Result<()> {
    tx.execute(
        "INSERT INTO objects VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET payload=excluded.payload",
        params![kind, id, payload],
    )?;
    Ok(())
}

fn upsert_vector(
    tx: &rusqlite::Transaction<'_>,
    kind: &str,
    id: &str,
    vector: &[f32],
) -> Result<()> {
    tx.execute(
        "INSERT INTO vectors VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data",
        params![kind, id, encode_vector(vector)],
    )?;
    Ok(())
}

/// True when `path` is an existing pre-v0.2 checkpoint: it carries the v0.1
/// `objects` table but no binary `vectors` table, so every vector is still
/// inline JSON inside a payload.
///
/// Such a file must not be adopted in place. [`CheckpointStore::open`] runs
/// [`SCHEMA`] unconditionally, which would add an empty `vectors` table to
/// the operator's only pre-upgrade copy — the migration guide's contract is
/// that the source is opened read-only and never written. Booting it also
/// materialises ~11 KB of JSON per 1024-d vector, the heap profile that
/// exhausted a 36 GB host's compressed-memory segments (v0.2 release notes:
/// 12.4 GB -> 2.6 GB live heap).
///
/// Errors are folded into `false`: an unreadable file is the caller's problem
/// to report, not this predicate's.
pub fn is_legacy_checkpoint(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.len() == 0 {
        return false;
    }
    let Ok(connection) = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return false;
    };
    let has = |name: &str| -> bool {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [name],
                |r| r.get::<_, bool>(0),
            )
            .unwrap_or(false)
    };
    has("objects") && !has("vectors")
}

fn allow_legacy_checkpoint() -> bool {
    std::env::var(ALLOW_LEGACY_ENV).is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Page-cache size for the checkpoint connection, in MiB.
///
/// [`CheckpointStore::restore`] walks ~12.6 M rows out of a multi-GB file.
/// SQLite's default cache is 2000 pages (8 MB at a 4 KB page size), which
/// cannot hold the interior levels of either b-tree — so nearly every row
/// costs a fresh `pread` of the `objects` tree. 256 MiB keeps those
/// interior levels resident and collapses most lookups to a single leaf read.
const CACHE_MIB_ENV: &str = "CONTEXTNEST_CHECKPOINT_CACHE_MIB";
const DEFAULT_CACHE_MIB: i64 = 256;

fn checkpoint_cache_mib() -> i64 {
    std::env::var(CACHE_MIB_ENV)
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_CACHE_MIB)
}

/// Page size for a *newly created* checkpoint, in bytes.
///
/// A full restore is bound by the number of page I/Os, not the byte count:
/// the operator volume is USB-attached and bills roughly 36 µs per I/O
/// whatever the transfer size, so a pass over the `objects` tree costs
/// `bytes / page_size` I/Os. Page size also decides whether a 4096-byte
/// vector fits inline — at 4096-byte pages every vector spills into its own
/// overflow page and costs a second I/O on top of the leaf read.
///
/// Measured on the 9.6 GB operator checkpoint, one cold pass per query:
///
/// ```text
/// page_size   basins JOIN   fragments JOIN   edges   vectors read   total
///      4096        28.5 s          39.6 s   18.6 s        59.0 s   ~87 s
///     65536         2.8 s           2.3 s   13.7 s         7.0 s   ~20 s
/// ```
///
/// SQLite ignores the pragma once a database has tables, so this only shapes
/// checkpoints that do not exist yet — an existing file keeps its page size
/// until it is rewritten by [`compact`].
const PAGE_SIZE_ENV: &str = "CONTEXTNEST_CHECKPOINT_PAGE_SIZE";
const DEFAULT_PAGE_SIZE: i64 = 65536;
/// SQLite requires a power of two in this range.
const MIN_PAGE_SIZE: i64 = 512;
const MAX_PAGE_SIZE: i64 = 65536;

fn checkpoint_page_size() -> i64 {
    std::env::var(PAGE_SIZE_ENV)
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v > 0 && v.count_ones() == 1 && (MIN_PAGE_SIZE..=MAX_PAGE_SIZE).contains(v))
        .unwrap_or(DEFAULT_PAGE_SIZE)
}

impl CheckpointStore {
    pub fn open(path: &Path, space: &str) -> Result<Self> {
        if is_legacy_checkpoint(path) && !allow_legacy_checkpoint() {
            return Err(format!(
                "{} is a pre-v0.2 checkpoint: vectors are still stored as inline JSON. \
                 Booting it would add a `vectors` table to your only pre-upgrade copy and \
                 load ~11 KB of JSON per vector into memory. Compact it into a new file \
                 first:\n    contextnest checkpoint compact --from {} --into {}.compacted.sqlite\n\
                 See docs/upgrading/v0.2.0.md. Set {ALLOW_LEGACY_ENV}=1 to adopt it in place anyway.",
                path.display(),
                path.display(),
                path.display(),
            )
            .into());
        }
        let lock = super::tenants::lock_database(path)?;
        let mut connection = Connection::open(path)?;
        // Must run before `SCHEMA` creates the first table: SQLite honours
        // `page_size` only on a database that has none, and ignores it
        // afterwards — which is the behaviour we want for an existing
        // checkpoint. See `checkpoint_page_size`.
        connection.execute_batch(&format!("PRAGMA page_size={};", checkpoint_page_size()))?;
        connection.execute_batch(SCHEMA)?;
        // SCHEMA leaves the cache at SQLite's 2000-page default; the restore
        // scan is the one place that matters. See `checkpoint_cache_mib`.
        connection.execute_batch(&format!(
            "PRAGMA cache_size=-{};PRAGMA temp_store=MEMORY;",
            checkpoint_cache_mib() * 1024
        ))?;
        let old: Option<String> = connection
            .query_row("SELECT space FROM identity LIMIT 1", [], |r| r.get(0))
            .optional()?;
        if old.as_deref() != Some(space) {
            let tx = connection.transaction()?;
            tx.execute_batch("DELETE FROM objects;DELETE FROM vectors;DELETE FROM completed;DELETE FROM identity;DELETE FROM retries;")?;
            tx.execute("INSERT INTO identity VALUES(?1)", [space])?;
            tx.commit()?;
        }
        Ok(Self {
            _lock: lock,
            connection: Mutex::new(connection),
        })
    }
    pub fn save(
        &self,
        id: &str,
        snapshot: CanonicalSnapshot,
        metadata: &HashMap<String, Value>,
    ) -> Result<()> {
        let mut db = self.connection.lock().unwrap();
        let tx = db.transaction()?;
        let deleted: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tombstones WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if deleted {
            return Err("fragment was deleted".into());
        }
        for mut fragment in snapshot.fragments {
            let vector = std::mem::take(&mut fragment.content);
            upsert_object(
                &tx,
                "fragment",
                &fragment.id,
                &serde_json::to_string(&fragment)?,
            )?;
            upsert_vector(&tx, "fragment", &fragment.id, &vector)?;
        }
        for mut basin in snapshot.basins {
            let center = std::mem::take(&mut basin.center);
            upsert_object(&tx, "basin", &basin.id, &serde_json::to_string(&basin)?)?;
            upsert_vector(&tx, "basin", &basin.id, &center)?;
        }
        for mut node in snapshot.nodes {
            // A node shares its fragment's vector; restore reuses that row.
            node.content = Vec::new();
            upsert_object(&tx, "node", &node.id, &serde_json::to_string(&node)?)?;
        }
        for edge in snapshot.edges {
            upsert_object(&tx, "edge", &edge.id, &serde_json::to_string(&edge)?)?;
        }
        tx.execute("INSERT INTO completed VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET metadata=excluded.metadata",params![id,serde_json::to_string(metadata)?])?;
        tx.execute("DELETE FROM retries WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Stream the canonical state into `emit` in dependency order without
    /// materialising it: basins are parsed first (their membership decides
    /// which fragments survive), then fragments, the filtered basins, nodes
    /// and edges follow in batches of [`RESTORE_BATCH`].
    ///
    /// A fragment survives when its text sidecar exists (`allowed`), it is
    /// not tombstoned, it has a node, and a basin lists it. Nodes, edges and
    /// basin membership are then restricted to surviving fragments.
    pub fn restore(
        &self,
        allowed: &HashSet<String>,
        mut emit: impl FnMut(RestoreBatch) -> Result<()>,
    ) -> Result<RestoreSummary> {
        // This phase runs for minutes on a large substrate and used to log
        // nothing between "vector arena is file-backed" and the final
        // "restored canonical operator checkpoint" — so a slow boot and a hung
        // boot looked identical from outside. Report stage transitions and a
        // throttled progress line instead.
        let started = std::time::Instant::now();
        let mut last_progress = started;
        let db = self.connection.lock().unwrap();
        let mut deleted = HashSet::new();
        for row in db
            .prepare("SELECT id FROM tombstones")?
            .query_map([], |r| r.get::<_, String>(0))?
        {
            deleted.insert(row?);
        }
        let mut node_ids = HashSet::new();
        for row in db
            .prepare("SELECT id FROM objects WHERE kind='node'")?
            .query_map([], |r| r.get::<_, String>(0))?
        {
            node_ids.insert(row?);
        }

        let mut basins: Vec<AttractorBasin> = Vec::new();
        let mut member_ids: HashSet<String> = HashSet::new();
        {
            let mut statement = db.prepare(
                "SELECT o.payload, v.data FROM objects o \
                 LEFT JOIN vectors v ON v.kind='basin' AND v.id=o.id WHERE o.kind='basin'",
            )?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let mut basin: AttractorBasin = serde_json::from_str(&row.get::<_, String>(0)?)?;
                if basin.center.is_empty() {
                    let Some(center) = row
                        .get::<_, Option<Vec<u8>>>(1)?
                        .as_deref()
                        .and_then(decode_vector)
                    else {
                        tracing::warn!(basin = %basin.id, "checkpoint basin has no vector; skipped");
                        continue;
                    };
                    basin.center = center;
                }
                member_ids.extend(basin.associated_fragments.iter().cloned());
                basins.push(basin);
            }
        }

        tracing::info!(
            basins = basins.len(),
            members = member_ids.len(),
            elapsed_s = started.elapsed().as_secs(),
            "checkpoint restore: basins loaded"
        );

        let mut kept: HashSet<String> = HashSet::new();
        let mut scanned: usize = 0;
        {
            let mut statement = db.prepare(
                "SELECT o.id, o.payload, v.data FROM objects o \
                 LEFT JOIN vectors v ON v.kind='fragment' AND v.id=o.id WHERE o.kind='fragment'",
            )?;
            let mut rows = statement.query([])?;
            let mut batch = Vec::with_capacity(RESTORE_BATCH);
            while let Some(row) = rows.next()? {
                let id: String = row.get(0)?;
                scanned += 1;
                if !allowed.contains(&id)
                    || deleted.contains(&id)
                    || !node_ids.contains(&id)
                    || !member_ids.contains(&id)
                {
                    continue;
                }
                let mut fragment: MemoryFragment = serde_json::from_str(&row.get::<_, String>(1)?)?;
                if fragment.content.is_empty() {
                    let Some(vector) = row
                        .get::<_, Option<Vec<u8>>>(2)?
                        .as_deref()
                        .and_then(decode_vector)
                    else {
                        tracing::warn!(fragment = %id, "checkpoint fragment has no vector; skipped");
                        continue;
                    };
                    fragment.content = vector;
                }
                kept.insert(id);
                batch.push(fragment);
                if batch.len() == RESTORE_BATCH {
                    emit(RestoreBatch::Fragments(std::mem::take(&mut batch)))?;
                    if last_progress.elapsed() >= std::time::Duration::from_secs(10) {
                        tracing::info!(
                            scanned,
                            kept = kept.len(),
                            elapsed_s = started.elapsed().as_secs(),
                            "checkpoint restore: fragments"
                        );
                        last_progress = std::time::Instant::now();
                    }
                }
            }
            if !batch.is_empty() {
                emit(RestoreBatch::Fragments(batch))?;
            }
        }
        drop(node_ids);
        drop(member_ids);

        tracing::info!(
            scanned,
            kept = kept.len(),
            basins = basins.len(),
            elapsed_s = started.elapsed().as_secs(),
            "checkpoint restore: reader finished streaming"
        );

        basins.retain_mut(|b| {
            b.associated_fragments.retain(|id| kept.contains(id));
            !b.associated_fragments.is_empty()
        });
        while !basins.is_empty() {
            let rest = basins.split_off(basins.len().saturating_sub(RESTORE_BATCH));
            emit(RestoreBatch::Basins(rest))?;
        }

        {
            let mut statement = db.prepare("SELECT id, payload FROM objects WHERE kind='node'")?;
            let mut rows = statement.query([])?;
            let mut batch = Vec::with_capacity(RESTORE_BATCH);
            let mut seen: usize = 0;
            while let Some(row) = rows.next()? {
                seen += 1;
                if !kept.contains(&row.get::<_, String>(0)?) {
                    continue;
                }
                let node: NodeRow = serde_json::from_str(&row.get::<_, String>(1)?)?;
                batch.push(MemoryNode::from(node));
                if batch.len() == RESTORE_BATCH {
                    emit(RestoreBatch::Nodes(std::mem::take(&mut batch)))?;
                    if last_progress.elapsed() >= std::time::Duration::from_secs(10) {
                        tracing::info!(
                            seen,
                            elapsed_s = started.elapsed().as_secs(),
                            "checkpoint restore: nodes"
                        );
                        last_progress = std::time::Instant::now();
                    }
                }
            }
            if !batch.is_empty() {
                emit(RestoreBatch::Nodes(batch))?;
            }
        }
        tracing::info!(
            elapsed_s = started.elapsed().as_secs(),
            "checkpoint restore: nodes done"
        );

        {
            let mut statement = db.prepare("SELECT payload FROM objects WHERE kind='edge'")?;
            let mut rows = statement.query([])?;
            let mut batch = Vec::with_capacity(RESTORE_BATCH);
            let mut seen: usize = 0;
            while let Some(row) = rows.next()? {
                seen += 1;
                let edge: ConnectionEdge = serde_json::from_str(&row.get::<_, String>(0)?)?;
                if !kept.contains(&edge.source) || !kept.contains(&edge.target) {
                    continue;
                }
                batch.push(edge);
                if batch.len() == RESTORE_BATCH {
                    emit(RestoreBatch::Edges(std::mem::take(&mut batch)))?;
                    if last_progress.elapsed() >= std::time::Duration::from_secs(10) {
                        let secs = started.elapsed().as_secs().max(1);
                        tracing::info!(
                            seen,
                            rows_per_s = seen as u64 / secs,
                            elapsed_s = secs,
                            "checkpoint restore: edges"
                        );
                        last_progress = std::time::Instant::now();
                    }
                }
            }
            if !batch.is_empty() {
                emit(RestoreBatch::Edges(batch))?;
            }
        }
        tracing::info!(
            elapsed_s = started.elapsed().as_secs(),
            "checkpoint restore: edges done"
        );

        let mut metadata = HashMap::new();
        for row in db
            .prepare("SELECT id,metadata FROM completed")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        {
            let (id, meta) = row?;
            if kept.contains(&id) {
                metadata.insert(id, serde_json::from_str(&meta)?);
            }
        }
        Ok(RestoreSummary {
            fragments: kept.len(),
            metadata,
            deleted,
        })
    }

    pub fn is_deleted(&self, id: &str) -> Result<bool> {
        Ok(self.connection.lock().unwrap().query_row(
            "SELECT EXISTS(SELECT 1 FROM tombstones WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?)
    }

    pub fn save_retry(
        &self,
        id: &str,
        state: Option<&super::consolidation::RetryState>,
    ) -> Result<()> {
        let db = self.connection.lock().unwrap();
        if let Some(state) = state {
            db.execute("INSERT INTO retries VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", params![id,serde_json::to_string(state)?])?;
        } else {
            db.execute("DELETE FROM retries WHERE id=?1", [id])?;
        }
        Ok(())
    }

    pub fn load_retries(&self) -> Result<HashMap<String, super::consolidation::RetryState>> {
        let db = self.connection.lock().unwrap();
        let mut states = HashMap::new();
        for row in db
            .prepare("SELECT id,payload FROM retries")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        {
            let (id, payload) = row?;
            states.insert(id, serde_json::from_str(&payload)?);
        }
        Ok(states)
    }

    pub fn forget(&self, id: &str) -> Result<()> {
        let mut db = self.connection.lock().unwrap();
        let tx = db.transaction()?;
        tx.execute("INSERT OR IGNORE INTO tombstones VALUES(?1)", [id])?;
        tx.execute("DELETE FROM completed WHERE id=?1", [id])?;
        tx.execute("DELETE FROM retries WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn tail(&self, id: &str) -> Result<Option<super::transcript_tail::Checkpoint>> {
        let db = self.connection.lock().unwrap();
        let payload: Option<String> = db
            .query_row("SELECT payload FROM tails WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        payload
            .map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn save_tail(
        &self,
        id: &str,
        checkpoint: &super::transcript_tail::Checkpoint,
    ) -> Result<()> {
        self.connection.lock().unwrap().execute("INSERT INTO tails VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![id,serde_json::to_string(checkpoint)?])?;
        Ok(())
    }
}

pub async fn bootstrap(
    services: &ContextNestServices,
    path: &Path,
) -> std::result::Result<(), String> {
    let path = path.to_owned();
    let space = format!(
        "{}:pipeline-{}",
        services.embedding.space_identity(),
        super::tenants::types::PIPELINE_VERSION
    );
    let checkpoint = compute::run(move || {
        CheckpointStore::open(&path, &space)
            .map(Arc::new)
            .map_err(|e| e.to_string())
    })
    .await??;

    if let Some(dir) = std::env::var_os("CONTEXTNEST_VECTOR_ARENA_DIR") {
        let dir = std::path::PathBuf::from(dir);
        services
            .attractor_manager
            .vector_arena()
            .attach_file(&dir, "operator-arena")
            .map_err(|e| e.to_string())?;
        tracing::info!(dir = %dir.display(), "vector arena is file-backed");
    }

    let allowed: HashSet<String> = services
        .fragment_texts
        .read()
        .await
        .keys()
        .cloned()
        .collect();

    // Stream the checkpoint straight into the manager: the SQLite reader
    // runs on a blocking thread and hands over bounded batches, so boot
    // never holds a second full copy of the canonical state.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<RestoreBatch>(4);
    let reader = checkpoint.clone();
    let load = compute::run(move || {
        reader
            .restore(&allowed, |batch| {
                tx.blocking_send(batch)
                    .map_err(|_| "restore consumer stopped".into())
            })
            .map_err(|e| e.to_string())
    });
    let manager = services.attractor_manager.clone();
    let apply = async move {
        while let Some(batch) = rx.recv().await {
            match batch {
                RestoreBatch::Fragments(fragments) => manager.restore_fragments(fragments).await,
                RestoreBatch::Basins(basins) => manager.restore_basins(basins).await,
                RestoreBatch::Nodes(nodes) => manager.restore_graph(nodes, Vec::new()),
                RestoreBatch::Edges(edges) => manager.restore_graph(Vec::new(), edges),
            }
        }
    };
    let (loaded, ()) = tokio::join!(load, apply);
    let summary = loaded??;

    let retry_reader = checkpoint.clone();
    let mut retries =
        compute::run(move || retry_reader.load_retries().map_err(|e| e.to_string())).await??;
    {
        let texts = services.fragment_texts.read().await;
        retries.retain(|id, _| texts.contains_key(id) && !summary.deleted.contains(id));
    }
    services.consolidation_queue.restore_retries(retries);
    services
        .fragment_metadata
        .write()
        .await
        .extend(summary.metadata);
    for id in summary.deleted {
        if let Some(session) = services.session_index.find_session(&id).await {
            services.session_index.hard_remove(&session, &id).await;
        }
        services.fragment_texts.write().await.remove(&id);
        services.fragment_metadata.write().await.remove(&id);
        // The active view is also scrubbed by the canonical absence at retrieval.
    }
    services
        .checkpoint
        .set(checkpoint)
        .map_err(|_| "checkpoint already initialized".to_owned())?;
    tracing::info!(
        fragments = summary.fragments,
        arena = ?services.attractor_manager.vector_arena().stats(),
        "restored canonical operator checkpoint without embedding"
    );
    Ok(())
}

pub async fn persist(
    services: &ContextNestServices,
    id: &str,
    basins: &[String],
    metadata: HashMap<String, Value>,
) -> std::result::Result<(), String> {
    if let Some(checkpoint) = services.checkpoint.get() {
        let snapshot = services
            .attractor_manager
            .durable_snapshot(Some(id), Some(basins))
            .await;
        if snapshot.fragments.is_empty()
            || snapshot.nodes.is_empty()
            || !snapshot
                .basins
                .iter()
                .any(|b| b.associated_fragments.contains(id))
        {
            return Err("canonical state is incomplete; completion was not persisted".into());
        }
        let checkpoint = checkpoint.clone();
        let id = id.to_owned();
        compute::run(move || {
            checkpoint
                .save(&id, snapshot, &metadata)
                .map_err(|e| e.to_string())
        })
        .await??;
    }
    Ok(())
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CompactReport {
    pub fragments: usize,
    pub basins: usize,
    pub nodes: usize,
    pub edges: usize,
    pub source_bytes: u64,
    pub output_bytes: u64,
}

/// Rewrite the checkpoint at `from` into the binary-vector format at `into`.
///
/// The source is opened read-only and never modified. It must not be in
/// use: a running server holds its lock, and a long read transaction would
/// pin its `-wal` file. `into` must not exist. Swap files manually after
/// checking the report.
pub fn compact(from: &Path, into: &Path) -> Result<CompactReport> {
    let _source_lock = super::tenants::lock_database(from).map_err(|_| {
        format!(
            "{} is in use (stop `contextnest serve` first)",
            from.display()
        )
    })?;
    if into.exists() {
        return Err(format!("{} already exists; refusing to overwrite", into.display()).into());
    }
    let source = Connection::open_with_flags(
        from,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let space: String = source.query_row("SELECT space FROM identity LIMIT 1", [], |r| r.get(0))?;
    let target = CheckpointStore::open(into, &space)?;
    let mut out = target.connection.lock().unwrap();
    // The output is a fresh file: a crash means re-running into a new path,
    // so durability per commit buys nothing. A large page cache keeps the
    // growing (kind,id) index resident; together with key-ordered reads
    // below, inserts become appends instead of random B-tree rewrites
    // (a 15 GB checkpoint went I/O-bound at ~50 MB/min without this).
    //
    // `journal_mode=MEMORY` matters as much as the cache. The target is
    // opened through `CheckpointStore::open`, which runs `SCHEMA` and so
    // inherits WAL — every write then lands in a `-wal` sidecar that grows
    // to roughly the size of the source and is checkpointed back into the
    // main file afterwards, doubling write volume. On a 9.6 GB source that
    // measured ~4 MB/s (240 MB main + 262 MB wal in two minutes). The
    // rollback journal is kept in memory instead; a crash still just means
    // deleting the partial output.
    out.execute_batch(
        "PRAGMA journal_mode=MEMORY;PRAGMA synchronous=OFF;\
         PRAGMA cache_size=-524288;PRAGMA temp_store=MEMORY;",
    )?;
    let mut report = CompactReport::default();

    for table in ["completed", "tombstones", "retries", "tails"] {
        let columns = if table == "tombstones" { 1 } else { 2 };
        let select = format!("SELECT * FROM {table}");
        let insert = if columns == 1 {
            format!("INSERT OR IGNORE INTO {table} VALUES(?1)")
        } else {
            format!("INSERT OR IGNORE INTO {table} VALUES(?1,?2)")
        };
        let tx = out.transaction()?;
        {
            let mut read = source.prepare(&select)?;
            let mut rows = read.query([])?;
            let mut write = tx.prepare(&insert)?;
            while let Some(row) = rows.next()? {
                if columns == 1 {
                    write.execute([row.get::<_, String>(0)?])?;
                } else {
                    write.execute([row.get::<_, String>(0)?, row.get::<_, String>(1)?])?;
                }
            }
        }
        tx.commit()?;
    }

    let has_vectors: bool = source.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='vectors')",
        [],
        |r| r.get(0),
    )?;
    if has_vectors {
        let tx = out.transaction()?;
        {
            let mut read =
                source.prepare("SELECT kind, id, data FROM vectors ORDER BY kind, id")?;
            let mut rows = read.query([])?;
            let mut write = tx.prepare("INSERT OR IGNORE INTO vectors VALUES(?1,?2,?3)")?;
            while let Some(row) = rows.next()? {
                write.execute(params![
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?
                ])?;
            }
        }
        tx.commit()?;
    }

    // Key order (the source's primary-key index) makes every output insert
    // an append to both the objects and vectors B-trees.
    let total: u64 = source.query_row("SELECT count(*) FROM objects", [], |r| r.get(0))?;
    let mut read = source.prepare("SELECT kind, id, payload FROM objects ORDER BY kind, id")?;
    let mut rows = read.query([])?;
    let mut tx = out.transaction()?;
    let mut in_tx = 0usize;
    let mut done = 0u64;
    while let Some(row) = rows.next()? {
        done += 1;
        if done % 250_000 == 0 {
            tracing::info!(done, total, "checkpoint compact progress");
        }
        let kind: String = row.get(0)?;
        let id: String = row.get(1)?;
        let payload: String = row.get(2)?;
        let field = match kind.as_str() {
            "fragment" => {
                report.fragments += 1;
                Some("content")
            }
            "node" => {
                report.nodes += 1;
                Some("content")
            }
            "basin" => {
                report.basins += 1;
                Some("center")
            }
            _ => {
                report.edges += usize::from(kind == "edge");
                None
            }
        };
        match field {
            Some(field) => {
                let mut value: Value = serde_json::from_str(&payload)?;
                if !value.is_object() {
                    return Err(format!("{kind} {id}: payload is not a JSON object").into());
                }
                let inline = value
                    .get_mut(field)
                    .map(Value::take)
                    .and_then(|v| serde_json::from_value::<Vec<f32>>(v).ok())
                    .unwrap_or_default();
                value[field] = Value::Array(Vec::new());
                upsert_object(&tx, &kind, &id, &serde_json::to_string(&value)?)?;
                if !inline.is_empty() && kind != "node" {
                    upsert_vector(&tx, &kind, &id, &inline)?;
                }
            }
            None => upsert_object(&tx, &kind, &id, &payload)?,
        }
        in_tx += 1;
        if in_tx == 10_000 {
            tx.commit()?;
            tx = out.transaction()?;
            in_tx = 0;
        }
    }
    tx.commit()?;
    out.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    drop(out);
    drop(target);
    report.source_bytes = std::fs::metadata(from)?.len();
    report.output_bytes = std::fs::metadata(into)?.len();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::attractors::connection_network::ConnectionType;

    fn fragment(id: &str, content: Vec<f32>) -> MemoryFragment {
        MemoryFragment {
            id: id.into(),
            content,
            importance: 0.5,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            attractor_basin_id: None,
            connections: HashSet::new(),
            confidence: 0.5,
        }
    }

    fn node(id: &str, content: Vec<f32>) -> MemoryNode {
        MemoryNode {
            id: id.into(),
            node_type: MemoryNodeType::Fragment,
            content,
            importance: 0.5,
            last_accessed: Utc::now(),
            access_frequency: 0.0,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![id.into()],
        }
    }

    fn basin(id: &str, center: Vec<f32>, members: &[&str]) -> AttractorBasin {
        use crate::memory::attractors::attractor_basin::BasinType;
        let mut basin = AttractorBasin::new(center, 1.0, 1.0, BasinType::Secondary).unwrap();
        basin.id = id.into();
        basin.associated_fragments = members.iter().map(|m| m.to_string()).collect();
        basin
    }

    fn edge(id: &str, source: &str, target: &str) -> ConnectionEdge {
        ConnectionEdge {
            id: id.into(),
            source: source.into(),
            target: target.into(),
            weight: 0.9,
            connection_type: ConnectionType::Semantic,
            strength: 0.9,
            bidirectional: true,
            created_at: Utc::now(),
            last_reinforced: Utc::now(),
            usage_count: 0,
        }
    }

    fn snapshot() -> CanonicalSnapshot {
        CanonicalSnapshot {
            fragments: vec![
                fragment("a", vec![1.0, 0.5]),
                fragment("b", vec![0.25, 2.0]),
            ],
            basins: vec![basin("basin-1", vec![0.5, 0.5], &["a", "b"])],
            nodes: vec![node("a", vec![1.0, 0.5]), node("b", vec![0.25, 2.0])],
            edges: vec![edge("e1", "a", "b")],
            norms: HashMap::new(),
        }
    }

    fn collect(store: &CheckpointStore, allowed: &[&str]) -> (CanonicalSnapshot, RestoreSummary) {
        let allowed = allowed.iter().map(|s| s.to_string()).collect();
        let mut out = CanonicalSnapshot::default();
        let summary = store
            .restore(&allowed, |batch| {
                match batch {
                    RestoreBatch::Fragments(v) => out.fragments.extend(v),
                    RestoreBatch::Basins(v) => out.basins.extend(v),
                    RestoreBatch::Nodes(v) => out.nodes.extend(v),
                    RestoreBatch::Edges(v) => out.edges.extend(v),
                }
                Ok(())
            })
            .unwrap();
        (out, summary)
    }

    #[test]
    fn binary_format_round_trips_and_stores_no_json_vectors() {
        let dir = tempfile::tempdir().unwrap();
        let store = CheckpointStore::open(&dir.path().join("c.sqlite"), "space").unwrap();
        store.save("a", snapshot(), &HashMap::new()).unwrap();

        let db = store.connection.lock().unwrap();
        let longest: i64 = db
            .query_row(
                "SELECT max(length(payload)) FROM objects WHERE kind!='basin'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let vectors: i64 = db
            .query_row("SELECT count(*) FROM vectors", [], |r| r.get(0))
            .unwrap();
        drop(db);
        assert!(
            longest < 400,
            "payloads carry no vector arrays ({longest} bytes)"
        );
        assert_eq!(vectors, 3, "two fragments + one basin, nodes share");

        let (restored, summary) = collect(&store, &["a", "b"]);
        assert_eq!(summary.fragments, 2);
        let mut contents: Vec<_> = restored
            .fragments
            .iter()
            .map(|f| f.content.clone())
            .collect();
        contents.sort_by(|x, y| x[0].total_cmp(&y[0]));
        assert_eq!(contents, vec![vec![0.25, 2.0], vec![1.0, 0.5]]);
        assert_eq!(restored.basins[0].center, vec![0.5, 0.5]);
        assert!(restored.nodes.iter().all(|n| n.content.is_empty()));
        assert_eq!(restored.edges.len(), 1);
    }

    #[test]
    fn legacy_inline_json_rows_still_load_and_compact_converts_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.sqlite");
        {
            let store = CheckpointStore::open(&path, "space").unwrap();
            let mut db = store.connection.lock().unwrap();
            let tx = db.transaction().unwrap();
            let snap = snapshot();
            for f in &snap.fragments {
                upsert_object(&tx, "fragment", &f.id, &serde_json::to_string(f).unwrap()).unwrap();
            }
            for b in &snap.basins {
                upsert_object(&tx, "basin", &b.id, &serde_json::to_string(b).unwrap()).unwrap();
            }
            for n in &snap.nodes {
                upsert_object(&tx, "node", &n.id, &serde_json::to_string(n).unwrap()).unwrap();
            }
            for e in &snap.edges {
                upsert_object(&tx, "edge", &e.id, &serde_json::to_string(e).unwrap()).unwrap();
            }
            tx.execute("INSERT INTO tombstones VALUES('gone')", [])
                .unwrap();
            tx.commit().unwrap();
        }
        {
            let legacy = CheckpointStore::open(&path, "space").unwrap();
            let (restored, _) = collect(&legacy, &["a", "b"]);
            assert_eq!(restored.fragments.len(), 2);
            assert_eq!(restored.basins[0].center, vec![0.5, 0.5]);
        }

        let into = dir.path().join("compact.sqlite");
        let report = compact(&path, &into).unwrap();
        assert_eq!(
            (report.fragments, report.basins, report.nodes, report.edges),
            (2, 1, 2, 1)
        );
        assert!(compact(&path, &into).is_err(), "never overwrites");

        let compacted = CheckpointStore::open(&into, "space").unwrap();
        assert!(compacted.is_deleted("gone").unwrap());
        let (restored, summary) = collect(&compacted, &["a", "b"]);
        assert_eq!(summary.fragments, 2);
        let a = restored.fragments.iter().find(|f| f.id == "a").unwrap();
        assert_eq!(a.content, vec![1.0, 0.5]);
        assert_eq!(restored.basins[0].center, vec![0.5, 0.5]);
        let db = compacted.connection.lock().unwrap();
        let inline: i64 = db
            .query_row(
                "SELECT count(*) FROM objects WHERE payload LIKE '%\"content\":[1%' OR payload LIKE '%\"center\":[0%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(inline, 0);
    }

    #[test]
    fn compact_refuses_a_checkpoint_in_use() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.sqlite");
        let _live = CheckpointStore::open(&path, "space").unwrap();
        let err = compact(&path, &dir.path().join("out.sqlite")).unwrap_err();
        assert!(err.to_string().contains("in use"), "{err}");
    }

    fn has_vectors_table(path: &Path) -> bool {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='vectors')",
                [],
                |r| r.get::<_, bool>(0),
            )
            .unwrap()
    }

    /// A whole-file pre-v0.2 checkpoint is refused by default: adopting it
    /// would write a `vectors` table into the operator's only pre-upgrade
    /// copy and load inline JSON vectors at ~11 KB each.
    #[test]
    fn legacy_checkpoint_is_refused_unless_explicitly_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v0_1.sqlite");
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch(
                "CREATE TABLE identity(space TEXT NOT NULL);
                 CREATE TABLE objects(kind TEXT,id TEXT,payload TEXT NOT NULL,PRIMARY KEY(kind,id));
                 INSERT INTO identity VALUES('space');
                 INSERT INTO objects VALUES('fragment','a','{\"id\":\"a\",\"content\":[1.0,0.5]}');",
            )
            .unwrap();
        }
        assert!(is_legacy_checkpoint(&path));
        assert!(!has_vectors_table(&path));

        std::env::remove_var(ALLOW_LEGACY_ENV);
        let message = match CheckpointStore::open(&path, "space") {
            Ok(_) => panic!("a pre-v0.2 checkpoint must not be adopted by default"),
            Err(err) => err.to_string(),
        };
        assert!(message.contains("pre-v0.2"), "{message}");
        assert!(
            !has_vectors_table(&path),
            "a refused boot must leave the source untouched"
        );

        std::env::set_var(ALLOW_LEGACY_ENV, "1");
        let store = CheckpointStore::open(&path, "space").unwrap();
        std::env::remove_var(ALLOW_LEGACY_ENV);
        assert!(has_vectors_table(&path), "the override adopts it in place");
        drop(store);
    }

    #[test]
    fn fresh_and_v0_2_checkpoints_are_not_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join("fresh.sqlite");
        assert!(
            !is_legacy_checkpoint(&fresh),
            "a missing file is not legacy"
        );
        let store = CheckpointStore::open(&fresh, "space").unwrap();
        store.save("a", snapshot(), &HashMap::new()).unwrap();
        assert!(!is_legacy_checkpoint(&fresh), "a v0.2 file is not legacy");
    }

    /// Serialises the tests that read or write [`PAGE_SIZE_ENV`]. Every
    /// `CheckpointStore::open` consults it, so a concurrent override would
    /// change the page size another test asserts on.
    fn page_size_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn read_page_size(path: &Path) -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row("PRAGMA page_size", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn a_fresh_checkpoint_uses_the_large_page_size() {
        // Restore is bound by page I/O count, not bytes: the operator volume
        // bills ~36 µs per I/O whatever the transfer size, and a 4096-byte
        // vector only fits inline on a page larger than 4096. A regression
        // here silently multiplies every boot's I/O by 16.
        let _guard = page_size_env_lock();
        std::env::remove_var(PAGE_SIZE_ENV);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pages.sqlite");
        drop(CheckpointStore::open(&path, "space").unwrap());
        assert_eq!(read_page_size(&path), DEFAULT_PAGE_SIZE);
        assert_eq!(DEFAULT_PAGE_SIZE, 65536);
    }

    #[test]
    fn page_size_override_is_validated_before_it_reaches_sqlite() {
        let _guard = page_size_env_lock();
        std::env::set_var(PAGE_SIZE_ENV, "8192");
        assert_eq!(checkpoint_page_size(), 8192);
        // Neither of these is a legal SQLite page size; both must fall back
        // rather than produce a checkpoint that cannot be opened.
        std::env::set_var(PAGE_SIZE_ENV, "6000"); // not a power of two
        assert_eq!(checkpoint_page_size(), DEFAULT_PAGE_SIZE);
        std::env::set_var(PAGE_SIZE_ENV, "131072"); // above the 64 KiB ceiling
        assert_eq!(checkpoint_page_size(), DEFAULT_PAGE_SIZE);
        std::env::remove_var(PAGE_SIZE_ENV);
    }

    #[test]
    fn compact_writes_the_large_page_size() {
        // `compact` is the only way an existing 4 KiB checkpoint becomes a
        // 64 KiB one, since SQLite ignores the pragma on a populated database.
        let _guard = page_size_env_lock();
        std::env::remove_var(PAGE_SIZE_ENV);
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.sqlite");
        let into = dir.path().join("into.sqlite");
        let store = CheckpointStore::open(&from, "space").unwrap();
        store.save("a", snapshot(), &HashMap::new()).unwrap();
        drop(store);
        compact(&from, &into).unwrap();
        assert_eq!(read_page_size(&into), DEFAULT_PAGE_SIZE);
    }

    #[test]
    fn restore_filters_unlisted_deleted_and_orphaned_rows() {
        let dir = tempfile::tempdir().unwrap();
        let store = CheckpointStore::open(&dir.path().join("c.sqlite"), "space").unwrap();
        store.save("a", snapshot(), &HashMap::new()).unwrap();
        store.forget("b").unwrap();
        let (restored, summary) = collect(&store, &["a", "b"]);
        assert_eq!(summary.fragments, 1);
        assert!(summary.deleted.contains("b"));
        assert_eq!(restored.nodes.len(), 1);
        assert!(
            restored.edges.is_empty(),
            "edge to a deleted fragment is dropped"
        );
        assert_eq!(
            restored.basins[0].associated_fragments,
            HashSet::from(["a".to_string()])
        );
    }
}
