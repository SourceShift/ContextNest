//! Hugging Face Inference API embedding provider.
//!
//! Talks to <https://api-inference.huggingface.co/pipeline/feature-extraction/{model}>
//! for any model that has the `feature-extraction` task enabled — covers
//! the bulk of the embedding model ecosystem (sentence-transformers/*,
//! BAAI/bge-*, intfloat/e5-*, Alibaba-NLP/gte-*, etc.).
//!
//! Requires an HF API token. Read-only tokens are fine — embedding is an
//! inference-only operation.

use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use std::time::Duration;

use crate::error::{ContextNestError, ContextNestResult};
use crate::services::embedding_providers::EmbeddingProvider;

/// Hugging Face Inference API embedding provider.
pub struct HuggingFaceEmbeddingProvider {
    api_url: String,
    api_token: String,
    model: String,
    dimension: usize,
    client: Client,
}

impl HuggingFaceEmbeddingProvider {
    /// Build a new HF provider.
    ///
    /// `model` is the HF model id (e.g.
    /// `"sentence-transformers/all-MiniLM-L6-v2"`,
    /// `"BAAI/bge-large-en-v1.5"`). `dimension` is the model's declared
    /// output size; provider asserts the first response matches.
    /// `api_token` is the HF read token (`hf_...`).
    ///
    /// Optional `base_url` overrides the inference API host — useful for
    /// HF Inference Endpoints (your-own-deployment.endpoints.huggingface.cloud).
    /// Pass empty string to use the public inference API.
    pub fn new(
        model: impl Into<String>,
        api_token: impl Into<String>,
        dimension: usize,
        base_url: impl Into<String>,
    ) -> Self {
        let model = model.into();
        let base = base_url.into();
        let api_url = if base.is_empty() {
            format!(
                "https://api-inference.huggingface.co/pipeline/feature-extraction/{}",
                model
            )
        } else {
            // Dedicated inference endpoint: caller-supplied URL is the
            // full endpoint already.
            base.trim_end_matches('/').to_string()
        };
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest Client::new should never fail with default config");
        Self {
            api_url,
            api_token: api_token.into(),
            model,
            dimension,
            client,
        }
    }
}

#[derive(Serialize)]
struct HfRequest<'a> {
    inputs: &'a str,
    options: HfOptions,
}

#[derive(Serialize)]
struct HfOptions {
    wait_for_model: bool,
}

#[async_trait]
impl EmbeddingProvider for HuggingFaceEmbeddingProvider {
    fn name(&self) -> &str {
        "huggingface"
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    async fn embed(&self, text: &str) -> ContextNestResult<Vec<f32>> {
        let body = HfRequest {
            inputs: text,
            // `wait_for_model: true` parks the request until a cold model
            // has loaded. Without it the first call hits a 503 + retry
            // ladder. We prefer a slow first call over silently lost
            // requests; the substrate retries idempotently anyway.
            options: HfOptions {
                wait_for_model: true,
            },
        };

        let resp = self
            .client
            .post(&self.api_url)
            .bearer_auth(&self.api_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                ContextNestError::Api(format!(
                    "HuggingFace embedding request to {} failed: {}",
                    self.api_url, e
                ))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ContextNestError::Api(format!(
                "HuggingFace embedding ({}) returned {}: {}",
                self.model, status, text
            )));
        }

        // HF's feature-extraction returns either a flat array (single
        // input) or a nested array (batch). We send single, so flat.
        // Some HF models wrap the output as `[[float, float, ...]]` —
        // the inference router does extra pooling. Handle both shapes.
        let raw: serde_json::Value = resp.json().await.map_err(|e| {
            ContextNestError::Api(format!(
                "Failed to parse HuggingFace embedding response: {}",
                e
            ))
        })?;

        let vec = parse_hf_embedding(&raw).ok_or_else(|| {
            ContextNestError::Api(format!(
                "HuggingFace returned an unexpected embedding shape for model {}: {}",
                self.model, raw
            ))
        })?;

        if vec.len() != self.dimension {
            return Err(ContextNestError::Api(format!(
                "HuggingFace ({}) returned embedding of length {} but provider was configured for {}",
                self.model,
                vec.len(),
                self.dimension
            )));
        }

        Ok(vec)
    }
}

/// Parse the HF response which may be either:
/// 1. `[f32, f32, ...]`  (single-input, pre-pooled)
/// 2. `[[f32, f32, ...]]` (single-input, list-wrapped — some routers)
/// 3. `[[[f32, ...]]]` (token-level; not what we want — sentence-transformers
///    models return this without pooling enabled. Caller must pick a model
///    that has pooling in its pipeline config)
fn parse_hf_embedding(raw: &serde_json::Value) -> Option<Vec<f32>> {
    if let Some(arr) = raw.as_array() {
        // Try shape 1: flat array of numbers
        if arr.iter().all(|v| v.is_number()) {
            return arr
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect::<Vec<_>>()
                .into();
        }
        // Try shape 2: single nested array
        if arr.len() == 1 {
            if let Some(inner) = arr[0].as_array() {
                if inner.iter().all(|v| v.is_number()) {
                    return inner
                        .iter()
                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                        .collect::<Vec<_>>()
                        .into();
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_shape() {
        let raw: serde_json::Value = serde_json::from_str("[0.1, 0.2, 0.3]").unwrap();
        let parsed = parse_hf_embedding(&raw).unwrap();
        assert_eq!(parsed, vec![0.1_f32, 0.2, 0.3]);
    }

    #[test]
    fn parses_nested_single_shape() {
        let raw: serde_json::Value = serde_json::from_str("[[0.5, 0.6, 0.7]]").unwrap();
        let parsed = parse_hf_embedding(&raw).unwrap();
        assert_eq!(parsed, vec![0.5_f32, 0.6, 0.7]);
    }

    #[test]
    fn rejects_token_level_shape() {
        // 3-level nesting = un-pooled token-level output; we don't pool
        // ourselves, the model config should
        let raw: serde_json::Value = serde_json::from_str("[[[0.1], [0.2]]]").unwrap();
        assert!(parse_hf_embedding(&raw).is_none());
    }

    #[test]
    fn rejects_garbage() {
        let raw: serde_json::Value = serde_json::from_str("\"oops\"").unwrap();
        assert!(parse_hf_embedding(&raw).is_none());
    }

    #[test]
    fn uses_default_inference_api_when_base_empty() {
        let p = HuggingFaceEmbeddingProvider::new(
            "sentence-transformers/all-MiniLM-L6-v2",
            "hf_token",
            384,
            "",
        );
        assert!(p
            .api_url
            .starts_with("https://api-inference.huggingface.co"));
        assert!(p.api_url.contains("all-MiniLM-L6-v2"));
    }

    #[test]
    fn dedicated_endpoint_uses_caller_url() {
        let p = HuggingFaceEmbeddingProvider::new(
            "BAAI/bge-large-en-v1.5",
            "hf_token",
            1024,
            "https://my-deploy.endpoints.huggingface.cloud/",
        );
        assert_eq!(p.api_url, "https://my-deploy.endpoints.huggingface.cloud");
    }
}
