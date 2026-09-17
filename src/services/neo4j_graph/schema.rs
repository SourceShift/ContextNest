//! DDL for the agent-memory property graph, plus the re-export of the
//! label/edge whitelist the DDL is derived from.
//!
//! `IF NOT EXISTS` throughout, so `ensure_schema` is idempotent and safe
//! to call on every boot.
//!
//! ## Deliberate omissions
//!
//! **No vector index.** An earlier design put one over a shared `:Entity`
//! label, but no node in this schema carries an embedding property and no
//! writer would populate one — it would be decorative. Add it if and when
//! embeddings actually land in the graph.
//!
//! **No `Recovery` or `Decision` nodes.** `recovery_memory` and
//! `decision_basins` have no writers anywhere in the pipeline, so those
//! labels would have constraints but never a node.
//!
//! **No DDL for relationship types.** Neo4j creates a relationship type on
//! first use; only the endpoint labels need to exist. The permitted
//! `(type, from_label, to_label)` triples are enforced at the HTTP
//! boundary by [`is_known_edge_type`], not by the database.

// The whitelist lives in `graph_backend` because the request validator in
// `crate::api::graph` needs it in builds where this module is compiled
// out. Re-exported here so the DDL and the validator are visibly reading
// the same constants.
pub use crate::services::graph_backend::{
    is_known_edge_type, is_known_node_label, EdgeType, NodeLabel, EDGE_TYPES, NODE_LABELS,
};

use crate::error::{ContextNestError, ContextNestResult};
use neo4rs::{query, Graph};

/// One uniqueness constraint per label, keyed on the property that
/// identifies a node of that label ([`NODE_LABELS`]).
const CONSTRAINTS: [&str; 4] = [
    "CREATE CONSTRAINT run_id_unique IF NOT EXISTS FOR (n:Run) REQUIRE n.id IS UNIQUE",
    "CREATE CONSTRAINT trace_id_unique IF NOT EXISTS FOR (n:Trace) REQUIRE n.id IS UNIQUE",
    "CREATE CONSTRAINT gradient_target_id_unique IF NOT EXISTS FOR (n:GradientTarget) REQUIRE n.id IS UNIQUE",
    "CREATE CONSTRAINT task_class_name_unique IF NOT EXISTS FOR (n:TaskClass) REQUIRE n.name IS UNIQUE",
];

/// Apply the schema DDL. Fails loudly on the first statement the server
/// rejects — the *caller* downgrades that failure to a `warn!` and a
/// disabled backend, so a schema error cannot stop the substrate booting.
pub async fn ensure_schema(graph: &Graph) -> ContextNestResult<()> {
    for ddl in CONSTRAINTS {
        graph
            .run(query(ddl))
            .await
            .map_err(|e| ContextNestError::Database(format!("neo4j schema DDL failed: {e}")))?;
    }
    Ok(())
}
