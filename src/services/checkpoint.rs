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

/// Rows of raw payloads read per parallel parse wave.
///
/// A wave costs `workers - 1` thread spawns, so bigger waves mean fewer of
/// them; smaller waves hold less memory, since a wave keeps the raw JSON and
/// the parsed structs alive side by side. 16 384 rows is ~5.5 MB of edge JSON
/// at the measured 336 bytes per edge — small next to the 2.9 GB the restored
/// substrate already occupies.
const PARSE_WAVE: usize = 16384;

/// How many edges share one `edge_packs` row.
///
/// A restore is bound by how many rows SQLite *steps*, not by bytes. Measured
/// against the live checkpoint: stepping 11.4 M edge rows and fetching their
/// payloads costs 42-51 s, while stepping the same rows through the
/// index alone (no payload) costs 6.7 s and the payloads are only 3.9 GB —
/// about 6 s of raw I/O. The cost is per row, at roughly 2.5 us, which is why
/// packing 64 edges into each row cuts the edge pass by that factor. The
/// consumed bytes do not change; only how often SQLite's per-row VM runs.
///
/// At ~336 bytes per edge, 64 edges is ~21 KB of JSON — comfortably inside the
/// inline limit of a 64 KiB page, so a pack does not spill to an overflow page.
const EDGES_PER_PACK: usize = 64;

/// Flush a pack early once its JSON passes this. Individual edges are not size
/// bounded, so a run of large ones must not push a pack past the inline limit.
const EDGE_PACK_MAX_BYTES: usize = 48 * 1024;

/// Widest fan-out a parse wave will use, whatever the core count.
///
/// A wave turns allocator- and memory-bandwidth-bound well before it turns
/// core-bound, and the consumer runs on the same machine at the same time, so
/// this stays clear of the core count on a large box.
const PARSE_MAX_WORKERS: usize = 8;

/// Overrides the parse fan-out. `1` restores single-threaded behaviour, which
/// is the escape hatch if the fan-out ever regresses.
const PARSE_WORKERS_ENV: &str = "CONTEXTNEST_RESTORE_PARSE_WORKERS";

/// Threads to fan a parse wave across.
fn parse_workers() -> usize {
    std::env::var(PARSE_WORKERS_ENV)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        })
        .clamp(1, PARSE_MAX_WORKERS)
}

/// Lift up to [`PARSE_WAVE`] rows out of `rows` with `take`.
///
/// `take` returns `Ok(None)` for a row the caller can reject before the parse
/// — a cheap set lookup — so a rejected row never has its payload allocated.
/// Returns the surviving rows and how many rows were stepped; the two differ
/// when `take` rejects, and the progress log wants the raw row count.
///
/// Rows are owned because the parse runs on other threads and a borrowed
/// `Row` cannot outlive the step that produced it.
fn read_wave<R>(
    rows: &mut rusqlite::Rows<'_>,
    mut take: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<Option<R>>,
) -> Result<(Vec<R>, usize)> {
    let mut wave = Vec::with_capacity(PARSE_WAVE);
    let mut seen = 0usize;
    while seen < PARSE_WAVE {
        let Some(row) = rows.next()? else { break };
        seen += 1;
        if let Some(item) = take(row)? {
            wave.push(item);
        }
    }
    Ok((wave, seen))
}

/// Map `parse` over a wave of already-read rows on `workers` threads, in order.
///
/// The wave item is whatever the caller lifted out of the row — a payload
/// string for edges, a payload plus a vector blob for basins and fragments —
/// because rusqlite's `Rows` borrows the statement and cannot cross a thread
/// boundary. The read therefore stays on the calling thread and only the parse
/// fans out.
///
/// `parse` returns `Ok(None)` to drop an item, which is how the edge pass
/// applies its `kept` filter without a second pass over the wave.
///
/// This is deliberately *not* sold as the fix for the edge pass. Probing the
/// live checkpoint (`probe_edge_reader_without_apply`) showed the parse is
/// nearly free next to the per-row SQLite step: reading 11.4 M edge rows
/// without parsing at all takes 65 s against 62 s with it, and eight threads
/// bought only 1.2x. The fan-out earns its keep where the parse is a real
/// share of the work — a packed wave item is a 64-edge JSON array rather than
/// a single edge, and the fragment and basin payloads are large — and it costs
/// little where it is not.
///
/// Order is preserved: the wave is split into contiguous slices and the slices
/// are collected in spawn order.
fn map_wave_parallel<I, T, F>(raw: Vec<I>, workers: usize, parse: F) -> Result<Vec<T>>
where
    // `Sync`, not just `Send`: each worker borrows a slice of the wave, so
    // `&[I]` has to cross the thread boundary.
    I: Send + Sync,
    T: Send,
    F: Fn(&I) -> Result<Option<T>> + Sync,
{
    if workers <= 1 || raw.len() < 2 {
        let mut out = Vec::with_capacity(raw.len());
        for payload in &raw {
            if let Some(item) = parse(payload)? {
                out.push(item);
            }
        }
        return Ok(out);
    }

    let parse = &parse;
    let slice_len = raw.len().div_ceil(workers);
    let mut parsed: Vec<Vec<T>> = Vec::with_capacity(workers);
    std::thread::scope(|scope| -> Result<()> {
        let handles: Vec<_> = raw
            .chunks(slice_len)
            .map(|slice| {
                scope.spawn(move || -> Result<Vec<T>> {
                    let mut out = Vec::with_capacity(slice.len());
                    for payload in slice {
                        if let Some(item) = parse(payload)? {
                            out.push(item);
                        }
                    }
                    Ok(out)
                })
            })
            .collect();
        for handle in handles {
            parsed.push(
                handle
                    .join()
                    .map_err(|_| "parse worker panicked".to_string())??,
            );
        }
        Ok(())
    })?;
    Ok(parsed.into_iter().flatten().collect())
}

/// Edges only, with the `kept` filter applied after the parse.
fn parse_edges_across_cores(
    raw: Vec<String>,
    workers: usize,
    kept: &HashSet<String>,
) -> Result<Vec<ConnectionEdge>> {
    map_wave_parallel(raw, workers, |payload| {
        let edge: ConnectionEdge = serde_json::from_str(payload)?;
        Ok((kept.contains(&edge.source) && kept.contains(&edge.target)).then_some(edge))
    })
}

/// One [`EDGES_PER_PACK`]-sized array per row, filtered after the parse.
fn parse_packs_across_cores(
    raw: Vec<String>,
    workers: usize,
    kept: &HashSet<String>,
) -> Result<Vec<ConnectionEdge>> {
    let packs: Vec<Vec<ConnectionEdge>> = map_wave_parallel(raw, workers, |payload| {
        let pack: Vec<ConnectionEdge> = serde_json::from_str(payload)?;
        Ok(Some(
            pack.into_iter()
                .filter(|edge| kept.contains(&edge.source) && kept.contains(&edge.target))
                .collect(),
        ))
    })?;
    Ok(packs.into_iter().flatten().collect())
}

/// Drain raw edge payload rows into `emit` in [`RESTORE_BATCH`] chunks.
///
/// `parse` receives a whole wave, which is where the two edge sources differ:
/// a packed row holds an array of edges, a leftover `objects` row holds one.
fn stream_edge_rows<F>(
    rows: &mut rusqlite::Rows<'_>,
    workers: usize,
    mut parse: F,
    batch: &mut Vec<ConnectionEdge>,
    emit: &mut impl FnMut(RestoreBatch) -> Result<()>,
) -> Result<usize>
where
    F: FnMut(Vec<String>, usize) -> Result<Vec<ConnectionEdge>>,
{
    let mut wave: Vec<String> = Vec::with_capacity(PARSE_WAVE);
    let mut seen = 0usize;
    while let Some(row) = rows.next()? {
        seen += 1;
        wave.push(row.get(0)?);
        if wave.len() == PARSE_WAVE {
            push_edges(parse(std::mem::take(&mut wave), workers)?, batch, emit)?;
            wave.reserve(PARSE_WAVE);
        }
    }
    push_edges(parse(std::mem::take(&mut wave), workers)?, batch, emit)?;
    Ok(seen)
}

fn push_edges(
    edges: Vec<ConnectionEdge>,
    batch: &mut Vec<ConnectionEdge>,
    emit: &mut impl FnMut(RestoreBatch) -> Result<()>,
) -> Result<()> {
    for edge in edges {
        batch.push(edge);
        if batch.len() == RESTORE_BATCH {
            emit(RestoreBatch::Edges(std::mem::take(batch)))?;
        }
    }
    Ok(())
}

/// Split a run of serialised edges into pack payloads.
///
/// The edges arrive as individual JSON objects and are joined into a JSON
/// array, so nothing is parsed and re-serialised on the way in — the bytes
/// written are exactly the bytes read. `None` ends a pack early at
/// [`EDGES_PER_PACK`] *or* [`EDGE_PACK_MAX_BYTES`], whichever comes first.
struct EdgePacker {
    buf: Vec<String>,
    bytes: usize,
}

impl EdgePacker {
    fn new() -> Self {
        Self {
            buf: Vec::with_capacity(EDGES_PER_PACK),
            bytes: 0,
        }
    }

    /// Add one serialised edge; returns a finished pack when one fills up.
    fn push(&mut self, payload: String) -> Option<String> {
        if self.buf.is_empty() {
            self.bytes = 2; // the surrounding []
        }
        self.bytes += payload.len() + 1; // + the joining comma
        self.buf.push(payload);
        if self.buf.len() >= EDGES_PER_PACK || self.bytes >= EDGE_PACK_MAX_BYTES {
            self.take()
        } else {
            None
        }
    }

    /// Finish the current pack, if any.
    fn take(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            return None;
        }
        self.bytes = 0;
        Some(format!(
            "[{}]",
            self.buf.drain(..).collect::<Vec<_>>().join(",")
        ))
    }
}

/// Set to `1` to let [`CheckpointStore::open`] adopt a pre-v0.2 checkpoint in
/// place. Off by default — see [`is_legacy_checkpoint`].
const ALLOW_LEGACY_ENV: &str = "CONTEXTNEST_ALLOW_LEGACY_CHECKPOINT";

const SCHEMA: &str = "PRAGMA journal_mode=WAL;PRAGMA synchronous=FULL;
    CREATE TABLE IF NOT EXISTS identity(space TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS objects(kind TEXT,id TEXT,payload TEXT NOT NULL,PRIMARY KEY(kind,id));
    CREATE TABLE IF NOT EXISTS edge_packs(seq INTEGER PRIMARY KEY,payload TEXT NOT NULL);
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
        // One decision for the whole restore: the row read stays on this
        // thread (rusqlite's `Rows` borrows the statement), only the parse
        // fans out.
        let workers = parse_workers();
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
            loop {
                let (wave, seen) = read_wave(&mut rows, |row| {
                    Ok(Some((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<Vec<u8>>>(1)?,
                    )))
                })?;
                if seen == 0 {
                    break;
                }
                let parsed: Vec<AttractorBasin> = map_wave_parallel(
                    wave,
                    workers,
                    |(payload, data)| {
                        let mut basin: AttractorBasin = serde_json::from_str(payload)?;
                        if basin.center.is_empty() {
                            let Some(center) = data.as_deref().and_then(decode_vector) else {
                                tracing::warn!(basin = %basin.id, "checkpoint basin has no vector; skipped");
                                return Ok(None);
                            };
                            basin.center = center;
                        }
                        Ok(Some(basin))
                    },
                )?;
                // Members are folded in on this thread so the `kept` filter
                // downstream sees the same set regardless of worker count.
                for basin in parsed {
                    member_ids.extend(basin.associated_fragments.iter().cloned());
                    basins.push(basin);
                }
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
            loop {
                // The four id filters are set lookups, so they run inside the
                // read: a fragment that fails them never has its payload
                // allocated, let alone parsed.
                let (wave, seen) = read_wave(&mut rows, |row| {
                    let id: String = row.get(0)?;
                    if !allowed.contains(&id)
                        || deleted.contains(&id)
                        || !node_ids.contains(&id)
                        || !member_ids.contains(&id)
                    {
                        return Ok(None);
                    }
                    Ok(Some((
                        id,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<Vec<u8>>>(2)?,
                    )))
                })?;
                if seen == 0 {
                    break;
                }
                scanned += seen;
                let parsed: Vec<(String, MemoryFragment)> = map_wave_parallel(
                    wave,
                    workers,
                    |(id, payload, data)| {
                        let mut fragment: MemoryFragment = serde_json::from_str(payload)?;
                        if fragment.content.is_empty() {
                            let Some(vector) = data.as_deref().and_then(decode_vector) else {
                                tracing::warn!(fragment = %id, "checkpoint fragment has no vector; skipped");
                                return Ok(None);
                            };
                            fragment.content = vector;
                        }
                        Ok(Some((id.clone(), fragment)))
                    },
                )?;
                // `kept` is what nodes, basin membership and edges are later
                // filtered against, so it is filled here, in wave order, on
                // the one thread that owns it.
                for (id, fragment) in parsed {
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
            // Two sources, and the unpacked delta goes first: an edge that a
            // later `save` rewrote must not be shadowed by its older packed
            // copy. `restore_edge` drops a duplicate by id against the source
            // node's incident list, so whichever is inserted first is kept.
            let packed: bool =
                db.query_row("SELECT EXISTS(SELECT 1 FROM edge_packs)", [], |r| r.get(0))?;
            tracing::info!(workers, packed, "checkpoint restore: edges starting");
            let mut batch: Vec<ConnectionEdge> = Vec::with_capacity(RESTORE_BATCH);

            if packed {
                let mut statement = db.prepare("SELECT payload FROM edge_packs ORDER BY seq")?;
                let mut rows = statement.query([])?;
                let packs = stream_edge_rows(
                    &mut rows,
                    workers,
                    |wave, w| parse_packs_across_cores(wave, w, &kept),
                    &mut batch,
                    &mut emit,
                )?;
                tracing::info!(
                    packs,
                    elapsed_s = started.elapsed().as_secs(),
                    "checkpoint restore: edge packs done"
                );
            }

            // Edges written since the last compaction are still one row each in
            // `objects`. Empty on a freshly compacted checkpoint, where the
            // index seek that finds nothing costs nothing.
            let mut statement = db.prepare("SELECT payload FROM objects WHERE kind='edge'")?;
            let mut rows = statement.query([])?;
            let delta = stream_edge_rows(
                &mut rows,
                workers,
                |wave, w| parse_edges_across_cores(wave, w, &kept),
                &mut batch,
                &mut emit,
            )?;
            if delta > 0 {
                tracing::info!(delta, "checkpoint restore: unpacked edge delta");
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
    let mut packer = EdgePacker::new();
    let mut pack_seq: u64 = 0;
    while let Some(row) = rows.next()? {
        done += 1;
        if done % 250_000 == 0 {
            tracing::info!(done, total, "checkpoint compact progress");
        }
        let kind: String = row.get(0)?;
        let id: String = row.get(1)?;
        let payload: String = row.get(2)?;

        // Edges leave `objects` altogether. They are 90% of the rows and the
        // whole of restore's per-row cost, and nothing reads an edge by id —
        // `restore` only ever streams them — so they go into `edge_packs` 64
        // to a row. See `EDGES_PER_PACK` for the measurements behind that.
        if kind == "edge" {
            report.edges += 1;
            if let Some(pack) = packer.push(payload) {
                tx.execute(
                    "INSERT INTO edge_packs(seq,payload) VALUES(?1,?2)",
                    params![pack_seq, pack],
                )?;
                pack_seq += 1;
                in_tx += 1;
                if in_tx == 10_000 {
                    tx.commit()?;
                    tx = out.transaction()?;
                    in_tx = 0;
                }
            }
            continue;
        }

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
            _ => None,
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
    if let Some(pack) = packer.take() {
        tx.execute(
            "INSERT INTO edge_packs(seq,payload) VALUES(?1,?2)",
            params![pack_seq, pack],
        )?;
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

    /// A snapshot with `nodes` nodes and `nodes * fan` edges among them, so a
    /// compaction has more than one pack's worth of edges to move. No edge is a
    /// self-loop: `i` and `i * 7 + 1` mod `nodes` are never equal, since
    /// `6i = -1 (mod nodes)` has no solution.
    ///
    /// Every node also gets a fragment of the same id and membership in one
    /// basin — `restore` derives its `kept` set from fragments that are both
    /// node-backed and basin-attached, so a bare node/edge graph restores as
    /// nothing.
    fn wide_snapshot(nodes: usize, fan: usize) -> CanonicalSnapshot {
        let (nodes_vec, fragments): (Vec<_>, Vec<_>) = (0..nodes)
            .map(|i| {
                let id = format!("n{i}");
                let content = vec![i as f32, 1.0];
                (node(&id, content.clone()), fragment(&id, content))
            })
            .unzip();
        let members: Vec<String> = (0..nodes).map(|i| format!("n{i}")).collect();
        let member_refs: Vec<&str> = members.iter().map(|s| s.as_str()).collect();
        let edges = (0..nodes * fan)
            .map(|i| {
                edge(
                    &format!("e{i}"),
                    &format!("n{}", i % nodes),
                    &format!("n{}", (i * 7 + 1) % nodes),
                )
            })
            .collect();
        CanonicalSnapshot {
            fragments,
            basins: vec![basin("basin-all", vec![0.0, 0.0], &member_refs)],
            nodes: nodes_vec,
            edges,
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
    fn edge_packer_emits_json_arrays_of_at_most_one_pack() {
        let mut packer = EdgePacker::new();
        let mut packs = Vec::new();
        // One short of a full pack: nothing is emitted yet.
        for i in 0..EDGES_PER_PACK - 1 {
            assert!(packer.push(format!("{{\"i\":{i}}}")).is_none());
        }
        // The pack-full edge flushes, and the next edge starts a fresh pack.
        let full = packer
            .push(format!("{{\"i\":{}}}", EDGES_PER_PACK - 1))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<serde_json::Value>>(&full)
                .unwrap()
                .len(),
            EDGES_PER_PACK
        );
        assert!(packer.push("{\"i\":\"tail\"}".to_string()).is_none());
        packs.push(packer.take().unwrap());
        assert_eq!(
            serde_json::from_str::<Vec<serde_json::Value>>(&packs[0])
                .unwrap()
                .len(),
            1,
            "a partial pack holds only what was pushed into it"
        );
        assert!(packer.take().is_none(), "an emptied packer yields nothing");
    }

    #[test]
    fn edge_packer_flushes_early_when_a_pack_would_outgrow_the_page() {
        // Individual edges are not size bounded, so one oversized payload must
        // not be left to accumulate siblings past the inline page limit.
        let mut packer = EdgePacker::new();
        let big = "x".repeat(EDGE_PACK_MAX_BYTES);
        assert!(packer.push(big).is_some(), "crossing the byte cap flushes");
    }

    #[test]
    fn compact_moves_edges_into_packs_and_restore_reassembles_them() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.sqlite");
        let into = dir.path().join("into.sqlite");
        // 200 nodes * 5 = 1000 edges, i.e. 16 packs at 64 edges per pack.
        let snapshot = wide_snapshot(200, 5);
        let expected: HashSet<String> = snapshot.edges.iter().map(|e| e.id.clone()).collect();
        let store = CheckpointStore::open(&from, "space").unwrap();
        store.save("all", snapshot, &HashMap::new()).unwrap();
        drop(store);

        compact(&from, &into).unwrap();

        let db = Connection::open(&into).unwrap();
        let unpacked: i64 = db
            .query_row("SELECT count(*) FROM objects WHERE kind='edge'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let packs: i64 = db
            .query_row("SELECT count(*) FROM edge_packs", [], |r| r.get(0))
            .unwrap();
        let first: String = db
            .query_row(
                "SELECT payload FROM edge_packs ORDER BY seq LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        drop(db);

        assert_eq!(unpacked, 0, "every edge moved out of `objects` into a pack");
        assert_eq!(packs, (expected.len()).div_ceil(EDGES_PER_PACK) as i64);
        assert!(
            first.starts_with('[') && first.ends_with(']'),
            "a pack is a JSON array of edges"
        );
        assert_eq!(
            serde_json::from_str::<Vec<ConnectionEdge>>(&first)
                .unwrap()
                .len(),
            EDGES_PER_PACK
        );

        let allowed: Vec<String> = (0..200).map(|i| format!("n{i}")).collect();
        let refs: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
        let restored = CheckpointStore::open(&into, "space").unwrap();
        let (out, summary) = collect(&restored, &refs);

        assert_eq!(
            out.edges.len(),
            expected.len(),
            "no edge lost across the pack"
        );
        assert_eq!(summary.fragments, 200, "the node-backed fragments restore");
        let got: HashSet<String> = out.edges.iter().map(|e| e.id.clone()).collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn worker_count_does_not_change_what_restores() {
        // Every wave source — fragments, basins, packed edges — must produce
        // the same state whether the parse ran serially or fanned out. The
        // filters (four id lookups for fragments, `kept` for edges) run on
        // different sides of the fan-out, so this is the test that catches a
        // filter that started depending on worker count.
        let _guard = edge_parse_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.sqlite");
        let store = CheckpointStore::open(&from, "space").unwrap();
        store
            .save("all", wide_snapshot(200, 5), &HashMap::new())
            .unwrap();
        drop(store);

        let allowed: Vec<String> = (0..200).map(|i| format!("n{i}")).collect();
        let refs: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();

        let mut runs = Vec::new();
        for workers in ["1", "8"] {
            std::env::set_var(PARSE_WORKERS_ENV, workers);
            assert_eq!(parse_workers(), workers.parse::<usize>().unwrap());
            let store = CheckpointStore::open(&from, "space").unwrap();
            let (out, summary) = collect(&store, &refs);
            let mut fragment_ids: Vec<String> =
                out.fragments.iter().map(|f| f.id.clone()).collect();
            fragment_ids.sort();
            runs.push((
                summary.fragments,
                fragment_ids,
                out.basins.len(),
                out.nodes.len(),
                out.edges.len(),
            ));
        }
        std::env::remove_var(PARSE_WORKERS_ENV);

        assert_eq!(runs[0], runs[1], "worker count must not change the result");
        assert_eq!(
            runs[0].0, 200,
            "the fixture's fragments must actually reach the restore"
        );
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

    /// Measures the edge pass of [`CheckpointStore::restore`] **without** the
    /// consumer — single-threaded against fanned-out — to size the fix.
    ///
    /// The boot's reader plateaus at ~162 k rows/s and the edge stage costs 48 s
    /// of an 80 s boot, on an 11-core box near load 5. That is one of two very
    /// different problems: the reader *is* the limit (SQLite blob -> `String`
    /// -> `serde_json`, and fanning out the parse helps), or the reader is only
    /// matching a slower `restore_graph` (and fanning out does nothing). The
    /// first run of this probe settled it for the reader: with a warm page
    /// cache it ran 2.2x *slower* than cold, which no I/O-bound loop does.
    ///
    /// This drives the real [`parse_edges_across_cores`] rather than a copy, so
    /// the parallel figure is a direct prediction of the new boot. Rounds
    /// alternate single/parallel so drift in machine load hits both arms
    /// equally — absolute pass numbers are not comparable across a minutes-long
    /// run on a shared box.
    ///
    /// ```text
    /// CONTEXTNEST_PROBE_CHECKPOINT=<file> cargo test --lib --release \
    ///   probe_edge_reader_without_apply -- --ignored --nocapture
    /// ```
    ///
    /// Read-only, and `#[ignore]`d so no CI run ever needs a multi-GB file.
    #[test]
    #[ignore = "needs CONTEXTNEST_PROBE_CHECKPOINT pointed at a real checkpoint"]
    fn probe_edge_reader_without_apply() {
        let Ok(path) = std::env::var("CONTEXTNEST_PROBE_CHECKPOINT") else {
            eprintln!("set CONTEXTNEST_PROBE_CHECKPOINT to a checkpoint path");
            return;
        };
        let db = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open checkpoint read-only");

        // The real filter set, so the fan-out pays the same hash lookups the
        // boot pays.
        let kept: HashSet<String> = db
            .prepare("SELECT id FROM objects WHERE kind='node'")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        let workers = parse_workers();
        println!("kept={} parse_workers()={workers}", kept.len());

        for round in 1..=2 {
            for mode in [
                EdgeMode::BorrowOnly,
                EdgeMode::CopyOnly,
                EdgeMode::ParseSingle,
                EdgeMode::ParseParallel(workers),
            ] {
                let (seen, secs) = drain_edges(&db, &kept, mode);
                println!(
                    "round {round} [{:<16}]: seen={seen} in {secs:.1}s => {:.0} rows/s",
                    mode.label(),
                    seen as f64 / secs
                );
            }
        }
    }

    /// How much of the edge loop one probe arm performs.
    #[derive(Clone, Copy)]
    enum EdgeMode {
        /// Step the row and read the blob through SQLite's own buffer:
        /// no allocation, no copy, no parse. Isolates the scan itself.
        BorrowOnly,
        /// Copy every blob into an owned `String`. Adds what `restore` pays
        /// before it parses anything.
        CopyOnly,
        /// Copy and parse on one thread — `restore` as it was.
        ParseSingle,
        /// Copy on one thread, parse on `n`.
        ParseParallel(usize),
    }

    impl EdgeMode {
        fn label(self) -> &'static str {
            match self {
                EdgeMode::BorrowOnly => "borrow only",
                EdgeMode::CopyOnly => "copy only",
                EdgeMode::ParseSingle => "parse single",
                EdgeMode::ParseParallel(_) => "parse parallel",
            }
        }
    }

    /// One pass over every edge row in the given mode, returning
    /// `(rows seen, seconds)`. Wave size and filter match
    /// [`CheckpointStore::restore`].
    fn drain_edges(db: &Connection, kept: &HashSet<String>, mode: EdgeMode) -> (usize, f64) {
        let workers = match mode {
            EdgeMode::ParseParallel(w) => w,
            _ => 1,
        };
        let started = std::time::Instant::now();
        let mut seen = 0usize;
        let mut sink = 0usize;
        let mut wave: Vec<String> = Vec::with_capacity(PARSE_WAVE);
        {
            let mut statement = db
                .prepare("SELECT payload FROM objects WHERE kind='edge'")
                .unwrap();
            let mut rows = statement.query([]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                seen += 1;
                if matches!(mode, EdgeMode::BorrowOnly) {
                    if let rusqlite::types::ValueRef::Text(bytes) = row.get_ref(0).unwrap() {
                        sink += bytes.len();
                    }
                    continue;
                }
                wave.push(row.get::<_, String>(0).unwrap());
                if wave.len() == PARSE_WAVE {
                    sink = sink.wrapping_add(flush_wave(
                        std::mem::take(&mut wave),
                        workers,
                        kept,
                        mode,
                    ));
                    wave.reserve(PARSE_WAVE);
                }
            }
        }
        sink = sink.wrapping_add(flush_wave(wave, workers, kept, mode));
        std::hint::black_box(sink);
        (seen, started.elapsed().as_secs_f64())
    }

    fn flush_wave(
        wave: Vec<String>,
        workers: usize,
        kept: &HashSet<String>,
        mode: EdgeMode,
    ) -> usize {
        match mode {
            EdgeMode::BorrowOnly => 0,
            EdgeMode::CopyOnly => wave.len(),
            _ => parse_edges_across_cores(wave, workers, kept).unwrap().len(),
        }
    }

    /// The fan-out must be invisible in the output: same edges, same order.
    ///
    /// Restore's stream is documented as ordered, so a future reader should not
    /// have to discover that the parse reordered underneath it.
    #[test]
    fn parallel_edge_parse_matches_the_single_threaded_order() {
        let kept: HashSet<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        // Every 7th edge touches a node outside `kept`, so the filter runs on
        // both paths, and 5 000 rows split into several slices at every worker
        // count below.
        let raw: Vec<String> = (0..5000)
            .map(|i| {
                let source = if i % 7 == 0 { "dropped" } else { "a" };
                serde_json::to_string(&edge(&format!("e{i}"), source, "b")).unwrap()
            })
            .collect();

        let single = parse_edges_across_cores(raw.clone(), 1, &kept).unwrap();
        assert_eq!(single.len(), 5000 - 5000usize.div_ceil(7));
        assert!(single.iter().all(|e| e.source == "a" && e.target == "b"));

        let expected: Vec<&str> = single.iter().map(|e| e.id.as_str()).collect();
        for workers in [2usize, 3, 8, 64] {
            let wide = parse_edges_across_cores(raw.clone(), workers, &kept).unwrap();
            let got: Vec<&str> = wide.iter().map(|e| e.id.as_str()).collect();
            assert_eq!(
                got, expected,
                "fan-out over {workers} workers reordered the edges"
            );
        }
    }

    /// An empty or single-row wave must take the direct path, not spawn.
    #[test]
    fn parallel_edge_parse_handles_degenerate_waves() {
        let kept: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        assert!(parse_edges_across_cores(Vec::new(), 8, &kept)
            .unwrap()
            .is_empty());
        let one = vec![serde_json::to_string(&edge("e0", "a", "b")).unwrap()];
        assert_eq!(parse_edges_across_cores(one, 8, &kept).unwrap().len(), 1);
    }

    /// Serialises the test that reads or writes [`PARSE_WORKERS_ENV`].
    fn edge_parse_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn edge_parse_workers_respects_the_env_override_and_the_ceiling() {
        let _guard = edge_parse_env_lock();
        std::env::remove_var(PARSE_WORKERS_ENV);
        assert!((1..=PARSE_MAX_WORKERS).contains(&parse_workers()));

        std::env::set_var(PARSE_WORKERS_ENV, "1");
        assert_eq!(
            parse_workers(),
            1,
            "1 must restore single-threaded behaviour"
        );

        std::env::set_var(PARSE_WORKERS_ENV, "999");
        assert_eq!(parse_workers(), PARSE_MAX_WORKERS, "the ceiling must hold");

        std::env::set_var(PARSE_WORKERS_ENV, "not-a-number");
        assert!((1..=PARSE_MAX_WORKERS).contains(&parse_workers()));

        std::env::remove_var(PARSE_WORKERS_ENV);
    }
}
