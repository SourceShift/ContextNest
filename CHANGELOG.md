# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-05-18

Initial public release.

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
- **Integration test suites** — `tests/canonical_memory_chain.rs`
  (store → retrieve → reconstruct round-trip),
  `tests/seven_tools_api.rs` (full HTTP surface),
  `tests/llm_integration.rs` (provider configuration; skips when no key).
- **CI pipeline** — GitHub Actions runs `cargo fmt --check`,
  `cargo clippy --lib -- -D warnings`, `cargo test`, and `cargo audit`.
- **Documentation** — `README.md` quick-start, `CONTRIBUTING.md`
  (canonical pipeline + how to add a tool), `SECURITY.md` (responsible
  disclosure), this CHANGELOG.

### Notes

- Rust 1.80+ is required.
- No external services are required for the default test run.
- This is the first published version; future releases will follow
  [SemVer](https://semver.org).
