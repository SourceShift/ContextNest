# ContextNest — agent context

Open-source neural-field attractor memory substrate for LLM agents.
Single Rust binary (`contextnest`) exposing a seven-tool HTTP API plus
substrate observability, session queries, and Claude Code ingest hooks.

## Quick start

```bash
make cn-config          # one-time: copy config.example.toml → config.toml
make cn-build           # cargo build --release
make cn-serve           # release binary, WAL on, binds 127.0.0.1:28080
make cn-serve-dev       # fast-profile binary, auto-rebuilds
make cn-watch           # cargo-watch + fast profile
make cn-ingest SINCE=7d PROJECT=researcher   # backfill ~/.claude/projects/*
make cn-help            # full target list
```

**Port defaults disagree on purpose:** `cargo run -- serve` binds `0.0.0.0:8080`
(README default); `make cn-serve` binds `127.0.0.1:28080` (Makefile `CN_BIND`).
Web dashboard expects `28080` via `VITE_API_BASE_URL`.

## Four CI gates (must pass)

```bash
cargo build                                       # default features
cargo test                                        # all suites
cargo clippy --lib --no-deps -- -D warnings
cargo fmt --check
```

Run `cargo fmt` (no `--check`) before pushing. Multi-agent feature must
not be required for the default build: `cargo build --features multi-agent --lib`.

**Clippy debt baseline:** `origin/main` carries ~600 pre-existing clippy
errors from a newer rustc lint set. The gate for any PR is "no NEW errors
introduced by this diff", not "zero total errors" — filter clippy output
to files you actually touched before judging. The cleanup is a separate
hygiene task, not a per-PR blocker.

## Workflow conventions

Distilled into a separate reference: see `docs/development-workflow.md`
for the full rationale and examples. Daily-driver rules:

**Branches.** `feat/<slug>`, `fix/<slug>`, `refactor/<slug>`, `chore/<slug>`,
`docs/<slug>` — lowercase, hyphenated, never edit on `main`. Always branch
from `origin/main`, not from the currently-checked-out branch's HEAD.

**Commits.** Conventional Commits — `<type>(<scope>): <imperative subject ≤72 chars>`,
e.g. `fix(embedder): clamp input by max_input_length`. One concern per PR;
small atomic PRs over monolithic ones. Squash-merge feature PRs; `--no-ff`
only between long-lived branches. Never `--no-verify` to bypass a failing
hook — fix the gate or surface why the bypass is justified.

**Worktrees.** Sibling worktrees at
`/Volumes/docker-ssd/Migration/Development/worktrees/<slug>` (or
`../ContextNest-<slug>` for ad-hoc one-offs). Create with
`git worktree add -b feat/<slug> ../ContextNest-<slug> origin/main`. Each
worktree has its own `target/` (first build is cold) but `~/.contextnest/wal.jsonl`
is **shared** — back it up before any WAL-schema change. The Bash tool's
cwd resets between calls; `cd` into the worktree in every invocation.

**Merging to `main` is PR-only.** Direct push to `main` and local
`git fetch origin main` followed by merge are permission-denied by design.
Workflow: `gh pr create --base main --head <branch> --body-file <path>`
→ CI runs → merge via the GitHub UI **or** `gh pr merge <N> --squash
--delete-branch` once CI is green. Don't retry denied direct-push commands.

## Architecture map

| Path | Role |
|---|---|
| `src/api/tools.rs` | Seven canonical handlers + `create_tools_router` |
| `src/api/{sessions,inbox,field,substrate,stats,cc_hooks}.rs` | Sub-routers |
| `src/api/middleware/` | CORS, validation, metrics, error-intercept |
| `src/memory/attractors/` | `MemoryAttractorManager`, basins, connection network, decay |
| `src/services/{session_index,embedding,llm,consolidation,wal,graph}.rs` | Stateful services |
| `src/ingest/claude_code/` | Claude Code session backfill |
| `tests/seven_tools_api.rs`, `canonical_memory_chain.rs`, `llm_integration.rs` | Integration suites |
| `web/` | Vite + React 19 dashboard (port 5057) |

`MemoryAttractorManager::process_memories` is the single store entry point.
`retrieve` / `reconstruct` / `resonate` resolve session-affine ids via
`SessionIndex`, hydrate via `get_fragment`.

## Hot-path vs background consolidation (critical)

Live ingest writes sidecars only — `fragment_texts`, `fragment_metadata`,
`session_index`, `consolidation_queue`, WAL. The canonical attractor pipeline
runs on a background worker (default 500ms tick, 32 ids/batch, 4-way concurrent).

A fragment ingested <1s ago may be visible to `retrieve` but have no basin /
edges yet — it returns at similarity 0 and never anchors basin/connection
expansion. See `docs/architecture-honest.md` for the full grep-verifiable
lifecycle.

If `GET /api/v1/substrate/health` reports `basins.count == 0` on a populated
substrate, the worker isn't running or the embedder is down.

## Concurrency contract

- Never hold the manager lock across `await`. `adaptive_decay` enforces the
  pattern: read under lock → drop guard → execute async op.
- `SessionIndex` writers acquire all three maps (active/deleted/reverse)
  under one `write()`. Readers take only the map they need.
- `LlmService` wraps the provider client in `tokio::sync::Mutex` because the
  HTTP client isn't Send-safe across awaits.

## Adding a tool

The seven-tool surface is intentionally stable for v0.1.x. For approved
additions, see CONTRIBUTING.md — "How to add a new memory tool". Touch
`src/api/tools.rs`, register in `create_tools_router`, add ≥2 cases in
`tests/seven_tools_api.rs`.

## LLM + embedding config

LLM provider via env (no code change needed to switch):

```bash
CONTEXTNEST_LLM_PROVIDER=anthropic|openai|google
ANTHROPIC_API_KEY=... | OPENAI_API_KEY=... | GOOGLE_API_KEY=...
CONTEXTNEST_LLM_BASE_URL=https://proxy.example.com/v1   # optional (z.ai/LiteLLM)
```

Unset provider → `summarize` falls back to statistics-only; every other
tool unaffected.

Embedding provider in `config.toml` `[services.embedding]`. API keys resolve
in order: `api_key` literal → `api_key_env` → `$DEEPINFRA_API_KEY` →
`$OPENAI_API_KEY`. Never commit keys.

## Retrieve tuning (env knobs)

| Env | Default | Effect |
|---|---|---|
| `CONTEXTNEST_DECAY_HALF_LIFE_DAYS` | 60 | Global age-based decay half-life (days) |
| `CONTEXTNEST_DECAY_HALFLIFE_MULT_<KIND>` | per-kind | Scales the global half-life per `MemoryKind`. Defaults from `kind_registry::durability` (HarnessBridge Fig. 4): durable kinds (decision/verification/feature/learning/accomplishment/decision_made) ×2.0, volatile kinds (state/current_task/read_context/files_touched) ×0.5, everything else ×1.0. e.g. `CONTEXTNEST_DECAY_HALFLIFE_MULT_VERIFICATION=3.0`. |
| `CONTEXTNEST_RETRIEVE_BASIN_BOOST` | 0.7 | Basin-sibling outer multiplier |
| `CONTEXTNEST_RETRIEVE_CONNECTION_BOOST` | 0.5 | Graph-neighbor multiplier |
| `CONTEXTNEST_RETRIEVE_CONNECTION_MIN_WEIGHT` | 0.1 | Edge floor |
| `CONTEXTNEST_RETRIEVE_AUTO_RECONSTRUCT` | true | Auto-attach on chain queries |
| `CONTEXTNEST_RETRIEVE_TRUST_OBSERVED` | 1.0 | Trust multiplier for `provenance=observed` Verification records (receipt-confirmed). Untagged records always score 1.0. |
| `CONTEXTNEST_RETRIEVE_TRUST_PARTIAL` | 0.7 | Trust multiplier for `provenance=partial` (run exists, outcome unclassifiable) |
| `CONTEXTNEST_RETRIEVE_TRUST_CLAIMED` | 0.4 | Trust multiplier for `provenance=claimed` (no command cited; self-report only) |
| `CONTEXTNEST_RETRIEVE_TRUST_ABSENT` | 0.4 | Trust multiplier for `provenance=absent` (command cited but no matching receipt; fabricated reference) |
| `CONTEXTNEST_RETRIEVE_TRUST_CONTRADICTED` | 0.25 | Trust multiplier for `provenance=contradicted` (receipt disproved the claim). Set 0.0 to bury falsified claims. |
| `CONTEXTNEST_CONSOLIDATION_INTERVAL_MS` | 500 | Worker tick |
| `CONTEXTNEST_CONSOLIDATION_CONCURRENCY` | 4 | In-flight embedder calls |
| `CONTEXTNEST_MAX_CONNECTIONS_PER_NODE` | 32 | Top-K cap on edges created per new fragment in `create_connections_for_node`. Bounds avg_degree growth as the substrate fills. Lower → faster ingest, smaller graph; higher → richer connection-aware retrieval. |
| `CONTEXTNEST_CONNECTION_SIMILARITY_THRESHOLD` | 0.7 | Cosine-similarity floor for a peer to qualify as a connection candidate. Raise to 0.8 during backlog drain to halve fan-out. |

Full list in `docs/architecture-honest.md`.

## Gotchas

- **Single-fragment guard**: `process_memories` skips Step 3 (full reconstruction
  ingest) when `fragments.len() == 1` — every live `store` hits this path.
  Reconstruction runs on-demand via `reconstruct` or Phase 6 auto-attach.
- **Embedder model swaps invalidate basins** — there's no re-consolidation
  worker for that case yet.
- **`cargo run -- serve` vs `make cn-serve`** bind to different ports (see Quick start).
- **`profile.release` has `incremental = true`** (overrides upstream default) —
  the per-edit re-codegen drops from ~3min to ~20-30s.
- **`profile.fast`** (`cargo build --profile fast`, `target/fast/contextnest`) —
  ~5-15% slower runtime than release, ~6x faster compile; default for dev loop.
- **`cn-serve` warns but doesn't fail** when neither `DEEPINFRA_API_KEY` nor
  `OPENAI_API_KEY` is set; remote ingest will fail loudly at first call.
- **WAL is single-file persistence at `~/.contextnest/wal.jsonl`** — back it
  up to a `.bak-pre-<refactor>` sibling before any code change that touches
  WAL schema, session-id format, or migration logic. The migrator writes
  `.new` then renames original → `.bak` → new atomically; that `.bak` is
  the recovery breadcrumb — don't delete it until the new binary has run
  successfully for at least one session. The WAL is shared across worktrees.

## Observability — grep-verify tagline claims

Every claim in the README should have ≥1 caller outside `src/memory/attractors/`:

```bash
grep -rn "consolidation_queue.enqueue\|process_memories" src/api/ src/ingest/
grep -rn "decay_multiplier\|last_accessed"               src/api/tools.rs
grep -rn "basin_aware_expand\|list_basin_snapshots"      src/api/
grep -rn "connection_aware_expand\|list_neighbors"       src/api/ src/memory/
grep -rn "compute_reconstruction\|is_chain_query"        src/api/
grep -rn "get_substrate_health\|SubstrateHealth"         src/api/
```

Zero hits = the README claim has lost its caller — open an issue.

## Where to read next

- `docs/architecture.md` — conceptual model + per-tool sequence diagrams
- `docs/architecture-honest.md` — runtime truth, env knobs, grep-verify recipe
- `docs/usage.md` — copy-paste curl per tool + integration recipes
- `docs/development-workflow.md` — worktrees, branch/commit conventions, merge strategies, WAL safety
- `docs/roadmap/epics/` — neural-field-real (done), cross-session-learnings, cc-ingest
- `CONTRIBUTING.md` — canonical pipeline, four CI gates, adding a tool
