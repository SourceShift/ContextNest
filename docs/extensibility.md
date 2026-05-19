# Extensibility — any LLM, any embedding

ContextNest's seven-tool substrate is provider-agnostic by design. Out of
the box you get three LLM providers (Anthropic, OpenAI, Google) and three
embedding providers (Ollama, Hugging Face, generic HTTP); plugging
something the substrate doesn't ship natively takes ~10 lines of glue.

This doc covers the two extension points:

1. [LLM providers](#llm-providers) — anything that speaks the
   OpenAI-compatible shape works without code; anything else plugs in
   via the `LanguageModel` trait.
2. [Embedding providers](#embedding-providers) — three built-in
   providers + a generic HTTP shape that covers most external embedding
   APIs without writing Rust.

## LLM providers

### Built-in providers

| Provider | When to use | Setup |
|---|---|---|
| `anthropic` | Claude family, or any Anthropic-protocol proxy (z.ai GLM) | `CONTEXTNEST_LLM_PROVIDER=anthropic` + `ANTHROPIC_API_KEY=…` |
| `openai` | OpenAI, plus any OpenAI-compatible endpoint (Azure, LiteLLM, Ollama, vLLM, LM Studio, OpenRouter, Together, Groq, Mistral, Anyscale, Fireworks) | `CONTEXTNEST_LLM_PROVIDER=openai` + `OPENAI_API_KEY=…` + optional `CONTEXTNEST_LLM_BASE_URL=…` |
| `google` | Gemini family | `CONTEXTNEST_LLM_PROVIDER=google` + `GOOGLE_API_KEY=…` |

### Connecting any OpenAI-shaped service (no code)

Any service that accepts `POST /v1/chat/completions` with the OpenAI
request shape works without modification — Ollama, LiteLLM, vLLM, LM
Studio, OpenRouter, Together AI, Groq, Mistral, Anyscale, Fireworks, and
self-hosted gateways are all covered. Set:

```bash
CONTEXTNEST_LLM_PROVIDER=openai
CONTEXTNEST_LLM_BASE_URL=http://localhost:11434/v1   # local Ollama
CONTEXTNEST_LLM_MODEL=llama3
OPENAI_API_KEY=ollama-placeholder                     # any non-empty string
```

The placeholder key satisfies bearer-token middleware in proxies that
require *some* `Authorization: Bearer …` header. Local Ollama ignores it.

### Programmatic construction with the builder

For Rust applications embedding the substrate as a library — or for
integration tests with a mocked LLM — use [`LlmServiceBuilder`]:

```rust
use contextnest::services::LlmServiceBuilder;

let service = LlmServiceBuilder::new()
    .with_openai_compatible(
        "http://localhost:11434/v1",
        "ollama-placeholder",
        "llama3",
    )
    .build();
```

Variants:

- `with_anthropic(api_key, base_url, model)`
- `with_openai(api_key, base_url, model)`
- `with_google(api_key, base_url, model)`
- `with_openai_compatible(base_url, api_key, model)` — explicit
  OpenAI-shape with required base URL
- `with_custom_provider(name, Arc<dyn LanguageModel + Send + Sync>)` —
  caller-supplied implementation

### Connecting any provider (custom `LanguageModel` impl)

For non-OpenAI-shaped providers — exotic protocols, internal microservices,
or providers `llm-sdk-rs` doesn't have first-class support for — implement
the `LanguageModel` trait yourself and plug it in:

```rust
use std::sync::Arc;
use async_trait::async_trait;
use llm_sdk::{LanguageModel, LanguageModelInput, LanguageModelResponse};
use contextnest::services::LlmServiceBuilder;

struct MyCustomLlm { /* fields */ }

#[async_trait]
impl LanguageModel for MyCustomLlm {
    async fn generate(
        &self,
        input: LanguageModelInput,
    ) -> Result<LanguageModelResponse, llm_sdk::LanguageModelError> {
        // call your provider, return a normalised response
        todo!()
    }
}

let service = LlmServiceBuilder::new()
    .with_custom_provider("my-internal-llm", Arc::new(MyCustomLlm { /*…*/ }))
    .build();
```

The substrate doesn't care which provider sits behind the trait — it
calls `generate` and routes the response through the normal
`complete`/`summarize` paths.

## Embedding providers

The substrate's embedding layer dispatches through the
[`EmbeddingProvider`] trait:

```rust
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn name(&self) -> &str;
    fn dimension(&self) -> usize;
    async fn embed(&self, text: &str) -> ContextNestResult<Vec<f32>>;
    async fn embed_batch(&self, texts: &[&str]) -> ContextNestResult<Vec<Vec<f32>>>;
}
```

Three implementations ship out of the box; a custom provider is a single
new file.

### Built-in: `OllamaEmbeddingProvider`

Local Ollama embedding endpoint. No API key required.

```rust
use contextnest::services::OllamaEmbeddingProvider;

let p = OllamaEmbeddingProvider::new(
    "",                          // empty → default http://localhost:11434
    "nomic-embed-text",          // any model Ollama has pulled
    768,                         // model's output dimensionality
);
```

Works with `nomic-embed-text` (768-d), `mxbai-embed-large` (1024-d),
`all-minilm` (384-d), `snowflake-arctic-embed` (1024-d), `bge-large`
(1024-d), and anything else Ollama supports.

### Built-in: `HuggingFaceEmbeddingProvider`

HF Inference API. Requires an HF read token (`hf_…`).

```rust
use contextnest::services::HuggingFaceEmbeddingProvider;

let p = HuggingFaceEmbeddingProvider::new(
    "BAAI/bge-large-en-v1.5",    // any HF model with feature-extraction
    "hf_your_token",
    1024,
    "",                           // empty → public inference API
);
```

For HF Inference Endpoints (dedicated deployments), pass the full
endpoint URL as the fourth argument.

### Built-in: `CustomHttpEmbeddingProvider` — the escape hatch

The catch-all. Any JSON-over-HTTP embedding service maps to this provider
with a configuration object — no Rust code needed.

Coverage out of the box:

| Service | `input_field` | `embedding_path` | `model` field |
|---|---|---|---|
| **Voyage AI** | `input` | `data.0.embedding` | `voyage-3-large` |
| **Cohere v3** | `texts` | `embeddings.0` | `embed-english-v3.0` |
| **Cohere v1** | `texts` | `embeddings.0` | `embed-english-v2.0` |
| **Jina AI** | `input` | `data.0.embedding` | `jina-embeddings-v3` |
| **Mistral** | `input` | `data.0.embedding` | `mistral-embed` |
| **text-embeddings-inference** (HF official) | `inputs` | `0` | (omit) |
| **vLLM with embedding model** | `input` | `data.0.embedding` | model id |
| **Internal microservice** | anything | anything | (omit) |

Example — Voyage AI:

```rust
use std::collections::HashMap;
use contextnest::services::{CustomHttpEmbeddingConfig, CustomHttpEmbeddingProvider};

let config = CustomHttpEmbeddingConfig {
    name: "voyage".into(),
    endpoint: "https://api.voyageai.com/v1/embeddings".into(),
    api_key: Some(std::env::var("VOYAGE_API_KEY").unwrap()),
    input_field: "input".into(),
    model: Some("voyage-3-large".into()),
    extra_fields: HashMap::new(),
    embedding_path: "data.0.embedding".into(),
    dimension: 1024,
    extra_headers: HashMap::new(),
};
let p = CustomHttpEmbeddingProvider::new(config);
```

Cohere with input-type hint (extra field):

```rust
let mut extra = HashMap::new();
extra.insert("input_type".into(), serde_json::json!("search_document"));

let config = CustomHttpEmbeddingConfig {
    name: "cohere".into(),
    endpoint: "https://api.cohere.com/v2/embed".into(),
    api_key: Some(std::env::var("COHERE_API_KEY").unwrap()),
    input_field: "texts".into(),
    model: Some("embed-english-v3.0".into()),
    extra_fields: extra,
    embedding_path: "embeddings.0".into(),
    dimension: 1024,
    extra_headers: HashMap::new(),
};
```

### Writing a custom provider (when the HTTP escape hatch isn't enough)

For non-JSON protocols, gRPC services, or anything with stateful
authentication: implement the trait directly. Put your code in a new
file, register it where you construct the embedding service.

```rust
use async_trait::async_trait;
use contextnest::services::EmbeddingProvider;
use contextnest::error::ContextNestResult;

struct MyEmbedder {
    // your fields — gRPC stub, signed-URL machinery, etc.
}

#[async_trait]
impl EmbeddingProvider for MyEmbedder {
    fn name(&self) -> &str { "my-embedder" }
    fn dimension(&self) -> usize { 1024 }

    async fn embed(&self, text: &str) -> ContextNestResult<Vec<f32>> {
        // call your service, return the vector
        todo!()
    }

    // Optional: implement embed_batch for native batching.
}
```

The substrate consumes `Arc<dyn EmbeddingProvider + Send + Sync>`, so
your impl wraps cleanly: `let p: Arc<dyn EmbeddingProvider + Send + Sync>
= Arc::new(MyEmbedder { … });`.

## Verifying your provider works

Two layers of smoke tests:

```bash
# unit + integration tests for the extensibility surface
cargo test --test extensibility_smoke

# end-to-end test of the canonical store → retrieve → reconstruct chain
# with whichever provider is configured via env
cargo test --test canonical_memory_chain
```

If the canonical chain test passes with your provider configured,
substrate behaviour is verified end-to-end. If it fails with
`dimension mismatch`, double-check your provider's declared dimensions
matches what its endpoint returns.

## Reference

- [`src/services/embedding_providers/`](../src/services/embedding_providers/) — trait + built-in impls
- [`src/services/llm.rs`](../src/services/llm.rs) — `LlmProvider`, `LlmServiceBuilder`
- [`tests/extensibility_smoke.rs`](../tests/extensibility_smoke.rs) — copy-paste-friendly examples
- [`docs/architecture.md`](architecture.md) — how providers slot into the substrate as a whole
