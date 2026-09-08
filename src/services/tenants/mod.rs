//! Authenticated application/session memory. Provider transports and CPU
//! capacity are shared; records, canonical graphs, caches and jobs are scoped.
pub mod database;
pub mod import;
pub mod types;

use crate::memory::attractors::memory_attractor_manager::{
    MemoryProcessingRequest, ProcessingOptions, ProcessingPriority,
};
use crate::memory::attractors::{MemoryAttractorConfig, MemoryAttractorManager, MemoryFragment};
use crate::services::{compute, embedding::EmbeddingService, exact};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use database::{Database, Job};
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use types::*;

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unauthorized scope")]
    Unauthorized,
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("memory capacity is busy")]
    Busy,
    #[error("storage error: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("encoding error: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("{0}")]
    Internal(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantRegistration {
    pub id: String,
    pub token_env: String,
    #[serde(default)]
    pub policy: TenantPolicy,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub data_dir: PathBuf,
    pub operator_token_env: String,
    pub tenants: Vec<TenantRegistration>,
}

pub struct Tenant {
    pub id: String,
    pub policy: TenantPolicy,
    token: String,
    key: hmac::Key,
    database: Mutex<Database>,
    admission: Arc<Semaphore>,
    /// OS lock outlives SQLite. Two processes must never reset each other's leases.
    _lock: std::fs::File,
}
impl Tenant {
    pub async fn with_db<T: Send + 'static>(
        self: &Arc<Self>,
        work: impl FnOnce(&mut Database) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let this = self.clone();
        compute::run(move || {
            let mut db = this
                .database
                .lock()
                .map_err(|_| Error::Internal("tenant database lock poisoned".into()))?;
            work(&mut db)
        })
        .await
        .map_err(Error::Internal)?
    }
    fn capability(&self, scope: &MemoryScope) -> Result<String> {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(scope)?);
        let signature = URL_SAFE_NO_PAD.encode(hmac::sign(&self.key, payload.as_bytes()).as_ref());
        Ok(format!("cn1.{payload}.{signature}"))
    }
}

pub struct Registry {
    pub tenants: BTreeMap<String, Arc<Tenant>>,
    operator_token: String,
    embedding: EmbeddingService,
    requests: Arc<Semaphore>,
    network: Arc<Semaphore>,
}
#[derive(Serialize)]
pub struct SessionRegistration {
    pub scope: MemoryScope,
    pub session_token: String,
}
#[derive(Serialize)]
pub struct Hit {
    pub fragment_id: String,
    pub content: String,
    pub similarity: f32,
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl Registry {
    pub fn from_env(embedding: EmbeddingService) -> Result<Option<Arc<Self>>> {
        let Ok(file) = std::env::var("CONTEXTNEST_TENANTS_FILE") else {
            return Ok(None);
        };
        let settings: Settings = serde_json::from_slice(
            &std::fs::read(file).map_err(|e| Error::Internal(e.to_string()))?,
        )?;
        Self::open(settings, embedding).map(Some)
    }

    pub fn open(settings: Settings, embedding: EmbeddingService) -> Result<Arc<Self>> {
        if settings.tenants.is_empty() || settings.tenants.len() > 32 {
            return Err(Error::Invalid("register 1 to 32 tenants"));
        }
        let operator_token = read_secret(&settings.operator_token_env)?;
        private_directory(&settings.data_dir)?;
        let mut tenants = BTreeMap::new();
        for registration in settings.tenants {
            if !valid_id(&registration.id) || registration.id.contains('.') {
                return Err(Error::Invalid("invalid tenant ID"));
            }
            let token = read_secret(&registration.token_env)?;
            if constant_equal(&token, &operator_token)
                || tenants
                    .values()
                    .any(|t: &Arc<Tenant>| constant_equal(&t.token, &token))
            {
                return Err(Error::Invalid(
                    "tenant and operator credentials must be distinct",
                ));
            }
            let mut policy = registration.policy;
            if policy.embedding_space.is_empty() {
                policy.embedding_space = embedding.space_identity();
            }
            if policy.embedding_space != embedding.space_identity() {
                return Err(Error::Invalid(
                    "tenant embedding space does not match configured provider",
                ));
            }
            policy.validate().map_err(Error::Internal)?;
            let path = settings
                .data_dir
                .join(format!("{}.sqlite", registration.id));
            let lock = lock_database(&path)?;
            let database = Database::open(&path, &policy)?;
            let tenant = Arc::new(Tenant {
                id: registration.id.clone(),
                policy,
                key: hmac::Key::new(hmac::HMAC_SHA256, token.as_bytes()),
                token,
                database: Mutex::new(database),
                admission: Arc::new(Semaphore::new(8)),
                _lock: lock,
            });
            if tenants.insert(registration.id, tenant).is_some() {
                return Err(Error::Invalid("duplicate tenant"));
            }
        }
        Ok(Arc::new(Self {
            tenants,
            operator_token,
            embedding,
            requests: Arc::new(Semaphore::new(32)),
            network: Arc::new(Semaphore::new(4)),
        }))
    }

    pub fn is_operator(&self, token: &str) -> bool {
        constant_equal(&self.operator_token, token)
    }
    pub fn admit(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        self.requests
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)
    }
    pub fn authenticate_app(&self, token: &str) -> Result<Arc<Tenant>> {
        self.tenants
            .values()
            .find(|t| constant_equal(&t.token, token))
            .cloned()
            .ok_or(Error::Unauthorized)
    }
    pub async fn authenticate_session(&self, token: &str) -> Result<(Arc<Tenant>, MemoryScope)> {
        if token.len() > 2048 {
            return Err(Error::Unauthorized);
        }
        let mut parts = token.split('.');
        if parts.next() != Some("cn1") {
            return Err(Error::Unauthorized);
        }
        let payload = parts.next().ok_or(Error::Unauthorized)?;
        let signature = parts.next().ok_or(Error::Unauthorized)?;
        if parts.next().is_some() {
            return Err(Error::Unauthorized);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| Error::Unauthorized)?;
        let scope: MemoryScope = serde_json::from_slice(&bytes).map_err(|_| Error::Unauthorized)?;
        let tenant = self
            .tenants
            .get(&scope.tenant_id)
            .cloned()
            .ok_or(Error::Unauthorized)?;
        hmac::verify(
            &tenant.key,
            payload.as_bytes(),
            &URL_SAFE_NO_PAD
                .decode(signature)
                .map_err(|_| Error::Unauthorized)?,
        )
        .map_err(|_| Error::Unauthorized)?;
        let expected = scope.clone();
        tenant.with_db(move |db| db.verify(&expected)).await?;
        Ok((tenant, scope))
    }

    pub async fn register(
        &self,
        token: &str,
        session: String,
        mode: String,
    ) -> Result<SessionRegistration> {
        let tenant = self.authenticate_app(token)?;
        let id = tenant.id.clone();
        let policy = tenant.policy.clone();
        let scope = tenant
            .with_db(move |db| db.open_session(&id, &session, &mode, &policy))
            .await?;
        let session_token = tenant.capability(&scope)?;
        Ok(SessionRegistration {
            scope,
            session_token,
        })
    }

    pub async fn store(
        &self,
        tenant: Arc<Tenant>,
        scope: MemoryScope,
        input: StoreRequest,
    ) -> Result<Acceptance> {
        let _admission = tenant
            .admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let policy = tenant.policy.clone();
        tenant
            .with_db(move |db| db.accept(&scope, input, &policy))
            .await
    }

    pub async fn retrieve(
        &self,
        tenant: Arc<Tenant>,
        scope: MemoryScope,
        query: String,
        top_k: usize,
    ) -> Result<Vec<Hit>> {
        let _admission = tenant
            .admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        if query.trim().is_empty() || query.len() > 16_384 || top_k == 0 || top_k > 32 {
            return Err(Error::Invalid("invalid query limits"));
        }
        let _network = self
            .network
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let query_vector = self
            .embedding
            .isolated_cache()
            .generate_embedding(&query)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?;
        let norm = exact::norm(&query_vector).ok_or(Error::Invalid("invalid query vector"))?;
        let expected = scope.clone();
        // Revalidate after the provider await. Old requests cannot use a reset scope.
        let (_, snapshot, records) = tenant.with_db(move |db| db.read(&expected)).await?;
        let hits = compute::run(move || {
            let records: BTreeMap<_, _> = records
                .into_iter()
                .map(|r| (r.fragment_id.clone(), r))
                .collect();
            let ranked = exact::top_k(
                snapshot
                    .fragments
                    .iter()
                    .filter(|f| records.contains_key(&f.id))
                    .filter_map(|f| {
                        Some((
                            f.id.as_str(),
                            exact::cosine(
                                &query_vector,
                                norm,
                                &f.content,
                                *snapshot.norms.get(&f.id)?,
                            )?,
                        ))
                    }),
                top_k,
            );
            ranked
                .into_iter()
                .filter_map(|(id, similarity)| {
                    records.get(&id).map(|r| Hit {
                        fragment_id: id.clone(),
                        content: r.content.clone(),
                        similarity,
                        metadata: r.metadata.clone(),
                    })
                })
                .collect()
        })
        .await
        .map_err(Error::Internal)?;
        tenant.with_db(move |db| db.verify(&scope)).await?;
        Ok(hits)
    }

    /// Fixed worker count, one bounded provider request per worker. Round robin
    /// across tenants and least-recently-served sessions within each tenant.
    pub fn spawn_worker(self: &Arc<Self>) {
        let registry = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut cursor = 0usize;
            let mut maintenance = Instant::now();
            loop {
                let Some(registry) = registry.upgrade() else {
                    break;
                };
                let tenants: Vec<_> = registry.tenants.values().cloned().collect();
                if maintenance.elapsed() > Duration::from_secs(60) {
                    for tenant in &tenants {
                        let _ = tenant.with_db(|db| db.expire()).await;
                    }
                    maintenance = Instant::now();
                }
                let mut worked = false;
                for offset in 0..tenants.len() {
                    let index = (cursor + offset) % tenants.len();
                    let tenant = tenants[index].clone();
                    let id = tenant.id.clone();
                    match tenant.with_db(move |db| db.claim(&id)).await {
                        Ok(Some(job)) => {
                            registry.process_job(tenant, job).await;
                            cursor = (index + 1) % tenants.len();
                            worked = true;
                            break;
                        }
                        Ok(None) => {}
                        Err(e) => tracing::warn!(error=%e,"tenant job claim failed"),
                    }
                }
                drop(registry);
                // Pacing applies after successful work as well as an idle lap.
                tokio::time::sleep(Duration::from_millis(if worked { 100 } else { 500 })).await;
            }
        });
    }

    pub async fn process_job(&self, tenant: Arc<Tenant>, mut job: Job) {
        let result = async {
            let _network = self
                .network
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| Error::Busy)?;
            let embedding_started = Instant::now();
            let embedding = if let Some(vector) = job.embedding.clone() {
                vector
            } else {
                tokio::time::timeout(
                    Duration::from_secs(20),
                    self.embedding
                        .isolated_cache()
                        .generate_embedding(&job.record.content),
                )
                .await
                .map_err(|_| Error::Busy)?
                .map_err(|e| Error::Internal(e.to_string()))?
            };
            job.embedding_ms = embedding_started.elapsed().as_millis() as u64;
            drop(_network);
            if exact::norm(&embedding).is_none() {
                return Err(Error::Invalid("invalid embedding vector"));
            }
            let scope = job.scope.clone();
            tenant.with_db(move |db| db.verify(&scope)).await?;
            let policy = tenant.policy.clone();
            let work = job.clone();
            let runtime = tokio::runtime::Handle::current();
            let (snapshot, elapsed) = compute::run(move || {
                let started = Instant::now();
                runtime
                    .block_on(compute_job(work, embedding, policy))
                    .map(|snapshot| (snapshot, started.elapsed().as_millis() as u64))
            })
            .await
            .map_err(Error::Internal)??;
            let work = job.clone();
            tenant
                .with_db(move |db| db.complete(&work, snapshot, elapsed))
                .await?;
            Ok::<(), Error>(())
        }
        .await;
        if let Err(error) = result {
            let transient = match &error {
                Error::Busy | Error::Storage(_) => true,
                Error::Internal(message) => super::consolidation::looks_transient(message),
                _ => false,
            };
            // No provider payload, credential or transcript is logged here.
            tracing::warn!(tenant=%tenant.id,transient,attempt=job.attempts,"tenant processing failed");
            let _ = tenant.with_db(move |db| db.fail(&job, transient)).await;
        }
    }
}

pub(crate) async fn compute_job(
    job: Job,
    embedding: Vec<f32>,
    policy: TenantPolicy,
) -> Result<crate::memory::attractors::memory_attractor_manager::CanonicalSnapshot> {
    let config = MemoryAttractorConfig {
        basin_attach_threshold: policy.basin_attach_threshold,
        ..Default::default()
    };
    let manager = MemoryAttractorManager::new(config);
    manager.set_connection_policy(policy.max_connections, policy.connection_threshold);
    manager.restore_snapshot(job.snapshot).await;
    let live: std::collections::HashSet<_> = job.live_ids.into_iter().collect();
    for id in manager.list_fragment_ids().await {
        if !live.contains(&id) || id == job.record.fragment_id {
            manager
                .discard_fragment(&id)
                .await
                .map_err(|e| Error::Internal(e.to_string()))?;
        }
    }
    let now = chrono::Utc::now();
    let fragment_id = job.record.fragment_id.clone();
    let fragment = MemoryFragment {
        id: job.record.fragment_id,
        content: embedding,
        importance: job.record.importance,
        created_at: now,
        last_accessed: now,
        attractor_basin_id: None,
        connections: Default::default(),
        confidence: job.record.importance,
    };
    let request = MemoryProcessingRequest {
        id: job.lease,
        fragments: vec![fragment],
        options: ProcessingOptions {
            enable_attractor_creation: true,
            enable_connections: true,
            enable_reconstruction: false,
            enable_gap_filling: false,
            quality_threshold: 0.0,
            max_processing_time: Duration::from_secs(5),
        },
        priority: ProcessingPriority::Low,
        created_at: now,
    };
    let result = manager
        .process_memories(request)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;
    if !result.success {
        return Err(Error::Invalid("canonical processing failed"));
    }
    let snapshot = manager.durable_snapshot(None, None).await;
    if !snapshot.nodes.iter().any(|n| n.id == fragment_id)
        || !snapshot
            .basins
            .iter()
            .any(|b| b.associated_fragments.contains(&fragment_id))
    {
        return Err(Error::Invalid("canonical graph or basin was not committed"));
    }
    Ok(snapshot)
}

fn constant_equal(a: &str, b: &str) -> bool {
    let key = hmac::Key::new(hmac::HMAC_SHA256, b"ContextNest credential comparison v1");
    hmac::verify(&key, a.as_bytes(), hmac::sign(&key, b.as_bytes()).as_ref()).is_ok()
}
fn read_secret(name: &str) -> Result<String> {
    let value = std::env::var(name)
        .map_err(|_| Error::Invalid("credential environment variable is missing"))?;
    if value.len() < 32 {
        return Err(Error::Invalid("credentials must contain at least 32 bytes"));
    }
    Ok(value)
}
fn private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|e| Error::Internal(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| Error::Internal(e.to_string()))?;
    }
    Ok(())
}
pub(crate) fn lock_database(path: &Path) -> Result<std::fs::File> {
    use std::fs::OpenOptions;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))
        .map_err(|e| Error::Internal(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // flock is process-scoped, nonblocking and released automatically on exit.
        let result =
            unsafe { nix::libc::flock(file.as_raw_fd(), nix::libc::LOCK_EX | nix::libc::LOCK_NB) };
        if result != 0 {
            return Err(Error::Conflict(
                "tenant database is already owned by another process",
            ));
        }
    }
    Ok(file)
}

#[cfg(test)]
mod tests;
