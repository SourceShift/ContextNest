//! Per-fragment **content_density** — a 0-1 scalar that distinguishes
//! content-bearing fragments from terminology-bearing ones.
//!
//! ## Why this exists
//!
//! Today's `signal_weight` multiplier (`kind_registry::signal_weight`)
//! down-weights whole *kinds* (e.g. all `initial_prompt_window` at 0.0).
//! But it cannot distinguish *within* a kind: an `evidence_ref` that
//! reads `"arxiv:2603.16131"` (a bare id naming a topic) and an
//! `evidence_ref` that reads `"arxiv:2603.16131 — paper demonstrates a
//! sparse-mixture routing layer that drops attention compute by 38%
//! while maintaining MMLU within 0.4pt of the dense baseline"` both
//! get the same kind multiplier. Yet a query like `"sparse attention
//! techniques"` should rank the second much higher than the first.
//!
//! `content_density` adds that distinction. It is computed once at
//! consolidation time and multiplied into the retrieve score alongside
//! `kind_weight`. The default formula is normalized Shannon entropy
//! over whitespace-split tokens, attenuated by the fraction of tokens
//! that look like natural-language words (alphabetic-majority).
//!
//! ## Reading the score
//!
//! - `0.0` = a single token, empty text, or all-identical tokens (the
//!   degenerate "no information" case).
//! - `~0.2-0.4` = ID-shaped strings: `"arxiv:2603.16131"`,
//!   `"a1b2c3d4-..."`, file paths, isolated kind names. These match
//!   well under embeddings because they share substrings with the
//!   query but they don't carry intent — exactly the noise this scalar
//!   exists to demote.
//! - `~0.6-0.9` = sentences with varied vocabulary and natural-language
//!   structure. These are the fragments that should win retrieve.
//! - `1.0` = theoretical maximum (every token unique, fully
//!   alphabetic). Real text doesn't hit this; encyclopedic prose lands
//!   around 0.7-0.8.
//!
//! ## Not in this module
//!
//! - **LLM-scored density.** A 1-call density classifier per fragment
//!   would be more accurate but adds latency to the consolidation
//!   worker and an LLM dependency to ingest. The entropy proxy is
//!   ~free (a few microseconds per fragment) and is the documented
//!   IR signal-vs-noise scalar (see Manning et al., *Introduction to
//!   Information Retrieval*, ch. 6).
//! - **Per-language tokenization.** Whitespace + punctuation split is
//!   used unchanged for English / German / French / code. Languages
//!   that don't separate words (CJK) would over-score under this
//!   formula — explicit follow-up if we see that pattern in real
//!   ingest.

/// Compute the content density of a fragment text.
///
/// Returns a value in `[0.0, 1.0]`. Pure function, deterministic, no
/// allocations beyond a small per-call HashMap.
pub fn content_density(text: &str) -> f32 {
    let tokens = tokenize(text);
    if tokens.len() < 2 {
        // Single token (or empty) carries no information beyond its
        // own identity. The kind_weight already classifies single-token
        // fragments correctly (e.g. `kind: domain` value `"research"`),
        // so we don't double-penalise here — just return 0.
        return 0.0;
    }

    let entropy = normalized_token_entropy(&tokens);
    let wordishness = alphabetic_word_fraction(&tokens);

    // Multiplicative blend: density needs BOTH high token diversity AND
    // a meaningful share of natural-language tokens. An all-identifier
    // string like `"arxiv:2603.16131 arxiv:2603.16132 arxiv:2603.16133"`
    // has decent token diversity (3 unique) but ~0 wordishness, so it
    // correctly scores low. Pure prose scores high on both.
    let density = entropy * wordishness;

    // Clamp defensively against floating-point drift (entropy and
    // wordishness are both in [0, 1] by construction, but the product
    // can land at 1.0000001f32 in pathological cases).
    density.clamp(0.0, 1.0)
}

/// Split on whitespace + lightweight punctuation. We intentionally keep
/// `:` and `/` as token separators so `arxiv:2603.16131` splits into
/// `["arxiv", "2603", "16131"]` rather than counting as one giant token
/// that would inflate apparent diversity downstream.
fn tokenize(text: &str) -> Vec<&str> {
    text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                ',' | '.'
                    | ';'
                    | ':'
                    | '!'
                    | '?'
                    | '"'
                    | '\''
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '<'
                    | '>'
                    | '/'
                    | '\\'
                    | '|'
                    | '`'
                    | '~'
            )
    })
    .filter(|t| !t.is_empty())
    .collect()
}

/// Shannon entropy of the token distribution, normalised to `[0, 1]`
/// by dividing by `log2(n_tokens)` — the entropy of a uniform
/// distribution over `n` distinct items. Result is `1.0` when every
/// token is unique, `0.0` when one token dominates.
fn normalized_token_entropy(tokens: &[&str]) -> f32 {
    let n = tokens.len() as f32;
    let mut counts: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for t in tokens {
        *counts.entry(*t).or_insert(0) += 1;
    }

    if counts.len() < 2 {
        return 0.0;
    }

    let h: f32 = counts
        .values()
        .map(|&c| {
            let p = c as f32 / n;
            -p * p.log2()
        })
        .sum();

    let max_h = n.log2();
    if max_h <= 0.0 {
        return 0.0;
    }
    (h / max_h).clamp(0.0, 1.0)
}

/// Fraction of tokens whose characters are majority alphabetic (≥ 50%
/// letters). Filters out pure identifier strings, numbers, and
/// punctuation tokens. Natural-language prose scores ~0.9+; pure-id
/// dumps score ~0.0.
fn alphabetic_word_fraction(tokens: &[&str]) -> f32 {
    if tokens.is_empty() {
        return 0.0;
    }
    let wordish = tokens.iter().filter(|t| is_wordish(t)).count() as f32;
    wordish / tokens.len() as f32
}

fn is_wordish(token: &str) -> bool {
    let mut alpha = 0usize;
    let mut total = 0usize;
    for c in token.chars() {
        total += 1;
        if c.is_alphabetic() {
            alpha += 1;
        }
    }
    total >= 2 && alpha * 2 >= total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_returns_zero() {
        assert_eq!(content_density(""), 0.0);
        assert_eq!(content_density("   \t\n"), 0.0);
    }

    #[test]
    fn single_token_returns_zero() {
        assert_eq!(content_density("research"), 0.0);
        assert_eq!(content_density("arxiv"), 0.0);
    }

    #[test]
    fn identifier_string_scores_low() {
        // "arxiv:2603.16131" splits to ["arxiv", "2603", "16131"] —
        // diverse tokens, but only 1/3 is wordish. Density should land
        // below ~0.5 so it can't outrank a real prose hit.
        let d = content_density("arxiv:2603.16131");
        assert!(d < 0.5, "id-shaped string scored too high: {d}");
        assert!(d > 0.0, "id-shaped string should not be exactly zero: {d}");
    }

    #[test]
    fn pure_id_dump_scores_near_zero() {
        // Multiple arxiv ids — diverse tokens but only the `arxiv` token
        // is wordish (1/3 of distinct tokens, 1/3 of the stream). Score
        // lands around 0.22, which is the relevant property: it's a
        // ~3x demotion vs prose density of ~0.7, so retrieve scoring
        // will reliably rank prose above id-dumps even at equal
        // similarity + kind_weight.
        let d = content_density("arxiv:2603.16131 arxiv:2603.16132 arxiv:2603.16133");
        assert!(d < 0.3, "pure-id dump scored too high: {d}");
    }

    #[test]
    fn repeated_token_scores_zero() {
        assert_eq!(content_density("research research research research"), 0.0);
    }

    #[test]
    fn prose_scores_high() {
        let d = content_density(
            "Pulled last 200 May 2026 AI papers from PG and ranked them by jina cross-encoder",
        );
        assert!(d > 0.5, "prose scored too low: {d}");
        assert!(d <= 1.0, "density must be clamped");
    }

    #[test]
    fn discrimination_between_terminology_and_prose() {
        // The exact failure mode from the arxiv-research session search:
        // a bare id ("arxiv:2603.16131") shouldn't beat a content-bearing
        // sentence under the density multiplier.
        let terminology = content_density("arxiv:2603.16131");
        let prose = content_density("Pulled last 200 May 2026 AI papers from PG and ranked them");
        assert!(
            prose > terminology + 0.3,
            "density failed to discriminate prose ({prose}) from id ({terminology})"
        );
    }

    #[test]
    fn output_always_in_unit_interval() {
        for sample in [
            "",
            "a",
            "a a a",
            "the quick brown fox jumps over the lazy dog",
            "arxiv:2603.16131",
            "012-3456-789",
            "/Volumes/ssd-2/arxiv-db/reports/may2026-top200/REPORT.md",
            "the the the the the the and and and and and",
        ] {
            let d = content_density(sample);
            assert!(
                (0.0..=1.0).contains(&d),
                "density {d} out of [0,1] for sample {sample:?}"
            );
        }
    }
}
