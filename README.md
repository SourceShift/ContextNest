# ContextNest

[![Crates.io](https://img.shields.io/crates/v/contextnest.svg)](https://crates.io/crates/contextnest)
[![Docs.rs](https://docs.rs/contextnest/badge.svg)](https://docs.rs/contextnest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust 1.80+](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org/)

**Continual-learning memory substrate for LLM agents — neural-field attractor consolidation.**

ContextNest gives LLM agents a persistent, self-organising memory layer grounded in
neural-field attractor dynamics. Agents `store` fragments of knowledge, `retrieve` them
by semantic similarity, let the substrate `reconstruct` degraded memories, and detect
emergent patterns via `resonate` — all through a thin seven-tool HTTP API.

## Seven-tool API

All tools are HTTP POST endpoints under `/api/v1/tools/<name>` with JSON bodies.

| Tool | Purpose |
|------|---------|
| `store` | Persist a content fragment as a memory attractor |
| `retrieve` | Fetch relevant attractors for a query (cosine similarity) |
| `update` | Mutate an existing attractor's content or importance |
| `summarize` | Compact a memory region into a single attractor (LLM-backed when enabled) |
| `discard` | Remove an attractor (soft or hard delete, session-scoped) |
| `reconstruct` | Gap-filling reconstruction via the canonical attractor chain |
| `resonate` | Detect emergent activation patterns across the field |

## Architecture

```
HTTP POST /api/v1/tools/<name>
        |
        v
  src/api/tools.rs   (request/response shapes + seven handlers)
        |
        +---> MemoryAttractorManager     basin formation, connection-network
        |     src/memory/attractors/     indexing, adaptive decay, gap-filling,
        |                                reconstruction (canon Module 05)
        |
        +---> SessionIndex               session_id -> fragment_id routing;
        |     src/services/              the manager is session-agnostic
        |
        +---> fragment_texts             original text alongside embedding-only
              ContextNestServices        MemoryFragment (Vec<f32>)
```

`MemoryAttractorManager::process_memories` is the entry point for every `store` call.
It triggers basin formation, connection-network indexing, and reconstruction-store
population in a single pass. `retrieve`, `reconstruct`, and `resonate` resolve
session-affine IDs via `SessionIndex`, then hydrate canonical fragments via
`get_fragment`.

The `summarize` tool delegates to `LlmService` when a provider is configured, and falls
back to a statistics-only implementation when no API key is present.

## Quick start

Add to your `Cargo.toml`:

```toml
[dependencies]
contextnest = "0.1"
```

Run the HTTP server:

```bash
# Copy the environment template
cp .env.example .env

# Start the substrate
cargo run -- serve
```

The server binds to `0.0.0.0:8080` by default. All seven tools are immediately
available:

```bash
# Store a memory fragment
curl -s -X POST http://localhost:8080/api/v1/tools/store \
  -H 'Content-Type: application/json' \
  -d '{"content": "The attention mechanism scales as O(n^2) in sequence length.",
       "importance": 0.8}' | jq .

# Retrieve relevant memories
curl -s -X POST http://localhost:8080/api/v1/tools/retrieve \
  -H 'Content-Type: application/json' \
  -d '{"query": "transformer computational complexity", "top_k": 5}' | jq .
```

## LLM provider configuration

`LlmService` is multi-provider. Provider selection is config-driven via environment
variables; no code change is needed to switch:

```bash
# Anthropic (default model: claude-3-5-haiku-20241022)
CONTEXTNEST_LLM_PROVIDER=anthropic
ANTHROPIC_API_KEY=sk-ant-...

# OpenAI (default model: gpt-4o-mini)
CONTEXTNEST_LLM_PROVIDER=openai
OPENAI_API_KEY=sk-...

# Google (default model: gemini-2.0-flash)
CONTEXTNEST_LLM_PROVIDER=google
GOOGLE_API_KEY=...

# Override model or route through a proxy (e.g. z.ai / LiteLLM)
CONTEXTNEST_LLM_MODEL=claude-3-opus-20240229
CONTEXTNEST_LLM_BASE_URL=https://proxy.example.com/v1
```

When `CONTEXTNEST_LLM_PROVIDER` is unset, the substrate runs in degraded mode:
`summarize` returns a statistics-only result and all other tools are unaffected.

## Feature flags

| Flag | Default | Description |
|------|---------|-------------|
| _(none)_ | on | Single-agent continual-learning substrate |
| `multi-agent` | off | Field-based multi-agent coordination scaffold (v0.2+ work) |

```bash
cargo build --features multi-agent --lib
```

## Building and testing

```bash
# Build (default features)
cargo build --lib

# Full test sweep
cargo test

# Integration suites individually
cargo test --test seven_tools_api
cargo test --test canonical_memory_chain
cargo test --test llm_integration

# CI gates (must all pass)
cargo clippy --lib --no-deps -- -D warnings
cargo fmt --check
```

The project requires Rust 1.80+. No external services are required for the default
test run; LLM integration tests skip automatically when `CONTEXTNEST_LLM_PROVIDER`
is unset.

## Prerequisites

- Rust 1.80+ (`rustup update stable`)
- Neo4j (optional — graph service feature only)
- Redis (optional — rate limiting only)

## Documentation

- [CONTRIBUTING.md](CONTRIBUTING.md) — how to add a tool, CI gates, canonical pipeline
- [SECURITY.md](SECURITY.md) — vulnerability reporting
- [CHANGELOG.md](CHANGELOG.md) — version history

## License

MIT — see [LICENSE](LICENSE).
