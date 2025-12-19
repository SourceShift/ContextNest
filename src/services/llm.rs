//! LLM provider service for the ContextNest memory substrate.
//! Phase J wires `llm-sdk-rs` into the substrate so memory operations that
//! benefit from language understanding (e.g. `summarize`) get a real LLM-backed
//! body rather than a statistics-only approximation.
//! ## Multi-provider design
//! The [`LlmService`] wraps an [`LlmProvider`] enum whose variants hold
//! fully-constructed `llm-sdk-rs` model instances. Provider selection is
//! entirely config-driven via env vars; no code change is required to switch
//! between Anthropic, OpenAI, or Google, or to route traffic through a proxy
//! (e.g. z.ai GLM via an Anthropic-compatible endpoint).
//! ## Graceful degradation
//! [`LlmProvider::Disabled`] is the default when no API key is present. All
//! callers MUST check [`LlmService::is_enabled`] before calling `complete` /
//! `summarize` and degrade to a non-LLM code path rather than propagating an
//! error to the user. The `summarize` HTTP handler in `api/tools.rs` is the
//! canonical example of this pattern.

use std::sync::Arc;

// The package name on crates.io is `llm-sdk-rs`; the Rust crate name (lib
// name) is `llm_sdk` — no `_rs` suffix. This is a load-bearing gotcha for
// any downstream agent: always use `llm_sdk` in `use` paths.
use llm_sdk::{
    anthropic::{AnthropicModel, AnthropicModelOptions},
    google::{GoogleModel, GoogleModelOptions},
    openai::{OpenAIModel, OpenAIModelOptions},
    LanguageModel, LanguageModelInput, Message, Part,
};

use crate::error::{ContextNestError, ContextNestResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default Anthropic model used when `CONTEXTNEST_LLM_MODEL` is not set.
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-3-5-haiku-20241022";
/// Default OpenAI model used when `CONTEXTNEST_LLM_MODEL` is not set.
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
/// Default Google model used when `CONTEXTNEST_LLM_MODEL` is not set.
const DEFAULT_GOOGLE_MODEL: &str = "gemini-2.0-flash";

/// Summarization prompt template. `{target_tokens}` is replaced at call time;
/// the fragment list is appended after the template header. Keeping the prompt
/// here (not inline in the method) makes it easy for the next integrating agent
/// to override without hunting through handler logic.
const SUMMARIZE_PROMPT_TEMPLATE: &str = r#"You are a memory compression assistant for an LLM agent substrate.

Summarize the following memory fragments into a single cohesive summary of approximately {target_tokens} tokens. Preserve all key facts, entities, and relationships. Discard redundant phrasing. Output only the summary — no preamble, no explanation.

MEMORY FRAGMENTS:
{fragments}

SUMMARY:"#;

// ---------------------------------------------------------------------------
// Provider enum
// ---------------------------------------------------------------------------

/// Holds the live provider model instance (or `Disabled` sentinel).
/// Each variant carries an `Arc<dyn LanguageModel>` rather than the concrete
/// type so the rest of the code stays provider-agnostic after construction.
/// The `Arc` allows `LlmService` to be `Clone + Send + Sync` cheaply.
#[derive(Clone)]
pub enum LlmProvider {
    /// No LLM configured — `complete` / `summarize` return
    /// [`ContextNestError::Configuration`]. Callers must check
    /// [`LlmService::is_enabled`] first.
    Disabled,
    /// Anthropic Claude family (or any Anthropic-protocol-compatible proxy
    /// such as z.ai when `CONTEXTNEST_LLM_BASE_URL` is set).
    Anthropic {
        model: Arc<dyn LanguageModel + Send + Sync>,
    },
    /// OpenAI / compatible endpoints (LiteLLM, Azure OpenAI, etc.).
    OpenAi {
        model: Arc<dyn LanguageModel + Send + Sync>,
    },
    /// Google Gemini.
    Google {
        model: Arc<dyn LanguageModel + Send + Sync>,
    },
}

// Manual Debug so we don't require LanguageModel: Debug.
impl std::fmt::Debug for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmProvider::Disabled => write!(f, "LlmProvider::Disabled"),
            LlmProvider::Anthropic { .. } => write!(f, "LlmProvider::Anthropic"),
            LlmProvider::OpenAi { .. } => write!(f, "LlmProvider::OpenAi"),
            LlmProvider::Google { .. } => write!(f, "LlmProvider::Google"),
        }
    }
}

// ---------------------------------------------------------------------------
// LlmService
// ---------------------------------------------------------------------------

/// Config-driven LLM abstraction backing substrate operations that require
/// language understanding (summarize compression, gap-fill content generation,
/// decay-driven consolidation summaries).
/// Always present as a field on [`crate::services::ContextNestServices`];
/// callers must check [`Self::is_enabled`] before invoking [`Self::complete`]
/// or [`Self::summarize`] and degrade gracefully when it returns `false`.
#[derive(Clone, Debug)]
pub struct LlmService {
    inner: Arc<LlmProvider>,
}

impl LlmService {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Construct from environment variables. Returns an [`LlmService`] with
    /// [`LlmProvider::Disabled`] when no recognisable provider config is
    /// present — so [`crate::services::ContextNestServices::new`] never fails
    /// just because a dev hasn't set up an LLM key.
    /// ### Resolution order (first match wins)
    /// 1. `CONTEXTNEST_LLM_PROVIDER` = `"anthropic"` + `ANTHROPIC_API_KEY`
    /// 2. `CONTEXTNEST_LLM_PROVIDER` = `"openai"` + `OPENAI_API_KEY`
    /// 3. `CONTEXTNEST_LLM_PROVIDER` = `"google"` + `GOOGLE_API_KEY`
    /// 4. Legacy auto-detect: if `ANTHROPIC_API_KEY` is set → `anthropic`;
    ///    else if `OPENAI_API_KEY` is set → `openai`;
    ///    else if `GOOGLE_API_KEY` is set → `google`;
    ///    else → `Disabled`.
    /// ### Optional overrides
    /// * `CONTEXTNEST_LLM_BASE_URL` — overrides the provider's default API
    ///   endpoint, enabling z.ai GLM, LiteLLM proxies, or local Ollama
    ///   instances without any code change.
    /// * `CONTEXTNEST_LLM_MODEL` — overrides the default model id for the
    ///   selected provider.
    /// No network call is made at construction time; the provider model is
    /// configured but the first actual HTTP call happens on the first
    /// `complete` invocation.
    pub fn from_env() -> Self {
        let provider_name = std::env::var("CONTEXTNEST_LLM_PROVIDER")
            .ok()
            .map(|s| s.to_lowercase());
        let base_url = std::env::var("CONTEXTNEST_LLM_BASE_URL").ok();
        let model_override = std::env::var("CONTEXTNEST_LLM_MODEL").ok();

        let provider = match provider_name.as_deref() {
            Some("anthropic") => {
                let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
                if api_key.is_empty() {
                    tracing::warn!(
                        "CONTEXTNEST_LLM_PROVIDER=anthropic but ANTHROPIC_API_KEY is not set; \
                         LLM disabled"
                    );
                    LlmProvider::Disabled
                } else {
                    build_anthropic(api_key, base_url, model_override)
                }
            }
            Some("openai") => {
                let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
                if api_key.is_empty() {
                    tracing::warn!(
                        "CONTEXTNEST_LLM_PROVIDER=openai but OPENAI_API_KEY is not set; \
                         LLM disabled"
                    );
                    LlmProvider::Disabled
                } else {
                    build_openai(api_key, base_url, model_override)
                }
            }
            Some("google") => {
                let api_key = std::env::var("GOOGLE_API_KEY").unwrap_or_default();
                if api_key.is_empty() {
                    tracing::warn!(
                        "CONTEXTNEST_LLM_PROVIDER=google but GOOGLE_API_KEY is not set; \
                         LLM disabled"
                    );
                    LlmProvider::Disabled
                } else {
                    build_google(api_key, base_url, model_override)
                }
            }
            Some(unknown) => {
                tracing::warn!(
                    provider = unknown,
                    "Unknown CONTEXTNEST_LLM_PROVIDER value; LLM disabled"
                );
                LlmProvider::Disabled
            }
            // No explicit provider — attempt auto-detect from well-known key vars.
            None => {
                if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
                    if !api_key.is_empty() {
                        tracing::debug!(
                            "Auto-detected ANTHROPIC_API_KEY; enabling Anthropic provider"
                        );
                        return Self {
                            inner: Arc::new(build_anthropic(api_key, base_url, model_override)),
                        };
                    }
                }
                if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
                    if !api_key.is_empty() {
                        tracing::debug!("Auto-detected OPENAI_API_KEY; enabling OpenAI provider");
                        return Self {
                            inner: Arc::new(build_openai(api_key, base_url, model_override)),
                        };
                    }
                }
                if let Ok(api_key) = std::env::var("GOOGLE_API_KEY") {
                    if !api_key.is_empty() {
                        tracing::debug!("Auto-detected GOOGLE_API_KEY; enabling Google provider");
                        return Self {
                            inner: Arc::new(build_google(api_key, base_url, model_override)),
                        };
                    }
                }
                LlmProvider::Disabled
            }
        };

        Self {
            inner: Arc::new(provider),
        }
    }

    /// Construct directly with a given provider — useful for tests that want to
    /// inject a specific (possibly mock) provider without touching env vars.
    pub fn new(provider: LlmProvider) -> Self {
        Self {
            inner: Arc::new(provider),
        }
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Returns `true` when a real LLM provider is wired up.
    /// Callers MUST check this before calling [`Self::complete`] or
    /// [`Self::summarize`] and degrade to a non-LLM path when it returns
    /// `false`. The HTTP handlers follow this pattern so that CI runs without
    /// any API key continue to pass.
    pub fn is_enabled(&self) -> bool {
        !matches!(*self.inner, LlmProvider::Disabled)
    }

    // -----------------------------------------------------------------------
    // Core operations
    // -----------------------------------------------------------------------

    /// One-shot text completion using the configured provider.
    /// Returns the first text part from the model's response. Returns
    /// [`ContextNestError::Configuration`] when the provider is
    /// [`LlmProvider::Disabled`] — callers should always call
    /// [`Self::is_enabled`] first and degrade rather than surfacing this error
    /// to end users.
    pub async fn complete(&self, prompt: &str) -> ContextNestResult<String> {
        let model = match &*self.inner {
            LlmProvider::Disabled => {
                return Err(ContextNestError::Configuration(
                    "LLM provider not configured".to_string(),
                ))
            }
            LlmProvider::Anthropic { model } => model.clone(),
            LlmProvider::OpenAi { model } => model.clone(),
            LlmProvider::Google { model } => model.clone(),
        };

        let input = LanguageModelInput {
            messages: vec![Message::user([Part::text(prompt)])],
            ..Default::default()
        };

        let response = model
            .generate(input)
            .await
            .map_err(|e| ContextNestError::Api(format!("LLM provider error: {e}")))?;

        // Extract the first text part from the response content.
        let text = response
            .content
            .into_iter()
            .find_map(|part| {
                if let Part::Text(text_part) = part {
                    Some(text_part.text)
                } else {
                    None
                }
            })
            .unwrap_or_default();

        Ok(text)
    }

    /// Summarize a slice of texts down to approximately `target_tokens` tokens.
    /// Builds the canonical ContextNest summarization prompt (system+user form)
    /// and forwards to [`Self::complete`]. Returns the summary as a plain
    /// string suitable for storing as a new fragment via the `store` pipeline.
    /// `target_tokens` is passed to the prompt so the model calibrates its
    /// output length. `None` defaults to 200 tokens (a reasonable "one
    /// paragraph" target for most memory regions).
    /// Returns [`ContextNestError::Configuration`] when the provider is
    /// `Disabled`; callers should check [`Self::is_enabled`] first.
    pub async fn summarize(
        &self,
        texts: &[String],
        target_tokens: Option<usize>,
    ) -> ContextNestResult<String> {
        if matches!(*self.inner, LlmProvider::Disabled) {
            return Err(ContextNestError::Configuration(
                "LLM provider not configured".to_string(),
            ));
        }

        let target = target_tokens.unwrap_or(200);
        let fragments_block = texts
            .iter()
            .enumerate()
            .map(|(i, t)| format!("[{}] {}", i + 1, t))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = SUMMARIZE_PROMPT_TEMPLATE
            .replace("{target_tokens}", &target.to_string())
            .replace("{fragments}", &fragments_block);

        self.complete(&prompt).await
    }
}

// ---------------------------------------------------------------------------
// Provider construction helpers
// ---------------------------------------------------------------------------

/// Build an Anthropic provider model. Accepts an optional `base_url` override
/// so callers can route to z.ai or another Anthropic-protocol-compatible proxy.
fn build_anthropic(
    api_key: String,
    base_url: Option<String>,
    model_id: Option<String>,
) -> LlmProvider {
    let model_id = model_id.unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.to_string());
    tracing::info!(model = %model_id, base_url = ?base_url, "LLM: Anthropic provider configured");
    let options = AnthropicModelOptions {
        api_key,
        base_url,
        ..Default::default()
    };
    let model = AnthropicModel::new(model_id, options);
    LlmProvider::Anthropic {
        model: Arc::new(model),
    }
}

/// Build an OpenAI provider model with optional base URL override.
fn build_openai(
    api_key: String,
    base_url: Option<String>,
    model_id: Option<String>,
) -> LlmProvider {
    let model_id = model_id.unwrap_or_else(|| DEFAULT_OPENAI_MODEL.to_string());
    tracing::info!(model = %model_id, base_url = ?base_url, "LLM: OpenAI provider configured");
    let options = OpenAIModelOptions {
        api_key,
        base_url,
        ..Default::default()
    };
    let model = OpenAIModel::new(model_id, options);
    LlmProvider::OpenAi {
        model: Arc::new(model),
    }
}

/// Build a Google Gemini provider model with optional base URL override.
fn build_google(
    api_key: String,
    base_url: Option<String>,
    model_id: Option<String>,
) -> LlmProvider {
    let model_id = model_id.unwrap_or_else(|| DEFAULT_GOOGLE_MODEL.to_string());
    tracing::info!(model = %model_id, base_url = ?base_url, "LLM: Google provider configured");
    let options = GoogleModelOptions {
        api_key,
        base_url,
        ..Default::default()
    };
    let model = GoogleModel::new(model_id, options);
    LlmProvider::Google {
        model: Arc::new(model),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// When no LLM env vars are set the service must construct successfully and
    /// report itself as disabled. This is the CI / offline-dev guarantee: no
    /// API key → no panic, no error at startup.
    #[test]
    fn from_env_with_no_keys_returns_disabled() {
        // Temporarily clear any keys that might be set in the developer's
        // environment so the test is hermetic. We restore them afterward.
        let provider_backup = std::env::var("CONTEXTNEST_LLM_PROVIDER").ok();
        let anthropic_backup = std::env::var("ANTHROPIC_API_KEY").ok();
        let openai_backup = std::env::var("OPENAI_API_KEY").ok();
        let google_backup = std::env::var("GOOGLE_API_KEY").ok();

        std::env::remove_var("CONTEXTNEST_LLM_PROVIDER");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("GOOGLE_API_KEY");

        let service = LlmService::from_env();
        let enabled = service.is_enabled();

        // Restore env vars so we don't pollute other tests that run in the
        // same process. This is best-effort; a panic before this point would
        // leave env dirty, but that only affects other tests in this module.
        if let Some(v) = provider_backup {
            std::env::set_var("CONTEXTNEST_LLM_PROVIDER", v);
        }
        if let Some(v) = anthropic_backup {
            std::env::set_var("ANTHROPIC_API_KEY", v);
        }
        if let Some(v) = openai_backup {
            std::env::set_var("OPENAI_API_KEY", v);
        }
        if let Some(v) = google_backup {
            std::env::set_var("GOOGLE_API_KEY", v);
        }

        assert!(
            !enabled,
            "from_env with no keys must yield a Disabled service"
        );
    }

    /// Calling `complete` on a `Disabled` service must return
    /// [`ContextNestError::Configuration`] — never panic or make a network
    /// call.
    #[tokio::test]
    async fn disabled_complete_returns_configuration_error() {
        let service = LlmService::new(LlmProvider::Disabled);
        let result = service.complete("test prompt").await;

        assert!(result.is_err(), "Disabled service must error on complete");
        match result.unwrap_err() {
            ContextNestError::Configuration(msg) => {
                assert!(
                    msg.contains("not configured"),
                    "error message should indicate LLM is not configured; got: {msg}"
                );
            }
            other => panic!("Expected Configuration error, got: {other:?}"),
        }
    }

    /// Calling `summarize` on a `Disabled` service must return
    /// [`ContextNestError::Configuration`] — same guarantee as `complete`.
    #[tokio::test]
    async fn disabled_summarize_returns_configuration_error() {
        let service = LlmService::new(LlmProvider::Disabled);
        let texts = vec![
            "memory fragment one".to_string(),
            "memory fragment two".to_string(),
        ];
        let result = service.summarize(&texts, Some(50)).await;

        assert!(result.is_err(), "Disabled service must error on summarize");
        match result.unwrap_err() {
            ContextNestError::Configuration(msg) => {
                assert!(
                    msg.contains("not configured"),
                    "error message should indicate LLM is not configured; got: {msg}"
                );
            }
            other => panic!("Expected Configuration error, got: {other:?}"),
        }
    }
}
