# Epic: 2026 arxiv-driven substrate improvements

**Status:** Sprint 1 shipped in v0.1.1 (T1 gate + reconsideration, T3 replay, T4 pruning; T2 was already implemented in v0.1.0). Sprint 2 and Sprint 3 remain backlog.
**Source:** literature survey against 2026 arxiv papers matching ContextNest's problem space (agent memory, consolidation, retrieval quality, provenance).
**Date opened:** 2026-09-17.

Each ticket below cites its source paper, names the ContextNest primitive it changes, sizes it (XS/S/M/L/XL), and points at the exact file(s) to touch. Sizes are effort estimates only; nothing here is authorized to land without its own PR, tests, and CI green.

The tickets are grouped into three sprints. Sprint 1 items are independent and can land in parallel. Sprint 2 assumes Sprint 1 is in. Research-tier items are read-only until a scoped design doc lands separately.

## Sprint 1 — high-signal, S-size, independent — **SHIPPED v0.1.1**

### T1. RecMem recurrence-gated consolidation — **SHIPPED v0.1.1**

- Paper: [RecMem — arXiv:2605.16045](https://arxiv.org/abs/2605.16045)
- Primitive: pre-pass inside the consolidation batch loop. Before invoking the embedder + basin/graph pipeline for a fragment, count how many peers in the same `(tenant_id, session_id)` already have cosine ≥ `θ_sim` to it. If the count is below `θ_count`, requeue the fragment as "subconscious" (sidecar remains, no basin, no graph node).
- Size: **S** (~1 day).
- Files: `src/services/consolidation.rs`, `src/config.rs`. New env: `CONTEXTNEST_CONSOLIDATION_RECURRENCE_MIN_COUNT` (default 5), reuse existing `CONTEXTNEST_CONNECTION_SIMILARITY_THRESHOLD` (default 0.7 — matches paper's `θ_sim`).
- Expected effect: paper reports 87% consolidation-token cut and accuracy 68.8% → 81.1% on LoCoMo. For ContextNest specifically, this trims embedder round-trips and the O(N·D) graph scan on sparse fragments — the two hotspots of the 2026-09-08 CPU report.
- Test plan: unit test proving a fragment with zero session-peers stays in the queue; integration test that its retrieval still works via sidecar; benchmark harness comparing consolidated-fragment count with/without the gate over a fixed corpus.

### T2. MemClaw pre-filter — move tenant scoping before cosine scan — **SHIPPED v0.1.0**

Already implemented in the v2 tenant API before this epic was opened. `src/api/tenants.rs::retrieve` authenticates the session capability and constructs `MemoryScope` before any similarity scoring runs; the v1 legacy `retrieve` handler in `src/api/tools.rs` also scopes to `active_fragments_session_map()` / `list_active(session_id)` before cosine. Ticket kept in this epic for traceability against the source paper.



- Paper: [MemClaw governed shared memory — arXiv:2606.24535](https://arxiv.org/html/2606.24535v1)
- Primitive: today `retrieve` filters by `session_id` post-similarity. The pre-v0.1.0 legacy code path retained global search when session was absent. Move the `(tenant_id, session_id)` filter to run **before** cosine scoring — the scoped candidate set is authoritative, similarity ranks it.
- Size: **XS–S** (~0.5 day).
- Files: `src/api/tools.rs` (retrieve), `src/api/middleware/request_context.rs`.
- Expected effect: multi-tenant isolation becomes correct-by-construction rather than probabilistic. Aligns with the v0.1.0 tenant isolation PR (#175). Small perf win: cosine scan is smaller when the scope filter runs first.
- Test plan: cross-tenant retrieve rejection tests already in `src/services/tenants/tests.rs`; add one that would have passed under the old post-filter but fails under pre-filter (poisoned candidate).

### T3. PROJECTMEM replay endpoint — **SHIPPED v0.1.1**

- Paper: [PROJECTMEM — arXiv:2606.12329](https://arxiv.org/html/2606.12329v1)
- Primitive: expose `GET /api/v1/substrate/replay?since=<ts>` that rebuilds derived state (basins, graph, session_index) from the input WAL alone. Diagnostic-only; does not touch the canonical checkpoint.
- Size: **S** (~1 day).
- Files: `src/api/substrate.rs`, `src/services/wal.rs`.
- Expected effect: adds a corruption-recovery escape hatch and validates the WAL is truly authoritative (matches the design claim in `docs/architecture-honest.md`). Does not change hot path.
- Test plan: integration test that seeds a session, deletes the canonical SQLite, calls `/replay`, and verifies retrieval still works.

### T4. Chain-of-Memory pruning on reconstruction — **SHIPPED v0.1.1**

- Paper: [Chain-of-Memory — arXiv:2601.14287](https://arxiv.org/abs/2601.14287)
- Primitive: in `reconstruct`'s chain assembly, drop path nodes whose cosine to the query exceeds a cutoff. Prevents off-topic fragments from being stitched into the reconstructed chain.
- Size: **S** (~1 day).
- Files: `src/api/tools.rs` (reconstruct handler).
- Expected effect: paper reports +7.5–10.4% absolute chain accuracy and −97% chain-token count. For ContextNest, cleaner `reconstruct` output; the auto-reconstruct path in `retrieve` benefits transitively.
- Test plan: fixture chain with a known off-topic fragment; assert it is pruned; assert relevance-ordered fragments survive.

## Sprint 2 — S-to-M, some ordering

### T5. Multi-factor retrieval value V(m)

- Paper: [Learning What to Remember — arXiv:2606.12945](https://arxiv.org/html/2606.12945)
- Primitive: replace the single `decay_multiplier` in `retrieve` with a linear combination V(m) = Σᵢ wᵢ fᵢ(m) over seven signals: reliability (provenance + kind), self-relevance, goal-relevance, emotional-intensity, value-alignment, task-utility, usage-frequency decay. ContextNest already tracks four of the seven (`MemoryKind`, `last_accessed`, `provenance`, cosine). Two new metadata fields added at ingest: `retrieval_count`, `session_affinity_score`. Weights initializable from paper's Table 3 — no training required.
- Size: **S–M** (~2–3 days).
- Files: `src/api/tools.rs` (retrieve), `src/memory/attractors/decay.rs`, `src/config.rs`.
- Expected effect: paper reports gold-evidence retention 0.770 vs 0.368 for recency-only, 0.518 for the best individual factor. Directly improves inbox surfacing and `retrieve` quality on long-lived sessions.
- Test plan: fixture with fragments varying on 3+ signals; assert the composite score matches paper's ordering; regression test that the existing per-kind half-life default still dominates when only kind + age vary.

### T6. HiGram MicroGraph path-level localization

- Paper: [Hierarchical Graph Memory with Path-level Localization — arXiv:2608.05095](https://arxiv.org/abs/2608.05095)
- Primitive: partition the connection graph into MicroGraphs indexed by 2–3 anchor keywords extracted from each fragment (TF-IDF or the existing summarizer output). At consolidation, `create_connections_for_node` pre-filters candidates to fragments sharing at least one anchor, instead of scanning the full graph. Inverted index `keyword → [fragment_id]` lives in the per-tenant SQLite (already present post-v0.1.0). Cold index build at startup from existing fragment metadata.
- Size: **M** (~few days).
- Files: `src/services/consolidation.rs`, `src/memory/attractors/connection_network.rs`, WAL migration in `src/services/wal.rs`, schema in `src/services/tenants/database.rs`.
- Expected effect: paper reports 50.62 F1 on LoCoMo using 7.2% of full-context tokens. For ContextNest, this is the direct successor to the v0.1.0 CPU repair — the exact-search scope shrinks from "all in-session peers" to "anchor-sharing peers". Basin scan cost drops proportionally.
- Test plan: benchmark harness comparing `create_connections_for_node` walltime with/without pre-filter on a fixed 10k-fragment session; correctness test that connection edges match the pre-filter-off baseline within a tolerance.

### T7. SCM ValueTagger + adaptive forgetting threshold

- Paper: [SCM Sleep-Consolidated Memory — arXiv:2604.20943](https://arxiv.org/html/2604.20943v1)
- Primitive: replace the current `(recency × per-kind half-life)` decay with a four-dimensional ValueTagger — novelty (0.30), task-relevance (0.35), emotional-intensity (0.20), repetition (0.15). Add an adaptive forgetting threshold θ_f = μ_i − σ_i · (|G|/target_size) that auto-prunes basins as the graph grows, replacing the manual `CONTEXTNEST_MAX_CONNECTIONS_PER_NODE` cap.
- Size: **S** (~2 days).
- Files: `src/memory/attractors/decay.rs`, `src/memory/attractors/attractor_basin.rs`, `src/config.rs`.
- Expected effect: basin size stays bounded without operator tuning; decay weights are auditable and match published values. Complements T5 (V(m) at retrieval) — this operates at consolidation time.
- Test plan: fill a session past target_size, assert θ_f prunes to target ± σ; regression test that per-kind durability multipliers still dominate for durable kinds.

### T8. Selective forgetting eviction pass

- Paper: [Selective Forgetting — arXiv:2608.28978](https://arxiv.org/html/2608.28978)
- Primitive: after each consolidation tick, walk the graph and mark nodes below the SCM adaptive forgetting threshold (T7) for lazy deletion. Deletion is logical (state transition), not physical — matches ContextNest's existing `discard` semantics.
- Size: **S** (~1 day, depends on T7).
- Files: `src/services/consolidation.rs`, `src/memory/attractors/connection_network.rs`.
- Expected effect: bounded `avg_degree` under long-running sessions; caps the basin scan cost that T6 already reduces. Together T6+T7+T8 gives complete control over graph growth rate.
- Test plan: long-running session simulation (10k inserts) with/without eviction; assert `avg_degree` stays within band.

## Sprint 3 — M, depends on Sprint 1+2

### T9. TiMem temporal-hierarchical memory tree

- Paper: [TiMem — arXiv:2601.02845](https://arxiv.org/pdf/2601.02845)
- Primitive: add a summary layer above the flat fragment list. Raw fragments roll up into session-summary nodes; session-summaries roll up into persona/tenant-level nodes. `reconstruct` and `resonate` traverse this tree instead of scanning all fragments. Complements T4 (chain pruning): the tree bounds the search scope; T4 prunes within-scope hits.
- Size: **M** (~1 week).
- Files: `src/api/tools.rs` (reconstruct, resonate, summarize), `src/memory/attractors/memory_attractor_manager.rs`, WAL migration.
- Expected effect: paper reports 75.3% on LoCoMo and −52% recall tokens vs a flat list. For ContextNest, `resonate` (which currently walks all session fragments) becomes O(tree-depth) instead of O(session-size).
- Test plan: fixture session with 3 topics × 3 summaries × N fragments; verify `resonate` returns the right topic's fragments in O(log N) hops.

### T10. MemGate neural retrieval trust gate

- Paper: [Beyond Similarity — MemGate — arXiv:2606.06054](https://arxiv.org/html/2606.06054v1)
- Primitive: insert a small (~9M parameter) neural gate between the cosine-ranked candidate list and the final result set in `retrieve`. Query-conditioned: blocks candidates that are semantically similar but contextually inadmissible (stale constraints, cross-session leakage that survived T2). Plug-in; no LLM modification.
- Size: **M** (~1 week, plus model-serving decision).
- Files: `src/api/tools.rs` (retrieve), `src/services/embedding.rs` (or a new `src/services/gate.rs`).
- Expected effect: complements the existing `{observed, partial, claimed, absent, contradicted}` provenance multipliers with a learned contextual admissibility signal. Effect is a lift on top of V(m) (T5) — not a replacement.
- Test plan: hold-out session pairs; verify gate blocks known-inadmissible high-cosine hits.
- Blocker: hosting/serving decision. Options: bundled ONNX runtime, remote endpoint, or defer until an in-repo model-serving substrate exists. Do not build until the hosting question has a design doc.

## Research-tier — read before committing

These are L/XL items. Each requires a scoped design doc under `docs/roadmap/rfc-*.md` before any implementation ticket is opened.

### R1. Field-theoretic continuous memory dynamics

- Paper: [Field-Theoretic Memory — arXiv:2602.21220](https://arxiv.org/html/2602.21220)
- Why not S/M: models memory as a scalar field φ(x,y,t) on a 2D semantic manifold with reaction-diffusion PDE. Retrieval scores `0.60·cosine + 0.15·|φ| + 0.15·importance + 0.10·recency`. Paper measures 9.4× processing overhead and 6.9× memory footprint at 10k fragments. Adopting whole would require replacing the basin-as-cluster model with a continuous manifold representation — affects `src/memory/attractors/`, WAL schema, and embedding pipeline simultaneously.
- What to extract without adopting whole: the diffusion term as a lightweight basin-neighbor bleed at consolidation time. That's a targeted ticket, not a wholesale switch. Design doc first.

### R2. AgeMem RL-trained memory operations

- Paper: cited in [Memory for Autonomous LLM Agents Survey — arXiv:2603.07670](https://arxiv.org/abs/2603.07670)
- Why not S/M: treats store, retrieve, update, summarize, discard as an RL policy trained through three stages (supervised warm-up, task-level RL, step-level GRPO). RL discovers non-obvious tactics like proactive summarization and selective discard.
- Blocker: session-level reward signals (task success / retrieval quality scores) that ContextNest does not currently instrument. Prereq is reward-collection infra — that's its own epic. Only revisit after cross-session-learnings epic ships instrumented outcome tracking.

### R3. Amory narrative-coherent consolidation

- Paper: [Amory — arXiv:2601.06282](https://arxiv.org/abs/2601.06282)
- Why not S/M: reconstructs conversation fragments into episodic narratives preserving contextual momentum before passing to semantic graph storage. Binding step requires either a purpose-trained small classifier or a mandatory LLM call per consolidation batch. The LLM-per-batch path violates the hot-path contract; the classifier path requires labeled chain data ContextNest does not yet accumulate.
- Revisit trigger: after T9 (TiMem tree) lands and produces labeled coherence data as a side effect.

## Gap in the 2026 literature — potential CN contribution

Two ContextNest-specific concerns have no direct 2026 arxiv coverage:

1. **Provenance-aware trust scoring grounded in tool-call receipts.** Existing hallucination-detection work (arXiv:2606.04990, arXiv:2608.29307) is output-side. ContextNest's `{observed, partial, claimed, absent, contradicted}` scheme is ahead of the published field. **Blog post or short paper is warranted.**
2. **Crash-safe incremental WAL for background consolidation workers.** Closest is arXiv:2606.12329, but it does not cover a worker crashing mid-batch and leaving basins in a partial state. ContextNest's `.bak-pre-<refactor>` recovery pattern is engineering folklore, not published. A short pattern paper would fit the systems track.

## Build order — recommended

**Sprint 1** (all S, parallelizable, ~1 week total): T1 + T2 + T3 + T4.
**Sprint 2** (S–M, T7 → T8, T5, T6 in parallel, ~2 weeks): T5 + T6 + T7 + T8.
**Sprint 3** (M, depends on Sprint 2): T9. T10 deferred until model-serving design lands.

L/XL research tier stays in `docs/roadmap/rfc-*.md` as scoped design docs before any ticket opens.

## Sources

Every citation was resolved via direct arxiv URL fetch during the survey (see accompanying research report). No paper below 2026 is included; where a 2025 paper had a 2026 follow-up, only the follow-up is cited.

- [RecMem — arXiv:2605.16045](https://arxiv.org/abs/2605.16045)
- [Learning What to Remember — arXiv:2606.12945](https://arxiv.org/html/2606.12945)
- [HiGram — arXiv:2608.05095](https://arxiv.org/abs/2608.05095)
- [TiMem — arXiv:2601.02845](https://arxiv.org/pdf/2601.02845)
- [SCM — arXiv:2604.20943](https://arxiv.org/html/2604.20943v1)
- [Chain-of-Memory — arXiv:2601.14287](https://arxiv.org/abs/2601.14287)
- [MemGate — arXiv:2606.06054](https://arxiv.org/html/2606.06054v1)
- [Selective Forgetting — arXiv:2608.28978](https://arxiv.org/html/2608.28978)
- [PROJECTMEM — arXiv:2606.12329](https://arxiv.org/html/2606.12329v1)
- [MemClaw — arXiv:2606.24535](https://arxiv.org/html/2606.24535v1)
- [Field-Theoretic Memory — arXiv:2602.21220](https://arxiv.org/html/2602.21220)
- [Amory — arXiv:2601.06282](https://arxiv.org/abs/2601.06282)
- [Memory for Autonomous LLM Agents Survey — arXiv:2603.07670](https://arxiv.org/abs/2603.07670)
