# Tenant/session and CPU implementation audit

Status: implementation and local validation complete, 2026-09-08; live activation pending. Implementation is isolated in the
`feat/tenant-session-cpu` and LibWit `feat/contextnest-session-memory` worktrees.
The running services and live WAL have not been migrated or restarted.

## Pass 1: requirements and implementation

Reviewed the task checklist, `docs/roadmap/epics/tenant-session-memory.md`,
`docs/architecture-honest.md`, `docs/ingest/claude-code-hooks.md`,
`docs/roadmap/epics/neural-field-real.md`, and LibWit's session foundation
and conversation-memory code. The scope is application tenant plus persisted
session identity; a separate cross-session knowledge bank is not this archive.

Closed gaps found during this pass:

- Actual router composition needed an authenticated fallback for unimplemented
  legacy URLs. Session tokens now cannot reach legacy/global surfaces.
- Health caching needed mutation invalidation while retaining a separate age
  statistics cache. The original health test now passes unchanged.
- Hooks needed bounded detached-task admission and upgrades to generated
  commands. Header files carry operator credentials without copying secrets
  into settings or process arguments.
- Deleted sessions needed to release quota while retaining their tombstone.
- Legacy terminal errors and retry backoff needed durable restart state.
- LibWit recall needed an independent deadline while sharing registration
  with background stores. A new test covers this failure mode.

Implementation/documentation subtasks discovered by pass 1 and resolved:

- [x] Give the CLI HTTP ingestion sink the same operator header-file support.
- [x] Separate tenant processing time from embedding and queue wait, and expose
  cumulative process CPU time in operator health without reading memory vectors.
- [x] Update the accepted design, architecture and hook documentation with the
  final API, limits, durability semantics, activation and migration procedures.
- [x] Complete final build/test/lint delta/format checks and an offline exact
  scoring benchmark, then repeat the requirements audit.

Live acceptance remains a separate operator replay: run the updated artifact,
restart LibWit with its application configuration, verify two real sessions and
measure CPU/recall latency under concurrent traffic. No source or fixture test
is treated as that live proof.

## Pass 2: final requirement and documentation review

Re-read the task checklist, accepted design and updated operator guide, then
LibWit's session-foundation requirements and graph-memory spec. The optional
shared knowledge bank is independent of this conversation archive. The final
implementation satisfies the source requirements below; the production
workload/physical-crash acceptance gates remain separate.

| Requirement | Final implementation and evidence |
|---|---|
| Tenant plus session ownership | Signed capabilities, application registration, and real-router negative cases in `src/services/tenants/tests.rs`; every legacy data route and fallback requires operator authorization. |
| Smaller exact search | Per-session canonical snapshots, cached norms, bounded deterministic top-K, validated vectors; exact-reference fixtures and the offline benchmark below. |
| Bounded work | Global request/CPU/provider limits, per-tenant admission, fair tenant/session scheduler, durable retry budgets, bounded hook tasks and active caches. |
| Atomic durable acceptance/completion | SQLite FULL/WAL commit before v2 202; revision/generation/lease checks; injected transaction failure and reopen fixtures. No physical power-loss test was performed. |
| Reset/delete/revision correctness | Old jobs cannot commit or fail a new generation; cache invalidation, quota-releasing deletion tombstones, duplicate/revision/discard cases. |
| Legacy CPU and restart repairs | Paced successful batches, cached graph norms and incident edges, scalar cached health, durable canonical state/retries and complete-line transcript checkpoints. |
| LibWit lifecycle and latency | Real harness IDs, start-bound voice archive, shared registration with independent deadlines, late-result suppression, bounded stores and isolated fallback. |
| Operational integration | Source-aware Make targets/build identity, private config generator, authenticated generated hooks/CLI, dry-run copied-WAL importer, documented activation. |

The second pass found no further source implementation gaps. The one new lint
finding used `Option::is_none_or`, which is newer than the declared Rust 1.80
minimum. Replacing it with `map_or(true, ...)` removed that diagnostic. Existing
tests were not rewritten to accommodate the implementation.

## Verification receipts

Commands ran from their respective implementation worktrees:

| Command | Observed result |
|---|---|
| `CARGO_BUILD_JOBS=2 cargo build` | Passed; final output in `/tmp/cn-final-build-2.log`. |
| `CARGO_BUILD_JOBS=2 cargo test` | 805 passed, 0 failed; 4 documentation examples ignored. Final run: `/tmp/cn-final-tests-3.log`. |
| `CARGO_BUILD_JOBS=2 cargo clippy --locked --lib --no-deps --message-format=json -- -D warnings` | 616 existing diagnostics on both base and branch; 0 introduced. Raw command exits 101 on both. Multiset comparison uses diagnostic path/code/message, not line numbers. |
| `cargo fmt --all -- --check` | Passed. |
| `git diff --check` | Passed in both worktrees. |
| `node tools/run-session-memory-tests.cjs` | 13 passed, 0 failed; `/tmp/libwit-final-tests.log`. |
| `node tools/check-session-memory-types.cjs 33e7bef` | 48 baseline and 48 current diagnostics, no introduced errors; `/tmp/libwit-final-type-delta.log`. |
| Focused `tsc --noEmit` on memory modules and session-isolation tests | Passed. Full Electron typechecking retains pre-existing generated-module/type errors. |
| `make -n cn-serve` | Confirmed Cargo release build precedes launch; no service started. |
| Temporary private-configuration fixture | Four mode-600 files, app/operator credential separation, no secret output, overwrite refusal passed. |

Clippy baseline: unmodified source at `d2be927`. Its full diagnostic log is
`/tmp/cn-baseline-clippy.jsonl`; final branch output is
`/tmp/cn-final-clippy-2.jsonl`. The repository's gate is no new diagnostics, not
zero existing debt. LibWit has no configured origin remote; its baseline is
local commit `33e7bef`.

## Offline CPU-kernel evidence

Ran `./target/debug/examples/scoped_exact_benchmark` on synthetic 256-dimensional
vectors, top-K 32. [Machine-readable result](2026-09-08-scoped-exact-benchmark.json):

| Scenario | Candidates | Debug elapsed time |
|---|---:|---:|
| Original cosine/norm calculation and full sort | 10,000 | 120.096 ms |
| Cached norms and bounded top-K | 10,000 | 53.333 ms |
| Exact search inside one session | 500 | 3.953 ms |

The two global paths returned the same IDs and scores within 1e-6. This is one
synthetic debug-profile run, without provider or HTTP latency, made while build
work was also present. It establishes reference parity and a local kernel
comparison; it does not establish a release/server CPU percentage or production
recall latency under another tenant's traffic.

## Operator acceptance still pending

- [ ] Restart from the implementation worktree and verify build identity.
- [ ] Start LibWit with its private application environment and replay real A/B
  interviews, explicit resume, cancellation and offline fallback.
- [ ] Replay the same live workload as the CPU report and measure steady-state
  CPU, health latency and recall latency during unrelated tenant traffic.
- [ ] If legacy import is desired, review the copied-WAL dry run and apply only
  to a new output directory; this is optional, not required for an empty LibWit tenant.

No running service was started, restarted or terminated. The original WAL was
backed up at `~/.contextnest/wal.jsonl.bak-pre-tenant-session-20260908-101645`
before persistence changes. Private configuration is prepared but inactive.
