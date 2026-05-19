//! Ollama embedding provider.
//!
//! Talks to a local (or remote) Ollama instance via the `/api/embeddings`
//! endpoint. Works with any embedding model Ollama has pulled —
//! `nomic-embed-text` (768-d), `mxbai-embed-large` (1024-d),
//! `all-minilm` (384-d), `snowflake-arctic-embed` (1024-d), etc.
//!
//! No API key required (Ollama is local + un-authenticated by default).
//! Configure once with the base URL + model id and the substrate has a
//! local-first embedding path with zero per-call cost.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::{ContextNestError, ContextNestResult};
use crate::services::embedding_providers::EmbeddingProvider;

/// Ollama embedding provider.
///
/// Construction does NOT make a network call — the first HTTP request
/// happens on the first `embed` invocation. This lets `EmbeddingService`
/// stay constructible even when Ollama is down at startup.
pub struct OllamaEmbeddingProvider {
    base_url: String,
    model: String,
    dimension: usize,
    client: Client,
}

impl OllamaEmbeddingProvider {
    /// Build a new Ollama provider.
    ///
    /// `base_url` defaults to `http://localhost:11434` when an empty string
    /// is passed. `model` is the Ollama model identifier (must have been
    /// pulled with `ollama pull <model>` before use). `dimension` is the
    /// declared output dimensionality of the chosen model — Ollama doesn't
    /// expose this in metadata, so the caller specifies it and the
    /// provider asserts the first returned vector matches.
    pub fn new(base_url: impl Into<String>, model: impl Into<String>, dimension: usize) -> Self {
        let base_url = base_url.into();
        let base_url = if base_url.is_empty() {
            "http://localhost:11434".to_string()
        } else {
            base_url.trim_end_matches('/').to_string()
        };
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest Client::new should never fail with default config");
        Self {
            base_url,
            model: model.into(),
            dimension,
            client,
        }
    }
}

#[derive(Serialize)]
struct OllamaEmbeddingRequest<'a> {
    model: &'a str,
    prompt: &'a str,
}

#[derive(Deserialize)]
struct OllamaEmbeddingResponse {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for OllamaEmbeddingProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    async fn embed(&self, text: &str) -> ContextNestResult<Vec<f32>> {
        let url = format!("{}/api/embeddings", self.base_url);
        let body = OllamaEmbeddingRequest {
            model: &self.model,
            prompt: text,
        };

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                ContextNestError::Api(format!("Ollama embedding request to {} failed: {}", url, e))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ContextNestError::Api(format!(
                "Ollama embedding returned {}: {}",
                status, text
            )));
        }

        let parsed: OllamaEmbeddingResponse = resp.json().await.map_err(|e| {
            ContextNestError::Api(format!("Failed to parse Ollama embedding response: {}", e))
        })?;

        // Assert the declared dimension matches what came back. Mismatch is
        // almost always a config mistake (wrong dimension number for the
        // chosen model) — fail loud so the operator sees it on the first
        // call rather than getting silent corruption downstream.
        if parsed.embedding.len() != self.dimension {
            return Err(ContextNestError::Api(format!(
                "Ollama returned embedding of length {} but provider was configured for {}; \
                 check the dimension passed to OllamaEmbeddingProvider::new",
                parsed.embedding.len(),
                self.dimension
            )));
        }

        Ok(parsed.embedding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_localhost_when_base_url_empty() {
        let p = OllamaEmbeddingProvider::new("", "nomic-embed-text", 768);
        assert_eq!(p.base_url, "http://localhost:11434");
        assert_eq!(p.name(), "ollama");
        assert_eq!(p.dimension(), 768);
    }

    #[test]
    fn trims_trailing_slash() {
        let p = OllamaEmbeddingProvider::new(
            "http://my-ollama.local:11434/",
            "mxbai-embed-large",
            1024,
        );
        assert_eq!(p.base_url, "http://my-ollama.local:11434");
        assert_eq!(p.dimension(), 1024);
    }
}
