use crate::context::field::{FieldProperties, NeuralField, SemanticPattern};
use crate::context::memory::{AttractorField, MemoryAttractor};
use crate::error::ContextNestResult;
use crate::services::embedding::{EmbeddingService, SemanticFieldEmbedding};

// Use models directly from the models module
use crate::models::{Screen, Style, Theme, Widget};
// (UI-node helpers stay alongside the graph service)
// Use generic GraphNode instead

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub node_type: String,
    pub properties: std::collections::HashMap<String, serde_json::Value>,
    pub children: Vec<String>,
}
type Relationship = String; // Simplified
use crate::{
    error::{ContextNestError, Result},
    models::*,
};
use chrono::Utc;
use neo4rs::{query, Graph};
use std::collections::HashMap;
use std::sync::Arc;

/// Enhanced graph database service with Context Engineering integration
#[derive(Clone)]
pub struct GraphService {
    graph: Option<Arc<Graph>>,
    is_mock: bool,
    embedding_service: Option<Arc<EmbeddingService>>,
    neural_field_cache: Arc<tokio::sync::RwLock<HashMap<String, NeuralField>>>,
    memory_field_cache: Arc<tokio::sync::RwLock<HashMap<String, AttractorField>>>,
}

impl GraphService {
    pub async fn new(
        uri: &str,
        username: &str,
        password: &str,
        database: &str,
    ) -> ContextNestResult<Self> {
        let graph = Graph::new(uri, username, password)
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        Ok(Self {
            graph: Some(Arc::new(graph)),
            is_mock: false,
            embedding_service: None,
            neural_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            memory_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        })
    }

    /// Create a mock GraphService for development without database connection
    pub async fn new_mock() -> ContextNestResult<Self> {
        Ok(Self {
            graph: None,
            is_mock: true,
            embedding_service: None,
            neural_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            memory_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        })
    }

    /// Create minimal GraphService that bypasses all database operations
    pub async fn new_minimal() -> ContextNestResult<Self> {
        Ok(Self {
            graph: None,
            is_mock: true,
            embedding_service: None,
            neural_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            memory_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        })
    }

    /// Check if this is a mock service
    pub fn is_mock(&self) -> bool {
        self.is_mock
    }

    /// Get graph reference or return error for mock services
    fn get_graph(&self) -> ContextNestResult<&Arc<Graph>> {
        if self.is_mock {
            return Err(ContextNestError::Database(
                "Operation not supported in mock mode".to_string(),
            ));
        }

        self.graph
            .as_ref()
            .ok_or_else(|| ContextNestError::Database("Graph not initialized".to_string()))
    }

    /// Create GraphService with embedding integration
    pub async fn new_with_embeddings(
        uri: &str,
        username: &str,
        password: &str,
        database: &str,
        embedding_service: Arc<EmbeddingService>,
    ) -> ContextNestResult<Self> {
        let graph = Graph::new(uri, username, password)
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        Ok(Self {
            graph: Some(Arc::new(graph)),
            is_mock: false,
            embedding_service: Some(embedding_service),
            neural_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            memory_field_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        })
    }

    /// Create necessary indexes for performance
    pub async fn create_indexes(&self) -> ContextNestResult<()> {
        let graph = self.get_graph()?;
        let queries = vec![
            "CREATE INDEX IF NOT EXISTS FOR (f:File) ON (f.path)",
            "CREATE INDEX IF NOT EXISTS FOR (s:Screen) ON (s.name)",
            "CREATE INDEX IF NOT EXISTS FOR (w:Widget) ON (w.id)",
            "CREATE INDEX IF NOT EXISTS FOR (st:Style) ON (st.name)",
            "CREATE INDEX IF NOT EXISTS FOR (t:Theme) ON (t.name)",
            "CREATE INDEX IF NOT EXISTS FOR (k:BusinessKPI) ON (k.name)",
            "CREATE INDEX IF NOT EXISTS FOR (i:IssueLog) ON (i.id)",
        ];

        for query_str in queries {
            graph
                .run(query(query_str))
                .await
                .map_err(|e| ContextNestError::Database(e.to_string()))?;
        }

        // Create vector indexes for embeddings
        let vector_queries = vec![
            "CREATE VECTOR INDEX widget_embeddings IF NOT EXISTS FOR (w:Widget) ON (w.vectorEmbedding) OPTIONS {indexConfig: {`vector.dimensions`: 1536, `vector.similarity_function`: 'cosine'}}",
            "CREATE VECTOR INDEX issue_embeddings IF NOT EXISTS FOR (i:IssueLog) ON (i.vectorEmbedding) OPTIONS {indexConfig: {`vector.dimensions`: 1536, `vector.similarity_function`: 'cosine'}}",
            "CREATE VECTOR INDEX semantic_pattern_embeddings IF NOT EXISTS FOR (sp:SemanticPattern) ON (sp.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1536, `vector.similarity_function`: 'cosine'}}",
            "CREATE VECTOR INDEX memory_attractor_embeddings IF NOT EXISTS FOR (ma:MemoryAttractor) ON (ma.center) OPTIONS {indexConfig: {`vector.dimensions`: 1536, `vector.similarity_function`: 'cosine'}}",
        ];

        for query_str in vector_queries {
            // Vector indexes might fail in community edition, so we'll continue on error
            if let Err(e) = graph.run(query(query_str)).await {
                tracing::warn!("Failed to create vector index: {}", e);
            }
        }

        Ok(())
    }

    /// Create or update a widget
    pub async fn upsert_widget(&self, widget: &Widget) -> ContextNestResult<()> {
        let query_str = r#"
            MERGE (w:Widget {id: $id})
            SET w.type = $type,
                w.sourceCode = $sourceCode,
                w.startOffset = $startOffset,
                w.endOffset = $endOffset,
                w.properties = $properties,
                w.vectorEmbedding = $vectorEmbedding,
                w.updatedAt = datetime()
            RETURN w
        "#;

        let mut q = query(query_str);
        q = q.param("id", widget.id.clone());
        q = q.param("type", widget.widget_type.clone());
        q = q.param("sourceCode", widget.source_code.clone());
        q = q.param("startOffset", widget.start_offset as i64);
        q = q.param("endOffset", widget.end_offset as i64);
        q = q.param("properties", serde_json::to_string(&widget.properties)?);

        if let Some(embedding) = &widget.vector_embedding {
            q = q.param("vectorEmbedding", embedding.clone());
        } else {
            q = q.param("vectorEmbedding", Vec::<f32>::new());
        }

        if self.is_mock {
            tracing::debug!("Mock GraphService: skipping database operation");
            return Ok(());
        }

        let graph = self.get_graph()?;
        graph
            .run(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        Ok(())
    }

    /// Get widget by ID
    pub async fn get_widget(&self, id: &str) -> ContextNestResult<Option<Widget>> {
        let query_str = r#"
            MATCH (w:Widget {id: $id})
            RETURN w.id as id, w.type as type, w.sourceCode as sourceCode,
                   w.startOffset as startOffset, w.endOffset as endOffset,
                   w.properties as properties, w.vectorEmbedding as vectorEmbedding
        "#;

        let mut q = query(query_str);
        q = q.param("id", id);

        if self.is_mock {
            tracing::debug!("Mock GraphService: returning empty result for query");
            return Ok(None);
        }

        let graph = self.get_graph()?;
        let mut result = graph
            .execute(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        if let Ok(Some(row)) = result.next().await {
            let widget = Widget {
                id: row.get::<String>("id").unwrap_or_default(),
                widget_type: row.get::<String>("widget_type").unwrap_or_default(),
                source_code: row.get::<String>("source_code").unwrap_or_default(),
                start_offset: row.get::<i64>("start_offset").unwrap_or_default() as usize,
                end_offset: row.get::<i64>("end_offset").unwrap_or_default() as usize,
                properties: row
                    .get::<serde_json::Value>("properties")
                    .unwrap_or_default(),
                vector_embedding: row.get::<Vec<f32>>("vectorEmbedding").ok(),
            };

            Ok(Some(widget))
        } else {
            Ok(None)
        }
    }

    /// Find widgets similar to given embedding
    pub async fn find_similar_widgets(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> ContextNestResult<Vec<Widget>> {
        // Try vector search first (if available)
        if let Ok(widgets) = self.vector_search_widgets(embedding, limit).await {
            return Ok(widgets);
        }

        // Fallback to getting all widgets and computing similarity
        self.similarity_search_widgets(embedding, limit).await
    }

    /// Vector search using Neo4j vector index
    async fn vector_search_widgets(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> ContextNestResult<Vec<Widget>> {
        let query_str = r#"
            CALL db.index.vector.queryNodes('widget_embeddings', $limit, $embedding)
            YIELD node, score
            RETURN node.id as id, node.type as type, node.sourceCode as sourceCode,
                   node.startOffset as startOffset, node.endOffset as endOffset,
                   node.properties as properties, score
            ORDER BY score DESC
        "#;

        let mut q = query(query_str);
        q = q.param("embedding", embedding.to_vec());
        q = q.param("limit", limit as i64);

        if self.is_mock {
            tracing::debug!("Mock GraphService: returning empty result for query");
            return Ok(Vec::new());
        }

        let graph = self.get_graph()?;
        let mut result = graph
            .execute(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        let mut widgets = Vec::new();

        while let Ok(Some(row)) = result.next().await {
            let widget = Widget {
                id: row.get::<String>("id").unwrap_or_default(),
                widget_type: row.get::<String>("widget_type").unwrap_or_default(),
                source_code: row.get::<String>("source_code").unwrap_or_default(),
                start_offset: row.get::<i64>("start_offset").unwrap_or_default() as usize,
                end_offset: row.get::<i64>("end_offset").unwrap_or_default() as usize,
                properties: row
                    .get::<serde_json::Value>("properties")
                    .unwrap_or_default(),
                vector_embedding: None, // Don't return embeddings in search results
            };
            widgets.push(widget);
        }

        Ok(widgets)
    }

    /// Fallback similarity search with in-memory computation
    async fn similarity_search_widgets(
        &self,
        target_embedding: &[f32],
        limit: usize,
    ) -> ContextNestResult<Vec<Widget>> {
        let query_str = r#"
            MATCH (w:Widget)
            WHERE w.vectorEmbedding IS NOT NULL
            RETURN w.id as id, w.type as type, w.sourceCode as sourceCode,
                   w.startOffset as startOffset, w.endOffset as endOffset,
                   w.properties as properties, w.vectorEmbedding as vectorEmbedding
        "#;

        if self.is_mock {
            tracing::debug!("Mock GraphService: returning empty result for query");
            return Ok(Vec::new());
        }

        let graph = self.get_graph()?;
        let mut result = graph
            .execute(query(query_str))
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        let mut candidates = Vec::new();

        while let Ok(Some(row)) = result.next().await {
            // For now, use placeholder similarity (will be properly implemented later)
            let similarity = 0.5f32;

            let widget = Widget {
                id: row.get::<String>("id").unwrap_or_default(),
                widget_type: row.get::<String>("widget_type").unwrap_or_default(),
                source_code: row.get::<String>("source_code").unwrap_or_default(),
                start_offset: row.get::<i64>("start_offset").unwrap_or_default() as usize,
                end_offset: row.get::<i64>("end_offset").unwrap_or_default() as usize,
                properties: row
                    .get::<serde_json::Value>("properties")
                    .unwrap_or_default(),
                vector_embedding: None,
            };

            candidates.push((similarity, widget));
        }

        // Sort by similarity and take top results
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        Ok(candidates
            .into_iter()
            .take(limit)
            .map(|(_, widget)| widget)
            .collect())
    }

    /// Get component tree for a screen
    pub async fn get_component_tree(
        &self,
        screen_name: &str,
    ) -> ContextNestResult<serde_json::Value> {
        let query_str = r#"
            MATCH (s:Screen {name: $screenName})-[:CONTAINS*0..]->(w:Widget)
            OPTIONAL MATCH (w)-[:IS_CHILD_OF]->(parent:Widget)
            RETURN w.id as id, w.type as type, w.sourceCode as sourceCode,
                   parent.id as parentId
            ORDER BY w.startOffset
        "#;

        let mut q = query(query_str);
        q = q.param("screenName", screen_name);

        if self.is_mock {
            tracing::debug!("Mock GraphService: returning empty JSON object");
            return Ok(serde_json::json!({}));
        }

        let graph = self.get_graph()?;
        let mut result = graph
            .execute(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        let mut widgets = Vec::new();
        let mut parent_map = std::collections::HashMap::new();

        while let Ok(Some(row)) = result.next().await {
            let id = row.get::<String>("id").unwrap_or_default();
            let widget_type = row.get::<String>("type").unwrap_or_default();
            let source_code = row.get::<String>("sourceCode").unwrap_or_default();
            let parent_id = row.get::<Option<String>>("parentId").unwrap_or(None);

            let widget_info = serde_json::json!({
                "id": id,
                "type": widget_type,
                "sourceCode": source_code,
                "children": []
            });

            widgets.push(widget_info);

            if let Some(parent) = parent_id {
                parent_map.insert(id, parent);
            }
        }

        // Build tree structure (simplified - would need proper tree building logic)
        let tree = serde_json::json!({
            "screen": screen_name,
            "widgets": widgets
        });

        Ok(tree)
    }

    /// Create or update a screen
    pub async fn upsert_screen(&self, screen: &Screen) -> ContextNestResult<()> {
        let query_str = r#"
            MERGE (s:Screen {name: $name})
            SET s.routeName = $routeName,
                s.updatedAt = datetime()
            RETURN s
        "#;

        let mut q = query(query_str);
        q = q.param("name", screen.name.clone());
        q = q.param("routeName", screen.route_name.clone());

        if self.is_mock {
            tracing::debug!("Mock GraphService: skipping database operation");
            return Ok(());
        }

        let graph = self.get_graph()?;
        graph
            .run(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        Ok(())
    }

    /// Create relationship between widgets
    pub async fn create_widget_relationship(
        &self,
        from_id: &str,
        to_id: &str,
        relationship_type: Relationship,
    ) -> ContextNestResult<()> {
        let rel_type = &relationship_type; // Relationship is already a String

        let query_str = format!(
            r#"
            MATCH (from {{id: $fromId}}), (to {{id: $toId}})
            MERGE (from)-[:{}]->(to)
            "#,
            rel_type
        );

        let mut q = query(&query_str);
        q = q.param("fromId", from_id);
        q = q.param("toId", to_id);

        if self.is_mock {
            tracing::debug!("Mock GraphService: skipping database operation");
            return Ok(());
        }

        let graph = self.get_graph()?;
        graph
            .run(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        Ok(())
    }

    /// Health check for graph database with comprehensive diagnostics
    pub async fn health_check(&self) -> ContextNestResult<bool> {
        if self.is_mock {
            tracing::debug!("Mock GraphService: health check passed");
            return Ok(true);
        }

        let graph = match self.get_graph() {
            Ok(g) => g,
            Err(_) => return Ok(false),
        };

        // Test basic connectivity
        match graph.run(query("RETURN 1")).await {
            Ok(_) => {
                tracing::debug!("Graph database basic connectivity check passed");

                // Additional health checks
                if let Err(e) = self.verify_database_constraints().await {
                    tracing::warn!("Graph database constraint verification failed: {}", e);
                    return Ok(false);
                }

                // Check cache health
                self.verify_cache_health().await;

                tracing::info!("Graph database health check passed");
                Ok(true)
            }
            Err(e) => {
                tracing::error!("Graph database connectivity check failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Verify database constraints and indexes
    async fn verify_database_constraints(&self) -> ContextNestResult<()> {
        if self.is_mock {
            tracing::debug!("Mock GraphService: skipping database constraint verification");
            return Ok(());
        }

        let graph = self.get_graph()?;

        // Check if basic indexes exist for performance
        let index_query = query("SHOW INDEXES");
        match graph.run(index_query).await {
            Ok(_) => {
                tracing::debug!("Database indexes accessible");
                Ok(())
            }
            Err(e) => {
                tracing::warn!("Could not access database indexes: {}", e);
                Err(ContextNestError::Database(format!(
                    "Index check failed: {}",
                    e
                )))
            }
        }
    }

    /// Verify cache health and clear stale entries if needed
    async fn verify_cache_health(&self) {
        let neural_cache_size = self.neural_field_cache.read().await.len();
        let memory_cache_size = self.memory_field_cache.read().await.len();

        tracing::debug!(
            "Cache health: neural_fields={}, memory_fields={}",
            neural_cache_size,
            memory_cache_size
        );

        // Clear cache if it gets too large (simple memory management)
        if neural_cache_size > 1000 {
            tracing::info!("Clearing neural field cache due to size limit");
            self.neural_field_cache.write().await.clear();
        }

        if memory_cache_size > 1000 {
            tracing::info!("Clearing memory field cache due to size limit");
            self.memory_field_cache.write().await.clear();
        }
    }

    /// Get project context (themes, styles, KPIs)
    pub async fn get_project_context(&self) -> ContextNestResult<ProjectContext> {
        if self.is_mock {
            tracing::debug!("Mock GraphService: returning empty project context");
            return Ok(ProjectContext {
                themes: Vec::new(),
                styles: Vec::new(),
                screens: Vec::new(),
            });
        }

        let graph = self.get_graph()?;

        // Get all themes
        let themes_query = "MATCH (t:Theme) RETURN t.name as name, t.isDarkMode as isDarkMode";
        let mut themes_result = graph
            .execute(query(themes_query))
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        let mut themes = Vec::new();
        while let Ok(Some(row)) = themes_result.next().await {
            themes.push(Theme {
                name: row.get::<String>("name").unwrap_or_default(),
                is_dark_mode: row.get::<bool>("isDarkMode").unwrap_or(false),
            });
        }

        // Get all styles
        let styles_query =
            "MATCH (s:Style) RETURN s.name as name, s.value as value, s.source as source";
        let mut styles_result = graph
            .execute(query(styles_query))
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        let mut styles = Vec::new();
        while let Ok(Some(row)) = styles_result.next().await {
            styles.push(Style {
                name: row.get::<String>("name").unwrap_or_default(),
                value: row.get::<String>("value").unwrap_or_default(),
                source: row.get::<String>("source").unwrap_or_default(),
            });
        }

        // Get all screens
        let screens_query = "MATCH (s:Screen) RETURN s.name as name, s.routeName as routeName";
        let mut screens_result = graph
            .execute(query(screens_query))
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        let mut screens = Vec::new();
        while let Ok(Some(row)) = screens_result.next().await {
            screens.push(Screen {
                name: row.get::<String>("name").unwrap_or_default(),
                route_name: row.get::<String>("route_name").unwrap_or_default(),
            });
        }

        Ok(ProjectContext {
            themes,
            styles,
            screens,
        })
    }

    /// Context Engineering: Store neural field in graph database
    pub async fn store_neural_field(
        &self,
        field_id: &str,
        field: &NeuralField,
    ) -> ContextNestResult<()> {
        // Store field metadata
        let field_query = r#"
            MERGE (nf:NeuralField {id: $fieldId})
            SET nf.patternCount = $patternCount,
                nf.properties = $properties,
                nf.updatedAt = datetime()
            RETURN nf
        "#;

        let mut q = query(field_query);
        q = q.param("fieldId", field_id);
        q = q.param("patternCount", field.patterns.len() as i64);
        q = q.param("properties", serde_json::to_string(&field.properties)?);

        if self.is_mock {
            tracing::debug!("Mock GraphService: skipping database operation");
            return Ok(());
        }

        let graph = self.get_graph()?;
        graph
            .run(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        // Store individual patterns
        for pattern in &field.patterns {
            self.store_semantic_pattern(field_id, pattern).await?;
        }

        // Cache the field
        {
            let mut cache = self.neural_field_cache.write().await;
            cache.insert(field_id.to_string(), field.clone());
        }

        Ok(())
    }

    /// Store semantic pattern in graph database
    async fn store_semantic_pattern(
        &self,
        field_id: &str,
        pattern: &SemanticPattern,
    ) -> ContextNestResult<()> {
        let pattern_query = r#"
            MERGE (sp:SemanticPattern {id: $patternId})
            SET sp.embedding = $embedding,
                sp.strength = $strength,
                sp.resonance = $resonance,
                sp.content = $content,
                sp.activationCount = $activationCount,
                sp.decayRate = $decayRate,
                sp.createdAt = $createdAt,
                sp.lastActivated = $lastActivated
            WITH sp
            MATCH (nf:NeuralField {id: $fieldId})
            MERGE (nf)-[:CONTAINS_PATTERN]->(sp)
        "#;

        let mut q = query(pattern_query);
        q = q.param("patternId", pattern.id.clone());
        q = q.param("fieldId", field_id);
        q = q.param("embedding", pattern.embedding.clone());
        q = q.param("strength", pattern.strength);
        q = q.param("resonance", pattern.resonance);
        q = q.param("content", pattern.content.clone());
        q = q.param("activationCount", pattern.activation_count as i64);
        q = q.param("decayRate", pattern.decay_rate);
        q = q.param("createdAt", pattern.created_at.to_rfc3339());
        q = q.param("lastActivated", pattern.last_activated.to_rfc3339());

        if self.is_mock {
            tracing::debug!("Mock GraphService: skipping database operation");
            return Ok(());
        }

        let graph = self.get_graph()?;
        graph
            .run(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;
        Ok(())
    }

    /// Load neural field from graph database
    pub async fn load_neural_field(
        &self,
        field_id: &str,
    ) -> ContextNestResult<Option<NeuralField>> {
        // Check cache first
        {
            let cache = self.neural_field_cache.read().await;
            if let Some(field) = cache.get(field_id) {
                return Ok(Some(field.clone()));
            }
        }

        // Load field metadata
        let field_query = r#"
            MATCH (nf:NeuralField {id: $fieldId})
            RETURN nf.properties as properties
        "#;

        let mut q = query(field_query);
        q = q.param("fieldId", field_id);

        if self.is_mock {
            tracing::debug!("Mock GraphService: returning empty result for query");
            return Ok(None);
        }

        let graph = self.get_graph()?;
        let mut result = graph
            .execute(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        if let Ok(Some(row)) = result.next().await {
            let properties_str = row.get::<String>("properties").unwrap_or_default();
            let properties: FieldProperties =
                serde_json::from_str(&properties_str).unwrap_or_default();

            // Load patterns
            let patterns = self.load_field_patterns(field_id).await?;

            let field = NeuralField {
                patterns,
                properties,
                attractors: Vec::new(), // Initialize empty attractors
                state: crate::context::field::FieldState::default(), // Initialize default state
                agency_level: 0.0,
                self_assessment_enabled: false,
                goal_setting_enabled: false,
                residue_sensitivity: 0.5,
                compression_ratio: 0.7,
            };

            // Cache the loaded field
            {
                let mut cache = self.neural_field_cache.write().await;
                cache.insert(field_id.to_string(), field.clone());
            }

            Ok(Some(field))
        } else {
            Ok(None)
        }
    }

    /// Load patterns for a neural field
    async fn load_field_patterns(&self, field_id: &str) -> ContextNestResult<Vec<SemanticPattern>> {
        let patterns_query = r#"
            MATCH (nf:NeuralField {id: $fieldId})-[:CONTAINS_PATTERN]->(sp:SemanticPattern)
            RETURN sp.id as id, sp.embedding as embedding, sp.strength as strength,
                   sp.resonance as resonance, sp.content as content,
                   sp.activationCount as activationCount, sp.decayRate as decayRate,
                   sp.createdAt as createdAt, sp.lastActivated as lastActivated
        "#;

        let mut q = query(patterns_query);
        q = q.param("fieldId", field_id);

        if self.is_mock {
            tracing::debug!("Mock GraphService: returning empty patterns");
            return Ok(Vec::new());
        }

        let graph = self.get_graph()?;
        let mut result = graph
            .execute(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;
        let mut patterns = Vec::new();

        while let Ok(Some(row)) = result.next().await {
            let pattern = SemanticPattern {
                id: row.get::<String>("id").unwrap_or_default(),
                embedding: row.get::<Vec<f32>>("embedding").unwrap_or_default(),
                strength: row.get::<f32>("strength").unwrap_or(0.0),
                resonance: row.get::<f32>("resonance").unwrap_or(0.0),
                content: row.get::<String>("content").unwrap_or_default(),
                activation_count: row.get::<i64>("activationCount").unwrap_or(0) as usize,
                decay_rate: row.get::<f32>("decayRate").unwrap_or(0.01),
                created_at: row
                    .get::<String>("createdAt")
                    .ok()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|| Utc::now()),
                last_activated: row
                    .get::<String>("lastActivated")
                    .ok()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|| Utc::now()),
                delete_reason: None,
                deleted_at: None,
            };
            patterns.push(pattern);
        }

        Ok(patterns)
    }

    /// Store memory attractor field in graph database
    pub async fn store_memory_field(
        &self,
        field_id: &str,
        memory_field: &AttractorField,
    ) -> ContextNestResult<()> {
        // Store memory field metadata
        let field_query = r#"
            MERGE (mf:MemoryField {id: $fieldId})
            SET mf.attractorCount = $attractorCount,
                mf.persistenceMetrics = $persistenceMetrics,
                mf.updatedAt = datetime()
            RETURN mf
        "#;

        let mut q = query(field_query);
        q = q.param("fieldId", field_id);
        q = q.param("attractorCount", memory_field.attractors.len() as i64);
        q = q.param(
            "persistenceMetrics",
            serde_json::to_string(&memory_field.persistence_metrics)?,
        );

        if self.is_mock {
            tracing::debug!("Mock GraphService: skipping database operation");
            return Ok(());
        }

        let graph = self.get_graph()?;
        graph
            .run(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        // Store individual attractors
        for (_key, attractor) in &memory_field.attractors {
            self.store_memory_attractor(field_id, attractor).await?;
        }

        // Cache the memory field
        {
            let mut cache = self.memory_field_cache.write().await;
            cache.insert(field_id.to_string(), memory_field.clone());
        }

        Ok(())
    }

    /// Store memory attractor in graph database
    async fn store_memory_attractor(
        &self,
        field_id: &str,
        attractor: &MemoryAttractor,
    ) -> ContextNestResult<()> {
        let attractor_query = r#"
            MERGE (ma:MemoryAttractor {id: $attractorId})
            SET ma.center = $center,
                ma.strength = $strength,
                ma.radius = $radius,
                ma.importance = $importance,
                ma.content = $content,
                ma.accessCount = $accessCount,
                ma.connections = $connections,
                ma.createdAt = $createdAt,
                ma.lastAccessed = $lastAccessed
            WITH ma
            MATCH (mf:MemoryField {id: $fieldId})
            MERGE (mf)-[:CONTAINS_ATTRACTOR]->(ma)
        "#;

        let mut q = query(attractor_query);
        q = q.param("attractorId", attractor.id.clone());
        q = q.param("fieldId", field_id);
        q = q.param("center", attractor.center.clone());
        q = q.param("strength", attractor.strength);
        q = q.param("radius", attractor.radius);
        q = q.param("importance", attractor.importance);
        q = q.param("content", attractor.content.clone());
        q = q.param("accessCount", attractor.access_count as i64);
        q = q.param(
            "connections",
            serde_json::to_string(&attractor.connections)?,
        );
        q = q.param("createdAt", attractor.created_at.to_rfc3339());
        q = q.param("lastAccessed", attractor.last_accessed.to_rfc3339());

        if self.is_mock {
            tracing::debug!("Mock GraphService: skipping database operation");
            return Ok(());
        }

        let graph = self.get_graph()?;
        graph
            .run(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;
        Ok(())
    }

    /// Find semantic patterns by field-aware similarity
    pub async fn find_field_aware_patterns(
        &self,
        target_embedding: &[f32],
        field_id: &str,
        limit: usize,
    ) -> ContextNestResult<Vec<FieldAwareSearchResult>> {
        // First try vector search if available
        if let Ok(results) = self
            .vector_search_patterns(target_embedding, field_id, limit)
            .await
        {
            return Ok(results);
        }

        // Fallback to manual similarity computation
        self.manual_search_patterns(target_embedding, field_id, limit)
            .await
    }

    /// Vector-based pattern search with field context
    async fn vector_search_patterns(
        &self,
        embedding: &[f32],
        field_id: &str,
        limit: usize,
    ) -> ContextNestResult<Vec<FieldAwareSearchResult>> {
        let query_str = r#"
            MATCH (nf:NeuralField {id: $fieldId})-[:CONTAINS_PATTERN]->(sp:SemanticPattern)
            CALL db.index.vector.queryNodes('semantic_pattern_embeddings', $limit, $embedding)
            YIELD node, score
            WHERE node = sp
            RETURN sp.id as id, sp.content as content, sp.strength as strength,
                   sp.resonance as resonance, score
            ORDER BY score DESC
        "#;

        let mut q = query(query_str);
        q = q.param("fieldId", field_id);
        q = q.param("embedding", embedding.to_vec());
        q = q.param("limit", limit as i64);

        if self.is_mock {
            tracing::debug!("Mock GraphService: returning empty search results");
            return Ok(Vec::new());
        }

        let graph = self.get_graph()?;
        let mut result = graph
            .execute(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;
        let mut results = Vec::new();

        while let Ok(Some(row)) = result.next().await {
            results.push(FieldAwareSearchResult {
                pattern_id: row.get::<String>("id").unwrap_or_default(),
                content: row.get::<String>("content").unwrap_or_default(),
                similarity_score: row.get::<f32>("score").unwrap_or(0.0),
                field_strength: row.get::<f32>("strength").unwrap_or(0.0),
                field_resonance: row.get::<f32>("resonance").unwrap_or(0.0),
                combined_score: row.get::<f32>("score").unwrap_or(0.0)
                    * row.get::<f32>("strength").unwrap_or(0.0)
                    * row.get::<f32>("resonance").unwrap_or(0.0),
            });
        }

        Ok(results)
    }

    /// Manual pattern search with field context
    async fn manual_search_patterns(
        &self,
        target_embedding: &[f32],
        field_id: &str,
        limit: usize,
    ) -> ContextNestResult<Vec<FieldAwareSearchResult>> {
        let patterns = self.load_field_patterns(field_id).await?;
        let mut candidates = Vec::new();

        for pattern in patterns {
            let similarity = cosine_similarity(target_embedding, &pattern.embedding);
            let combined_score = similarity * pattern.strength * pattern.resonance;

            candidates.push(FieldAwareSearchResult {
                pattern_id: pattern.id,
                content: pattern.content,
                similarity_score: similarity,
                field_strength: pattern.strength,
                field_resonance: pattern.resonance,
                combined_score,
            });
        }

        // Sort by combined score and take top results
        candidates.sort_by(|a, b| {
            b.combined_score
                .partial_cmp(&a.combined_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(limit);

        Ok(candidates)
    }

    /// Create relationships between patterns based on field resonance
    pub async fn create_pattern_resonance_relationships(
        &self,
        field_id: &str,
        threshold: f32,
    ) -> ContextNestResult<usize> {
        if self.is_mock {
            tracing::debug!("Mock GraphService: skipping resonance relationships creation");
            return Ok(0);
        }

        let patterns = self.load_field_patterns(field_id).await?;
        let mut relationships_created = 0;
        let graph = self.get_graph()?;

        for i in 0..patterns.len() {
            for j in (i + 1)..patterns.len() {
                let similarity = cosine_similarity(&patterns[i].embedding, &patterns[j].embedding);

                if similarity > threshold {
                    // Create resonance relationship
                    let rel_query = r#"
                        MATCH (sp1:SemanticPattern {id: $pattern1Id})
                        MATCH (sp2:SemanticPattern {id: $pattern2Id})
                        MERGE (sp1)-[r:RESONATES_WITH]->(sp2)
                        SET r.similarity = $similarity,
                            r.createdAt = datetime()
                    "#;

                    let mut q = query(rel_query);
                    q = q.param("pattern1Id", patterns[i].id.clone());
                    q = q.param("pattern2Id", patterns[j].id.clone());
                    q = q.param("similarity", similarity);

                    graph
                        .run(q)
                        .await
                        .map_err(|e| ContextNestError::Database(e.to_string()))?;
                    relationships_created += 1;
                }
            }
        }

        Ok(relationships_created)
    }

    /// Integrate widgets with semantic fields for enhanced context
    pub async fn integrate_widget_with_field(
        &self,
        widget_id: &str,
        field_id: &str,
        embedding_service: &EmbeddingService,
    ) -> ContextNestResult<WidgetFieldIntegration> {
        // Get widget
        let widget = self
            .get_widget(widget_id)
            .await?
            .ok_or_else(|| ContextNestError::Api("Widget not found".to_string()))?;

        // Load field
        let field = self
            .load_neural_field(field_id)
            .await?
            .ok_or_else(|| ContextNestError::Api("Neural field not found".to_string()))?;

        // Generate semantic field embedding for widget
        let field_embedding = embedding_service
            .generate_semantic_field_embedding(&widget.source_code, Some(&field))
            .await?;

        // Find resonant patterns
        let resonant_patterns = self
            .find_field_aware_patterns(&field_embedding.enhanced_embedding, field_id, 5)
            .await?;

        // Create relationships
        if !self.is_mock {
            let graph = self.get_graph()?;
            for pattern_result in &resonant_patterns {
                if pattern_result.combined_score > 0.5 {
                    let rel_query = r#"
                        MATCH (w:Widget {id: $widgetId})
                        MATCH (sp:SemanticPattern {id: $patternId})
                        MERGE (w)-[r:RESONATES_WITH_PATTERN]->(sp)
                        SET r.resonanceScore = $score,
                            r.createdAt = datetime()
                    "#;

                    let mut q = query(rel_query);
                    q = q.param("widgetId", widget_id);
                    q = q.param("patternId", pattern_result.pattern_id.clone());
                    q = q.param("score", pattern_result.combined_score);

                    graph
                        .run(q)
                        .await
                        .map_err(|e| ContextNestError::Database(e.to_string()))?;
                }
            }
        }

        Ok(WidgetFieldIntegration {
            widget_id: widget_id.to_string(),
            field_id: field_id.to_string(),
            field_embedding,
            resonant_patterns,
            integration_timestamp: Utc::now(),
        })
    }

    /// Clear neural field cache
    pub async fn clear_field_cache(&self) {
        let mut neural_cache = self.neural_field_cache.write().await;
        neural_cache.clear();

        let mut memory_cache = self.memory_field_cache.write().await;
        memory_cache.clear();
    }

    /// Get field-based analytics for the graph
    pub async fn get_field_analytics(&self, field_id: &str) -> ContextNestResult<FieldAnalytics> {
        let patterns_query = r#"
            MATCH (nf:NeuralField {id: $fieldId})-[:CONTAINS_PATTERN]->(sp:SemanticPattern)
            RETURN count(sp) as patternCount,
                   avg(sp.strength) as avgStrength,
                   avg(sp.resonance) as avgResonance,
                   max(sp.strength) as maxStrength,
                   min(sp.strength) as minStrength
        "#;

        let mut q = query(patterns_query);
        q = q.param("fieldId", field_id);

        if self.is_mock {
            tracing::debug!("Mock GraphService: returning default field analytics");
            return Ok(FieldAnalytics {
                field_id: field_id.to_string(),
                pattern_count: 0,
                average_strength: 0.0,
                average_resonance: 0.0,
                max_strength: 0.0,
                min_strength: 0.0,
                resonance_connections: 0,
                average_similarity: 0.0,
                field_coherence: 0.0,
                analysis_timestamp: chrono::Utc::now(),
            });
        }

        let graph = self.get_graph()?;
        let mut result = graph
            .execute(q)
            .await
            .map_err(|e| ContextNestError::Database(e.to_string()))?;

        if let Ok(Some(row)) = result.next().await {
            let resonance_query = r#"
                MATCH (nf:NeuralField {id: $fieldId})-[:CONTAINS_PATTERN]->(sp1:SemanticPattern)
                MATCH (sp1)-[r:RESONATES_WITH]->(sp2:SemanticPattern)
                RETURN count(r) as resonanceConnections,
                       avg(r.similarity) as avgSimilarity
            "#;

            let mut rq = query(resonance_query);
            rq = rq.param("fieldId", field_id);

            let mut resonance_result = graph
                .execute(rq)
                .await
                .map_err(|e| ContextNestError::Database(e.to_string()))?;

            let (resonance_connections, avg_similarity) =
                if let Ok(Some(rrow)) = resonance_result.next().await {
                    (
                        rrow.get::<i64>("resonanceConnections").unwrap_or(0) as usize,
                        rrow.get::<f32>("avgSimilarity").unwrap_or(0.0),
                    )
                } else {
                    (0, 0.0)
                };

            Ok(FieldAnalytics {
                field_id: field_id.to_string(),
                pattern_count: row.get::<i64>("patternCount").unwrap_or(0) as usize,
                average_strength: row.get::<f32>("avgStrength").unwrap_or(0.0),
                average_resonance: row.get::<f32>("avgResonance").unwrap_or(0.0),
                max_strength: row.get::<f32>("maxStrength").unwrap_or(0.0),
                min_strength: row.get::<f32>("minStrength").unwrap_or(0.0),
                resonance_connections,
                average_similarity: avg_similarity,
                field_coherence: (row.get::<f32>("avgStrength").unwrap_or(0.0)
                    + row.get::<f32>("avgResonance").unwrap_or(0.0))
                    / 2.0,
                analysis_timestamp: Utc::now(),
            })
        } else {
            Ok(FieldAnalytics {
                field_id: field_id.to_string(),
                pattern_count: 0,
                average_strength: 0.0,
                average_resonance: 0.0,
                max_strength: 0.0,
                min_strength: 0.0,
                resonance_connections: 0,
                average_similarity: 0.0,
                field_coherence: 0.0,
                analysis_timestamp: Utc::now(),
            })
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ProjectContext {
    pub themes: Vec<Theme>,
    pub styles: Vec<Style>,
    pub screens: Vec<Screen>,
}

/// Field-aware search result with Context Engineering metrics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FieldAwareSearchResult {
    pub pattern_id: String,
    pub content: String,
    pub similarity_score: f32,
    pub field_strength: f32,
    pub field_resonance: f32,
    pub combined_score: f32,
}

/// Widget-field integration result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WidgetFieldIntegration {
    pub widget_id: String,
    pub field_id: String,
    pub field_embedding: SemanticFieldEmbedding,
    pub resonant_patterns: Vec<FieldAwareSearchResult>,
    pub integration_timestamp: chrono::DateTime<Utc>,
}

/// Field analytics for monitoring field health and performance
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FieldAnalytics {
    pub field_id: String,
    pub pattern_count: usize,
    pub average_strength: f32,
    pub average_resonance: f32,
    pub max_strength: f32,
    pub min_strength: f32,
    pub resonance_connections: usize,
    pub average_similarity: f32,
    pub field_coherence: f32,
    pub analysis_timestamp: chrono::DateTime<Utc>,
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}
