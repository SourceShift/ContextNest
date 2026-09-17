//! Graph-projection backend abstraction.
//!
//! The substrate's learning entities (runs, traces, gradient targets, task
//! classes) are projected into a property graph that callers query over
//! HTTP. That projection is **best-effort**: the canonical substrate never
//! depends on it, so the trait below has a null implementation
//! ([`DisabledGraph`]) that is what both an un-compiled Neo4j and a
//! Neo4j that failed to come up at boot look like from the API layer.
//!
//! This module is compiled in **every** build — it must not import a
//! database driver. The feature-gated driver code lives in
//! [`crate::services::neo4j_graph`] and is reachable only through
//! [`build_graph_backend`], whose `#[cfg]` split keeps its callers in
//! [`crate::services::ContextNestServices::new`] identical in both builds.
//!
//! ## The whitelist is the injection barrier
//!
//! Cypher cannot parameterise a node label or a relationship type
//! (`MERGE (n:$label)` is not valid syntax), so batching by label means
//! interpolating a string into the query text. When that string comes from
//! a request body it is a Cypher-injection hole. [`NODE_LABELS`] and
//! [`EDGE_TYPES`] are therefore the *only* strings ever interpolated, and
//! both the HTTP validator (`crate::api::graph`) and the schema DDL
//! (`neo4j_graph::schema`) read these constants rather than keeping their
//! own copies — a duplicated list drifts, and a drifted list is an open
//! injection hole.

use crate::error::{ContextNestError, ContextNestResult};
use crate::Config;
use std::sync::Arc;

/// One node to project. `props` is passed to Neo4j as query parameters
/// (never interpolated), so its keys are not constrained.
#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub props: serde_json::Map<String, serde_json::Value>,
}

/// One edge to project. `from_label` / `to_label` / `edge_type` are
/// validated against [`EDGE_TYPES`] before use; `props` stays a parameter.
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: String,
    pub from_label: String,
    pub edge_type: String,
    pub to: String,
    pub to_label: String,
    pub props: serde_json::Map<String, serde_json::Value>,
}

/// Rows written by an [`GraphBackend::upsert`] call.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct UpsertCounts {
    pub nodes: usize,
    pub edges: usize,
}

/// A node label and the property that identifies a node of that label.
/// `key` is what the uniqueness constraint is declared on in
/// `neo4j_graph::schema`.
#[derive(Debug, Clone, Copy)]
pub struct NodeLabel {
    pub label: &'static str,
    pub key: &'static str,
}

/// A relationship type together with the endpoint labels it is allowed to
/// connect. Validating the triple (not just the type name) is what stops a
/// request from creating `(:Run)-[:OF_CLASS]->(:GradientTarget)` and
/// corrupting the projection's shape.
#[derive(Debug, Clone, Copy)]
pub struct EdgeType {
    pub edge_type: &'static str,
    pub from_label: &'static str,
    pub to_label: &'static str,
}

/// Every node label the projection writes. Deliberately excludes
/// `Recovery` and `Decision`: `recovery_memory` and `decision_basins` have
/// no writers, so those labels would be decorative.
pub const NODE_LABELS: [NodeLabel; 4] = [
    NodeLabel {
        label: "Run",
        key: "id",
    },
    NodeLabel {
        label: "Trace",
        key: "id",
    },
    NodeLabel {
        label: "GradientTarget",
        key: "id",
    },
    NodeLabel {
        label: "TaskClass",
        key: "name",
    },
];

/// Every relationship type the projection writes, with its permitted
/// endpoints.
pub const EDGE_TYPES: [EdgeType; 4] = [
    EdgeType {
        edge_type: "HAS_TRACE",
        from_label: "Run",
        to_label: "Trace",
    },
    EdgeType {
        edge_type: "LINKED_TO",
        from_label: "Trace",
        to_label: "GradientTarget",
    },
    EdgeType {
        edge_type: "OF_CLASS",
        from_label: "Trace",
        to_label: "TaskClass",
    },
    EdgeType {
        edge_type: "OF_CLASS",
        from_label: "GradientTarget",
        to_label: "TaskClass",
    },
];

/// True when `label` may be interpolated into a Cypher statement.
pub fn is_known_node_label(label: &str) -> bool {
    NODE_LABELS.iter().any(|l| l.label == label)
}

/// True when this exact `(type, from_label, to_label)` triple may be
/// interpolated into a Cypher statement.
pub fn is_known_edge_type(edge_type: &str, from_label: &str, to_label: &str) -> bool {
    EDGE_TYPES
        .iter()
        .any(|e| e.edge_type == edge_type && e.from_label == from_label && e.to_label == to_label)
}

/// The projection's storage. Every method is best-effort from the
/// caller's point of view: `upsert` answers `Ok` with zero counts rather
/// than failing, and reads distinguish "this build has no graph" from
/// "the graph exists but this query failed".
#[async_trait::async_trait]
pub trait GraphBackend: Send + Sync {
    /// Project `nodes` and `edges`. A disabled backend returns
    /// `Ok(UpsertCounts::default())` — whether the caller reports that as
    /// `skipped` is the caller's business, not this trait's.
    async fn upsert(
        &self,
        nodes: &[GraphNode],
        edges: &[GraphEdge],
    ) -> ContextNestResult<UpsertCounts>;

    /// Gradient targets credited by `trace_id`.
    async fn credit_chain(&self, trace_id: &str) -> ContextNestResult<Vec<serde_json::Value>>;

    /// Task classes reached from `target` — the
    /// `GradientTarget → TaskClass` fan-out.
    async fn siblings(&self, target: &str) -> ContextNestResult<Vec<serde_json::Value>>;

    /// `Ok(Some(ids))` — a route exists. `Ok(None)` — both endpoints
    /// exist, no route connects them. `Err(NotFound)` — an endpoint id is
    /// absent. Collapsing `Ok(None)` into `Err(NotFound)` would lie to the
    /// caller about node existence.
    async fn traverse(
        &self,
        from: &str,
        to: &str,
        max_hops: u8,
    ) -> ContextNestResult<Option<Vec<String>>>;

    /// False when no projection is configured or the database could not
    /// be reached at boot. Callers short-circuit on this: writes answer
    /// `skipped`, reads answer `404`.
    fn is_available(&self) -> bool;
}

/// The null backend: what an un-compiled feature, a disabled graph
/// service, and a boot-time connection failure all look like.
pub struct DisabledGraph;

#[async_trait::async_trait]
impl GraphBackend for DisabledGraph {
    async fn upsert(
        &self,
        _nodes: &[GraphNode],
        _edges: &[GraphEdge],
    ) -> ContextNestResult<UpsertCounts> {
        Ok(UpsertCounts::default())
    }

    async fn credit_chain(&self, trace_id: &str) -> ContextNestResult<Vec<serde_json::Value>> {
        Err(ContextNestError::NotFound(format!(
            "graph backend disabled; no credit chain for trace {trace_id}"
        )))
    }

    async fn siblings(&self, target: &str) -> ContextNestResult<Vec<serde_json::Value>> {
        Err(ContextNestError::NotFound(format!(
            "graph backend disabled; no siblings for target {target}"
        )))
    }

    async fn traverse(
        &self,
        from: &str,
        to: &str,
        _max_hops: u8,
    ) -> ContextNestResult<Option<Vec<String>>> {
        Err(ContextNestError::NotFound(format!(
            "graph backend disabled; cannot traverse {from} -> {to}"
        )))
    }

    fn is_available(&self) -> bool {
        false
    }
}

/// Resolve the configured projection backend.
///
/// Never returns an error and never propagates one: a Neo4j that is down,
/// misconfigured, or too slow to answer must not stop the substrate from
/// booting. Every failure inside the feature-gated branch degrades to
/// [`DisabledGraph`].
pub async fn build_graph_backend(config: &Config) -> Arc<dyn GraphBackend> {
    #[cfg(feature = "neo4j-graph")]
    {
        crate::services::neo4j_graph::build(config).await
    }
    #[cfg(not(feature = "neo4j-graph"))]
    {
        let _ = config;
        Arc::new(DisabledGraph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_upsert_succeeds_with_zero_counts() {
        let counts = DisabledGraph
            .upsert(&[], &[])
            .await
            .expect("a disabled backend must not fail an upsert");
        assert_eq!(counts.nodes, 0);
        assert_eq!(counts.edges, 0);
    }

    #[tokio::test]
    async fn disabled_reads_are_not_found() {
        assert!(matches!(
            DisabledGraph.credit_chain("trace:1").await,
            Err(ContextNestError::NotFound(_))
        ));
        assert!(matches!(
            DisabledGraph.siblings("grad:1").await,
            Err(ContextNestError::NotFound(_))
        ));
        assert!(matches!(
            DisabledGraph.traverse("a", "b", 3).await,
            Err(ContextNestError::NotFound(_))
        ));
    }

    #[test]
    fn disabled_reports_unavailable() {
        assert!(!DisabledGraph.is_available());
    }

    #[test]
    fn whitelist_rejects_unknown_labels_and_triples() {
        assert!(is_known_node_label("Trace"));
        assert!(!is_known_node_label("Widget"));

        assert!(is_known_edge_type("LINKED_TO", "Trace", "GradientTarget"));
        // Correct type name, wrong endpoint labels.
        assert!(!is_known_edge_type("LINKED_TO", "Run", "GradientTarget"));
        assert!(!is_known_edge_type("Widget", "Trace", "GradientTarget"));
    }
}
