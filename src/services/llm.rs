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
    /// Caller-supplied [`LanguageModel`] implementation. The escape hatch
    /// for any provider, proxy, or test double the substrate doesn't ship
    /// natively. Construct via
    /// [`LlmServiceBuilder::with_custom_provider`].
    ///
    /// The `name` field is used in logs and metrics only — it does not
    /// affect dispatch.
    Custom {
        name: String,
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
            LlmProvider::Custom { name, .. } => write!(f, "LlmProvider::Custom({})", name),
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
    ///
    /// ### Optional overrides
    ///
    /// * `CONTEXTNEST_LLM_BASE_URL` — overrides the provider's default API
    ///   endpoint, enabling z.ai GLM, LiteLLM proxies, or local Ollama
    ///   instances without any code change.
    /// * `CONTEXTNEST_LLM_MODEL` — overrides the default model id for the
    ///   selected provider.
    ///
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

    /// Identifier for the configured provider, suitable for the
    /// `owned_by` field of OpenAI's `/models` endpoint or any other
    /// caller that wants to display "what's actually wired up". Returns
    /// `None` when the service is [`LlmProvider::Disabled`].
    ///
    /// The string values are stable identifiers — clients can pattern-
    /// match against them — so they live on this method rather than
    /// being derived from `Debug` impls.
    pub fn provider_kind(&self) -> Option<&'static str> {
        match &*self.inner {
            LlmProvider::Disabled => None,
            LlmProvider::Anthropic { .. } => Some("anthropic"),
            LlmProvider::OpenAi { .. } => Some("openai"),
            LlmProvider::Google { .. } => Some("google"),
            LlmProvider::Custom { .. } => Some("custom"),
        }
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
            LlmProvider::Custom { model, .. } => model.clone(),
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

    /// Multi-message chat completion using the configured provider.
    ///
    /// The structured cousin of [`Self::complete`]: accepts a full
    /// conversation (system + user + assistant + tool messages), per-call
    /// generation knobs (temperature, max_tokens, ...), and returns the
    /// model's text + token usage rather than just a string. Designed as
    /// the integration point for the v0.3 LLM proxy
    /// (`src/api/llm_proxy`); see `docs/roadmap/v0.3-llm-proxy.md`.
    ///
    /// Provider routing: this method uses whichever provider the service
    /// was constructed with. Multi-provider routing keyed off the request's
    /// `model` field is slice 1.3 of the v0.3 plan; until then, the
    /// `model` parameter on `ChatCompletionOpts` is informational only —
    /// the configured provider's default model is what actually runs.
    ///
    /// Returns [`ContextNestError::Configuration`] when the provider is
    /// [`LlmProvider::Disabled`]; callers SHOULD check
    /// [`Self::is_enabled`] first and map the error to a transport-
    /// appropriate response (the proxy returns 503).
    pub async fn complete_chat(
        &self,
        opts: ChatCompletionOpts,
    ) -> ContextNestResult<ChatCompletionResult> {
        let model = match &*self.inner {
            LlmProvider::Disabled => {
                return Err(ContextNestError::Configuration(
                    "LLM provider not configured".to_string(),
                ))
            }
            LlmProvider::Anthropic { model } => model.clone(),
            LlmProvider::OpenAi { model } => model.clone(),
            LlmProvider::Google { model } => model.clone(),
            LlmProvider::Custom { model, .. } => model.clone(),
        };

        // Translate the public ChatMessage shape into llm-sdk-rs's
        // Message + Part. Empty content on a user/system message becomes
        // an empty text part rather than skipping the message — the
        // turn order matters even if a message carries no visible text
        // (e.g. assistant-tool-call without textual content).
        let mut llm_messages: Vec<Message> = Vec::with_capacity(opts.messages.len());
        for m in &opts.messages {
            let part = Part::text(m.content.clone());
            let msg = match m.role {
                ChatRole::System => {
                    // llm-sdk-rs models system content via the
                    // `system_prompt` field on `LanguageModelInput`, not
                    // via a `Message::system`. We aggregate multiple
                    // system messages into one system_prompt below.
                    continue;
                }
                ChatRole::User => Message::user([part]),
                ChatRole::Assistant => Message::assistant([part]),
                ChatRole::Tool => {
                    // No first-class tool-role message in 0.3; map to a
                    // user turn carrying the tool result text. Slice 1.3
                    // will revisit when tool-calling is wired through.
                    Message::user([part])
                }
            };
            llm_messages.push(msg);
        }

        // Aggregate any system messages into a single system_prompt — the
        // common case is one system message anyway; multiple are joined
        // with newlines.
        let system_prompt: Option<String> = {
            let combined: Vec<String> = opts
                .messages
                .iter()
                .filter(|m| matches!(m.role, ChatRole::System))
                .map(|m| m.content.clone())
                .collect();
            if combined.is_empty() {
                None
            } else {
                Some(combined.join("\n\n"))
            }
        };

        let input = LanguageModelInput {
            system_prompt,
            messages: llm_messages,
            temperature: opts.temperature.map(|t| t as f64),
            top_p: opts.top_p.map(|t| t as f64),
            max_tokens: opts.max_tokens,
            seed: opts.seed.map(|s| s as i64),
            presence_penalty: opts.presence_penalty.map(|p| p as f64),
            frequency_penalty: opts.frequency_penalty.map(|p| p as f64),
            ..Default::default()
        };

        let response = model
            .generate(input)
            .await
            .map_err(|e| ContextNestError::Api(format!("LLM provider error: {e}")))?;

        // Concatenate every text part in response.content. A model that
        // returns multiple text parts (e.g. interspersed with tool_use
        // parts not yet handled) gets joined with empty separator so the
        // visible text is the user-facing string. Tool-use part handling
        // arrives with slice 1.2.x or 1.3 depending on routing scope.
        let text = response
            .content
            .iter()
            .filter_map(|p| {
                if let Part::Text(t) = p {
                    Some(t.text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");

        let (input_tokens, output_tokens) = response
            .usage
            .as_ref()
            .map(|u| (u.input_tokens, u.output_tokens))
            .unwrap_or((0, 0));

        Ok(ChatCompletionResult {
            text,
            input_tokens,
            output_tokens,
        })
    }
}

// ---------------------------------------------------------------------------
// Public chat-completion types
// ---------------------------------------------------------------------------

/// Conversation role for [`ChatMessage`]. Matches the OpenAI wire format
/// (`system` / `user` / `assistant` / `tool`); the proxy translates from
/// `api::llm_proxy::openai_shapes::Role` into this enum 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

/// One turn in a chat completion request. Carries a role + text content.
/// Multimodal / structured content parts are reduced to text by the proxy
/// before reaching this type — slice 1.2 ships text-only chat completion;
/// multimodal pass-through is a later slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

/// Knobs the chat-completion handler can set when calling the LLM. Mirrors
/// the OpenAI request surface that matters for cache-key derivation
/// (`temperature`, `max_tokens`, `system_prompt` derived from messages),
/// keeping the cache layer in Phase 2 able to consume this shape directly.
#[derive(Debug, Clone, Default)]
pub struct ChatCompletionOpts {
    /// Provider-specific model identifier from the request. Informational
    /// only in slice 1.2 — multi-provider routing keyed off this lands in
    /// slice 1.3.
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub seed: Option<u64>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
}

/// Output of [`LlmService::complete_chat`]. The proxy maps this into the
/// OpenAI `ChatCompletionsResponse` shape (id, object, created, model,
/// choices, usage) — `text` becomes the single choice's message content,
/// the token counters land in the `usage` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatCompletionResult {
    pub text: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
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
// Builder — programmatic construction (the extensibility entry point)
// ---------------------------------------------------------------------------

/// Programmatic construction path for [`LlmService`].
///
/// Use this when env-var-driven config is the wrong fit — most commonly:
/// integration tests with a mock LLM, embedding the substrate as a library
/// inside another Rust app that owns its own config story, or plugging a
/// caller-supplied custom provider implementing the `LanguageModel` trait
/// directly.
///
/// ```ignore
/// use std::sync::Arc;
/// use contextnest::services::{LlmService, LlmServiceBuilder};
///
/// // Use a built-in provider:
/// let service = LlmServiceBuilder::new()
///     .with_openai_compatible(
///         "http://localhost:11434/v1",
///         "ollama",          // any non-empty key works for local Ollama
///         "llama3",
///     )
///     .build();
///
/// // Or plug a fully custom LanguageModel impl:
/// // let service = LlmServiceBuilder::new()
/// //     .with_custom_provider("my-internal-llm", Arc::new(my_model))
/// //     .build();
/// ```
pub struct LlmServiceBuilder {
    provider: LlmProvider,
}

impl Default for LlmServiceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmServiceBuilder {
    /// Start with a disabled provider. Add a provider via one of the
    /// `with_*` methods before calling [`Self::build`].
    pub fn new() -> Self {
        Self {
            provider: LlmProvider::Disabled,
        }
    }

    /// Build an Anthropic provider with explicit credentials.
    pub fn with_anthropic(
        mut self,
        api_key: impl Into<String>,
        base_url: Option<String>,
        model: Option<String>,
    ) -> Self {
        self.provider = build_anthropic(api_key.into(), base_url, model);
        self
    }

    /// Build an OpenAI provider with explicit credentials.
    pub fn with_openai(
        mut self,
        api_key: impl Into<String>,
        base_url: Option<String>,
        model: Option<String>,
    ) -> Self {
        self.provider = build_openai(api_key.into(), base_url, model);
        self
    }

    /// Build a Google provider with explicit credentials.
    pub fn with_google(
        mut self,
        api_key: impl Into<String>,
        base_url: Option<String>,
        model: Option<String>,
    ) -> Self {
        self.provider = build_google(api_key.into(), base_url, model);
        self
    }

    /// Configure any OpenAI-compatible endpoint — Ollama, LiteLLM, vLLM,
    /// LM Studio, OpenRouter, Together, Groq, Anyscale, Fireworks,
    /// Mistral, Anyscale, or any self-hosted gateway speaking the
    /// `/v1/chat/completions` shape.
    ///
    /// `api_key` may be a placeholder string for endpoints that don't
    /// enforce auth (local Ollama). It is still sent in the
    /// `Authorization: Bearer …` header because most proxies require
    /// *some* value and reject empty bearers.
    pub fn with_openai_compatible(
        mut self,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        self.provider = build_openai(api_key.into(), Some(base_url.into()), Some(model.into()));
        self
    }

    /// Plug a caller-supplied [`LanguageModel`] implementation. The
    /// substrate dispatches through the trait object — anything that
    /// satisfies `llm_sdk::LanguageModel + Send + Sync` works.
    ///
    /// `name` appears in logs and metrics (`LlmProvider::Custom(<name>)`)
    /// and is the only thing distinguishing custom providers from each
    /// other on the wire.
    pub fn with_custom_provider(
        mut self,
        name: impl Into<String>,
        model: Arc<dyn LanguageModel + Send + Sync>,
    ) -> Self {
        self.provider = LlmProvider::Custom {
            name: name.into(),
            model,
        };
        self
    }

    /// Finalise the builder into an [`LlmService`].
    ///
    /// A builder that never had a `with_*` call returns a disabled
    /// service — same contract as [`LlmService::from_env`] without keys.
    pub fn build(self) -> LlmService {
        LlmService::new(self.provider)
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
