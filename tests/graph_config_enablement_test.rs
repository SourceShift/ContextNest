//! Enabling the Neo4j graph backend must not require spelling out every
//! nested graph tunable. `src/services/neo4j_graph/mod.rs::build` gates on
//! three things only — `services.graph` present, `enabled`, and
//! `storage.backend_type == Neo4j` — so a config that sets exactly those
//! three must load.
//!
//! These tests exist because the graph config structs carry no serde
//! defaults, which makes any partial `[services.graph]` table a hard parse
//! error at boot. An operator following the docs would hit a process that
//! refuses to start, not a projection that quietly skips.

use contextnest::config::GraphStorageBackend;
use contextnest::Config;

/// The minimal config an operator would write to turn the projection on.
const MINIMAL_NEO4J: &str = r#"
[services.graph]
enabled = true

[services.graph.storage]
backend_type = { Neo4j = { url = "neo4j://localhost:7687", database = "neo4j" } }
"#;

fn neo4j_backend(cfg: &Config) -> Option<&GraphStorageBackend> {
    cfg.services.graph.as_ref().map(|g| &g.storage.backend_type)
}

#[test]
fn minimal_neo4j_config_parses() {
    let cfg: Config =
        toml::from_str(MINIMAL_NEO4J).expect("a graph-only config must not fail to parse");

    match neo4j_backend(&cfg) {
        Some(GraphStorageBackend::Neo4j { url, database }) => {
            assert_eq!(url, "neo4j://localhost:7687");
            assert_eq!(database, "neo4j");
        }
        other => panic!("expected a Neo4j backend, got {other:?}"),
    }
}

/// The sibling fields the operator did NOT mention must fall back to their
/// documented defaults rather than smuggling in a value that disables things.
#[test]
fn omitted_graph_tunables_keep_their_defaults() {
    let cfg: Config = toml::from_str(MINIMAL_NEO4J).expect("must parse");
    let graph = cfg.services.graph.as_ref().expect("graph section present");

    assert!(graph.enabled, "`enabled = true` was set explicitly");
    assert_eq!(
        graph.storage.connection.timeout_seconds,
        contextnest::config::GraphConnectionConfig::default().timeout_seconds,
        "connection timeout must default, not zero"
    );
    assert!(
        graph.algorithms.enabled,
        "algorithms config must default, not read as disabled"
    );
    assert!(
        graph.performance.cache.enabled,
        "performance config must default, not read as disabled"
    );
}

/// `enabled = false` is the documented off switch and must survive parsing.
#[test]
fn disabled_graph_section_parses_and_stays_disabled() {
    let cfg: Config = toml::from_str(
        r#"
[services.graph]
enabled = false

[services.graph.storage]
backend_type = { Neo4j = { url = "neo4j://localhost:7687", database = "" } }
"#,
    )
    .expect("must parse");

    let graph = cfg.services.graph.as_ref().expect("graph section present");
    assert!(!graph.enabled);
}

/// A config with no graph section at all is the stock case: gate 1 in
/// `build()` must see `None` and pick `DisabledGraph`.
#[test]
fn absent_graph_section_is_none() {
    let cfg: Config = toml::from_str("[services.embedding]\n").expect("must parse");
    assert!(
        cfg.services.graph.is_none(),
        "an absent [services.graph] must be None, not a defaulted Some"
    );
}
