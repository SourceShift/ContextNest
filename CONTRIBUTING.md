# Contributing to ContextNest

ContextNest is a continual-learning memory substrate for LLM agents built on neural-field
attractor consolidation. It is a substrate, not a beginner-friendly framework. Contributions
are welcome, but please read this document before opening a pull request.

## Getting the test suite green locally

You need Rust 1.80 or later (`rustup update stable`). No external services are required
for the default test run; the LLM integration tests skip automatically when
`CONTEXTNEST_LLM_PROVIDER` is unset.

```bash
# Clone
git clone https://github.com/SourceShift/ContextNest.git
cd ContextNest

# Build the library (default feature set)
cargo build --lib

# Run all library unit tests
cargo test --lib

# Run the three integration test suites
cargo test --test seven_tools_api
cargo test --test canonical_memory_chain
cargo test --test llm_integration

# Build with multi-agent feature enabled
cargo build --features multi-agent --lib
```

All four CI gates must be green before a PR is mergeable (see below).

## The four CI gates

Every pull request must pass these commands with zero warnings treated as errors:

```bash
cargo build                              # default features
cargo test                               # all tests
cargo clippy --lib --no-deps -- -D warnings
cargo fmt --check
```

Run `cargo fmt` (without `--check`) to auto-format before pushing.

## Canonical pipeline architecture

Before contributing to `src/memory/` or `src/api/tools.rs`, understand the canonical pipeline:

```
HTTP POST /api/v1/tools/<name>
        |
        v
  api/tools.rs  (seven handlers: store / retrieve / update / summarize /
                                  discard / reconstruct / resonate)
        |
        +---> MemoryAttractorManager   (canonical Module 05 attractor physics:
        |     src/memory/attractors/   basin formation, connection-network indexing,
        |                              adaptive decay, gap-filling, reconstruction)
        |
        +---> SessionIndex             (session-id -> fragment-id routing;
        |     src/services/            the manager itself is session-agnostic)
        |
        +---> fragment_texts           (HashMap<fragment_id, String>; canonical
              (ContextNestServices)    MemoryFragment carries Vec<f32> embeddings
                                       only — original text lives here)
```

`MemoryAttractorManager::process_memories` is the primary entry point.  Each `store`
call triggers basin formation, connection-network indexing, and reconstruction-store
population.  `retrieve`, `reconstruct`, and `resonate` resolve session-affine IDs via
`SessionIndex` then hydrate canonical fragments via `get_fragment`.

`LlmService` (Phase J, `src/services/llm.rs`) backs the `summarize` tool.  It wraps an
`LlmProvider` enum selected entirely by env vars — no code change is needed to switch
providers.

## How to add a new memory tool

The seven-tool surface is intentionally stable for v0.1.x.  If you are proposing an
eighth tool, open a discussion first.  For any approved addition:

1. Add a request/response struct pair in `src/api/tools.rs` (derive `Deserialize` /
   `Serialize`; use `#[serde(default)]` for optional fields).
2. Add an async handler function that receives `State<Arc<ContextNestServices>>` and
   `Json<YourRequest>`, calls into `MemoryAttractorManager` or the relevant
   `ContextNestServices` field, and returns `impl IntoResponse`.
3. Register the route in the `memory_tools_router()` function with `.route("/api/v1/tools/<name>", post(your_handler))`.
4. Add at least two integration test cases in `tests/seven_tools_api.rs` — one happy
   path, one error/edge case.
5. Update the tool table in the `src/api/tools.rs` module doc comment.

## Opting into `--features multi-agent`

The `multi-agent` feature flag (defined in `Cargo.toml`) gates field-based multi-agent
coordination scaffolding intended for v0.2+ work.  It compiles cleanly on top of the
default substrate but exposes no stable API surface in v0.1.x.  To compile with it:

```bash
cargo build --features multi-agent --lib
cargo test  --features multi-agent
```

Do not land any v0.1.x changes that require `multi-agent` for the default build to
compile.

## License

By contributing you agree that your changes will be released under the existing MIT
License (`LICENSE` at the repository root).
