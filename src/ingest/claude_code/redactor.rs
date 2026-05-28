//! Pattern-based redactor for sensitive data in Claude Code transcripts.
//!
//! Transcripts contain whatever the user typed. Sometimes that's API
//! keys, customer data, internal IDs. Without a filter, the substrate
//! indexes all of it and any later `retrieve` exposes it. This module
//! is the surgical default; the nuclear option is the per-project
//! `.cn-ignore` opt-out checked in `mod.rs`.
//!
//! See `docs/ingest/privacy.md` for the threat model + configuration.

use regex::Regex;
use serde::Deserialize;
use std::path::Path;

/// One match rule — a compiled regex paired with the label that
/// replaces each hit (rendered as `[REDACTED:<label>]`).
#[derive(Debug, Clone)]
pub struct RedactionRule {
    pub name: String,
    pub regex: Regex,
    pub label: String,
}

/// Result of running redactor against a piece of text.
#[derive(Debug, Clone)]
pub struct RedactionResult {
    /// Text with every match replaced by `[REDACTED:<label>]`.
    pub text: String,
    /// Number of distinct matches replaced.
    pub num_redactions: usize,
    /// Byte length of the input text BEFORE redaction.
    pub original_len: usize,
    /// Byte length of the output text AFTER redaction.
    pub redacted_len: usize,
    /// Byte length of the INPUT spans that were matched + removed —
    /// this is the honest "how much of the original was secrets"
    /// signal, independent of how long the replacement labels are.
    pub redacted_chars: usize,
}

impl RedactionResult {
    /// True when more than 75% of the original text was matched by the
    /// redactor — signal that the memory is mostly secrets, not
    /// content. Callers (see `RedactingSink`) typically skip storing
    /// these to avoid filling the substrate with `[REDACTED:…]` noise.
    pub fn mostly_redacted(&self) -> bool {
        self.original_len > 0 && self.redacted_chars * 4 > self.original_len * 3
    }
}

/// Pattern-based redactor. Stateless after construction; safe to clone
/// and share. Apply via [`Redactor::redact`].
#[derive(Debug, Clone)]
pub struct Redactor {
    rules: Vec<RedactionRule>,
}

impl Redactor {
    /// Construct a redactor with no rules. Mostly useful for tests that
    /// want to verify the no-op path.
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// Built-in rules that are always on by default. Each pattern is
    /// chosen to have very low false-positive rate on normal English
    /// text — we'd rather miss a few real secrets than redact random
    /// alphanumeric runs in technical writing.
    ///
    /// Credit-card numbers are validated with the Luhn check to keep
    /// generic 13–19-digit runs (UUIDs, build IDs, phone numbers) from
    /// false-positive triggering.
    pub fn with_defaults() -> Self {
        let mut rules: Vec<RedactionRule> = Vec::new();
        let push = |rules: &mut Vec<RedactionRule>, name: &str, pat: &str, label: &str| {
            if let Ok(regex) = Regex::new(pat) {
                rules.push(RedactionRule {
                    name: name.to_string(),
                    regex,
                    label: label.to_string(),
                });
            }
        };
        push(
            &mut rules,
            "aws_access_key",
            r"\bAKIA[0-9A-Z]{16}\b",
            "aws_access_key",
        );
        push(
            &mut rules,
            "github_pat_classic",
            r"\bghp_[A-Za-z0-9]{36}\b",
            "github_pat",
        );
        push(
            &mut rules,
            "github_pat_fine_grained",
            r"\bgithub_pat_[A-Za-z0-9_]{82}\b",
            "github_pat",
        );
        push(
            &mut rules,
            "anthropic_api_key",
            r"\bsk-ant-[A-Za-z0-9_-]{32,}\b",
            "anthropic_api_key",
        );
        push(
            &mut rules,
            "openai_api_key",
            r"\bsk-(?:proj-)?[A-Za-z0-9_-]{32,}\b",
            "openai_api_key",
        );
        push(
            &mut rules,
            "gcp_service_account",
            r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.iam\.gserviceaccount\.com",
            "gcp_service_account",
        );
        push(&mut rules, "ssn", r"\b\d{3}-\d{2}-\d{4}\b", "ssn");
        // Credit-card candidate — Luhn-checked at apply time, see redact().
        push(
            &mut rules,
            "credit_card_candidate",
            r"\b\d{13,19}\b",
            "credit_card",
        );

        Self { rules }
    }

    /// Load user-supplied patterns from a TOML file and merge with the
    /// built-in defaults. Returns the default set when the file is
    /// absent or malformed (logged at warn level) — the absence of a
    /// config must never silently disable the built-in protections.
    pub fn from_config(path: &Path) -> Self {
        let mut base = Self::with_defaults();
        if !path.exists() {
            return base;
        }
        let contents = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "redactor config unreadable; using built-in defaults only"
                );
                return base;
            }
        };
        match toml::from_str::<RedactConfig>(&contents) {
            Ok(cfg) => {
                for rule in cfg.patterns {
                    match Regex::new(&rule.regex) {
                        Ok(regex) => base.rules.push(RedactionRule {
                            name: rule.name,
                            regex,
                            label: rule.label,
                        }),
                        Err(e) => tracing::warn!(
                            name = %rule.name,
                            error = %e,
                            "invalid regex in redactor config; skipping rule"
                        ),
                    }
                }
            }
            Err(e) => tracing::warn!(
                path = %path.display(),
                error = %e,
                "redactor config malformed; using built-in defaults only"
            ),
        }
        base
    }

    /// Apply every rule to `text`. Rules are applied in declaration
    /// order; the first match at any byte position wins. The Luhn check
    /// is enforced for `credit_card_candidate` matches so generic
    /// numeric runs don't false-positive.
    pub fn redact(&self, text: &str) -> RedactionResult {
        let original_len = text.len();
        let mut out = String::with_capacity(original_len);
        let mut cursor = 0usize;
        let mut count = 0usize;
        let mut redacted_chars = 0usize;
        // Greedy left-to-right: at each cursor position, find the
        // earliest rule match, apply it, advance past the match.
        loop {
            let mut next: Option<(usize, usize, &RedactionRule)> = None;
            for rule in &self.rules {
                let Some(m) = rule.regex.find_at(text, cursor) else {
                    continue;
                };
                if rule.name == "credit_card_candidate" && !luhn_ok(m.as_str()) {
                    continue;
                }
                match next {
                    None => next = Some((m.start(), m.end(), rule)),
                    Some((start, _, _)) if m.start() < start => {
                        next = Some((m.start(), m.end(), rule))
                    }
                    _ => {}
                }
            }
            match next {
                None => {
                    out.push_str(&text[cursor..]);
                    break;
                }
                Some((start, end, rule)) => {
                    out.push_str(&text[cursor..start]);
                    out.push_str(&format!("[REDACTED:{}]", rule.label));
                    redacted_chars += end - start;
                    cursor = end;
                    count += 1;
                }
            }
        }
        let redacted_len = out.len();
        RedactionResult {
            text: out,
            num_redactions: count,
            original_len,
            redacted_len,
            redacted_chars,
        }
    }
}

/// Luhn checksum — drops false positives for 13–19-digit numeric runs
/// that aren't valid card numbers (build IDs, IMEIs sometimes pass but
/// that's a rare-enough collision to be acceptable).
fn luhn_ok(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let mut sum = 0u32;
    for (i, &d) in digits.iter().rev().enumerate() {
        let v = if i % 2 == 1 {
            let doubled = d * 2;
            if doubled > 9 {
                doubled - 9
            } else {
                doubled
            }
        } else {
            d
        };
        sum += v;
    }
    sum % 10 == 0
}

#[derive(Debug, Deserialize)]
struct RedactConfig {
    #[serde(default)]
    patterns: Vec<RedactPattern>,
}

#[derive(Debug, Deserialize)]
struct RedactPattern {
    name: String,
    regex: String,
    label: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_redactor_is_noop() {
        let r = Redactor::empty();
        let result = r.redact("hello world");
        assert_eq!(result.text, "hello world");
        assert_eq!(result.num_redactions, 0);
        assert!(!result.mostly_redacted());
    }

    #[test]
    fn defaults_catch_aws_access_key() {
        let r = Redactor::with_defaults();
        let result = r.redact("config AKIAIOSFODNN7EXAMPLE goes here");
        assert!(result.text.contains("[REDACTED:aws_access_key]"));
        assert_eq!(result.num_redactions, 1);
    }

    #[test]
    fn defaults_catch_github_pat() {
        let r = Redactor::with_defaults();
        let result = r.redact("token ghp_abcdefghijklmnopqrstuvwxyzABCDEFGHIJ here");
        assert!(result.text.contains("[REDACTED:github_pat]"));
    }

    #[test]
    fn defaults_catch_anthropic_key() {
        let r = Redactor::with_defaults();
        let key = "sk-ant-api03-xRb9Z2nh4Q-FF8jXdkRJzN3wYy1abc-def-ghi";
        let result = r.redact(&format!("api {} ok", key));
        assert!(result.text.contains("[REDACTED:anthropic_api_key]"));
    }

    #[test]
    fn defaults_catch_ssn() {
        let r = Redactor::with_defaults();
        let result = r.redact("SSN 123-45-6789 in file");
        assert!(result.text.contains("[REDACTED:ssn]"));
    }

    #[test]
    fn luhn_filters_non_card_digit_runs() {
        let r = Redactor::with_defaults();
        // Random 16-digit run that fails Luhn — must NOT be redacted.
        let result = r.redact("build id 1234567890123456 here");
        assert_eq!(
            result.num_redactions, 0,
            "non-Luhn digit run leaked: {}",
            result.text
        );
    }

    #[test]
    fn luhn_passes_valid_card_number() {
        let r = Redactor::with_defaults();
        // Well-known test card 4242424242424242 (passes Luhn).
        let result = r.redact("paid with 4242424242424242 today");
        assert!(
            result.text.contains("[REDACTED:credit_card]"),
            "got: {}",
            result.text
        );
    }

    #[test]
    fn english_text_has_zero_redactions() {
        let r = Redactor::with_defaults();
        let prose = "The quick brown fox jumps over the lazy dog. \
                     We refactored the substrate module and added \
                     three new test cases yesterday afternoon.";
        let result = r.redact(prose);
        assert_eq!(
            result.num_redactions, 0,
            "false positive in: {}",
            result.text
        );
    }

    #[test]
    fn mostly_redacted_flags_secrets_only_text() {
        let r = Redactor::with_defaults();
        // A memory composed almost entirely of secrets (5 tokens, minimal
        // surrounding glue) — after redaction the byte count collapses
        // because [REDACTED:label] is short relative to each 40+ char
        // token. This is the "secrets dump" we want to skip storing.
        let just_tokens = "\
            ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
            ghp_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb \
            ghp_cccccccccccccccccccccccccccccccccccc \
            ghp_dddddddddddddddddddddddddddddddddddd \
            ghp_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let result = r.redact(just_tokens);
        assert!(
            result.mostly_redacted(),
            "secrets-dump must trip mostly_redacted, got: {:?}",
            result
        );
    }

    #[test]
    fn one_token_in_normal_text_not_mostly_redacted() {
        let r = Redactor::with_defaults();
        // Single token surrounded by normal prose — must NOT trip
        // mostly_redacted (the meaningful signal is still there).
        let prose = "While debugging the auth middleware, I found the \
                     production token ghp_abcdefghijklmnopqrstuvwxyzABCDEFGHIJ \
                     was leaked into a log line. Rotated it via the GitHub UI \
                     and added a guard test.";
        let result = r.redact(prose);
        assert_eq!(result.num_redactions, 1);
        assert!(
            !result.mostly_redacted(),
            "single-secret prose must not be flagged, got: {:?}",
            result
        );
    }

    #[test]
    fn multiple_matches_in_one_text() {
        let r = Redactor::with_defaults();
        let mixed = "key1 ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
                     and key2 AKIAIOSFODNN7EXAMPLE";
        let result = r.redact(mixed);
        assert_eq!(result.num_redactions, 2, "got: {}", result.text);
    }

    #[test]
    fn user_pattern_loads_and_redacts() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("redact.toml");
        std::fs::write(
            &cfg_path,
            r#"
patterns = [
    { name = "internal_id", regex = "CUST-[A-Z0-9]{8}", label = "customer_id" },
]
"#,
        )
        .unwrap();
        let r = Redactor::from_config(&cfg_path);
        let result = r.redact("customer CUST-AB12CD34 placed order");
        assert!(
            result.text.contains("[REDACTED:customer_id]"),
            "got: {}",
            result.text
        );
    }

    #[test]
    fn missing_config_file_falls_back_to_defaults() {
        let path = Path::new("/nonexistent/redact.toml");
        let r = Redactor::from_config(path);
        let result = r.redact("token ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa here");
        assert_eq!(result.num_redactions, 1, "defaults must still apply");
    }
}
