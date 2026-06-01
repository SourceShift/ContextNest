//! `GET /llm/v1/cache/stats` — LLM proxy cache statistics (v0.3 Phase 2 slice 2.4).
//!
//! Exposes the in-memory cache's hit/miss/entry counters as JSON. Read-
//! only; no side effects. The payload mirrors `LlmCacheService::stats()`
//! verbatim so operators can wire this into Prometheus / Grafana via
//! existing JSON-scraping exporters without writing a translator.
//!
//! ## Response shape
//!
//! ```json
//! {
//!   "total_entries": 42,
//!   "total_hits":   123,
//!   "total_misses": 27,
//!   "hit_rate": 0.82
//! }
//! ```
//!
//! `hit_rate` is `hits / (hits + misses)` as `f32`, or `0.0` when no
//! lookups have happened yet. Documented in `services::llm_cache::CacheStats`.
//!
//! ## What this slice does NOT do
//!
//! - Does not return per-model breakdown (`hit_rate_by_model`) — would
//!   require richer counter accounting; lands when 2.4 demo workloads
//!   show the breakdown is load-bearing.
//! - Does not return latency p95 — the cache's `lookup()` is sync and
//!   sub-microsecond; a synthetic p95 number would just be lock-contention
//!   noise. Real-world p95 comes from the handler's HTTP latency
//!   histogram in slice 2.5.
//! - Does not return cost-saved estimates — needs per-model token-pricing
//!   tables, planned for Phase 3.

use axum::{extract::State, Json};

use crate::services::llm_cache::CacheStats;
use crate::services::ContextNestServices;

/// `GET /llm/v1/cache/stats` handler. Read-only snapshot of cache counters.
pub async fn cache_stats(State(services): State<ContextNestServices>) -> Json<CacheStats> {
    Json(services.llm_cache.stats())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::llm_cache::{derive_cache_key, LlmCacheService};

    /// Round-trip the JSON envelope so a Prometheus JSON scraper / curl
    /// inspecting the field names doesn't break when the struct is
    /// renamed. Pins the wire contract.
    #[test]
    fn cache_stats_serialises_with_expected_field_names() {
        let stats = CacheStats {
            total_entries: 42,
            total_hits: 123,
            total_misses: 27,
            hit_rate: 0.82,
        };
        let body = serde_json::to_value(&stats).expect("serialise");
        // Pin each field name. If a future refactor renames them, the
        // dashboard / scraper integration breaks silently.
        assert_eq!(body["total_entries"], 42);
        assert_eq!(body["total_hits"], 123);
        assert_eq!(body["total_misses"], 27);
        assert!((body["hit_rate"].as_f64().unwrap() - 0.82).abs() < 1e-4);
    }

    #[test]
    fn cache_stats_reports_zero_state_for_empty_cache() {
        // Empty cache reports all zeros + hit_rate = 0.0. The
        // dashboard reads this on cold start; an "unset" sentinel
        // here would break the type contract (hit_rate is f32 not
        // Option<f32>).
        let cache = LlmCacheService::new();
        let s = cache.stats();
        assert_eq!(s.total_entries, 0);
        assert_eq!(s.total_hits, 0);
        assert_eq!(s.total_misses, 0);
        assert_eq!(s.hit_rate, 0.0);
    }

    #[test]
    fn cache_stats_reflects_hit_and_miss_activity() {
        let cache = LlmCacheService::new();
        let key = derive_cache_key("p", "m", Some(0.7), "sys", vec![1.0, 0.0]);

        // 1 miss
        let _ = cache.lookup(&key, None);

        // Insert + 2 hits
        cache.insert(
            key.clone(),
            crate::api::llm_proxy::openai_shapes::ChatCompletionsResponse {
                id: "x".into(),
                object: "chat.completion".into(),
                created: 0,
                model: "m".into(),
                choices: vec![],
                usage: None,
                system_fingerprint: None,
            },
        );
        cache.lookup(&key, None);
        cache.lookup(&key, None);

        let s = cache.stats();
        assert_eq!(s.total_entries, 1);
        assert_eq!(s.total_hits, 2);
        assert_eq!(s.total_misses, 1);
        assert!((s.hit_rate - 2.0 / 3.0).abs() < 1e-4);
    }
}
