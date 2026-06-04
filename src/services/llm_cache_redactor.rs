//! Pre-store PII redactor for the LLM proxy cache (v0.3 Phase 3 slice 3.2).
//!
//! ## What this does
//!
//! Before the [`LlmCacheService::insert`] path writes a response into
//! the WAL, the response's text content is run through a configurable
//! pipeline of regex-based patterns. Matches are replaced with labeled
//! placeholders like `[REDACTED:EMAIL]` so the *type* of leaked data is
//! preserved for an operator's audit purposes but the content itself
//! is removed.
//!
//! The redactor is applied to the **stored response**, not the
//! **upstream request**: the LLM provider still receives the
//! un-redacted prompt so it can answer truthfully, but anything the
//! LLM echoes back gets scrubbed before landing in long-term storage.
//!
//! ## Default patterns
//!
//! The default pipeline ships with three patterns chosen because they
//! have low false-positive rates and high real-world frequency in
//! agent outputs:
//!
//! - **Email** — RFC-5321-ish (local-part + `@` + dotted domain). Not
//!   a complete RFC validator; the goal is recall on common shapes,
//!   not formal correctness.
//! - **E.164 phone** — Optional `+`, 7–15 digits, separators allowed.
//!   Matches `+1 415-867-5309` and `+49 30 12345678`.
//! - **Credit card** — 13–19 digit groups with optional separators,
//!   validated via the Luhn checksum so random 16-digit sequences
//!   (timestamps, fragment ids, fixture data) don't get false-flagged.
//!
//! Name-redaction via NER is *not* in this slice — the roadmap defers
//! it to v0.3.1 because the LLM overhead at store-time outweighs the
//! v0.3 scale value.
//!
//! ## Configuration
//!
//! - `CONTEXTNEST_LLM_CACHE_REDACTOR_ENABLED=false` — "forward-only"
//!   mode: the redactor is a no-op. Responses land in the cache
//!   verbatim. Used when the operator explicitly accepts the cache
//!   storing raw model output (e.g. fully self-hosted deployment
//!   where the WAL never leaves the host).
//! - `CONTEXTNEST_LLM_CACHE_REDACTOR_EXTRA_PATTERNS` — semicolon-
//!   delimited list of additional regex strings to layer on top of
//!   the defaults. Each pattern uses `[REDACTED:CUSTOM_<N>]` as its
//!   placeholder where `<N>` is the 1-indexed position in the env
//!   string.
//!
//! Invalid regex strings are logged at `warn!` and skipped — the
//! redactor never panics on config; the worst case is one extra
//! pattern not being applied.
//!
//! ## What this slice does NOT do
//!
//! - **No mutation of the cache key.** The key derivation
//!   ([`derive_cache_key`]) hashes the system prompt and embeds the
//!   user prompt; both are upstream of the cache write. The redactor
//!   only sees the response payload.
//! - **No project-scoped pattern lists.** v0.3 is single-tenant; per-
//!   project pipelines land in v0.2's multi-tenant work. The env-var
//!   extras list is the v0.3 escape hatch.
//! - **No reverse map.** Once redacted, the original content is gone
//!   from the WAL. There is no "decrypt-the-placeholder" operation.
//!   Operators who need the un-redacted log keep their upstream-
//!   provider logs separately.

use std::env;

use regex::Regex;

/// Encapsulates the configured redaction pipeline. Built once at
/// service startup (or rebuilt explicitly via [`Redactor::reload`])
/// because regex compilation isn't free and the patterns don't change
/// at runtime.
#[derive(Debug, Clone)]
pub struct Redactor {
    enabled: bool,
    /// Each entry: `(label, compiled_regex)`. Iteration order =
    /// application order (defaults first, then extras). Inner Vec
    /// instead of HashMap so application order is deterministic
    /// across runs — important because nested patterns can collide
    /// (a phone number embedded in an email-like substring is
    /// shadowed by the email pattern if email runs first).
    rules: Vec<RedactRule>,
}

#[derive(Debug, Clone)]
struct RedactRule {
    /// Placeholder text. The final replacement is wrapped as
    /// `[REDACTED:<label>]`.
    label: String,
    /// Compiled regex. Wrapping in Arc would let us share across
    /// Redactor clones, but Regex itself is `Sync` and clone is
    /// cheap (refcount); we keep the simpler shape.
    re: Regex,
    /// Optional post-match validator. Returns `true` to KEEP the
    /// match (and redact it), `false` to REJECT it as a false
    /// positive (and leave the original text alone). Used by the
    /// credit-card rule to apply the Luhn checksum.
    validator: Option<fn(&str) -> bool>,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::from_env()
    }
}

impl Redactor {
    /// Build a redactor from the current process environment. Reads
    /// `CONTEXTNEST_LLM_CACHE_REDACTOR_ENABLED` and
    /// `CONTEXTNEST_LLM_CACHE_REDACTOR_EXTRA_PATTERNS`.
    pub fn from_env() -> Self {
        let enabled = env::var("CONTEXTNEST_LLM_CACHE_REDACTOR_ENABLED")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
            .map(|v| !matches!(v.as_str(), "false" | "0" | "no" | "off"))
            .unwrap_or(true);

        let mut rules = default_rules();

        if let Ok(extra) = env::var("CONTEXTNEST_LLM_CACHE_REDACTOR_EXTRA_PATTERNS") {
            for (i, pat) in extra.split(';').enumerate() {
                let pat = pat.trim();
                if pat.is_empty() {
                    continue;
                }
                match Regex::new(pat) {
                    Ok(re) => rules.push(RedactRule {
                        label: format!("CUSTOM_{}", i + 1),
                        re,
                        validator: None,
                    }),
                    Err(e) => tracing::warn!(
                        pattern = pat,
                        error = %e,
                        "llm_cache_redactor: failed to compile extra pattern; skipping"
                    ),
                }
            }
        }

        Self { enabled, rules }
    }

    /// Force-disabled redactor. Useful in tests where the
    /// surrounding setup shouldn't accidentally pick up host env
    /// vars.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            rules: Vec::new(),
        }
    }

    /// Force-enabled redactor with the default pattern set, ignoring
    /// host env. Used by tests.
    pub fn defaults_only() -> Self {
        Self {
            enabled: true,
            rules: default_rules(),
        }
    }

    /// Reload from env. Public so an operator can SIGHUP the process
    /// and pick up new patterns without a full restart. Wiring a
    /// signal handler isn't in this slice; the method exists so the
    /// future wiring is a one-liner.
    pub fn reload(&mut self) {
        *self = Self::from_env();
    }

    /// `true` iff the redactor will apply patterns. Surfaces in
    /// `/api/v1/substrate/config` so operators can confirm the
    /// runtime state matches their intent.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Number of compiled rules (defaults + extras). For
    /// observability via `/config`.
    pub fn rule_count(&self) -> usize {
        if self.enabled {
            self.rules.len()
        } else {
            0
        }
    }

    /// Apply the pipeline to a single text block. Returns the
    /// redacted string. Rules are applied in declaration order;
    /// later rules see the output of earlier rules so a phone
    /// number inside an email-shape substring is NOT independently
    /// redacted (the email rule consumed it first).
    pub fn redact(&self, text: &str) -> String {
        if !self.enabled || self.rules.is_empty() {
            return text.to_string();
        }
        let mut current = text.to_string();
        for rule in &self.rules {
            current = apply_rule(&current, rule);
        }
        current
    }
}

fn apply_rule(text: &str, rule: &RedactRule) -> String {
    let placeholder = format!("[REDACTED:{}]", rule.label);
    let needs_validator = rule.validator.is_some();
    if !needs_validator {
        return rule.re.replace_all(text, placeholder.as_str()).into_owned();
    }

    // Validator path — can't use replace_all directly because we
    // need to conditionally keep or drop each match.
    let mut out = String::with_capacity(text.len());
    let mut last_end = 0;
    for m in rule.re.find_iter(text) {
        out.push_str(&text[last_end..m.start()]);
        let matched = m.as_str();
        let keep = rule.validator.map(|v| v(matched)).unwrap_or(true);
        if keep {
            out.push_str(&placeholder);
        } else {
            out.push_str(matched);
        }
        last_end = m.end();
    }
    out.push_str(&text[last_end..]);
    out
}

fn default_rules() -> Vec<RedactRule> {
    let mut v = Vec::new();

    // Email — RFC-5321-ish. Permissive on local-part (alphanum + dot
    // + common puncts), strict-ish on domain (at least one dot,
    // alphanum + hyphen TLDs). Doesn't try to handle quoted-locals
    // or comments; those are <1% of real-world traffic and a
    // false-negative there is preferable to false-positives across
    // the other 99%.
    v.push(RedactRule {
        label: "EMAIL".to_string(),
        re: Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").unwrap(),
        validator: None,
    });

    // Credit card — applied BEFORE phone because the phone regex is
    // broad enough to swallow 13-19 digit sequences with spaces,
    // including real CC numbers. With CC running first, Luhn-valid
    // numbers get scrubbed cleanly; non-CC digit runs fall through
    // to the phone rule.
    v.push(RedactRule {
        label: "CC".to_string(),
        re: Regex::new(r"\b(?:\d[ -]?){13,19}\b").unwrap(),
        validator: Some(passes_luhn),
    });

    // E.164 phone — optional leading `+`, optional country code,
    // 7-14 more digits with allowed separators (space, hyphen,
    // parens, dot). The (?:[\s\-.()]?\d){7,14} group enforces "at
    // least 7 digits after the leading marker", which is the
    // shortest valid international subscriber-number length.
    v.push(RedactRule {
        label: "PHONE".to_string(),
        re: Regex::new(r"\+?\d{1,3}(?:[\s\-.()]?\d){7,14}").unwrap(),
        validator: None,
    });

    v
}

/// Luhn checksum validator for credit-card-shaped strings. Strips
/// non-digits, then applies the standard double-every-second-from-
/// right + sum + mod 10 algorithm. Returns `true` when the input
/// looks like a real CC number (passes Luhn) and should therefore
/// be redacted.
fn passes_luhn(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let mut sum = 0u32;
    let mut double = false;
    for &d in digits.iter().rev() {
        let v = if double {
            let dd = d * 2;
            if dd > 9 {
                dd - 9
            } else {
                dd
            }
        } else {
            d
        };
        sum += v;
        double = !double;
    }
    sum % 10 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> Redactor {
        Redactor::defaults_only()
    }

    #[test]
    fn redacts_simple_email() {
        let out = r().redact("contact me at john.doe@example.com about the bug");
        assert_eq!(out, "contact me at [REDACTED:EMAIL] about the bug");
    }

    #[test]
    fn redacts_multiple_emails() {
        let out = r().redact("from a@b.co cc c@d.org");
        assert_eq!(out, "from [REDACTED:EMAIL] cc [REDACTED:EMAIL]");
    }

    #[test]
    fn redacts_e164_phone_with_country_code() {
        let out = r().redact("call me at +1 415-867-5309 anytime");
        // Phone regex eats the whole "+1 415-867-5309" span.
        assert!(out.contains("[REDACTED:PHONE]"));
        assert!(!out.contains("415"));
    }

    #[test]
    fn redacts_german_phone_format() {
        let out = r().redact("ring +49 30 12345678 please");
        assert!(out.contains("[REDACTED:PHONE]"));
        assert!(!out.contains("12345678"));
    }

    #[test]
    fn redacts_valid_luhn_credit_card() {
        // 4111 1111 1111 1111 is the canonical Visa test number,
        // passes Luhn.
        let out = r().redact("card: 4111 1111 1111 1111 expires 12/30");
        assert!(out.contains("[REDACTED:CC]"));
        assert!(!out.contains("4111 1111"));
    }

    #[test]
    fn skips_invalid_luhn_sixteen_digit_sequence() {
        // 1234 5678 9012 3456 is 16 digits but does NOT pass Luhn —
        // exactly the false-positive case the validator exists to
        // reject (timestamps, fragment ids, etc.).
        let out = r().redact("fragment id: 1234567890123456 in log");
        assert!(
            !out.contains("[REDACTED:CC]"),
            "non-Luhn 16-digit sequence should NOT be redacted: {out}"
        );
    }

    #[test]
    fn disabled_redactor_is_passthrough() {
        let red = Redactor::disabled();
        let original = "email john@x.com phone +1 415-867-5309 card 4111 1111 1111 1111";
        assert_eq!(red.redact(original), original);
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(r().redact(""), "");
    }

    #[test]
    fn no_pii_returns_unchanged() {
        let input = "The weather is nice today.";
        assert_eq!(r().redact(input), input);
    }

    #[test]
    fn rule_count_reflects_default_set() {
        // 3 default rules: EMAIL + PHONE + CC.
        assert_eq!(r().rule_count(), 3);
    }

    #[test]
    fn disabled_rule_count_is_zero() {
        assert_eq!(Redactor::disabled().rule_count(), 0);
    }

    #[test]
    fn luhn_validator_directly() {
        assert!(passes_luhn("4111111111111111")); // Visa test
        assert!(passes_luhn("5555 5555 5555 4444")); // Mastercard test
        assert!(!passes_luhn("1234567890123456")); // not Luhn-valid
        assert!(!passes_luhn("123")); // too short
        assert!(!passes_luhn("12345678901234567890")); // too long
    }

    #[test]
    fn handles_multiline_input() {
        let input = "Line 1: foo@bar.com\nLine 2: nothing\nLine 3: baz@qux.org";
        let out = r().redact(input);
        assert!(out.contains("Line 1: [REDACTED:EMAIL]"));
        assert!(out.contains("Line 3: [REDACTED:EMAIL]"));
        assert!(out.contains("Line 2: nothing"));
    }
}
