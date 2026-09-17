# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.4] — 2026-09-17

### Added

- **RecMem reconsideration wake + pass counters.** `GET /api/v1/substrate/consolidation`
  now returns two new fields: `reconsidered_enqueued` (fragments cleared from the
  subconscious store and re-enqueued because a peer arrived) and
  `reconsidered_and_passed` (subset that consolidated Done on their next pass).
  The ratio is the wake-up-to-pass success rate — operators tune the T1 gate
  thresholds from live metrics instead of custom instrumentation. See
  `docs/roadmap/epics/2026-arxiv-improvements.md` (T1 follow-up). ([#183])

### Changed

- Tests: `tests/sprint1_arxiv_2026_test.rs` `ENV_LOCK` uses
  `PoisonError::into_inner` recovery so a single failing test no longer
  cascades to downstream env-mutating tests that share the lock.

[#183]: https://github.com/SourceShift/ContextNest/pull/183

## [0.1.3] — 2026-09-17

### Added

- **T5 frequency-of-use retrieval boost** ([arXiv:2606.12945](https://arxiv.org/html/2606.12945)).
  First half of the Multi-factor Value Model. `retrieve` now bumps
  `retrieval_count` in `fragment_metadata` on every returned hit; a new
  `frequency_multiplier` returns `1 + w * log2(count + 1)` where
  `w = CONTEXTNEST_RETRIEVE_FREQUENCY_WEIGHT` (default 0.1). Fragments never
  retrieved score neutral 1.0. Weight `0.0` disables the boost. Appended to
  the existing scoring chain: `base * decay * kind_weight * density * trust * frequency`. ([#180])
- **Env-knob documentation.** `CLAUDE.md`'s retrieve-tuning table now covers
  `CONTEXTNEST_CONSOLIDATION_RECURRENCE_MIN_COUNT`,
  `CONTEXTNEST_RECONSTRUCT_COSINE_FLOOR`, and
  `CONTEXTNEST_RETRIEVE_FREQUENCY_WEIGHT`. ([#181])
- **Live-workload behaviour report.**
  `docs/reports/2026-09-17-sprint12-live-behavior.md` records what the v0.1.3
  binary does under the accumulated 195k-fragment production workload with
  the T1 gate enabled: bursty CPU (mean ~11%, median 0%), 85 % of new
  fragments deferred to the subconscious store, zero failures. Also lists
  what the report does NOT establish (production CPU cap, retrieve latency,
  retrieval quality) plus three follow-up measurement questions. ([#182])

[#180]: https://github.com/SourceShift/ContextNest/pull/180
[#181]: https://github.com/SourceShift/ContextNest/pull/181
[#182]: https://github.com/SourceShift/ContextNest/pull/182

## [0.1.2] — 2026-09-17

### Added

- **T1 reconsideration path.** When a fragment consolidates Done, the worker
  walks up to 32 same-session peers marked `_cn_recurrence_deferred=true`,
  clears the marker, and re-enqueues them. Deferred fragments now graduate
  naturally to full consolidation once their session accumulates enough
  peers — closes the "subconscious forever" loop from v0.1.1. Runs only when
  the gate is enabled. ([#179])
- **Observability field.** `GET /api/v1/substrate/consolidation` includes
  `deferred_subconscious` in its body — cumulative fragments held in the
  RecMem subconscious store since startup. Zero when the gate is disabled.

### Changed

- `docs/roadmap/epics/2026-arxiv-improvements.md` marks T1/T2/T3/T4 as
  shipped in v0.1.1 (T2 was already implemented in v0.1.0 in the v2 tenant
  API; annotation is retroactive for traceability).

[#179]: https://github.com/SourceShift/ContextNest/pull/179

## [0.1.1] — 2026-09-17

Sprint 1 of the [2026 arxiv-driven improvements epic](docs/roadmap/epics/2026-arxiv-improvements.md).
Three literature-driven substrate improvements plus a security fix, all
default-safe (no behaviour change unless the operator opts in).

### Added

- **T1 — RecMem recurrence-gated consolidation**
  ([arXiv:2605.16045](https://arxiv.org/abs/2605.16045)). Pre-embedding
  gate in the batch loop: skip basin + graph work when a fragment has no
  session peers over the cosine floor. Opt-in via
  `CONTEXTNEST_CONSOLIDATION_RECURRENCE_MIN_COUNT` (default `0` = disabled).
  Reuses `CONTEXTNEST_CONNECTION_SIMILARITY_THRESHOLD` (default `0.7`) as
  the peer floor — the paper's `θ_sim` recommendation matches ContextNest's
  existing default. Metrics gain `deferred_subconscious`. Paper reports
  87 % consolidation-token cut and 68.8 % → 81.1 % accuracy on LoCoMo. ([#178])
- **T3 — PROJECTMEM replay endpoint**
  ([arXiv:2606.12329](https://arxiv.org/html/2606.12329v1)). New
  `GET /api/v1/substrate/replay?dry_run=<bool>` diagnostic. Dry-run
  (default) reports WAL contents; `dry_run=false` re-enqueues every stored
  fragment id. Idempotent via the existing `_cn_consolidated` short-circuit.
  Diagnostic escape hatch for canonical-checkpoint corruption. ([#178])
- **T4 — Chain-of-Memory reconstruction pruning**
  ([arXiv:2601.14287](https://arxiv.org/abs/2601.14287)). New
  `CONTEXTNEST_RECONSTRUCT_COSINE_FLOOR` env (default `0.15`) drops
  off-topic candidates before top-K truncation in `compute_reconstruction`.
  Floor `0.0` preserves pre-Sprint-1 behaviour. Paper reports
  +7.5–10.4 % chain accuracy and −97 % chain-token count. ([#178])
- **2026 arxiv improvements roadmap epic**
  (`docs/roadmap/epics/2026-arxiv-improvements.md`) — 12 tickets sourced
  from a 2026-only literature survey, sized S/M/L/XL, each pointing at the
  exact source file(s) to touch. ([#176])

### Fixed

- **RUSTSEC-2026-0285.** Bumped `rustls` `0.23.40` → `0.23.45` to clear the
  TLS 1.3 handshake advisory (messages incorrectly accepted across
  encryption-level boundaries). Also pulls in `aws-lc-rs` `1.17.0` →
  `1.18.1`, `aws-lc-sys` `0.41.0` → `0.45.0`, and `rustls-webpki` `0.103.13`
  → `0.103.15`. ([#177])

### Compatibility

Every new feature is default-off or default-mild:

- T1 gate defaults to disabled (`MIN_COUNT=0`); existing operators see no
  change.
- T3 endpoint is additive; existing endpoints unchanged.
- T4 default floor `0.15` mildly prunes very-off-topic reconstruction
  candidates; set `0.0` to disable.

[#176]: https://github.com/SourceShift/ContextNest/pull/176
[#177]: https://github.com/SourceShift/ContextNest/pull/177
[#178]: https://github.com/SourceShift/ContextNest/pull/178

## [0.1.0] — 2026-09-08

First public release. Tagged from
[commit `67058d5`](https://github.com/SourceShift/ContextNest/commit/67058d5)
after PR #175 landed application-tenant and session-scoped isolation.

### Added

- **Seven-tool HTTP API** under `/api/v1/tools/<name>` exposing the substrate
  to LLM agents — `store`, `retrieve`, `update`, `summarize`, `discard`,
  `reconstruct`, and `resonate`. Request and response shapes live in
  `src/api/tools.rs`; the Axum server is in `src/api/server.rs`.
- **Neural-field substrate** (`src/context/`). The continuous field
  representation (`field.rs`), attractor evolution
  (`attractor_dynamics.rs`), multi-attractor coupling (`multi_attractor.rs`,
  `harmonic_integration.rs`), and phase-sync / resonance primitives
  (`phase_sync.rs`, `resonance_activation.rs`).
- **Memory-attractor engine** (`src/memory/attractors/`). Basin formation
  + adaptive decay, connection-network indexing, retrieval optimisation,
  reconstruction-store population, and the gap-filling engine.
  `MemoryAttractorManager::process_memories` is the single entry point that
  ties these together for every `store` call.
- **Reconstructive memory pipeline** (`src/context/memory_reconstruction.rs`,
  `memory_reconstruction_coordinator.rs`, `gap_identification.rs`,
  `semantic_continuity_restoration.rs`, `fragment_bridge.rs`,
  `historical_state_recovery.rs`). Resolves session-affine IDs through
  `SessionIndex` and hydrates canonical fragments through the manager.
- **Application-tenant + session-scoped isolation** (`src/api/tenants.rs`,
  `src/services/tenants/`). One SQLite DB per tenant; session-local basins
  and graph; bounded fair CPU scheduler; idempotent
  `(tenant_id, session_id, source_event_id)` ingest; durable canonical
  checkpoint alongside the JSONL WAL; incremental transcript tailing with
  commit-only offsets. Legacy operator memory becomes operator-only when
  tenant mode is configured.
- **Multi-provider LLM service** (`src/services/llm.rs`). Anthropic, OpenAI,
  and Google are all selectable through `CONTEXTNEST_LLM_PROVIDER`; model and
  base-URL are config-overridable so proxies (z.ai, LiteLLM, vLLM) drop in
  without code changes. `summarize` falls back to a statistics-only path
  when no provider is configured, so the rest of the API stays usable in
  unconfigured deployments.
- **Backing services** — Neo4j graph persistence (`src/services/graph.rs`,
  `graph_enhanced.rs`), tree-sitter Rust parser (`src/services/parser.rs`),
  embedding service with provider abstraction (`src/services/embedding.rs`),
  and the in-memory session index (`src/services/session_index.rs`).
- **Pareto-Lang protocol engine** (`src/protocols/`,
  `src/context/protocols.rs`) for declarative tool composition.
- **HTTP middleware stack** (`src/api/middleware/`) — CORS, compression,
  request-context propagation, error interception, structured logging,
  metrics, performance timing, validation, and security headers. All wired
  through Tower layers; user-controlled auth is handled by the deployment's
  reverse proxy (see README "Authentication & deployment").
- **CLI binary** (`src/bin/contextnest.rs`, `src/cli/`) with a `serve`
  subcommand that boots the Axum server using the runtime configuration in
  `src/config.rs`.
- **Security primitives** (`src/security/`) — `PathValidator` (directory-
  traversal defence: input canonicalisation + scoped symlink check), real
  AES / RSA / ECDSA / EdDSA keygen, and a trusted on-disk key store.
  Test-only `InMemoryKeyEncryption` / `InMemoryKeyStorage` doubles are gated
  behind `#[cfg(test)]` so they cannot be instantiated by production
  binaries.
- **Meta-recursive learning scaffold** (`src/context/meta_recursive.rs`,
  `recursive_learning.rs`, `pattern_recognition.rs`,
  `emergence_detection.rs`) for v0.2+ continual-learning work. Behaviour is
  documented as no-signal placeholder paths in v0.1.0.
- **`multi-agent` feature flag** gating `multi_agent_field.rs`,
  `coordinated_formation.rs`, `collective_emergence.rs`, and
  `self_organizing_emergence.rs`. Default build is single-agent.
- **Provenance-aware trust scoring** for Verification-kind records:
  `{observed, partial, claimed, absent, contradicted}` → trust multipliers
  `1.0 / 0.7 / 0.4 / 0.4 / 0.25`. Down-weights self-reported claims that
  the ingest extractor could not ground against a real tool receipt.
- **Integration test suites** — `tests/canonical_memory_chain.rs`
  (store → retrieve → reconstruct round-trip),
  `tests/seven_tools_api.rs` (full HTTP surface),
  `tests/llm_integration.rs` (provider configuration; skips when no key),
  `tests/canonical_checkpoint_test.rs` (durable-acceptance and
  crash-recovery round-trip).
- **CI pipeline** — GitHub Actions runs `cargo fmt --check`,
  `cargo clippy --lib -- -D warnings`, `cargo test`, and `cargo audit`.
- **Documentation** — `README.md` quick-start, `CONTRIBUTING.md`
  (canonical pipeline + how to add a tool), `SECURITY.md` (responsible
  disclosure), this CHANGELOG, and the honest architecture notes
  (`docs/architecture-honest.md`).

### Notes

- Rust 1.80+ is required.
- No external services are required for the default test run.
- Future releases follow [SemVer](https://semver.org).
