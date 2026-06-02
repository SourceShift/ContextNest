//! Kind taxonomy + signal-class weights.
//!
//! The substrate ingests many fragment kinds (see
//! [`crate::ingest::claude_code::extractor`]). Some of them are
//! user-authored intent (decision, learning, feature) — high topic
//! signal. Some are system-emitted state (state, current_task,
//! files_touched) — moderate signal. One is boilerplate
//! (initial_prompt_window) — actively poisons topic queries because
//! it contains `[user turn N]` prefixes that match every query
//! mentioning common tokens like "user" or "turn".
//!
//! This module encodes that distinction once so every downstream
//! consumer (retrieve, basin formation, connection-network edges,
//! decay) can apply it consistently. Without this registry each
//! consumer had to re-implement the noise/signal call ad-hoc; PR #115
//! shipped a tactical `exclude_kinds` knob that requires every caller
//! to pass the noise list manually. This module is the structural fix.
//!
//! ## Design
//!
//! Each known kind maps to a [`KindSpec`] with three fields:
//!
//! - [`SignalClass`] — categorical: UserContent / SystemState / Boilerplate
//! - [`Cardinality`] — once-per-session vs per-event (informational; used
//!   by basin formation diversity checks)
//! - `default_weight` — `f32` multiplier applied to similarity scores
//!   during retrieve. Defaults: UserContent=1.0, SystemState=0.5,
//!   Boilerplate=0.0 (effectively excluded).
//!
//! Unknown kinds default to UserContent with weight 1.0 — when in
//! doubt we treat as signal, not noise. The registry is purely
//! additive: adding a new kind to the extractor without updating the
//! registry produces a "treat as user content" behaviour that's safe.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Signal vs noise classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum SignalClass {
    /// Agent- or user-authored content reflecting topical intent.
    /// Examples: decision, learning, accomplishment, feature.
    /// Full weight at retrieve scoring + always included in basin
    /// formation as primary signal.
    UserContent,
    /// System-emitted state snapshots that are useful context but
    /// not primary topic signal. Examples: state, current_task,
    /// files_touched. Downweighted at retrieve scoring (default 0.5×)
    /// + may share basins with UserContent but never dominates.
    SystemState,
    /// Boilerplate the ingest pipeline emits once per session that
    /// contains high-volume shared substrings. Examples:
    /// initial_prompt_window. Default weight 0.0 (effectively
    /// excluded from retrieve scoring) + basin formation refuses to
    /// merge with non-Boilerplate fragments.
    Boilerplate,
}

impl SignalClass {
    /// Default multiplier applied to a fragment's similarity score
    /// during retrieve. Configurable per-deployment via env vars,
    /// but the defaults are what the substrate ships with.
    pub fn default_weight(self) -> f32 {
        match self {
            SignalClass::UserContent => 1.0,
            SignalClass::SystemState => 0.5,
            SignalClass::Boilerplate => 0.0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SignalClass::UserContent => "user_content",
            SignalClass::SystemState => "system_state",
            SignalClass::Boilerplate => "boilerplate",
        }
    }
}

/// How many fragments of this kind a typical session contains.
///
/// Used by basin-formation diversity checks (D4): when a candidate
/// basin's membership skews to OncePerSession kinds the formation
/// logic refuses to merge in PerEvent UserContent — those would be
/// drowned out at retrieve time anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum Cardinality {
    /// Exactly one (sometimes zero) per session. Examples:
    /// initial_prompt_window, session_title, domain, summary.
    OncePerSession,
    /// Many per session. Examples: decision, learning, accomplishment.
    PerEvent,
}

/// Specification for one known fragment kind.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct KindSpec {
    pub name: &'static str,
    pub signal_class: SignalClass,
    pub cardinality: Cardinality,
    pub default_weight: f32,
}

/// Built-in registry mapping the kinds emitted by
/// `src/ingest/claude_code/extractor.rs` to their specs.
///
/// Adding a new kind to the extractor without updating this registry
/// is safe — `kind_spec` returns a permissive `UserContent` default
/// for unknown names so new kinds get treated as signal until proven
/// otherwise.
fn registry() -> &'static HashMap<&'static str, KindSpec> {
    static REG: OnceLock<HashMap<&'static str, KindSpec>> = OnceLock::new();
    REG.get_or_init(|| {
        // Macro reduces the per-entry boilerplate. Each row:
        //   (name, signal_class, cardinality)
        // Weight is derived from signal_class default unless explicitly
        // overridden below.
        let mut m: HashMap<&'static str, KindSpec> = HashMap::new();
        let mut add = |name: &'static str, sc: SignalClass, card: Cardinality| {
            m.insert(
                name,
                KindSpec {
                    name,
                    signal_class: sc,
                    cardinality: card,
                    default_weight: sc.default_weight(),
                },
            );
        };

        // ── Boilerplate (the noise that prompted this entire taxonomy) ──
        add(
            "initial_prompt_window",
            SignalClass::Boilerplate,
            Cardinality::OncePerSession,
        );

        // ── SystemState — useful context, lower retrieve weight ──
        add(
            "session_title",
            SignalClass::SystemState,
            Cardinality::OncePerSession,
        );
        add(
            "domain",
            SignalClass::SystemState,
            Cardinality::OncePerSession,
        );
        add("state", SignalClass::SystemState, Cardinality::PerEvent);
        add(
            "current_task",
            SignalClass::SystemState,
            Cardinality::PerEvent,
        );
        add(
            "files_touched",
            SignalClass::SystemState,
            Cardinality::PerEvent,
        );
        add(
            "read_context",
            SignalClass::SystemState,
            Cardinality::PerEvent,
        );

        // ── UserContent — primary topic signal, full weight ──
        add(
            "goal_phase",
            SignalClass::UserContent,
            Cardinality::PerEvent,
        );
        add(
            "accomplishment",
            SignalClass::UserContent,
            Cardinality::PerEvent,
        );
        add("learning", SignalClass::UserContent, Cardinality::PerEvent);
        add("todo", SignalClass::UserContent, Cardinality::PerEvent);
        add(
            "user_action",
            SignalClass::UserContent,
            Cardinality::PerEvent,
        );
        add("decision", SignalClass::UserContent, Cardinality::PerEvent);
        add(
            "decision_made",
            SignalClass::UserContent,
            Cardinality::PerEvent,
        );
        add("blocker", SignalClass::UserContent, Cardinality::PerEvent);
        add(
            "summary",
            SignalClass::UserContent,
            Cardinality::OncePerSession,
        );
        add("feature", SignalClass::UserContent, Cardinality::PerEvent);
        add(
            "verification",
            SignalClass::UserContent,
            Cardinality::PerEvent,
        );
        add(
            "evidence_ref",
            SignalClass::UserContent,
            Cardinality::PerEvent,
        );
        add("failure", SignalClass::UserContent, Cardinality::PerEvent);
        add(
            "prompt_directive",
            SignalClass::UserContent,
            Cardinality::PerEvent,
        );
        add(
            "assumption",
            SignalClass::UserContent,
            Cardinality::PerEvent,
        );
        add("artifact", SignalClass::UserContent, Cardinality::PerEvent);
        add(
            "memory_candidate",
            SignalClass::UserContent,
            Cardinality::PerEvent,
        );
        add("risk_flag", SignalClass::UserContent, Cardinality::PerEvent);

        m
    })
}

/// Permissive default for kinds not in the registry. UserContent +
/// PerEvent + weight=1.0 means new ingest kinds get full topic-signal
/// treatment until somebody adds them to the registry with a
/// different classification.
fn default_spec(name: &'static str) -> KindSpec {
    KindSpec {
        name,
        signal_class: SignalClass::UserContent,
        cardinality: Cardinality::PerEvent,
        default_weight: SignalClass::UserContent.default_weight(),
    }
}

/// Look up the spec for a kind name.
///
/// Returns the registered spec when known, else a permissive
/// `UserContent / PerEvent / weight=1.0` default. Never panics, never
/// returns an error — the registry is fail-open for forward compat.
pub fn kind_spec(name: &str) -> KindSpec {
    if let Some(spec) = registry().get(name) {
        return *spec;
    }
    // Leak isn't a leak — the string is logged/stored elsewhere but
    // we return the KindSpec with a static name fallback so the
    // signature stays `KindSpec` not `Cow<'static, str>`. Unknown
    // kinds use the literal "unknown" sentinel; callers that need
    // the original string have it from the call site.
    default_spec("unknown")
}

/// Convenience accessor — the retrieve handler multiplies similarity
/// by this weight when scoring fragments.
///
/// Allows per-kind env overrides via
/// `CONTEXTNEST_KIND_WEIGHT_<KIND_UPPER>` (e.g.
/// `CONTEXTNEST_KIND_WEIGHT_INITIAL_PROMPT_WINDOW=0.2` to re-include
/// boilerplate at low weight instead of fully excluding). Env value
/// must parse as f32 in [0.0, 1.0]; bad values fall back to the
/// registered default.
pub fn signal_weight(kind: &str) -> f32 {
    let spec = kind_spec(kind);
    let env_name = format!(
        "CONTEXTNEST_KIND_WEIGHT_{}",
        kind.to_uppercase().replace('-', "_")
    );
    if let Ok(raw) = std::env::var(&env_name) {
        if let Ok(parsed) = raw.parse::<f32>() {
            if parsed.is_finite() && (0.0..=1.0).contains(&parsed) {
                return parsed;
            }
        }
    }
    spec.default_weight
}

/// True iff the registry considers this kind Boilerplate (the
/// strongest noise class). Used by `basin_aware_expand` and basin
/// formation to keep boilerplate out of UserContent basins.
pub fn is_boilerplate(kind: &str) -> bool {
    kind_spec(kind).signal_class == SignalClass::Boilerplate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_prompt_window_is_boilerplate() {
        let spec = kind_spec("initial_prompt_window");
        assert_eq!(spec.signal_class, SignalClass::Boilerplate);
        assert_eq!(spec.default_weight, 0.0);
        assert!(is_boilerplate("initial_prompt_window"));
    }

    #[test]
    fn user_content_kinds_get_full_weight() {
        for k in [
            "decision",
            "learning",
            "accomplishment",
            "feature",
            "todo",
            "decision_made",
        ] {
            let spec = kind_spec(k);
            assert_eq!(
                spec.signal_class,
                SignalClass::UserContent,
                "expected {k} = UserContent"
            );
            assert_eq!(spec.default_weight, 1.0, "expected {k} weight = 1.0");
        }
    }

    #[test]
    fn system_state_kinds_get_half_weight() {
        for k in ["state", "current_task", "files_touched", "domain"] {
            let spec = kind_spec(k);
            assert_eq!(
                spec.signal_class,
                SignalClass::SystemState,
                "expected {k} = SystemState"
            );
            assert_eq!(spec.default_weight, 0.5, "expected {k} weight = 0.5");
        }
    }

    #[test]
    fn unknown_kinds_default_to_user_content() {
        let spec = kind_spec("brand_new_kind_2027");
        assert_eq!(spec.signal_class, SignalClass::UserContent);
        assert_eq!(spec.default_weight, 1.0);
        assert!(!is_boilerplate("brand_new_kind_2027"));
    }

    #[test]
    fn env_override_takes_precedence_when_valid() {
        // Sequential test to avoid races with other parallel tests.
        // Uses a unique kind name so other tests can't interfere.
        std::env::set_var("CONTEXTNEST_KIND_WEIGHT_ENV_TEST_KIND", "0.42");
        let w = signal_weight("env_test_kind");
        std::env::remove_var("CONTEXTNEST_KIND_WEIGHT_ENV_TEST_KIND");
        assert!(
            (w - 0.42).abs() < 1e-6,
            "env override must be applied; got {w}"
        );
    }

    #[test]
    fn env_override_out_of_range_falls_back_to_default() {
        std::env::set_var("CONTEXTNEST_KIND_WEIGHT_ENV_OOR_KIND", "2.5");
        let w = signal_weight("env_oor_kind");
        std::env::remove_var("CONTEXTNEST_KIND_WEIGHT_ENV_OOR_KIND");
        // Unknown kind → UserContent default = 1.0
        assert_eq!(w, 1.0);
    }

    #[test]
    fn boilerplate_default_weight_zero_means_excluded() {
        // The contract that links the kind registry to the
        // exclude_kinds band-aid: boilerplate weight = 0.0 means
        // these fragments effectively don't contribute to retrieve
        // ranking. Operators can re-include them via env override.
        assert_eq!(SignalClass::Boilerplate.default_weight(), 0.0);
    }

    #[test]
    fn cardinality_marks_once_per_session_correctly() {
        for k in [
            "initial_prompt_window",
            "session_title",
            "domain",
            "summary",
        ] {
            assert_eq!(
                kind_spec(k).cardinality,
                Cardinality::OncePerSession,
                "expected {k} = OncePerSession"
            );
        }
        for k in ["decision", "learning", "feature", "verification"] {
            assert_eq!(
                kind_spec(k).cardinality,
                Cardinality::PerEvent,
                "expected {k} = PerEvent"
            );
        }
    }

    #[test]
    fn signal_class_string_form_is_stable() {
        // Pin the wire format — these strings appear in
        // `score_breakdown` responses (D3-F design) and dashboard UI.
        // A future rename here would silently break consumers.
        assert_eq!(SignalClass::UserContent.as_str(), "user_content");
        assert_eq!(SignalClass::SystemState.as_str(), "system_state");
        assert_eq!(SignalClass::Boilerplate.as_str(), "boilerplate");
    }
}
