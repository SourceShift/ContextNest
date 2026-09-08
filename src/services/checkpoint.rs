//! Canonical checkpoints for the existing operator WAL. Input replay remains
//! compatible; completed vectors/basins/edges restore without re-embedding.
use crate::memory::attractors::memory_attractor_manager::CanonicalSnapshot;
use crate::services::{compute, ContextNestServices};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

type RestoredCheckpoint = (
    CanonicalSnapshot,
    HashMap<String, HashMap<String, Value>>,
    HashSet<String>,
);

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub struct CheckpointStore {
    _lock: std::fs::File,
    connection: Mutex<Connection>,
}
impl CheckpointStore {
    pub fn open(path: &Path, space: &str) -> Result<Self> {
        let lock = super::tenants::lock_database(path)?;
        let mut connection = Connection::open(path)?;
        connection.execute_batch("PRAGMA journal_mode=WAL;PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS identity(space TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS objects(kind TEXT,id TEXT,payload TEXT NOT NULL,PRIMARY KEY(kind,id));
            CREATE TABLE IF NOT EXISTS completed(id TEXT PRIMARY KEY,metadata TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS tombstones(id TEXT PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS retries(id TEXT PRIMARY KEY,payload TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS tails(id TEXT PRIMARY KEY,payload TEXT NOT NULL);")?;
        let old: Option<String> = connection
            .query_row("SELECT space FROM identity LIMIT 1", [], |r| r.get(0))
            .optional()?;
        if old.as_deref() != Some(space) {
            let tx = connection.transaction()?;
            tx.execute_batch("DELETE FROM objects;DELETE FROM completed;DELETE FROM identity;DELETE FROM retries;")?;
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
        for (kind, entries) in [
            (
                "fragment",
                snapshot
                    .fragments
                    .into_iter()
                    .map(|v| Ok((v.id.clone(), serde_json::to_string(&v)?)))
                    .collect::<Result<Vec<_>>>()?,
            ),
            (
                "basin",
                snapshot
                    .basins
                    .into_iter()
                    .map(|v| Ok((v.id.clone(), serde_json::to_string(&v)?)))
                    .collect::<Result<Vec<_>>>()?,
            ),
            (
                "node",
                snapshot
                    .nodes
                    .into_iter()
                    .map(|v| Ok((v.id.clone(), serde_json::to_string(&v)?)))
                    .collect::<Result<Vec<_>>>()?,
            ),
            (
                "edge",
                snapshot
                    .edges
                    .into_iter()
                    .map(|v| Ok((v.id.clone(), serde_json::to_string(&v)?)))
                    .collect::<Result<Vec<_>>>()?,
            ),
        ] {
            for (key, payload) in entries {
                tx.execute("INSERT INTO objects VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET payload=excluded.payload",params![kind,key,payload])?;
            }
        }
        tx.execute("INSERT INTO completed VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET metadata=excluded.metadata",params![id,serde_json::to_string(metadata)?])?;
        tx.execute("DELETE FROM retries WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn load(&self, allowed: &HashSet<String>) -> Result<RestoredCheckpoint> {
        let db = self.connection.lock().unwrap();
        let mut snapshot = CanonicalSnapshot::default();
        let mut deleted = HashSet::new();
        for row in db
            .prepare("SELECT id FROM tombstones")?
            .query_map([], |r| r.get::<_, String>(0))?
        {
            deleted.insert(row?);
        }
        for row in db
            .prepare("SELECT kind,payload FROM objects")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        {
            let (kind, payload) = row?;
            match kind.as_str() {
                "fragment" => snapshot.fragments.push(serde_json::from_str(&payload)?),
                "basin" => snapshot.basins.push(serde_json::from_str(&payload)?),
                "node" => snapshot.nodes.push(serde_json::from_str(&payload)?),
                "edge" => snapshot.edges.push(serde_json::from_str(&payload)?),
                _ => {}
            }
        }
        let node_ids: HashSet<_> = snapshot.nodes.iter().map(|n| n.id.clone()).collect();
        let member_ids: HashSet<_> = snapshot
            .basins
            .iter()
            .flat_map(|b| b.associated_fragments.iter().cloned())
            .collect();
        snapshot
            .fragments
            .retain(|f| node_ids.contains(&f.id) && member_ids.contains(&f.id));
        snapshot
            .fragments
            .retain(|f| allowed.contains(&f.id) && !deleted.contains(&f.id));
        let ids: HashSet<_> = snapshot.fragments.iter().map(|f| f.id.clone()).collect();
        snapshot.nodes.retain(|n| ids.contains(&n.id));
        snapshot
            .edges
            .retain(|e| ids.contains(&e.source) && ids.contains(&e.target));
        snapshot.basins.retain_mut(|b| {
            b.associated_fragments.retain(|id| ids.contains(id));
            !b.associated_fragments.is_empty()
        });
        let mut metadata = HashMap::new();
        for row in db
            .prepare("SELECT id,metadata FROM completed")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        {
            let (id, meta) = row?;
            if ids.contains(&id) {
                metadata.insert(id, serde_json::from_str(&meta)?);
            }
        }
        Ok((snapshot, metadata, deleted))
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
    let allowed = services
        .fragment_texts
        .read()
        .await
        .keys()
        .cloned()
        .collect();
    let reader = checkpoint.clone();
    let (snapshot, metadata, deleted) =
        compute::run(move || reader.load(&allowed).map_err(|e| e.to_string())).await??;
    let count = snapshot.fragments.len();
    let retry_reader = checkpoint.clone();
    let mut retries =
        compute::run(move || retry_reader.load_retries().map_err(|e| e.to_string())).await??;
    {
        let texts = services.fragment_texts.read().await;
        retries.retain(|id, _| texts.contains_key(id) && !deleted.contains(id));
    }
    services.consolidation_queue.restore_retries(retries);
    {
        let mut cache = services.embeddings_by_id.write().await;
        for f in &snapshot.fragments {
            cache.insert(f.id.clone(), f.content.clone());
        }
    }
    services.attractor_manager.restore_snapshot(snapshot).await;
    services.fragment_metadata.write().await.extend(metadata);
    for id in deleted {
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
        fragments = count,
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
