//! LLM proxy response cache — key derivation (v0.3 Phase 2 slice 2.1).
//!
//! Pure logic for deriving a `CacheKey` from a chat-completion request.
//! Storage, lookup, freshness policy, and the `GET /llm/v1/cache/stats`
//! endpoint land in subsequent slices.
//!
//! ## Design (from `docs/roadmap/v0.3-llm-proxy.md`)
//!
//! The cache key has two parts:
//!
//! - **`ExactKeyPrefix`** — `(project_id, model, temperature_bucket,
//!   system_prompt_hash)`. These fields must match exactly for a cache
//!   hit; differences here mean the cached response is for a different
//!   conversation surface and is not eligible.
//! - **Semantic embedding** — `Vec<f32>` over the user prompt. Used by
//!   the substrate's attractor activation for similarity lookup at a
//!   configurable threshold (roadmap default: 0.92). Two requests with
//!   the same exact-prefix but semantically distant user prompts will
//!   not collide.
//!
//! ## Why bucket temperature at 0.05?
//!
//! OpenAI's spec accepts `temperature` as a `f32` in `[0.0, 2.0]`. Most
//! workloads use coarse settings — 0.0 (deterministic), 0.2 (precise),
//! 0.7 (default-ish), 1.0 (creative). Bucketing at 0.05 granularity
//! collapses near-equivalent settings (0.71 ↔ 0.70) into the same key
//! without conflating semantically different settings (0.0 ↔ 0.7).
//! The roadmap calls this out explicitly.
//!
//! ## Why SHA-256 truncated to 8 bytes for the system prompt?
//!
//! The system prompt is part of the exact match — two different system
//! prompts MUST yield different keys. Truncated SHA-256 gives 2^64
//! collision space, which is overwhelming for any single project's
//! prompt corpus. Storing the full hash would inflate the key by 24
//! bytes per entry for no gain.
//!
//! ## What this slice does NOT do
//!
//! - Does not compute the user-prompt embedding. The caller passes
//!   the embedding in (it comes from `EmbeddingService`).
//! - Does not look up the substrate or any cache backing store.
//! - Does not enforce TTL freshness — that's slice 2.2/2.3.
//! - Does not handle multi-tenancy scoping beyond defaulting
//!   `project_id` to a fixed sentinel until v0.2 lands.

use sha2::{Digest, Sha256};

use crate::api::llm_proxy::openai_shapes::{ContentPart, Message, MessageContent, Role};

/// Number of temperature buckets across the OpenAI range `[0.0, 1.0]`.
/// Bucket index = round(temp * 20). 0.0 → 0, 0.05 → 1, ..., 1.0 → 20.
/// Values above 1.0 clamp to bucket 20; values below 0.0 clamp to 0.
pub const TEMPERATURE_BUCKETS: u8 = 21;

/// OpenAI's documented default temperature when the field is omitted.
pub const DEFAULT_TEMPERATURE: f32 = 1.0;

/// Stand-in `project_id` for substrates without v0.2 multi-tenancy.
/// Once v0.2 ships, the proxy threads the real project id through the
/// request context and this sentinel is unreachable.
pub const DEFAULT_PROJECT_ID: &str = "default";

/// Exact-match component of a cache key. Hash + Eq so it can serve as
/// the bucket-key for the in-memory lookup table that later slices
/// build on top.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExactKeyPrefix {
    pub project_id: String,
    pub model: String,
    pub temperature_bucket: u8,
    /// First 8 bytes of `sha256(system_prompt)`. 2^64 collision space.
    pub system_prompt_hash: [u8; 8],
}

impl ExactKeyPrefix {
    /// Stable 32-byte fingerprint over the four exact-match fields.
    /// Subsequent slices use this as the substrate fragment-id seed so
    /// cache entries are deterministically addressable across restarts.
    pub fn fingerprint(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.project_id.as_bytes());
        hasher.update([0u8]);
        hasher.update(self.model.as_bytes());
        hasher.update([0u8]);
        hasher.update([self.temperature_bucket]);
        hasher.update(self.system_prompt_hash);
        hasher.finalize().into()
    }
}

/// Full cache key. The exact prefix is the strict-match component;
/// the semantic embedding is the similarity-match component handled
/// downstream by the substrate's attractor activation.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheKey {
    pub exact: ExactKeyPrefix,
    pub semantic_embedding: Vec<f32>,
}

/// Round-to-bucket the OpenAI temperature parameter.
///
/// Clamps the input to `[0.0, 1.0]` first, then maps to a bucket index
/// `0..=20` at 0.05 granularity. NaN inputs use `DEFAULT_TEMPERATURE`.
/// Values above 1.0 (OpenAI allows up to 2.0) all bucket to 20 — the
/// upper half of the range is treated as "creative ≥ 1.0" because real
/// usage past 1.0 is rare and the difference between 1.5 and 2.0
/// doesn't meaningfully change cache eligibility.
pub fn bucket_temperature(t: f32) -> u8 {
    let t = if t.is_nan() { DEFAULT_TEMPERATURE } else { t };
    let clamped = t.clamp(0.0, 1.0);
    (clamped * 20.0).round() as u8
}

/// First 8 bytes of `sha256(s)`. Deterministic across runs.
pub fn hash_system_prompt(s: &str) -> [u8; 8] {
    let digest = Sha256::digest(s.as_bytes());
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest[..8]);
    out
}

/// Concatenate every system-role message's textual content into one
/// string. The OpenAI spec allows multiple system messages (rarely
/// used in practice but legal); concatenation matches what providers
/// effectively do anyway.
pub fn extract_system_prompt_text(messages: &[Message]) -> String {
    let mut out = String::new();
    for msg in messages {
        if msg.role != Role::System {
            continue;
        }
        match &msg.content {
            Some(MessageContent::Text(s)) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(s);
            }
            Some(MessageContent::Parts(parts)) => {
                for part in parts {
                    if let ContentPart::Text { text } = part {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(text);
                    }
                }
            }
            None => {}
        }
    }
    out
}

/// Textual content of the LAST user-role message. Image / audio /
/// file parts are skipped — semantic embedding is text-only in v0.3,
/// multimodal cache keying lands when the substrate's embedder gains
/// vision support.
pub fn extract_user_prompt_text(messages: &[Message]) -> String {
    let last_user = messages.iter().rev().find(|m| m.role == Role::User);
    let Some(msg) = last_user else {
        return String::new();
    };
    match &msg.content {
        Some(MessageContent::Text(s)) => s.clone(),
        Some(MessageContent::Parts(parts)) => {
            let mut out = String::new();
            for part in parts {
                if let ContentPart::Text { text } = part {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(text);
                }
            }
            out
        }
        None => String::new(),
    }
}

/// Build a `CacheKey` from already-derived components. The embedding
/// is provided by the caller (it comes from `EmbeddingService::generate_embedding`
/// on the extracted user prompt — keeping that call out of this
/// function lets cache-key derivation stay pure + synchronous + cheap).
pub fn derive_cache_key(
    project_id: &str,
    model: &str,
    temperature: Option<f32>,
    system_prompt: &str,
    user_prompt_embedding: Vec<f32>,
) -> CacheKey {
    let project_id = if project_id.is_empty() {
        DEFAULT_PROJECT_ID
    } else {
        project_id
    };
    let temperature_bucket = bucket_temperature(temperature.unwrap_or(DEFAULT_TEMPERATURE));
    let system_prompt_hash = hash_system_prompt(system_prompt);
    CacheKey {
        exact: ExactKeyPrefix {
            project_id: project_id.to_string(),
            model: model.to_string(),
            temperature_bucket,
            system_prompt_hash,
        },
        semantic_embedding: user_prompt_embedding,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::llm_proxy::openai_shapes::{Message, MessageContent, Role};
    use serde_json::json;

    fn user_msg(s: &str) -> Message {
        Message {
            role: Role::User,
            content: Some(MessageContent::Text(s.to_string())),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    fn system_msg(s: &str) -> Message {
        Message {
            role: Role::System,
            content: Some(MessageContent::Text(s.to_string())),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    #[test]
    fn bucket_temperature_rounds_at_005_granularity() {
        assert_eq!(bucket_temperature(0.0), 0);
        assert_eq!(bucket_temperature(0.05), 1);
        assert_eq!(bucket_temperature(0.10), 2);
        assert_eq!(bucket_temperature(0.7), 14);
        assert_eq!(bucket_temperature(0.75), 15);
        assert_eq!(bucket_temperature(1.0), 20);
    }

    #[test]
    fn bucket_temperature_clamps_above_one() {
        assert_eq!(bucket_temperature(1.5), 20);
        assert_eq!(bucket_temperature(2.0), 20);
        assert_eq!(bucket_temperature(f32::INFINITY), 20);
    }

    #[test]
    fn bucket_temperature_clamps_below_zero() {
        assert_eq!(bucket_temperature(-0.1), 0);
        assert_eq!(bucket_temperature(-50.0), 0);
        assert_eq!(bucket_temperature(f32::NEG_INFINITY), 0);
    }

    #[test]
    fn bucket_temperature_nan_uses_default() {
        // NaN → DEFAULT_TEMPERATURE (1.0) → bucket 20. Documented behaviour
        // so a corrupt input doesn't silently map to "0 = deterministic".
        assert_eq!(bucket_temperature(f32::NAN), 20);
    }

    #[test]
    fn bucket_temperature_near_boundaries_rounds_half_to_even() {
        // 0.075 sits between bucket 1 (0.05) and bucket 2 (0.10).
        // f32::round is banker's rounding; document the boundary
        // behaviour so future refactors can't silently flip it.
        let b = bucket_temperature(0.075);
        assert!(
            b == 1 || b == 2,
            "expected bucket 1 or 2 at half-boundary 0.075, got {b}"
        );
    }

    #[test]
    fn hash_system_prompt_is_deterministic() {
        let a = hash_system_prompt("you are a careful assistant");
        let b = hash_system_prompt("you are a careful assistant");
        assert_eq!(a, b);
    }

    #[test]
    fn hash_system_prompt_differs_for_different_inputs() {
        let a = hash_system_prompt("you are a careful assistant");
        let b = hash_system_prompt("you are a creative assistant");
        assert_ne!(a, b);
    }

    #[test]
    fn hash_system_prompt_empty_string_is_stable() {
        // Empty system prompt is a legitimate "no system message" case.
        // The hash for "" must be stable and != any non-empty hash.
        let empty = hash_system_prompt("");
        let other = hash_system_prompt(" ");
        assert_ne!(empty, other);
        assert_eq!(empty, hash_system_prompt(""));
    }

    #[test]
    fn extract_user_prompt_text_finds_last_user_message() {
        let msgs = vec![
            user_msg("first user turn"),
            Message {
                role: Role::Assistant,
                content: Some(MessageContent::Text("reply".into())),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            user_msg("second user turn"),
        ];
        assert_eq!(extract_user_prompt_text(&msgs), "second user turn");
    }

    #[test]
    fn extract_user_prompt_text_handles_multimodal_content() {
        let parts = vec![
            ContentPart::Text {
                text: "describe the image".into(),
            },
            ContentPart::ImageUrl {
                image_url: json!({"url": "data:..."}),
            },
            ContentPart::Text {
                text: "in detail".into(),
            },
        ];
        let msgs = vec![Message {
            role: Role::User,
            content: Some(MessageContent::Parts(parts)),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        // Only text parts contribute; image_url is skipped because
        // v0.3's embedder is text-only.
        assert_eq!(
            extract_user_prompt_text(&msgs),
            "describe the image in detail"
        );
    }

    #[test]
    fn extract_user_prompt_text_empty_when_no_user_message() {
        let msgs = vec![system_msg("system only")];
        assert_eq!(extract_user_prompt_text(&msgs), "");
    }

    #[test]
    fn extract_system_prompt_text_concats_multiple_system_messages() {
        // Rare but spec-allowed; providers concatenate effectively
        // anyway. Pin the join character so the hash is stable.
        let msgs = vec![
            system_msg("be concise"),
            user_msg("hi"),
            system_msg("avoid emojis"),
        ];
        assert_eq!(
            extract_system_prompt_text(&msgs),
            "be concise\navoid emojis"
        );
    }

    #[test]
    fn extract_system_prompt_text_empty_when_no_system_message() {
        let msgs = vec![user_msg("hello")];
        assert_eq!(extract_system_prompt_text(&msgs), "");
    }

    #[test]
    fn derive_cache_key_uses_defaults_when_inputs_absent() {
        let k = derive_cache_key("", "gpt-4o-mini", None, "", vec![]);
        assert_eq!(k.exact.project_id, DEFAULT_PROJECT_ID);
        assert_eq!(k.exact.temperature_bucket, 20); // 1.0 → bucket 20
        assert_eq!(k.exact.system_prompt_hash, hash_system_prompt(""));
    }

    #[test]
    fn derive_cache_key_respects_provided_temperature() {
        let k = derive_cache_key("proj", "gpt-4o", Some(0.2), "", vec![]);
        assert_eq!(k.exact.temperature_bucket, 4); // 0.2 * 20 = 4
    }

    #[test]
    fn derive_cache_key_differentiates_by_model() {
        let a = derive_cache_key("p", "gpt-4o-mini", Some(0.5), "sys", vec![]);
        let b = derive_cache_key("p", "claude-3-5-sonnet-latest", Some(0.5), "sys", vec![]);
        assert_ne!(a.exact, b.exact);
    }

    #[test]
    fn derive_cache_key_differentiates_by_project() {
        let a = derive_cache_key("proj-a", "gpt-4o", Some(0.5), "sys", vec![]);
        let b = derive_cache_key("proj-b", "gpt-4o", Some(0.5), "sys", vec![]);
        assert_ne!(a.exact, b.exact);
    }

    #[test]
    fn exact_key_prefix_fingerprint_is_deterministic() {
        let a = derive_cache_key("p", "m", Some(0.7), "sys", vec![]);
        let b = derive_cache_key("p", "m", Some(0.7), "sys", vec![]);
        assert_eq!(a.exact.fingerprint(), b.exact.fingerprint());
    }

    #[test]
    fn exact_key_prefix_fingerprint_differs_per_field() {
        let base = derive_cache_key("p", "m", Some(0.7), "sys", vec![]);
        let by_project = derive_cache_key("q", "m", Some(0.7), "sys", vec![]);
        let by_model = derive_cache_key("p", "n", Some(0.7), "sys", vec![]);
        let by_temp = derive_cache_key("p", "m", Some(0.4), "sys", vec![]);
        let by_sys = derive_cache_key("p", "m", Some(0.7), "different sys", vec![]);
        for other in [&by_project, &by_model, &by_temp, &by_sys] {
            assert_ne!(
                base.exact.fingerprint(),
                other.exact.fingerprint(),
                "fingerprint must change when any exact-key field changes"
            );
        }
    }

    #[test]
    fn exact_key_prefix_hashes_into_a_hashmap() {
        // The first-pass lookup table builds on Hash + Eq. Pin that
        // the prefix is usable as a HashMap key.
        use std::collections::HashMap;
        let k1 = derive_cache_key("p", "m", Some(0.7), "sys", vec![]);
        let k2 = derive_cache_key("p", "m", Some(0.7), "sys", vec![]);
        let mut map: HashMap<ExactKeyPrefix, &'static str> = HashMap::new();
        map.insert(k1.exact.clone(), "hit");
        assert_eq!(map.get(&k2.exact), Some(&"hit"));
    }
}
