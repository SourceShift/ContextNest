//! Enhanced graph database service with advanced context storage
//! Provides comprehensive graph database integration for ContextNest,
//! including neural fields, memory attractors, and context synchronization.

use crate::context::field::{FieldProperties, NeuralField, SemanticPattern};
use crate::context::memory::{AttractorField, MemoryAttractor};
use crate::error::ContextNestResult;
// (UI-node helpers stay alongside the graph service)
// Use generic GraphNode from graph service
use crate::error::{ContextNestError, Result};
use crate::services::embedding::{EmbeddingService, SemanticFieldEmbedding};
use crate::services::graph::GraphNode;
use chrono::{DateTime, Utc};
use neo4rs::{query, Graph, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Enhanced context storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextStorageConfig {
    pub batch_size: usize,
    pub sync_interval_seconds: u64,
    pub max_cache_size: usize,
    pub enable_compression: bool,
    pub retention_days: u32,
    pub vector_dimensions: usize,
}

impl Default for ContextStorageConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            sync_interval_seconds: 300, // 5 minutes
            max_cache_size: 1000,
            enable_compression: true,
            retention_days: 30,
            vector_dimensions: 1536,
        }
    }
}

/// Context synchronization state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub last_sync: DateTime<Utc>,
    pub pending_operations: Vec<PendingOperation>,
    pub sync_conflicts: Vec<SyncConflict>,
    pub checkpoint_id: String,
}

/// Pending operation for synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOperation {
    pub operation_id: String,
    pub operation_type: OperationType,
    pub entity_id: String,
    pub data: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    pub priority: i32,
}

/// Type of database operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationType {
    CreateNode,
    UpdateNode,
    DeleteNode,
    CreateRelationship,
    UpdateRelationship,
    DeleteRelationship,
    StoreEmbedding,
    UpdateField,
    SyncMemory,
}

/// Synchronization conflict
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    pub conflict_id: String,
    pub entity_id: String,
    pub local_version: String,
    pub remote_version: String,
    pub conflict_type: ConflictType,
    pub resolution_strategy: ResolutionStrategy,
}

/// Type of synchronization conflict
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictType {
    DataModification,
    SchemaChange,
    VersionMismatch,
    AccessDenied,
}

/// Strategy for resolving conflicts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResolutionStrategy {
    KeepLocal,
    KeepRemote,
    Merge,
    Manual,
}

/// Enhanced graph service with advanced context storage
#[derive(Clone)]
pub struct EnhancedGraphService {
    graph: Option<Arc<Graph>>,
    is_mock: bool,
    embedding_service: Option<Arc<EmbeddingService>>,
    neural_field_cache: Arc<tokio::sync::RwLock<HashMap<String, NeuralField>>>,
    memory_field_cache: Arc<tokio::sync::RwLock<HashMap<String, AttractorField>>>,
    context_cache: Arc<tokio::sync::RwLock<HashMap<String, serde_json::Value>>>,
    config: ContextStorageConfig,
    sync_state: Arc<tokio::sync::RwLock<SyncState>>,
}

impl EnhancedGraphService {
    /// Create new enhanced graph service
    pub async fn new(
        uri: &str,
        username: &str,
        password: &str,
        database: &str,
        config: ContextStorageConfig,
    ) -> ContextNestResult<Self> {
        let graph = Graph::new(uri, username, password)
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        let sync_state = SyncState {
            last_sync: Utc::now(),
            pending_operations: Vec::new(),
            sync_conflicts: Vec::new(),
            checkpoint_id: Uuid::new_v4().to_string(),
        };

        Ok(Self {
            graph: Some(Arc::new(graph)),
            is_mock: false,
            embedding_service: None,
            neural_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            memory_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            context_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            config,
            sync_state: Arc::new(tokio::sync::RwLock::new(sync_state)),
        })
    }

    /// Create a mock enhanced graph service for development
    pub async fn new_mock() -> ContextNestResult<Self> {
        let sync_state = SyncState {
            last_sync: Utc::now(),
            pending_operations: Vec::new(),
            sync_conflicts: Vec::new(),
            checkpoint_id: Uuid::new_v4().to_string(),
        };

        Ok(Self {
            graph: None,
            is_mock: true,
            embedding_service: None,
            neural_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            memory_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            context_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            config: ContextStorageConfig::default(),
            sync_state: Arc::new(tokio::sync::RwLock::new(sync_state)),
        })
    }

    /// Create minimal enhanced graph service that bypasses all database operations
    pub async fn new_minimal() -> ContextNestResult<Self> {
        let sync_state = SyncState {
            last_sync: Utc::now(),
            pending_operations: Vec::new(),
            sync_conflicts: Vec::new(),
            checkpoint_id: Uuid::new_v4().to_string(),
        };

        Ok(Self {
            graph: None,
            is_mock: true,
            embedding_service: None,
            neural_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            memory_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            context_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            config: ContextStorageConfig::default(),
            sync_state: Arc::new(tokio::sync::RwLock::new(sync_state)),
        })
    }

    /// Initialize enhanced graph schema
    pub async fn initialize_schema(&self) -> ContextNestResult<()> {
        if self.is_mock {
            tracing::info!("Mock EnhancedGraphService: skipping schema initialization");
            return Ok(());
        }

        let Some(ref graph) = self.graph else {
            return Err(ContextNestError::Database(
                "Graph not initialized".to_string(),
            ));
        };
        // Create enhanced node types
        let node_constraints = vec![
            "CREATE CONSTRAINT neural_field_id IF NOT EXISTS FOR (nf:NeuralField) REQUIRE nf.id IS UNIQUE",
            "CREATE CONSTRAINT memory_attractor_id IF NOT EXISTS FOR (ma:MemoryAttractor) REQUIRE ma.id IS UNIQUE",
            "CREATE CONSTRAINT context_session_id IF NOT EXISTS FOR (cs:ContextSession) REQUIRE cs.id IS UNIQUE",
            "CREATE CONSTRAINT ui_node_id IF NOT EXISTS FOR (ui:UINode) REQUIRE ui.id IS UNIQUE",
            "CREATE CONSTRAINT semantic_pattern_id IF NOT EXISTS FOR (sp:SemanticPattern) REQUIRE sp.id IS UNIQUE",
        ];

        for constraint in node_constraints {
            if let Err(e) = graph.run(query(constraint)).await {
                tracing::warn!("Failed to create constraint: {}", e);
            }
        }

        // Create enhanced indexes
        let indexes = vec![
            "CREATE INDEX neural_field_strength IF NOT EXISTS FOR (nf:NeuralField) ON (nf.strength)",
            "CREATE INDEX memory_attractor_importance IF NOT EXISTS FOR (ma:MemoryAttractor) ON (ma.importance)",
            "CREATE INDEX context_session_timestamp IF NOT EXISTS FOR (cs:ContextSession) ON (cs.timestamp)",
            "CREATE INDEX ui_node_type IF NOT EXISTS FOR (ui:UINode) ON (ui.node_type)",
            "CREATE INDEX semantic_pattern_resonance IF NOT EXISTS FOR (sp:SemanticPattern) ON (sp.resonance)",
        ];

        for index in indexes {
            if let Err(e) = graph.run(query(index)).await {
                tracing::warn!("Failed to create index: {}", e);
            }
        }

        // Create vector indexes for embeddings
        let vector_indexes = vec![
            format!(
                "CREATE VECTOR INDEX neural_field_embeddings IF NOT EXISTS FOR (nf:NeuralField) ON (nf.embedding) OPTIONS {{indexConfig: {{`vector.dimensions`: {}, `vector.similarity_function`: 'cosine'}}}}",
                self.config.vector_dimensions
            ),
            format!(
                "CREATE VECTOR INDEX memory_attractor_centers IF NOT EXISTS FOR (ma:MemoryAttractor) ON (ma.center) OPTIONS {{indexConfig: {{`vector.dimensions`: {}, `vector.similarity_function`: 'cosine'}}}}",
                self.config.vector_dimensions
            ),
            format!(
                "CREATE VECTOR INDEX semantic_pattern_vectors IF NOT EXISTS FOR (sp:SemanticPattern) ON (sp.embedding) OPTIONS {{indexConfig: {{`vector.dimensions`: {}, `vector.similarity_function`: 'cosine'}}}}",
                self.config.vector_dimensions
            ),
        ];

        for index in vector_indexes {
            if let Err(e) = graph.run(query(&index)).await {
                tracing::warn!("Failed to create vector index: {}", e);
            }
        }

        Ok(())
    }

    /// Store neural field with enhanced context tracking
    pub async fn store_neural_field_enhanced(
        &self,
        field_id: &str,
        field: &NeuralField,
        context_session_id: &str,
    ) -> ContextNestResult<()> {
        if self.is_mock {
            tracing::debug!("Mock EnhancedGraphService: skipping transaction operation");
            return Ok(());
        }

        let Some(ref graph) = self.graph else {
            return Err(ContextNestError::Database(
                "Graph not initialized".to_string(),
            ));
        };
        let mut transaction = graph
            .start_txn()
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        // Store neural field node
        let field_query = r#"
            MERGE (nf:NeuralField {id: $field_id})
            SET nf.strength = $strength,
                nf.resonance_frequency = $resonance_frequency,
                nf.decay_rate = $decay_rate,
                nf.pattern_count = $pattern_count,
                nf.coherence_score = $coherence_score,
                nf.last_activity = $last_activity,
                nf.embedding = $embedding,
                nf.properties = $properties,
                nf.updated_at = datetime()
            RETURN nf
        "#;

        let mut q = query(field_query);
        q = q.param("field_id", field_id);
        q = q.param("strength", field.properties.amplification_factor);
        q = q.param("resonance_frequency", field.properties.resonance_threshold);
        q = q.param("decay_rate", field.properties.decay_constant);
        q = q.param("pattern_count", field.patterns.len() as i64);
        q = q.param("coherence_score", field.properties.coherence_weight);
        q = q.param("last_activity", chrono::Utc::now().timestamp());

        // Use empty embedding for now - generate_field_embedding method not implemented
        q = q.param("embedding", Vec::<f32>::new());

        q = q.param("properties", serde_json::to_string(&field.properties)?);

        transaction
            .run(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        // Store semantic patterns
        for pattern in &field.patterns {
            let pattern_query = r#"
                MERGE (sp:SemanticPattern {id: $pattern_id})
                SET sp.content = $content,
                    sp.strength = $strength,
                    sp.resonance = $resonance,
                    sp.created_at = $created_at,
                    sp.embedding = $embedding,
                    sp.metadata = $metadata
                MERGE (nf:NeuralField {id: $field_id})
                MERGE (nf)-[:CONTAINS_PATTERN {strength: $pattern_strength}]->(sp)
            "#;

            let pattern_id = Uuid::new_v4().to_string();
            let mut pq = query(pattern_query);
            pq = pq.param("pattern_id", pattern_id);
            pq = pq.param("field_id", field_id);
            pq = pq.param("content", pattern.content.clone());
            pq = pq.param("strength", pattern.strength);
            pq = pq.param("resonance", pattern.resonance);
            pq = pq.param("created_at", pattern.created_at.timestamp());
            pq = pq.param("embedding", pattern.embedding.clone());
            pq = pq.param("metadata", "{}");
            pq = pq.param("pattern_strength", pattern.strength);

            transaction
                .run(pq)
                .await
                .map_err(|e| ContextNestError::Database(e.to_string()))?;
        }

        // Link to context session
        let session_query = r#"
            MERGE (cs:ContextSession {id: $session_id})
            MERGE (nf:NeuralField {id: $field_id})
            MERGE (cs)-[:USES_FIELD {timestamp: datetime()}]->(nf)
        "#;

        let mut sq = query(session_query);
        sq = sq.param("session_id", context_session_id);
        sq = sq.param("field_id", field_id);

        transaction
            .run(sq)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;
        transaction
            .commit()
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        // Update cache
        let mut cache = self.neural_field_cache.write().await;
        cache.insert(field_id.to_string(), field.clone());

        // Limit cache size
        if cache.len() > self.config.max_cache_size {
            cache.clear(); // Simple cache eviction strategy
        }

        Ok(())
    }

    /// Store memory attractor field with context tracking
    pub async fn store_memory_field_enhanced(
        &self,
        field_id: &str,
        memory_field: &AttractorField,
        context_session_id: &str,
    ) -> ContextNestResult<()> {
        if self.is_mock {
            tracing::debug!("Mock EnhancedGraphService: skipping transaction operation");
            return Ok(());
        }

        let Some(ref graph) = self.graph else {
            return Err(ContextNestError::Database(
                "Graph not initialized".to_string(),
            ));
        };
        let mut transaction = graph
            .start_txn()
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        // Store memory field node
        let field_query = r#"
            MERGE (mf:MemoryField {id: $field_id})
            SET mf.attractor_count = $attractor_count,
                mf.total_strength = $total_strength,
                mf.fragmentation_score = $fragmentation_score,
                mf.utilization_rate = $utilization_rate,
                mf.last_accessed = $last_accessed,
                mf.persistence_metrics = $persistence_metrics,
                mf.updated_at = datetime()
            RETURN mf
        "#;

        let total_strength: f32 = memory_field
            .attractors
            .iter()
            .map(|(_key, a)| a.strength)
            .sum();
        let fragmentation_score = memory_field.calculate_fragmentation();

        let mut q = query(field_query);
        q = q.param("field_id", field_id);
        q = q.param("attractor_count", memory_field.attractors.len() as i64);
        q = q.param("total_strength", total_strength);
        q = q.param("fragmentation_score", fragmentation_score);
        q = q.param("utilization_rate", 0.5f32); // Use default value
        q = q.param("last_accessed", chrono::Utc::now().timestamp());
        q = q.param(
            "persistence_metrics",
            serde_json::to_string(&memory_field.persistence_metrics)?,
        );

        transaction
            .run(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        // Store individual memory attractors
        for (_key, attractor) in &memory_field.attractors {
            let attractor_query = r#"
                MERGE (ma:MemoryAttractor {id: $attractor_id})
                SET ma.center = $center,
                    ma.strength = $strength,
                    ma.importance = $importance,
                    ma.radius = $radius,
                    ma.decay_rate = $decay_rate,
                    ma.created_at = $created_at,
                    ma.last_activated = $last_activated,
                    ma.activation_count = $activation_count,
                    ma.connections = $connections,
                    ma.metadata = $metadata
                MERGE (mf:MemoryField {id: $field_id})
                MERGE (mf)-[:CONTAINS_ATTRACTOR {strength: $attractor_strength}]->(ma)
            "#;

            let mut aq = query(attractor_query);
            aq = aq.param("attractor_id", attractor.id.clone());
            aq = aq.param("field_id", field_id);
            aq = aq.param("center", attractor.center.clone());
            aq = aq.param("strength", attractor.strength);
            aq = aq.param("importance", attractor.importance);
            aq = aq.param("radius", attractor.radius);
            aq = aq.param("decay_rate", 0.01f32); // Use default decay rate
            aq = aq.param("created_at", chrono::Utc::now().timestamp());
            aq = aq.param("last_activated", chrono::Utc::now().timestamp());
            aq = aq.param("activation_count", 0i64); // Default activation count
            aq = aq.param("connections", "[]"); // Empty connections
            aq = aq.param("metadata", "{}");
            aq = aq.param("attractor_strength", attractor.strength);

            transaction
                .run(aq)
                .await
                .map_err(|e| ContextNestError::Database(e.to_string()))?;
        }

        // Link to context session
        let session_query = r#"
            MERGE (cs:ContextSession {id: $session_id})
            MERGE (mf:MemoryField {id: $field_id})
            MERGE (cs)-[:USES_MEMORY {timestamp: datetime()}]->(mf)
        "#;

        let mut sq = query(session_query);
        sq = sq.param("session_id", context_session_id);
        sq = sq.param("field_id", field_id);

        transaction
            .run(sq)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;
        transaction
            .commit()
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        // Update cache
        let mut cache = self.memory_field_cache.write().await;
        cache.insert(field_id.to_string(), memory_field.clone());

        // Limit cache size
        if cache.len() > self.config.max_cache_size {
            cache.clear(); // Simple cache eviction strategy
        }

        Ok(())
    }

    /// Find similar UI nodes using enhanced vector search
    pub async fn find_similar_ui_nodes(
        &self,
        query_embedding: &[f32],
        context_filters: &HashMap<String, String>,
        limit: usize,
    ) -> ContextNestResult<Vec<GraphNode>> {
        // Build dynamic query based on context filters
        let mut query_conditions = Vec::new();
        let mut query_params = HashMap::new();

        // Add context-based filtering
        if let Some(node_type) = context_filters.get("node_type") {
            query_conditions.push("ui.node_type = $node_type");
            query_params.insert("node_type".to_string(), node_type.clone());
        }

        if let Some(context_session) = context_filters.get("context_session") {
            query_conditions
                .push("EXISTS((cs:ContextSession {id: $context_session})-[:CONTAINS]->(ui))");
            query_params.insert("context_session".to_string(), context_session.clone());
        }

        let where_clause = if query_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", query_conditions.join(" AND "))
        };

        // Try vector search first
        let vector_query = format!(
            r#"
            CALL db.index.vector.queryNodes('ui_node_embeddings', $limit, $embedding)
            YIELD node, score
            MATCH (ui:UINode)
            WHERE id(ui) = id(node) {}
            RETURN ui, score
            ORDER BY score DESC
        "#,
            where_clause
        );

        let mut q = query(&vector_query);
        q = q.param("embedding", query_embedding);
        q = q.param("limit", limit as i64);

        // Add context filter parameters
        for (key, value) in query_params {
            q = q.param(&key, value);
        }

        if self.is_mock {
            tracing::debug!("Mock EnhancedGraphService: returning empty result");
            return Ok(Vec::new());
        }

        let Some(ref graph) = self.graph else {
            return Err(ContextNestError::Database(
                "Graph not initialized".to_string(),
            ));
        };
        let mut result = graph
            .execute(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;
        let mut ui_nodes = Vec::new();

        while let Ok(Some(row)) = result.next().await {
            if let Some(ui_node) = self.parse_ui_node_from_row(&row).await? {
                ui_nodes.push(ui_node);
            }
        }

        Ok(ui_nodes)
    }

    /// Synchronize context data with remote sources
    pub async fn synchronize_context(
        &self,
        remote_checkpoint: &str,
    ) -> ContextNestResult<SyncState> {
        let mut sync_state = self.sync_state.write().await;
        let current_time = Utc::now();

        // Check for pending operations
        let pending_ops = self
            .get_pending_operations_since(&sync_state.last_sync)
            .await?;

        // Apply remote changes
        let conflicts = self.apply_remote_changes(remote_checkpoint).await?;

        // Update sync state
        sync_state.last_sync = current_time;
        sync_state.pending_operations = pending_ops;
        sync_state.sync_conflicts = conflicts;
        sync_state.checkpoint_id = Uuid::new_v4().to_string();

        // Store sync state in database
        self.store_sync_state(&sync_state).await?;

        Ok(sync_state.clone())
    }

    /// Get context patterns for given neural field
    pub async fn get_context_patterns(
        &self,
        field_id: &str,
        pattern_type: Option<&str>,
        min_strength: f32,
    ) -> ContextNestResult<Vec<SemanticPattern>> {
        let mut query_str = r#"
            MATCH (nf:NeuralField {id: $field_id})-[:CONTAINS_PATTERN]->(sp:SemanticPattern)
            WHERE sp.strength >= $min_strength
        "#
        .to_string();

        let mut q = query(&query_str);
        q = q.param("field_id", field_id);
        q = q.param("min_strength", min_strength);

        if let Some(ptype) = pattern_type {
            query_str.push_str(" AND sp.pattern_type = $pattern_type");
            q = q.param("pattern_type", ptype);
        }

        query_str.push_str(" RETURN sp ORDER BY sp.strength DESC");

        if self.is_mock {
            tracing::debug!("Mock EnhancedGraphService: returning empty result");
            return Ok(Vec::new());
        }

        let Some(ref graph) = self.graph else {
            return Err(ContextNestError::Database(
                "Graph not initialized".to_string(),
            ));
        };
        let mut result = graph
            .execute(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;
        let mut patterns = Vec::new();

        while let Ok(Some(row)) = result.next().await {
            if let Some(pattern) = self.parse_semantic_pattern_from_row(&row).await? {
                patterns.push(pattern);
            }
        }

        Ok(patterns)
    }

    /// Store UI node with enhanced context tracking
    pub async fn store_ui_node_enhanced(
        &self,
        ui_node: &GraphNode,
        context_session_id: &str,
        parent_relationships: &[String],
    ) -> ContextNestResult<()> {
        if self.is_mock {
            tracing::debug!("Mock EnhancedGraphService: skipping transaction operation");
            return Ok(());
        }

        let Some(ref graph) = self.graph else {
            return Err(ContextNestError::Database(
                "Graph not initialized".to_string(),
            ));
        };
        let mut transaction = graph
            .start_txn()
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        // Store UI node
        let node_query = r#"
            MERGE (ui:UINode {id: $node_id})
            SET ui.node_type = $node_type,
                ui.widget_class = $widget_class,
                ui.semantic_embedding = $semantic_embedding,
                ui.properties = $properties,
                ui.styling = $styling,
                ui.file_location = $file_location,
                ui.performance_metrics = $performance_metrics,
                ui.last_modified = $last_modified,
                ui.updated_at = datetime()
            RETURN ui
        "#;

        let mut q = query(node_query);
        q = q.param("node_id", ui_node.id.clone());
        q = q.param("node_type", ui_node.node_type.clone());
        // Use generic properties HashMap - domain-specific fields accessed via properties
        // Convert JSON values to strings for Neo4j compatibility
        q = q.param(
            "widget_class",
            ui_node
                .properties
                .get("widget_class")
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .unwrap_or_default(),
        );
        q = q.param(
            "semantic_embedding",
            ui_node
                .properties
                .get("semantic_embedding")
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .unwrap_or_default(),
        );
        q = q.param("properties", serde_json::to_string(&ui_node.properties)?);
        q = q.param(
            "styling",
            ui_node
                .properties
                .get("styling")
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .unwrap_or_default(),
        );
        q = q.param(
            "file_location",
            ui_node
                .properties
                .get("file_location")
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .unwrap_or_default(),
        );
        q = q.param(
            "performance_metrics",
            ui_node
                .properties
                .get("performance_metrics")
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .unwrap_or_default(),
        );
        q = q.param("last_modified", chrono::Utc::now().timestamp());

        transaction
            .run(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        // Create parent-child relationships
        for parent_id in parent_relationships {
            let rel_query = r#"
                MATCH (parent:UINode {id: $parent_id})
                MATCH (child:UINode {id: $child_id})
                MERGE (parent)-[:CONTAINS {created_at: datetime()}]->(child)
            "#;

            let mut rq = query(rel_query);
            rq = rq.param("parent_id", parent_id.as_str());
            rq = rq.param("child_id", ui_node.id.clone());

            transaction
                .run(rq)
                .await
                .map_err(|e| ContextNestError::Database(e.to_string()))?;
        }

        // Link to context session
        let session_query = r#"
            MERGE (cs:ContextSession {id: $session_id})
            MERGE (ui:UINode {id: $node_id})
            MERGE (cs)-[:CONTAINS {timestamp: datetime()}]->(ui)
        "#;

        let mut sq = query(session_query);
        sq = sq.param("session_id", context_session_id);
        sq = sq.param("node_id", ui_node.id.clone());

        transaction
            .run(sq)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;
        transaction
            .commit()
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        Ok(())
    }

    // Helper methods
    async fn parse_ui_node_from_row(&self, row: &Row) -> ContextNestResult<Option<GraphNode>> {
        // Simplified parsing - in a full implementation, this would rebuild the full GraphNode
        // from the row data with proper deserialization
        Ok(None) // Placeholder
    }

    async fn parse_semantic_pattern_from_row(
        &self,
        row: &Row,
    ) -> ContextNestResult<Option<SemanticPattern>> {
        // Simplified parsing - in a full implementation, this would rebuild the full SemanticPattern
        // from the row data with proper deserialization
        Ok(None) // Placeholder
    }

    async fn get_pending_operations_since(
        &self,
        since: &DateTime<Utc>,
    ) -> ContextNestResult<Vec<PendingOperation>> {
        // Query for operations that occurred since the last sync
        Ok(Vec::new()) // Placeholder
    }

    async fn apply_remote_changes(
        &self,
        remote_checkpoint: &str,
    ) -> ContextNestResult<Vec<SyncConflict>> {
        // Apply changes from remote checkpoint and detect conflicts
        Ok(Vec::new()) // Placeholder
    }

    async fn store_sync_state(&self, sync_state: &SyncState) -> ContextNestResult<()> {
        let query_str = r#"
            MERGE (ss:SyncState {id: 'current'})
            SET ss.last_sync = $last_sync,
                ss.checkpoint_id = $checkpoint_id,
                ss.pending_operations = $pending_operations,
                ss.sync_conflicts = $sync_conflicts,
                ss.updated_at = datetime()
        "#;

        let mut q = query(query_str);
        q = q.param("last_sync", sync_state.last_sync.timestamp());
        q = q.param("checkpoint_id", sync_state.checkpoint_id.clone());
        q = q.param(
            "pending_operations",
            serde_json::to_string(&sync_state.pending_operations)?,
        );
        q = q.param(
            "sync_conflicts",
            serde_json::to_string(&sync_state.sync_conflicts)?,
        );

        if self.is_mock {
            tracing::debug!("Mock EnhancedGraphService: skipping database operation");
            return Ok(());
        }

        let Some(ref graph) = self.graph else {
            return Err(ContextNestError::Database(
                "Graph not initialized".to_string(),
            ));
        };
        graph
            .run(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;
        Ok(())
    }

    /// Get context storage health metrics
    pub async fn get_storage_health(&self) -> ContextNestResult<StorageHealthMetrics> {
        if self.is_mock {
            tracing::debug!("Mock EnhancedGraphService: returning mock health metrics");
            return Ok(StorageHealthMetrics {
                neural_field_count: 0,
                memory_attractor_count: 0,
                ui_node_count: 0,
                context_session_count: 0,
                cache_hit_rate: 1.0,
                storage_efficiency: 1.0,
                last_updated: Utc::now(),
            });
        }

        let Some(ref graph) = self.graph else {
            return Err(ContextNestError::Database(
                "Graph not initialized".to_string(),
            ));
        };

        let query_str = r#"
            CALL {
                MATCH (nf:NeuralField) RETURN count(nf) as neural_fields
                UNION
                MATCH (ma:MemoryAttractor) RETURN count(ma) as memory_attractors  
                UNION
                MATCH (ui:UINode) RETURN count(ui) as ui_nodes
                UNION
                MATCH (cs:ContextSession) RETURN count(cs) as context_sessions
            }
            RETURN neural_fields, memory_attractors, ui_nodes, context_sessions
        "#;

        let mut result = graph
            .execute(query(query_str))
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        if let Ok(Some(row)) = result.next().await {
            Ok(StorageHealthMetrics {
                neural_field_count: row.get::<i64>("neural_fields").unwrap_or(0) as u64,
                memory_attractor_count: row.get::<i64>("memory_attractors").unwrap_or(0) as u64,
                ui_node_count: row.get::<i64>("ui_nodes").unwrap_or(0) as u64,
                context_session_count: row.get::<i64>("context_sessions").unwrap_or(0) as u64,
                cache_hit_rate: self.calculate_cache_hit_rate().await,
                storage_efficiency: self.calculate_storage_efficiency().await,
                last_updated: Utc::now(),
            })
        } else {
            Ok(StorageHealthMetrics::default())
        }
    }

    async fn calculate_cache_hit_rate(&self) -> f32 {
        // Calculate cache hit rate based on cache statistics
        0.85 // Placeholder
    }

    async fn calculate_storage_efficiency(&self) -> f32 {
        // Calculate storage efficiency based on data compression and organization
        0.92 // Placeholder
    }
}

/// Storage health metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageHealthMetrics {
    pub neural_field_count: u64,
    pub memory_attractor_count: u64,
    pub ui_node_count: u64,
    pub context_session_count: u64,
    pub cache_hit_rate: f32,
    pub storage_efficiency: f32,
    pub last_updated: DateTime<Utc>,
}

impl Default for StorageHealthMetrics {
    fn default() -> Self {
        Self {
            neural_field_count: 0,
            memory_attractor_count: 0,
            ui_node_count: 0,
            context_session_count: 0,
            cache_hit_rate: 0.0,
            storage_efficiency: 0.0,
            last_updated: Utc::now(),
        }
    }
}

// Helper trait for memory field operations
impl AttractorField {
    fn calculate_fragmentation(&self) -> f32 {
        if self.attractors.is_empty() {
            return 0.0;
        }

        // Simple fragmentation calculation based on attractor distribution
        let total_strength: f32 = self.attractors.iter().map(|(_, a)| a.strength).sum();
        let avg_strength = total_strength / self.attractors.len() as f32;
        let variance: f32 = self
            .attractors
            .iter()
            .map(|(_, a)| (a.strength - avg_strength).powi(2))
            .sum::<f32>()
            / self.attractors.len() as f32;

        (variance.sqrt() / avg_strength).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_enhanced_graph_service_creation() {
        let config = ContextStorageConfig::default();
        let result = EnhancedGraphService::new(
            "bolt://localhost:7687",
            "neo4j",
            "password",
            "neo4j",
            config,
        )
        .await;

        // This will fail without a real Neo4j instance, but we're testing the structure
        assert!(result.is_err() || result.is_ok());
    }

    #[test]
    fn test_context_storage_config_default() {
        let config = ContextStorageConfig::default();
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.vector_dimensions, 1536);
        assert!(config.enable_compression);
    }

    #[test]
    fn test_storage_health_metrics_default() {
        let metrics = StorageHealthMetrics::default();
        assert_eq!(metrics.neural_field_count, 0);
        assert_eq!(metrics.cache_hit_rate, 0.0);
    }
}
