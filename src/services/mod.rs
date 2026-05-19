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

pub mod context;
pub mod embedding;
pub mod embedding_providers;
pub mod graph;
pub mod graph_enhanced;
pub mod llm;
pub mod parser;
pub mod session_index;

pub use context::ContextManagerService;
pub use embedding::EmbeddingService;
pub use embedding_providers::{
    CustomHttpEmbeddingConfig, CustomHttpEmbeddingProvider, EmbeddingProvider,
    HuggingFaceEmbeddingProvider, OllamaEmbeddingProvider,
};
pub use graph::GraphService;
pub use graph_enhanced::EnhancedGraphService;
pub use llm::{LlmProvider, LlmService, LlmServiceBuilder};
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
    /// LLM provider abstraction (Phase J).
    /// Always present — may be [`crate::services::llm::LlmProvider::Disabled`]
    /// when no API key / provider config is present. Callers MUST check
    /// [`LlmService::is_enabled()`] before invoking `complete` or `summarize`
    /// and degrade gracefully when it returns `false`. This design ensures
    /// the substrate boots in fully offline / CI environments without error.
    pub llm: LlmService,
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

        Ok(Self {
            context_manager,
            graph,
            enhanced_graph,
            parser,
            embedding,
            attractor_manager,
            session_index,
            fragment_texts,
            llm,
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
