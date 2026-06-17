# Epic — Role-Tailored Agent ContextPack

**Status:** **5 of 6 PRs shipped (PR-5 skipped per measurement, PR-6 deferred).** Original estimate: ~3-5 dev-days across 6 PRs; actuals are tracked in the per-PR sections below.

**Owner:** TBA.

**Last updated:** 2026-06-17.

## Shipment status

| PR | Title | Status | Commit / Note |
|---|---|---|---|
| **PR-1** | mini-ork capsule swap + timeout-default fixes | ✅ shipped 2026-06-17 | mini-ork `6261eb8` (#21) |
| **PR-2** | CN consolidation backoff + adaptive concurrency | ✅ shipped 2026-06-17 | ContextNest `f0701ee` (#159) |
| **PR-3** | mini-ork role-tailored ContextPack helpers | ✅ shipped 2026-06-17 | mini-ork `0d0aaa2` (#23) |
| **PR-4** | mini-ork worker prompt template wires MO_CN_PREFETCH_DIR | ✅ shipped 2026-06-15 (eb9bd5d), restored 2026-06-17 after PR #18 regression | mini-ork `325c8c8` (#22) |
| **PR-5** | Optional composed `/api/v1/agent/context-pack` CN endpoint | ⏸ **skipped per PR-3 latency measurement** | per-endpoint composition completes in ~3-5s; well under planner LLM wall-clock. Keeping operational surface small. Revisit if a future workload makes the composition latency painful. |
| **PR-6** | Outcome feedback loop (EvoMem pattern) | ⏸ planned | Needs new CN endpoint `POST /api/v1/agent/outcome` + sidecar table. Separate PR. |

### Concrete proof-of-value already collected

- **Drift audit motivating the epic (2026-06-15):** a memory entry asserted 10 facts about a chapter-anchor schema; verification showed 3 outright wrong, 2 internally inconsistent, 2 incomplete. After PR-3 ships, planners running similar audits see the failed-assertion atoms (via `## Failures to avoid` capsule section + risk_flag clusters) before forming the plan.
- **CPU storm motivating PR-2 (2026-06-15 → 2026-06-17):** the consolidation worker pegged 92% CPU sustained 9h on a 296K-fragment backlog with embedder rate-limit retries. After PR-2 ships, the same binary running against the same backlog idles at 0.2% CPU because the worker now backs off when the embedder returns `engine_overloaded`.
- **Smoke Test Standard self-validating:** PR #18 silently dropped half of PR #17's wiring during a cross-merge — caught within hours of authoring the Standard by `scripts/smoke-cn-bridge.sh`. The same regression class then dropped PR-4's wiring a second time and was caught by direct grep during the PR-1 smoke run. The Standard is now also responsible for the per-PR `scripts/smoke-pr-<N>-<slug>.sh` deliverables shipped with #21, #22, #23, and ContextNest #159.

**Spans:** ContextNest (`docs/roadmap/epics/`) + mini-ork (`lib/`, `bin/`, `hooks/`, `docs/`).

**Companion docs:**
- `docs/CONTEXTNEST-INTEGRATION.md` in mini-ork (the v1 bridge merged as SourceShift/mini-ork#17).
- This file lives in ContextNest because the canonical role→endpoint mapping is CN-substrate-shaped; mini-ork is the consumer.

---

## The honest problem

The just-merged v1 bridge (SourceShift/mini-ork#17, commit `896ab88`) wires mini-ork's planner and worker subagents to ContextNest's substrate via three call sites: `cn_retrieve`, `cn_sessions_by_file`, and a UserPromptSubmit prefetch hook that bundles `cn_retrieve + cn_inbox + cn_features_recent`. That ships the **plumbing**.

But the **payload** is shallow:

1. **Same query, every role.** Planner, worker, verifier, reflector all receive a top-N similarity hit list against the brief embedding. No role-awareness. A verifier asking "what broke before?" gets the same atoms as a planner asking "what's the strategy?"
2. **Flat hits, not clusters.** `/api/v1/tools/retrieve` returns individual fragments. Duplicates leak in, semantically-identical paraphrases stack up, and the prompt gets noisy fast.
3. **`/api/v1/prompt-context/capsule` exists and is not called.** CN has a deterministic markdown-renderer that clusters atoms by kind, orders by what an agent most needs to know first (risks → decisions → failures → directives → verifications → evidence → reads → artifacts → assumptions), substring-filters by query, and caps output to ~8KB. Strictly better signal than raw retrieve for planner-style consumption. Built. Unused.
4. **Substrate features unused.** Basins (topic clusters with centroids), connections (graph neighbors), reconstruct (chain queries), resonate (co-retrieval signal), inbox priority filtering — all live, none surfaced.
5. **Background consolidation thrash.** The 296K-fragment backlog audit (2026-06-15) showed the consolidation worker pegging CPU at 92% for hours due to embedder rate-limit retries + lack of backoff + re-enqueue on every server restart. Indirect but real impact: when CN is overloaded, retrieve latency spikes past mini-ork's 2s timeout, and the bridge silently no-ops.

The drift audit on 2026-06-15 made the cost concrete: a planner reading a stale memory entry would have built a wrong plan for a chapter-anchor schema task. CN had fresher data; the bridge didn't ask for it the right way.

---

## Goal

Turn the v1 bridge from a search-engine shim into a **role-aware substrate consumer** that pulls the right slice of CN for each workflow node, with deterministic markdown output and graceful fallback when CN is down.

Two complementary tracks:

- **Mini-ork side:** add role-tailored ContextPack helpers backed by CN's already-exposed endpoints. No CN code changes for the bulk of value.
- **ContextNest side:** harden the consolidation worker so the bridge it backs is reliable, and add one cross-cutting endpoint (`/agent/context-pack`) only if mini-ork-side composition proves too chatty.

---

## Sub-epics

Six PRs total. **Numbering = ship order.** Each is independently mergeable.

### PR-1 — Mini-ork: capsule swap (quick win)

**Scope:** Replace `context_contextnest_atoms_md`'s flat retrieve with a call to `/api/v1/prompt-context/capsule`. Same call-site in `bin/mini-ork-plan`. ~50 LoC. Strictly improves planner injection quality (kind-ordered, deduplicated, ~8KB cap).

**Files:**
- `lib/cn_client.sh`: add `cn_capsule [since] [project] [query]` → calls `/api/v1/prompt-context/capsule` (`Accept: text/markdown`).
- `lib/context_assembler.sh`: change `context_contextnest_atoms_md` to prefer capsule; fall back to retrieve when capsule returns empty (e.g., consolidation hasn't caught up).
- `tests/unit/test_cn_client.sh`: add capsule case to in-process http stub.

**Acceptance:**
- Planner injection block now starts with `## Risks`, `## Decisions`, `## Failures to avoid` headings instead of flat `- [learning sim=0.62 ...]` lines.
- When CN returns 503 or empty, falls back to retrieve (same UX as before).
- Unit test green; smoke against live CN shows kind-ordered output.

**Effort:** ~half day.

---

### PR-2 — ContextNest: consolidation backoff + pre-truncate

**Scope:** Address the 92%-CPU finding from 2026-06-15. Two surgical fixes inside `src/services/consolidation.rs` (and a small one in `src/services/embedding.rs`):

1. **Detect `engine_overloaded` / `429` / `Model busy` error strings** in `consolidate_one`'s `embed: …` error path. Bubble a `RateLimited` failure tag up from `process_batch`.
2. **Exponential backoff in `run_worker`** when the batch's failure mix includes RateLimited. Start at `interval_ms`, double on each consecutive rate-limited batch, cap at `CONTEXTNEST_CONSOLIDATION_MAX_BACKOFF_MS` (default 30s). Reset on a clean batch.
3. **Reduce in-flight concurrency** when backing off (4 → 2 → 1, restore on clean batch).
4. **Pre-truncate input** before serializing the embed request — already done in `generate_embedding`, but the consolidation pipeline embeds the *full text* before the cache key calc. Move the truncate above the cache lookup so we don't waste JSON serialization on 80KB strings that get clipped to 8KB.

**Optional Phase 2 (separate PR if scope creeps):**
5. **Persistent consolidation cursor** — track last-processed fragment id in WAL or a sidecar so server restarts don't re-enqueue 296K fragments. Most user-visible CPU reduction.

**Files:**
- `src/services/consolidation.rs`: tag enum + backoff loop + reduced concurrency under pressure.
- `src/services/embedding.rs`: move truncate above cache key calc (test invariant: cache hits on equal-after-truncation inputs).
- Two new env knobs in `CLAUDE.md` table: `CONTEXTNEST_CONSOLIDATION_MAX_BACKOFF_MS`, `CONTEXTNEST_CONSOLIDATION_BACKOFF_CONCURRENCY_FLOOR`.
- Unit tests: `tests/consolidation_backoff.rs` (mock embedder returning `engine_overloaded` for first 3 calls, verify backoff curve).

**Acceptance:**
- Four CI gates green per CLAUDE.md.
- Local smoke: start CN with a forced rate-limit (use a known-throttled API key), observe backoff curve in `tracing` logs (lap_ms grows; concurrency drops).
- After backoff trigger, no CPU spike above 30% sustained.

**Effort:** ~1 dev-day (excluding persistent cursor; that's +1 day).

---

### PR-3 — Mini-ork: role-tailored ContextPack helpers

**Scope:** Generalize PR-1's single capsule call into per-role pack builders. Maps mini-ork's 8 workflow node types to specific CN endpoint combinations.

**Role mapping (canonical):**

| Role | Endpoints called | Pack section names |
|---|---|---|
| **planner** | `capsule?since=14d` + `sessions/by-intent?q=<task_class>` + `inbox?urgency=now` + `field/basins?project=<cwd>` | Risks · Decisions · Open user actions · Topic clusters |
| **researcher** | `tools/retrieve` (broad) + `sessions/by-feature?q=<brief_keywords>` + `sessions/find` (substring) | Semantic recall · Feature history · Substring matches |
| **implementer** | `sessions/by-file` (per file in `files_in_play`) + `features?since=48h&layer=<layer>` + `connections?node_id=<top_hit>` | Recent editors · Adjacent deliveries · Graph neighbors |
| **reviewer / verifier** | `capsule?kind=failure,verification` + `tools/retrieve` filter kind=failure | Known failures · Verifications already run |
| **reflector** | `sessions/:id/trajectory` (top-3 prior runs) + `sessions/by-intent` + `tools/summarize` | Prior-run trajectories · Same-intent runs · Summary |
| **publisher / rollback** | `features?since=24h` + `inbox` | Last 24h shipments · Blocking items |

**Files:**
- `lib/cn_client.sh`: add wrappers for `cn_basins`, `cn_connections_for`, `cn_inbox_filtered`, `cn_summarize`. Keep payload-rendering helpers `cn_render_capsule_md` etc. one per output shape.
- `lib/context_role_packs.sh` (new): dispatch table `context_role_pack_md <role> <brief> [files_csv]` → fan-out concurrent curls, merge with section headings.
- `bin/mini-ork-plan`: invoke `context_role_pack_md "planner" "$KICKOFF"` instead of today's `context_contextnest_atoms_md`.
- `bin/mini-ork-execute` (or whichever script dispatches workers): inject role pack into worker prompt env so workers get tactical context.
- `hooks/subagent-prefetch.sh`: take role hint from `MO_WORKFLOW_NODE_TYPE` env (passed via launcher), default to "implementer" if unset.

**Acceptance:**
- Dry-run plan against `kickoffs/oracle-hardening-v03.md` shows planner block with the 4 named sections.
- Dry-run worker (any kickoff with `files_in_play`) shows tactical block with editor history + adjacent deliveries.
- All bash tests green (`tests/run-all.sh unit security integration smoke`).
- `MO_DISABLE_CN=1` and CN-down paths still return empty silently per existing contract.

**Effort:** ~1.5 dev-days.

---

### PR-4 — Mini-ork: worker prompt template wires `MO_CN_PREFETCH_PATH`

**Scope:** Close the deferred follow-up from the v1 bridge — workers currently receive a prefetch markdown file but no prompt template references it.

**Files:**
- `bin/_worker-launcher.sh`: export `MO_CN_PREFETCH_PATH` to spawned worker session.
- Each recipe's worker prompt template under `recipes/<recipe>/prompts/`: add a conditional inline of the prefetch file at the top (only if `MO_CN_PREFETCH_PATH` set and file exists).
- Generic template addition for new recipes: `prompts/_worker-prefetch-header.md` snippet that recipes can include.

**Acceptance:**
- Worker prompt logs (in `.mini-ork/runs/<run>/logs/`) show the prefetch markdown inlined at the top.
- Recipes without the include still work — backward compatible.

**Effort:** ~half day.

---

### PR-5 — ContextNest: optional `/api/v1/agent/context-pack` (composed endpoint)

**Scope:** Conditional on PR-3 measurement. If role-tailored packs prove too chatty (5 endpoints × 200ms latency × 8 roles = noticeable planner stall), collapse the common patterns into a single CN endpoint that mini-ork hits with `?role=planner&brief_query=...`. CN runs all 4 sub-queries concurrently in Rust, caches the assembly, returns one markdown blob.

**Files:**
- `src/api/agent_context.rs` (new): handlers per role, internally calls the existing `retrieve` / `capsule` / `basins` / etc.
- `src/api/mod.rs` + `src/api/simple.rs`: register router.
- `tests/agent_context_api.rs`: 4 cases (one per role group).
- `lib/cn_client.sh` (mini-ork side, follow-up PR): switch role packs to use the single endpoint when available; fall back to per-endpoint composition when 404.

**Gate before starting PR-5:** Measure PR-3's actual latency in a real planner dispatch. Only ship PR-5 if the composed approach measurably beats per-endpoint composition (target: ≥30% latency reduction). Otherwise keep mini-ork-side composition — simpler operationally.

**Acceptance:**
- 4 CI gates green per CLAUDE.md (CN side).
- New endpoint returns within 800ms p50 for "planner" role against a 296K-fragment substrate.
- Mini-ork bash tests still green with single-endpoint switch.

**Effort:** ~1 dev-day (skippable if PR-3 latency is fine).

---

### PR-6 — Mini-ork: outcome feedback loop (EvoMem pattern)

**Scope:** Close the deferred Slice-5 from the v1 bridge plan. After `subagent_stop`, POST `{atom_ids_used[], outcome: success|fail, evidence}` to CN. CN bumps atom confidence on success, decays on contradiction.

**Why deferred to PR-6:** Needs a CN endpoint (`POST /api/v1/agent/outcome`) that doesn't exist yet. Ship after PR-5 if PR-5 lands (the agent_context.rs file is the natural home).

**Files:**
- ContextNest: `src/api/agent_context.rs` (extend with outcome handler) + sidecar table for `atom_outcomes`.
- Mini-ork: `hooks/subagent-stop.sh` extension to track which atom_ids the worker received in its prefetch + emit outcome at stop.

**Acceptance:**
- CN-side: `POST /agent/outcome` with array of atom_ids + outcome bumps `last_accessed` or a confidence field, no WAL schema break.
- Mini-ork side: smoke test that after a successful worker, the atoms it consumed have `last_accessed` advanced.

**Effort:** ~1 dev-day.

---

## Dependency graph

Two roots (PR-1, PR-2) feed PR-3. PR-3 → PR-4 and (after a measurement
gate) → optional PR-5. PR-4 + PR-5 both feed PR-6.

```
  PR-1 capsule swap     ──┐
  mini-ork (~0.5d)        │
                          ▼
                       PR-3 role-tailored packs ─┬─▶ PR-4 worker template wiring
                       mini-ork (~1.5d)          │   mini-ork (~0.5d)
                          ▲                      │              │
  PR-2 consolidation      │                      │ measurement  │
  ContextNest (~1d)     ──┘                      │ gate         │
                                                 ▼              │
                                            PR-5 composed       │
                                            ContextNest         │
                                            (~1d, optional) ────┤
                                                                ▼
                                                          PR-6 outcome feedback
                                                          both repos (~1d)
```

PR-1 and PR-2 are independent and can land in parallel. PR-2 is the CPU/reliability prereq for PR-3 to perform well under load.

---

## Acceptance criteria for the epic as a whole

Epic is **done** when:

1. Planner dispatches on real kickoffs produce prompts with kind-ordered ContextPack sections instead of flat similarity hits (PR-1 + PR-3).
2. Worker dispatches see prefetch markdown inlined in their prompt at session start (PR-4).
3. ContextNest consolidation worker no longer sustains >50% CPU for >10 minutes after startup on a populated substrate (PR-2).
4. End-to-end: a chapter-anchor-audit-style drift incident reproduced after the epic ships surfaces correct (not stale) atoms in the planner pack on the first dispatch.
5. CN-down fallback verified for each new role pack — silent degradation to local sqlite + skipped sections.

---

## Risk notes

- **Latency creep.** Per-role packs make multiple CN calls. Mitigate with concurrent `curl &` + `wait`, cn_available reachability cache, and the PR-5 measurement gate.
- **Capsule depends on kind metadata.** Atoms without a `kind` field don't appear in capsule output. Some legacy atoms in the substrate lack `kind` — partial coverage. Worst case: capsule returns smaller-than-expected; PR-1 falls back to retrieve.
- **Consolidation persistent cursor (PR-2 Phase 2) touches WAL.** Per CLAUDE.md gotchas, back up WAL before any code change that touches WAL schema. Use the `.bak-pre-cursor` naming convention.
- **PR-5 doubles surface area.** Adding `/api/v1/agent/*` endpoints expands the CN public API. Document them in the api map in CLAUDE.md before shipping. If PR-3 latency is fine, **skip PR-5** to keep the surface stable.
- **PR-6 outcome feedback is silent reinforcement.** A worker that succeeds with bad atoms in its prefetch will still bump those atoms. Bound the confidence delta per atom per day (e.g. ±0.05) so a single noisy worker can't pin a basin.

---

## Smoke Test Standard (required per PR)

Unit tests with stub HTTP servers prove the code compiles and the happy
path returns the right shape. They do **not** prove the user-visible
feature works. PR #17's `CN_TIMEOUT_SEC=2` bug shipped CI-green and
broke silently in real planner dispatches — the stub returned in
milliseconds, hiding the production timeout. Every PR in this epic
must ship a smoke harness that catches that class of failure.

A smoke harness is a `scripts/smoke-<pr-slug>.sh` shell script that:

1. **Runs against the live system** (real CN server, real mini-ork
   dispatch, real OpenAI embedder). Not mocks, not stubs. If the user's
   machine doesn't have CN running, the script starts it (or refuses
   with a clear "start CN first" message and exits 78).
2. **Exercises the actual code path** a user would hit. For mini-ork
   changes: invoke `mini-ork-plan` against a real kickoff in
   `kickoffs/`. For CN changes: POST to the real endpoint. Don't
   short-circuit via env vars that the production code doesn't see.
3. **Asserts on observable evidence**, not just exit codes. "Feature
   works" = "the file at `.mini-ork/runs/<run>/cn_prefetch/<sid>.md`
   contains at least N atoms" OR "the captured planner PROMPT_TEXT
   contains `## Risks`" — not just "the function returned 0".
4. **Produces a verifiable artifact** at a deterministic path
   (`tmp/smoke-evidence/<pr-slug>-<timestamp>.md`) listing what was
   tested, the actual outputs (truncated to 200 lines), pass/fail per
   assertion. A human reviewer should be able to `cat` the evidence
   file and say "yes this works" without re-running anything.
5. **Tests the failure paths too**, not just the happy path. Each
   harness must include:
   - **CN-down case**: kill CN (or set `CN_BASE_URL=http://127.0.0.1:1`),
     run the same flow, assert it degrades gracefully (no crash, empty
     section in output, exit 0).
   - **CN-slow case**: point CN_BASE_URL at a sink that delays >timeout,
     assert silent no-op without sustained CPU.
   - **`MO_DISABLE_CN=1` case**: assert short-circuit + no network
     traffic (verified via `lsof -i :28080` count before/after).
6. **Runs in under 2 minutes** so CI can include it without budget
   pressure. Heavier soaks (PR-2's 30-min CN CPU monitor) live as
   separate `scripts/soak-*.sh` invoked manually pre-merge.

### Evidence format

`tmp/smoke-evidence/<pr-slug>-<timestamp>.md`:

```markdown
# Smoke evidence: <PR title>
**Ran:** <timestamp>  **Branch:** <branch>  **Commit:** <sha>
**CN url:** <CN_BASE_URL>  **CN reachable:** yes|no

## Assertion 1: <name>
**Expected:** <one-line>
**Actual:** <one-line OR file path with truncated dump below>
**Verdict:** ✅ PASS | ❌ FAIL — <reason>

[... per assertion ...]

## Captured outputs
- planner PROMPT_TEXT (200 lines max): <inline or file ref>
- cn_prefetch file: <path + first 50 lines>
- cn call log: <paths + bodies>

## Failure-path coverage
- CN-down: ✅ degraded silently (planner produced plan w/ empty CN block)
- CN-slow (3s sink): ✅ silent no-op, no CPU spike
- MO_DISABLE_CN=1: ✅ zero network calls to :28080 during run
```

If a PR ships without a `scripts/smoke-<slug>.sh` that produces this
artifact and includes failure-path coverage, the PR is not done —
regardless of CI green or unit-test counts.

### Reference harness (retrofit for v1 bridge)

Before PR-1 ships, retrofit the v1 bridge (SourceShift/mini-ork#17) with
`scripts/smoke-cn-bridge.sh` to establish the pattern concretely. This
backfills the smoke gap that allowed CN_TIMEOUT_SEC=2 to ship.

The retrofit lives in mini-ork as a separate PR
(`feat/smoke-cn-bridge`) and serves as the copy-paste template for
every harness in this epic.

## Verification plan (per PR, layered on the Smoke Standard above)

| PR | Smoke harness | Failure-path cases | CI |
|---|---|---|---|
| PR-1 | `scripts/smoke-pr1-capsule-swap.sh` — planner dry-run produces PROMPT_TEXT containing `## Risks` / `## Decisions` / `## Failures to avoid` headings; capsule fallback to retrieve when capsule returns empty | CN-down, CN-slow, MO_DISABLE_CN, empty-capsule (fresh substrate) | bash tests + harness in CI |
| PR-2 | `scripts/smoke-pr2-consolidation-backoff.sh` — mock embedder returning `engine_overloaded` for first 3 calls; assert backoff curve (lap_ms doubles), concurrency drops, no >50% CPU sustained 30s | Real OpenAI rate-limit (use a throttled key), embedder-down (CN stays up), WAL-replay restart (no re-storm) | 4 CI gates + harness |
| PR-3 | `scripts/smoke-pr3-role-packs.sh` — fire planner + implementer + verifier packs against `kickoffs/oracle-hardening-v03.md`; assert each contains role-specific section names; assert distinct content (planner ≠ implementer pack) | Per-role CN-down, per-role MO_DISABLE_CN, missing-files-in-play (implementer pack should still render) | bash tests + harness |
| PR-4 | `scripts/smoke-pr4-worker-prefetch.sh` — spawn a real worker via `_worker-launcher.sh`, inspect `.mini-ork/runs/<run>/logs/<sid>.prompt` for prefetch header inlining, assert MO_CN_PREFETCH_PATH exported | Recipe-without-prefetch-include (backward compat), prefetch-file-missing (worker still spawns) | bash tests + harness |
| PR-5 | `scripts/smoke-pr5-composed-endpoint.sh` — curl `/api/v1/agent/context-pack?role=planner` 10× against live 296K-fragment substrate; assert p50<800ms, p99<2s; assert single-endpoint output equal-shape to per-endpoint composition fallback | CN-down composed (mini-ork falls back to per-endpoint), endpoint-returns-empty (planner pack still has local sqlite sections) | 4 CI gates + integration test + harness |
| PR-6 | `scripts/smoke-pr6-outcome-feedback.sh` — run a worker, capture atom_ids consumed in prefetch, succeed worker, assert atoms' `last_accessed` advanced in WAL via `/api/v1/fragments?id=<id>`; bound-check: confidence delta within ±0.05 cap | Outcome-endpoint-down (worker stop still completes), atom-not-found (silent skip, no crash) | bash + Rust + harness |

Each harness commits alongside its PR, exercises the live system, and
produces the evidence artifact described above. A reviewer reads the
evidence file before approving — that's the confidence gate.

---

## What's explicitly out of scope

- **No new vector store.** Stays on the current embedder + Qdrant-style in-memory similarity.
- **No retraining of an embedding gating model** (StackPlanner GRPO equivalent). Threshold-based gates only — revisit if signal proves weak.
- **No tool-call-level instrumentation** (e.g., capturing every Edit/Bash call as a substrate event). Hook framework only — substrate ingest stays at session-transcript granularity.
- **No multi-tenant ACL.** Single-user assumption preserved. Per-project filtering via `project_cwd` substring is the only authorization layer.

---

## References

- v1 bridge: SourceShift/mini-ork#17 (commit `896ab88`, 2026-06-15).
- 2026-06-15 drift audit motivating the role-tailored pack (chapter-anchor schema, 10 claims, 3 outright wrong).
- 2026-06-15 CPU audit motivating PR-2 (296K-fragment backlog, 92% CPU sustained 9h, embedder rate-limit retry storm).
- Arxiv backing for the design patterns:
  - StackPlanner (arXiv:2601.05890) — "Experience Search" pre-fetch + REVISE action.
  - Intrinsic Memory Agents (arXiv:2508.08997) — agent-role-scoped memory.
  - EvoMem (arXiv:2511.01912) — outcome-feedback decay (PR-6 pattern).
- `src/api/prompt_context.rs:637-700` — the capsule renderer that PR-1 surfaces.
- `src/services/consolidation.rs:461-507` — the worker loop that PR-2 hardens.
- mini-ork `lib/context_assembler.sh` + `bin/mini-ork-plan` — PR-1/PR-3 graft points.
