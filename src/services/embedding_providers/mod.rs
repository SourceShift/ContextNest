//! Pluggable embedding-provider abstraction.
//!
//! The seven-tool memory substrate is provider-agnostic by design — adding
//! support for a new embedding backend is meant to be a leaf-level change
//! (one new file in this module), not a refactor of the dispatch layer.
//!
//! # The trait
//!
//! All providers implement [`EmbeddingProvider`]. The substrate's
//! [`EmbeddingService`](super::embedding::EmbeddingService) holds an
//! `Arc<dyn EmbeddingProvider + Send + Sync>` and dispatches through it. A
//! custom provider becomes a drop-in replacement once it implements the
//! three methods of this trait.
//!
//! # Built-in providers
//!
//! - [`OllamaEmbeddingProvider`] — local Ollama (default
//!   `http://localhost:11434`), supports any embedding model Ollama has
//!   pulled (`nomic-embed-text`, `mxbai-embed-large`, etc.).
//! - [`HuggingFaceEmbeddingProvider`] — Hugging Face Inference API; works
//!   with any model that has the `feature-extraction` task enabled.
//! - [`CustomHttpEmbeddingProvider`] — generic HTTP shape for any service
//!   exposing a JSON `{ "input": "..." }` → `{ "embedding": [...] }`
//!   endpoint. The escape hatch for cohere, voyage, vLLM-with-jina,
//!   text-embeddings-inference, or any internal service.
//!
//! Existing OpenAI and Local providers remain in
//! [`super::embedding`](super::embedding) for backward compatibility; they
//! will be migrated to this trait in a follow-up patch without changing the
//! public surface.
//!
//! # Adding a custom provider
//!
//! See [`docs/extensibility.md`](../../../docs/extensibility.md) for the
//! end-to-end walkthrough; the short version is:
//!
//! ```ignore
//! use async_trait::async_trait;
//! use contextnest::services::embedding_providers::EmbeddingProvider;
//! use contextnest::error::ContextNestResult;
//!
//! struct MyProvider { /* fields */ }
//!
//! #[async_trait]
//! impl EmbeddingProvider for MyProvider {
//!     fn name(&self) -> &str { "my-provider" }
//!     fn dimension(&self) -> usize { 1024 }
//!     async fn embed(&self, text: &str) -> ContextNestResult<Vec<f32>> {
//!         // ...
//!         Ok(vec![0.0; 1024])
//!     }
//! }
//! ```

use crate::error::ContextNestResult;
use async_trait::async_trait;

pub mod custom_http;
pub mod huggingface;
pub mod ollama;

pub use custom_http::{CustomHttpEmbeddingConfig, CustomHttpEmbeddingProvider};
pub use huggingface::HuggingFaceEmbeddingProvider;
pub use ollama::OllamaEmbeddingProvider;

/// Pluggable embedding-provider interface.
///
/// All built-in providers and any user-provided custom provider implement
/// this trait. The substrate holds providers behind
/// `Arc<dyn EmbeddingProvider + Send + Sync>` so they can be swapped at
/// construction time without changing call sites.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Human-readable provider name used in logs + metrics.
    /// Example: `"ollama"`, `"huggingface"`, `"my-corp-embeddings"`.
    fn name(&self) -> &str;

    /// Embedding vector dimensionality. Used by the substrate to
    /// pre-allocate buffers and to detect dimension-mismatch when multiple
    /// providers are configured (an error condition the manager refuses to
    /// silently paper over).
    fn dimension(&self) -> usize;

    /// Embed a single text fragment. Returns a vector of length
    /// [`Self::dimension`].
    async fn embed(&self, text: &str) -> ContextNestResult<Vec<f32>>;

    /// Embed a batch of fragments. Default implementation calls
    /// [`Self::embed`] N times; providers with native batch endpoints
    /// (OpenAI, HuggingFace's batched mode) should override for efficiency.
    async fn embed_batch(&self, texts: &[&str]) -> ContextNestResult<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            out.push(self.embed(text).await?);
        }
        Ok(out)
    }
}
