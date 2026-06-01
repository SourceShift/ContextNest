//! Core services container.
//! v0.1.0 surface is intentionally minimal: graph, parser, embedding, context
//! manager. Plugin loading + hot reload + sandbox management were removed with
//! the plugin SDK.

use crate::config::{EmbeddingServicesConfig, ParserConfig};
use crate::error::ContextNestResult;
use crate::memory::attractors::{MemoryAttractorConfig, MemoryAttractorManager};
use crate::Config;
use std::collections::HashMap;
use std::sync::Arc;

pub mod consolidation;
pub mod context;
pub mod embedding;
pub mod embedding_providers;
pub mod fragment_id;
pub mod graph;
pub mod graph_enhanced;
pub mod llm;
pub mod llm_cache;
pub mod parser;
pub mod session_index;
pub mod wal;

pub use context::ContextManagerService;
pub use embedding::EmbeddingService;
pub use embedding_providers::{
    CustomHttpEmbeddingConfig, CustomHttpEmbeddingProvider, EmbeddingProvider,
    HuggingFaceEmbeddingProvider, OllamaEmbeddingProvider,
};
pub use graph::GraphService;
pub use graph_enhanced::EnhancedGraphService;
pub use llm::{LlmProvider, LlmService, LlmServiceBuilder};
pub use llm_cache::LlmCacheService;
pub use parser::ParserService;

/// Central service container (domain-agnostic).
#[derive(Clone)]
pub struct ContextNestServices {
    pub context_manager: ContextManagerService,
    pub graph: GraphService,
    pub enhanced_graph: EnhancedGraphService,
    pub parser: ParserService,
    pub embedding: EmbeddingService,
    /// Canonical attractor orchestrator per canon Module 05. Backs all
    /// seven memory tools (store/retrieve/update/summarize/discard/
    /// reconstruct/resonate) post-Phase H. Singleton (not per-session)
    /// because the manager maintains shared basin/connection/decay state;
    /// session affinity lives in [`Self::session_index`] alongside, and
    /// text content lives in [`Self::fragment_texts`] (canonical
    /// fragments carry embeddings only — text is the API layer's concern).
    pub attractor_manager: Arc<MemoryAttractorManager>,
    /// Thin session-to-fragment routing index required by the seven-tool
    /// memory API. The [`MemoryAttractorManager`] is session-agnostic by
    /// design; this index is the complementary layer that lets the API answer
    /// "which fragments belong to session X?" and "does fragment Y still
    /// belong to session X's active set?" without encoding session state
    /// into the attractor physics layer.
    pub session_index: Arc<crate::services::session_index::SessionIndex>,
    /// Text-content sidecar keyed by fragment id.
    /// The canonical [`crate::memory::attractors::MemoryFragment`] carries
    /// `content: Vec<f32>` — pure embedding tokens per canon Module 05 — so
    /// the original human-readable text doesn't survive the storage round
    /// trip on its own. The API layer keeps the source text here so
    /// `retrieve` can return what users actually wrote rather than a vector
    /// they have no use for. Wire is session-agnostic because fragments
    /// themselves are session-agnostic in the canonical IP; session affinity
    /// for "what to surface" lives in [`Self::session_index`].
    pub fragment_texts: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
    /// Per-fragment metadata sidecar — sibling to [`Self::fragment_texts`].
    /// Stores whatever the caller passed in `StoreRequest.metadata`, keyed
    /// by the same `fragment_id`. The `retrieve` handler reads this to
    /// support `metadata_filter`, so callers can answer queries like
    /// "show me pending decisions for session X" without changing the
    /// canonical `MemoryFragment` shape.
    ///
    /// Separate from `fragment_texts` (rather than a unified sidecar
    /// struct) because the other six tool handlers — `update`,
    /// `summarize`, `discard`, `reconstruct`, `resonate` — don't need
    /// metadata access; keeping them split means none of those handlers
    /// has to change. The diff stays scoped to store + retrieve.
    pub fragment_metadata:
        Arc<tokio::sync::RwLock<HashMap<String, HashMap<String, serde_json::Value>>>>,
    /// Retrieve co-occurrence log — counts how many times each unordered
    /// `(fragment_id, fragment_id)` pair was returned by the same
    /// `/api/v1/tools/retrieve` call. Drives the /field viz's "real
    /// resonance" edges (replacing the synthesized same-session
    /// placeholder edges). Bounded by capping to top-2000 pairs on
    /// insert when the map grows past 8000 entries — keeps memory
    /// linear while preserving the strongest signals.
    ///
    /// Lifecycle: in-memory only, lost on restart. The dashboard
    /// rebuilds the strongest signal within a few queries because the
    /// inbox/sessions polling drives retrieve calls naturally.
    pub connection_log: Arc<tokio::sync::RwLock<HashMap<(String, String), u32>>>,
    /// Per-fragment-id embedding cache. Populated lazily by the
    /// `/api/v1/fragments?with_embedding=true` handler. Lets a refresh
    /// of the /field view skip the EmbeddingService entirely on its
    /// second-and-later calls, turning a 1–5s rebuild into ~50ms map
    /// reads. Soft-capped at 20k entries (drops oldest-ish on
    /// overflow). Lost on restart.
    pub embeddings_by_id: Arc<tokio::sync::RwLock<HashMap<String, Vec<f32>>>>,
    /// Write-ahead log handle, populated **after** any startup replay so
    /// that replaying records doesn't re-trigger WAL appends (a classic
    /// double-log bug). `None` (uninitialised OnceCell) means no
    /// persistence is configured — handlers skip the append and the
    /// substrate stays purely in-memory, matching pre-WAL behaviour.
    ///
    /// `Arc<OnceCell<...>>` rather than a field swap because
    /// [`ContextNestServices`] is cloned into every Axum handler's
    /// `State<S>`. The OnceCell sits behind the same Arc as the other
    /// shared maps so all clones see the writer the moment it's set
    /// post-replay.
    pub wal: Arc<tokio::sync::OnceCell<wal::Wal>>,
    /// Background consolidation queue — Phase 1 of the neural-field
    /// epic. ServicesSink + WAL replay enqueue fragment ids here after
    /// writing sidecars; the worker drains the queue off the hot path
    /// and runs each fragment through `process_memories` so basins +
    /// connection-network nodes + reconstruction-store entries
    /// actually form for cc_hooks ingest. See
    /// `src/services/consolidation.rs` for the full design.
    pub consolidation_queue: Arc<consolidation::ConsolidationQueue>,
    /// LLM provider abstraction (Phase J).
    /// Always present — may be [`crate::services::llm::LlmProvider::Disabled`]
    /// when no API key / provider config is present. Callers MUST check
    /// [`LlmService::is_enabled()`] before invoking `complete` or `summarize`
    /// and degrade gracefully when it returns `false`. This design ensures
    /// the substrate boots in fully offline / CI environments without error.
    pub llm: LlmService,
    /// LLM proxy response cache. Best-effort — cache failures never
    /// fail the upstream call. Backs `POST /llm/v1/chat/completions`
    /// hit/miss path. v0.3 Phase 2 slice 2.3 wires this in; 2.5 will
    /// promote the semantic-match step to the substrate's attractor
    /// activation.
    pub llm_cache: LlmCacheService,
}

impl ContextNestServices {
    /// Create a new container with the given configuration.
    pub async fn new(config: Config) -> ContextNestResult<Self> {
        tracing::info!("Initializing ContextNest services");

        let graph = if config.database.use_mock {
            tracing::info!("GraphService: mock mode");
            GraphService::new_mock().await?
        } else {
            tracing::info!("GraphService: real Neo4j connection");
            GraphService::new(
                &config.database.neo4j_uri,
                &config.database.neo4j_username,
                &config.database.neo4j_password,
                &config.database.neo4j_database,
            )
            .await?
        };

        let enhanced_graph = if config.database.use_mock {
            EnhancedGraphService::new_mock().await?
        } else {
            EnhancedGraphService::new(
                &config.database.neo4j_uri,
                &config.database.neo4j_username,
                &config.database.neo4j_password,
                &config.database.neo4j_database,
                graph_enhanced::ContextStorageConfig::default(),
            )
            .await?
        };

        let parser = ParserService::new(ParserConfig::default())?;

        let embedding_config = config
            .services
            .embedding
            .as_ref()
            .cloned()
            .unwrap_or_else(EmbeddingServicesConfig::default);
        let embedding = EmbeddingService::new(embedding_config)?;

        let context_manager = ContextManagerService::new(embedding.clone());

        // Initialize the canonical MemoryAttractorManager. The default config
        // matches the canon's Module 05 defaults. `initialize()` brings up the
        // 4 sub-engines (basin manager, decay system, connection network,
        // gap filler) and the reconstruction protocol; if any sub-engine fails
        // to come up we propagate the error rather than silently degrading,
        // since the seven-tool API's `reconstruct` and `resonate` handlers
        // assume a live manager.
        let attractor_manager =
            Arc::new(MemoryAttractorManager::new(MemoryAttractorConfig::default()));
        attractor_manager.initialize().await?;

        let session_index = Arc::new(crate::services::session_index::SessionIndex::new());

        let fragment_texts = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        let fragment_metadata = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        let connection_log = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        let embeddings_by_id = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        let consolidation_queue = Arc::new(consolidation::ConsolidationQueue::new());

        // Construct the LLM service from environment. Returns Disabled when no
        // API key is present — never propagates an error so offline / CI starts
        // succeed regardless of key availability.
        let llm = LlmService::from_env();
        if llm.is_enabled() {
            tracing::info!("LlmService: provider is enabled");
        } else {
            tracing::info!(
                "LlmService: no provider configured (set CONTEXTNEST_LLM_PROVIDER + API key to enable)"
            );
        }

        // Shared WAL OnceCell — populated post-replay by
        // `bin/contextnest.rs::bootstrap_wal`. Both the substrate-side
        // `store` path and the LLM-cache `insert` path append through
        // the same writer once it's installed.
        let wal_cell: Arc<tokio::sync::OnceCell<wal::Wal>> = Arc::new(tokio::sync::OnceCell::new());

        // LLM proxy response cache — roadmap defaults (threshold 0.92,
        // TTL 3600s). Shares the WAL OnceCell with the substrate so
        // `WalRecord::LlmCacheInsert` records persist across restarts;
        // bootstrap replays them before serving traffic.
        let llm_cache = LlmCacheService::new().with_wal(wal_cell.clone());

        Ok(Self {
            context_manager,
            graph,
            enhanced_graph,
            parser,
            embedding,
            attractor_manager,
            session_index,
            fragment_texts,
            fragment_metadata,
            connection_log,
            embeddings_by_id,
            consolidation_queue,
            wal: wal_cell,
            llm,
            llm_cache,
        })
    }

    /// Create a container with default (mock-mode) config — useful for tests.
    pub async fn new_default() -> ContextNestResult<Self> {
        Self::new(Config::default()).await
    }

    pub async fn health_check(&self) -> ContextNestResult<HealthStatus> {
        let graph_ok = self.graph.health_check().await.unwrap_or(false);
        let enhanced_ok = self.enhanced_graph.get_storage_health().await.is_ok();
        let parser_ok = self.parser.health_check().await?;
        let embedding_ok = self.embedding.health_check().await?;

        let overall = graph_ok && enhanced_ok && parser_ok && embedding_ok;
        Ok(HealthStatus {
            overall,
            graph: graph_ok && enhanced_ok,
            parser: parser_ok,
            embedding: embedding_ok,
        })
    }
}

#[derive(Debug, serde::Serialize)]
pub struct HealthStatus {
    pub overall: bool,
    pub graph: bool,
    pub parser: bool,
    pub embedding: bool,
}
