//! Generic HTTP embedding provider — the universal escape hatch.
//!
//! Any service that accepts a JSON body containing the text under a
//! configurable field and returns a JSON body with the embedding under a
//! configurable JSON path can be plugged in without writing Rust.
//!
//! Concrete services this covers out of the box:
//!
//! - **Cohere** — `{ texts: ["..."], model: "..." }` → `{ embeddings: [[...]] }`
//! - **Voyage AI** — `{ input: ["..."], model: "..." }` → `{ data: [{ embedding: [...] }] }`
//! - **Jina AI** — `{ input: ["..."], model: "..." }` → `{ data: [{ embedding: [...] }] }`
//! - **text-embeddings-inference (HF official server)** — `{ inputs: "..." }` → `[[...]]`
//! - **Mistral embeddings** — `{ input: ["..."], model: "..." }` → `{ data: [{ embedding: [...] }] }`
//! - **vLLM with embedding model loaded** — OpenAI-shaped
//! - **Internal microservices** — whatever shape your team chose
//!
//! For shapes more exotic than "field name + JSON path", write a dedicated
//! provider implementing [`EmbeddingProvider`](super::EmbeddingProvider)
//! directly.

use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

use crate::error::{ContextNestError, ContextNestResult};
use crate::services::embedding_providers::EmbeddingProvider;

/// Configuration for the generic HTTP embedding provider.
///
/// All fields are optional except `endpoint`, `input_field`, and
/// `embedding_path`. Sensible defaults work for the OpenAI-shape (`input` /
/// `data.0.embedding`).
#[derive(Clone, Debug)]
pub struct CustomHttpEmbeddingConfig {
    /// Provider name used in logs.
    pub name: String,
    /// Full URL of the embedding endpoint.
    pub endpoint: String,
    /// Optional API key. Sent as `Authorization: Bearer <key>` when set.
    pub api_key: Option<String>,
    /// Field name in the request body holding the text input.
    /// Examples: `"input"` (OpenAI/Voyage/Jina/Mistral), `"inputs"` (HF TEI),
    /// `"text"` (cohere v1), `"texts"` (cohere v3).
    pub input_field: String,
    /// Optional `model` parameter included in the request body if set.
    pub model: Option<String>,
    /// Extra static fields merged into the request body.
    /// Useful for cohere `input_type: "search_document"`,
    /// voyage `truncation: "true"`, etc.
    pub extra_fields: HashMap<String, Value>,
    /// Dot-separated JSON path to the embedding vector in the response.
    /// Examples: `"data.0.embedding"` (OpenAI/Voyage/Jina/Mistral),
    /// `"embeddings.0"` (cohere v3), `"embedding"` (cohere v1 / Ollama-style),
    /// `"0"` (HF TEI returns `[[...]]`).
    pub embedding_path: String,
    /// Declared output dimensionality.
    pub dimension: usize,
    /// Optional extra headers (custom auth, vendor-specific signing, etc.).
    pub extra_headers: HashMap<String, String>,
}

/// Generic HTTP embedding provider built from a [`CustomHttpEmbeddingConfig`].
pub struct CustomHttpEmbeddingProvider {
    config: CustomHttpEmbeddingConfig,
    client: Client,
}

impl CustomHttpEmbeddingProvider {
    pub fn new(config: CustomHttpEmbeddingConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest Client::new should never fail with default config");
        Self { config, client }
    }
}

#[derive(Serialize)]
struct DynamicBody<'a> {
    #[serde(flatten)]
    fields: HashMap<&'a str, Value>,
}

#[async_trait]
impl EmbeddingProvider for CustomHttpEmbeddingProvider {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn dimension(&self) -> usize {
        self.config.dimension
    }

    async fn embed(&self, text: &str) -> ContextNestResult<Vec<f32>> {
        // Assemble the request body field-by-field. The text-input field
        // gets a JSON string; extra fields are passed through as-is.
        let mut fields: HashMap<&str, Value> = HashMap::new();
        fields.insert(
            self.config.input_field.as_str(),
            Value::String(text.to_string()),
        );
        if let Some(model) = &self.config.model {
            fields.insert("model", Value::String(model.clone()));
        }
        for (k, v) in &self.config.extra_fields {
            fields.insert(k.as_str(), v.clone());
        }
        let body = DynamicBody { fields };

        let mut req = self.client.post(&self.config.endpoint).json(&body);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }
        for (k, v) in &self.config.extra_headers {
            req = req.header(k, v);
        }

        let resp = req.send().await.map_err(|e| {
            ContextNestError::Api(format!(
                "Custom HTTP embedding ({}) request to {} failed: {}",
                self.config.name, self.config.endpoint, e
            ))
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ContextNestError::Api(format!(
                "Custom HTTP embedding ({}) returned {}: {}",
                self.config.name, status, text
            )));
        }

        let raw: Value = resp.json().await.map_err(|e| {
            ContextNestError::Api(format!(
                "Failed to parse custom HTTP embedding response: {}",
                e
            ))
        })?;

        let vec = extract_embedding(&raw, &self.config.embedding_path).ok_or_else(|| {
            ContextNestError::Api(format!(
                "Custom HTTP provider ({}) could not find embedding at path '{}' in response: {}",
                self.config.name, self.config.embedding_path, raw
            ))
        })?;

        if vec.len() != self.config.dimension {
            return Err(ContextNestError::Api(format!(
                "Custom HTTP provider ({}) returned embedding of length {} but configured for {}",
                self.config.name,
                vec.len(),
                self.config.dimension
            )));
        }

        Ok(vec)
    }
}

/// Walk a dot-separated JSON path. Numeric segments index arrays; everything
/// else is a string key on objects. Returns `Some(Vec<f32>)` if the final
/// value is a JSON array of numbers.
fn extract_embedding(root: &Value, path: &str) -> Option<Vec<f32>> {
    let mut cur = root;
    for seg in path.split('.') {
        if seg.is_empty() {
            continue;
        }
        cur = match cur {
            Value::Object(map) => map.get(seg)?,
            Value::Array(arr) => {
                let idx: usize = seg.parse().ok()?;
                arr.get(idx)?
            }
            _ => return None,
        };
    }
    let arr = cur.as_array()?;
    let out: Option<Vec<f32>> = arr.iter().map(|v| v.as_f64().map(|f| f as f32)).collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(path: &str) -> CustomHttpEmbeddingConfig {
        CustomHttpEmbeddingConfig {
            name: "test".into(),
            endpoint: "http://x/y".into(),
            api_key: None,
            input_field: "input".into(),
            model: None,
            extra_fields: HashMap::new(),
            embedding_path: path.into(),
            dimension: 3,
            extra_headers: HashMap::new(),
        }
    }

    #[test]
    fn extracts_openai_shape() {
        let raw: Value = serde_json::from_str(r#"{"data":[{"embedding":[0.1,0.2,0.3]}]}"#).unwrap();
        let v = extract_embedding(&raw, "data.0.embedding").unwrap();
        assert_eq!(v, vec![0.1_f32, 0.2, 0.3]);
    }

    #[test]
    fn extracts_cohere_v3_shape() {
        let raw: Value = serde_json::from_str(r#"{"embeddings":[[0.4,0.5,0.6]]}"#).unwrap();
        let v = extract_embedding(&raw, "embeddings.0").unwrap();
        assert_eq!(v, vec![0.4_f32, 0.5, 0.6]);
    }

    #[test]
    fn extracts_tei_shape() {
        let raw: Value = serde_json::from_str("[[0.7,0.8,0.9]]").unwrap();
        let v = extract_embedding(&raw, "0").unwrap();
        assert_eq!(v, vec![0.7_f32, 0.8, 0.9]);
    }

    #[test]
    fn missing_path_returns_none() {
        let raw: Value = serde_json::from_str(r#"{"data":[]}"#).unwrap();
        assert!(extract_embedding(&raw, "data.0.embedding").is_none());
    }

    #[test]
    fn config_builds_provider() {
        let c = cfg("data.0.embedding");
        let p = CustomHttpEmbeddingProvider::new(c);
        assert_eq!(p.name(), "test");
        assert_eq!(p.dimension(), 3);
    }
}
