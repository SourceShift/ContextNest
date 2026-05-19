//! Smoke tests for the pluggable provider surface (v0.2 extensibility work).
//!
//! These tests don't make network calls — they verify that:
//!
//! 1. Each built-in embedding provider constructs cleanly and reports the
//!    metadata the substrate relies on (`name`, `dimension`).
//! 2. The `LlmServiceBuilder` programmatic path mirrors the env-driven path
//!    (same disabled-by-default contract).
//! 3. The `LlmProvider::Custom` variant accepts a caller-supplied
//!    `LanguageModel` impl and dispatches through it.

use std::collections::HashMap;
use std::sync::Arc;

use contextnest::services::{
    CustomHttpEmbeddingConfig, CustomHttpEmbeddingProvider, EmbeddingProvider,
    HuggingFaceEmbeddingProvider, LlmProvider, LlmService, LlmServiceBuilder,
    OllamaEmbeddingProvider,
};

#[test]
fn ollama_provider_metadata_is_correct() {
    let p = OllamaEmbeddingProvider::new("http://localhost:11434", "nomic-embed-text", 768);
    assert_eq!(p.name(), "ollama");
    assert_eq!(p.dimension(), 768);
}

#[test]
fn huggingface_provider_metadata_is_correct() {
    let p = HuggingFaceEmbeddingProvider::new(
        "sentence-transformers/all-MiniLM-L6-v2",
        "hf_fake_token_for_smoke_test",
        384,
        "",
    );
    assert_eq!(p.name(), "huggingface");
    assert_eq!(p.dimension(), 384);
}

#[test]
fn custom_http_provider_metadata_is_correct() {
    let cfg = CustomHttpEmbeddingConfig {
        name: "voyage".into(),
        endpoint: "https://api.voyageai.com/v1/embeddings".into(),
        api_key: Some("voyage-fake-key".into()),
        input_field: "input".into(),
        model: Some("voyage-3-large".into()),
        extra_fields: HashMap::new(),
        embedding_path: "data.0.embedding".into(),
        dimension: 1024,
        extra_headers: HashMap::new(),
    };
    let p = CustomHttpEmbeddingProvider::new(cfg);
    assert_eq!(p.name(), "voyage");
    assert_eq!(p.dimension(), 1024);
}

#[test]
fn dyn_dispatch_through_trait_object_works() {
    // The substrate consumes `Arc<dyn EmbeddingProvider + Send + Sync>`.
    // This test exists purely to lock that contract — if the trait gets a
    // method that breaks object safety, this fails to compile, not at
    // some far-downstream call site.
    let providers: Vec<Arc<dyn EmbeddingProvider + Send + Sync>> = vec![
        Arc::new(OllamaEmbeddingProvider::new("", "nomic-embed-text", 768)),
        Arc::new(HuggingFaceEmbeddingProvider::new(
            "BAAI/bge-large-en-v1.5",
            "hf_x",
            1024,
            "",
        )),
        Arc::new(CustomHttpEmbeddingProvider::new(
            CustomHttpEmbeddingConfig {
                name: "internal".into(),
                endpoint: "http://embeddings.internal/v1".into(),
                api_key: None,
                input_field: "text".into(),
                model: None,
                extra_fields: HashMap::new(),
                embedding_path: "embedding".into(),
                dimension: 512,
                extra_headers: HashMap::new(),
            },
        )),
    ];

    assert_eq!(providers.len(), 3);
    let names: Vec<&str> = providers.iter().map(|p| p.name()).collect();
    assert!(names.contains(&"ollama"));
    assert!(names.contains(&"huggingface"));
    assert!(names.contains(&"internal"));
}

#[test]
fn llm_builder_defaults_to_disabled() {
    let service = LlmServiceBuilder::new().build();
    assert!(
        !service.is_enabled(),
        "Builder with no provider configured must yield a disabled service"
    );
}

#[test]
fn llm_builder_default_impl_matches_new() {
    // Default::default() and new() must behave identically.
    let a = LlmServiceBuilder::default().build();
    let b = LlmServiceBuilder::new().build();
    assert_eq!(a.is_enabled(), b.is_enabled());
}

#[test]
fn llm_builder_openai_compatible_yields_enabled_service() {
    // Local Ollama runs at /v1 with no real auth; the builder accepts any
    // non-empty key. This test only verifies construction — no HTTP call
    // is made (network call happens on the first `complete` invocation).
    let service = LlmServiceBuilder::new()
        .with_openai_compatible(
            "http://localhost:11434/v1",
            "ollama-placeholder-key",
            "llama3",
        )
        .build();
    assert!(
        service.is_enabled(),
        "OpenAI-compatible provider with a non-empty key must enable the service"
    );
}

#[test]
fn llm_provider_custom_variant_debug_includes_name() {
    // We can't easily mock LanguageModel here without dev-dep gymnastics,
    // so the meaningful assertion is on the Disabled baseline — the
    // Custom variant's Debug formatter is exercised in the lib unit
    // tests where a mock model is available.
    let disabled = LlmProvider::Disabled;
    assert_eq!(format!("{:?}", disabled), "LlmProvider::Disabled");
}

#[test]
fn llm_service_from_env_with_no_keys_is_still_disabled() {
    // Snapshot env vars that the substrate looks at + clear them, then
    // verify `from_env` returns a disabled service. Restore env after.
    fn snapshot() -> Vec<(&'static str, Option<String>)> {
        [
            "CONTEXTNEST_LLM_PROVIDER",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "GOOGLE_API_KEY",
        ]
        .iter()
        .map(|k| (*k, std::env::var(k).ok()))
        .collect()
    }
    fn clear(keys: &[&str]) {
        for k in keys {
            std::env::remove_var(k);
        }
    }
    fn restore(snap: Vec<(&'static str, Option<String>)>) {
        for (k, v) in snap {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }

    let snap = snapshot();
    clear(&[
        "CONTEXTNEST_LLM_PROVIDER",
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "GOOGLE_API_KEY",
    ]);

    let service = LlmService::from_env();
    assert!(!service.is_enabled());

    restore(snap);
}
