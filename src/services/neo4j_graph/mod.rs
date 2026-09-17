//! Neo4j-backed [`GraphBackend`] — the only module in the crate that
//! imports the driver.
//!
//! ## Boot safety is the load-bearing property
//!
//! [`build`] has exactly one failure mode: `Arc::new(DisabledGraph)`. It
//! never returns `Err` and never uses `?` on a database operation. A
//! Neo4j that is unreachable, misconfigured, too slow to answer, or
//! missing the schema therefore costs one `warn!` in the log and nothing
//! else — the substrate boots and serves traffic. Letting a driver error
//! escape here would re-introduce "Neo4j down stops boot", which is the
//! failure class this feature gate exists to prevent.
//!
//! ## Injection safety
//!
//! Node labels and relationship types cannot be Cypher parameters, so the
//! upsert batches interpolate them. They are interpolated **only** after
//! matching [`crate::services::graph_backend::NODE_LABELS`] /
//! [`crate::services::graph_backend::EDGE_TYPES`]. Node ids and property
//! maps always travel as query parameters.

pub mod schema;

use crate::config::{Config, GraphStorageBackend};
use crate::error::{ContextNestError, ContextNestResult};
use crate::services::graph_backend::{
    is_known_edge_type, is_known_node_label, node_key, DisabledGraph, GraphBackend, GraphEdge,
    GraphNode, UpsertCounts,
};
use neo4rs::{
    query, BoltBoolean, BoltFloat, BoltInteger, BoltList, BoltMap, BoltNull, BoltString, BoltType,
    ConfigBuilder, Graph, Row,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

/// Neo4j-backed projection.
pub struct Neo4jGraphService {
    graph: Graph,
}

/// Connect, verify, and install the schema — or degrade to
/// [`DisabledGraph`]. See the module doc: this function cannot fail.
pub async fn build(config: &Config) -> Arc<dyn GraphBackend> {
    // Gate 1: is a graph service configured at all?
    let Some(graph_config) = config.services.graph.as_ref() else {
        tracing::debug!("neo4j-graph: no graph service configured; projection disabled");
        return Arc::new(DisabledGraph);
    };

    // Gate 2: has the operator enabled it?
    if !graph_config.enabled {
        tracing::debug!("neo4j-graph: graph service disabled by config; projection disabled");
        return Arc::new(DisabledGraph);
    }

    // Gate 3: did the operator actually choose Neo4j? The default is
    // `InMemory`, so this is the common path on a stock config.
    let GraphStorageBackend::Neo4j { url, database } = &graph_config.storage.backend_type else {
        tracing::debug!("neo4j-graph: graph storage backend is not Neo4j; projection disabled");
        return Arc::new(DisabledGraph);
    };

    // The variant is where the operator chose Neo4j, so its values win;
    // `DatabaseConfig` supplies the credentials and the fallbacks.
    let uri = if url.trim().is_empty() {
        config.database.neo4j_uri.clone()
    } else {
        url.clone()
    };
    let database_name = if database.trim().is_empty() {
        config.database.neo4j_database.clone()
    } else {
        database.clone()
    };

    let connection = &graph_config.storage.connection;
    let timeout = Duration::from_secs(connection.timeout_seconds.max(1));

    let driver_config = match ConfigBuilder::new()
        .uri(uri.clone())
        .user(config.database.neo4j_username.clone())
        .password(config.database.neo4j_password.clone())
        .db(database_name.as_str())
        .max_connections(connection.max_connections)
        .build()
    {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::warn!(error = %e, uri = %uri, "neo4j-graph: invalid driver config; projection disabled");
            return Arc::new(DisabledGraph);
        }
    };

    // `Graph::connect` only constructs a lazy pool — it does not open a
    // socket. The liveness probe below is what actually proves the
    // database is reachable, and it is the step that needs a bound.
    let graph = match Graph::connect(driver_config) {
        Ok(graph) => graph,
        Err(e) => {
            tracing::warn!(error = %e, uri = %uri, "neo4j-graph: connection setup failed; projection disabled");
            return Arc::new(DisabledGraph);
        }
    };

    match tokio::time::timeout(timeout, graph.run(query("RETURN 1"))).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::warn!(error = %e, uri = %uri, "neo4j-graph: database unreachable; projection disabled");
            return Arc::new(DisabledGraph);
        }
        Err(_) => {
            tracing::warn!(
                timeout_seconds = timeout.as_secs(),
                uri = %uri,
                "neo4j-graph: connection probe timed out; projection disabled"
            );
            return Arc::new(DisabledGraph);
        }
    }

    if let Err(e) = schema::ensure_schema(&graph).await {
        tracing::warn!(error = %e, uri = %uri, "neo4j-graph: schema setup failed; projection disabled");
        return Arc::new(DisabledGraph);
    }

    tracing::info!(uri = %uri, database = %database_name, "neo4j-graph: projection enabled");
    Arc::new(Neo4jGraphService { graph })
}

fn db_error(e: neo4rs::Error) -> ContextNestError {
    ContextNestError::Database(format!("neo4j: {e}"))
}

fn decode_error(e: neo4rs::DeError) -> ContextNestError {
    ContextNestError::Serialization(format!("neo4j row decode failed: {e}"))
}

/// Convert a JSON value into a Bolt parameter value.
///
/// Hand-written because the driver's `TryFrom<serde_json::Value>` is
/// behind its non-default `json` feature, which this crate does not (and,
/// per its feature contract, must not) enable.
fn json_to_bolt(value: &serde_json::Value) -> BoltType {
    match value {
        serde_json::Value::Null => BoltType::Null(BoltNull),
        serde_json::Value::Bool(b) => BoltType::Boolean(BoltBoolean::new(*b)),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => BoltType::Integer(BoltInteger::new(i)),
            None => BoltType::Float(BoltFloat::new(n.as_f64().unwrap_or_default())),
        },
        serde_json::Value::String(s) => BoltType::String(BoltString::from(s.as_str())),
        serde_json::Value::Array(items) => BoltType::List(BoltList {
            value: items.iter().map(json_to_bolt).collect(),
        }),
        serde_json::Value::Object(map) => BoltType::Map(BoltMap {
            value: map
                .iter()
                .map(|(k, v)| (BoltString::from(k.as_str()), json_to_bolt(v)))
                .collect(),
        }),
    }
}

impl Neo4jGraphService {
    /// True when a node with this id exists under any label. Used to tell
    /// "no route between these nodes" apart from "no such node".
    async fn endpoint_exists(&self, id: &str) -> ContextNestResult<bool> {
        let mut stream = self
            .graph
            .execute(query("MATCH (n {id: $id}) RETURN count(n) AS c").param("id", id))
            .await
            .map_err(db_error)?;
        match stream.next().await.map_err(db_error)? {
            Some(row) => {
                let count: i64 = row.get("c").map_err(decode_error)?;
                Ok(count > 0)
            }
            None => Ok(false),
        }
    }
}

#[async_trait::async_trait]
impl GraphBackend for Neo4jGraphService {
    async fn upsert(
        &self,
        nodes: &[GraphNode],
        edges: &[GraphEdge],
    ) -> ContextNestResult<UpsertCounts> {
        // Re-validate here as well as at the HTTP boundary: this is the
        // last line of defence before a string is interpolated into
        // Cypher, and the trait is a public API other callers can reach.
        for node in nodes {
            if !is_known_node_label(&node.label) {
                return Err(ContextNestError::Validation(format!(
                    "unknown node label: {}",
                    node.label
                )));
            }
        }
        for edge in edges {
            if !is_known_edge_type(&edge.edge_type, &edge.from_label, &edge.to_label) {
                return Err(ContextNestError::Validation(format!(
                    "unknown edge {} ({} -> {})",
                    edge.edge_type, edge.from_label, edge.to_label
                )));
            }
        }

        // Sorted grouping keeps the statement order deterministic, which
        // keeps retries of the same batch byte-identical. Each group also
        // carries the label's identity property so the MERGE below keys on
        // it — `TaskClass` is identified by `name`, not `id`.
        let mut nodes_by_label: BTreeMap<&str, (&str, Vec<&GraphNode>)> = BTreeMap::new();
        for node in nodes {
            let key = node_key(&node.label).expect("label validated above");
            nodes_by_label
                .entry(node.label.as_str())
                .or_insert((key, Vec::new()))
                .1
                .push(node);
        }
        let mut edges_by_type: BTreeMap<&str, Vec<&GraphEdge>> = BTreeMap::new();
        for edge in edges {
            edges_by_type
                .entry(edge.edge_type.as_str())
                .or_default()
                .push(edge);
        }

        // One transaction for the whole batch: a partial batch must not
        // be able to land, or the idempotency callers rely on breaks.
        let mut txn = self.graph.start_txn().await.map_err(db_error)?;
        let mut counts = UpsertCounts::default();

        for (label, (key, group)) in &nodes_by_label {
            let cypher =
                format!("UNWIND $nodes AS n MERGE (x:{label} {{ {key}: n.id }}) SET x += n.props");
            let payload: Vec<HashMap<String, BoltType>> = group
                .iter()
                .map(|node| {
                    let mut entry = HashMap::new();
                    entry.insert(
                        "id".to_string(),
                        BoltType::String(BoltString::from(node.id.as_str())),
                    );
                    entry.insert(
                        "props".to_string(),
                        json_to_bolt(&serde_json::Value::Object(node.props.clone())),
                    );
                    entry
                })
                .collect();
            txn.run(query(&cypher).param("nodes", payload))
                .await
                .map_err(db_error)?;
            counts.nodes += group.len();
        }

        for (edge_type, group) in &edges_by_type {
            // `source`/`target` rather than `from`/`to` as parameter keys:
            // `FROM` is a Cypher keyword and `e.from` is asking for a parse
            // error.
            let cypher = format!(
                "UNWIND $edges AS e MATCH (a {{id: e.source}}) MATCH (b {{id: e.target}}) \
                 MERGE (a)-[r:{edge_type}]->(b) SET r += e.props"
            );
            let payload: Vec<HashMap<String, BoltType>> = group
                .iter()
                .map(|edge| {
                    let mut entry = HashMap::new();
                    entry.insert(
                        "source".to_string(),
                        BoltType::String(BoltString::from(edge.from.as_str())),
                    );
                    entry.insert(
                        "target".to_string(),
                        BoltType::String(BoltString::from(edge.to.as_str())),
                    );
                    entry.insert(
                        "props".to_string(),
                        json_to_bolt(&serde_json::Value::Object(edge.props.clone())),
                    );
                    entry
                })
                .collect();
            txn.run(query(&cypher).param("edges", payload))
                .await
                .map_err(db_error)?;
            counts.edges += group.len();
        }

        txn.commit().await.map_err(db_error)?;
        Ok(counts)
    }

    async fn credit_chain(&self, trace_id: &str) -> ContextNestResult<Vec<serde_json::Value>> {
        let mut stream = self
            .graph
            .execute(
                query(
                    "MATCH (t:Trace {id: $trace_id})-[:LINKED_TO]->(g:GradientTarget) \
                     RETURN g.id AS id, g.target AS target, g.signal AS signal, \
                     g.confidence AS confidence",
                )
                .param("trace_id", trace_id),
            )
            .await
            .map_err(db_error)?;
        let mut out = Vec::new();
        while let Some(row) = stream.next().await.map_err(db_error)? {
            out.push(row_to_json(&row)?);
        }
        Ok(out)
    }

    async fn siblings(&self, target: &str) -> ContextNestResult<Vec<serde_json::Value>> {
        let mut stream = self
            .graph
            .execute(
                query(
                    "MATCH (g:GradientTarget {id: $target})-[:OF_CLASS]->(c:TaskClass) \
                     RETURN c.name AS name",
                )
                .param("target", target),
            )
            .await
            .map_err(db_error)?;
        let mut out = Vec::new();
        while let Some(row) = stream.next().await.map_err(db_error)? {
            out.push(row_to_json(&row)?);
        }
        Ok(out)
    }

    async fn traverse(
        &self,
        from: &str,
        to: &str,
        max_hops: u8,
    ) -> ContextNestResult<Option<Vec<String>>> {
        // `max_hops` is interpolated, so it must be a validated integer:
        // an unbounded value here lets one request walk the whole graph.
        let cypher = format!(
            "MATCH p=(a {{id: $from}})-[*1..{max_hops}]->(b {{id: $to}}) \
             RETURN [n IN nodes(p) | n.id] AS ids LIMIT 1"
        );
        let mut stream = self
            .graph
            .execute(query(&cypher).param("from", from).param("to", to))
            .await
            .map_err(db_error)?;

        if let Some(row) = stream.next().await.map_err(db_error)? {
            let ids: Vec<String> = row.get("ids").map_err(decode_error)?;
            return Ok(Some(ids));
        }

        if self.endpoint_exists(from).await? && self.endpoint_exists(to).await? {
            Ok(None)
        } else {
            Err(ContextNestError::NotFound(format!(
                "graph endpoint absent: {from} -> {to}"
            )))
        }
    }

    fn is_available(&self) -> bool {
        true
    }
}

fn row_to_json(row: &Row) -> ContextNestResult<serde_json::Value> {
    row.to::<serde_json::Value>().map_err(decode_error)
}
