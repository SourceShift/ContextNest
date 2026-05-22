# Epic — Make the Neural-Field Substrate Real

**Status:** Proposal. ~2-3 weeks engineering across 7 PRs. Each phase
independently shippable; no big-bang merge.

**Owner:** TBA.

**Last updated:** 2026-05-21.

## The honest problem

ContextNest's tagline — *"neural-field attractor memory substrate"* —
describes the codebase's *aspiration*, not its *runtime behavior*.

A call-graph audit (see appendix) confirms that four central modules
are either dormant or unreachable from any user-facing HTTP path:

| Module | Lines of code | Reachable today? |
|---|---|---|
| `AttractorBasinManager` (basin formation, attraction force, energy, merge) | ~500 LOC | Populated only via `/api/v1/tools/store` HTTP path. Never queried at retrieve time. |
| `AdaptiveDecaySystem` (importance decay) | ~400 LOC | No HTTP / scheduled-job invokes it. Decay never influences `/retrieve` results. |
| `ConnectionNetwork` (learned graph, retrieve_memories with QueryType::Hybrid) | ~700 LOC | Populated only via `/store`. Its own retrieve method has zero callers from `src/api/`. |
| `ReconstructionProtocol` (gap-filling, canonical chain) | ~600 LOC | Exposed via `/api/v1/tools/reconstruct` endpoint that almost nobody calls. |

For data arriving from Claude Code hooks (the default ingest path)
the situation is worse: `ServicesSink::store` **explicitly skips**
`process_memories`, so basins and the connection graph never form for
cc_hooks-sourced fragments — which is 100% of live ingest.

For a user with 25k+ fragments from cc_hooks + WAL replay:
- Basin count: **0**
- ConnectionNetwork node count: **0**
- Decay applied to retrieve scoring: **never**

ContextNest *today* is a **fast local vector store with metadata
filter, co-retrieval edges (added in PR #24), and WAL durability**.
That's a real product. But it's a smaller claim than the README
makes, and shipping under an inflated tagline erodes trust the
moment anyone audits the call graph.

## Goal

Close the gap. Either every claim in the tagline has an active code
path, or we drop the claim. This epic commits to the first option.

## Architectural constraint

We discovered the hard way that synchronous attractor processing per
fragment is incompatible with our live-ingest performance budget:

- One LLM call per `process_memories` × thousands of cc_hooks
  fragments = hours of latency.
- This is exactly why `ServicesSink::store` skips `process_memories`
  (intentional design, not oversight).
- WAL replay has the same constraint — replaying 12k+ records
  through `process_memories` took infinite time before we routed
  around it.

So we **cannot** "just wire `process_memories` into ServicesSink and
restore_sidecars_bulk." The right architecture is **background
consolidation**: a worker that lazily processes fragments through
the attractor pipeline without blocking ingest.

## Out of scope

- Replacing the embedding model.
- Changing the WAL format.
- Multi-tenant / multi-user namespacing (separate epic).
- Cross-machine sync (separate epic).

## Phases

Each phase is a self-contained PR. Order matters — later phases
build on the data structures earlier phases populate. But every
phase delivers user-visible value on its own.

---

### Phase 1 — Background consolidation worker (foundation)

**Goal:** Lazily process every fragment through basin assignment
and ConnectionNetwork insertion without blocking ingest.

**Build:**

- A new `ConsolidationWorker` task spawned at server startup, kept
  alive for the server's lifetime.
- Persistent watermark `consolidated_up_to: fragment_id` (stored in
  WAL sidecar or simple JSON file) so restarts resume.
- Per-fragment work, in order:
  1. Read text + metadata from sidecars.
  2. Generate embedding (uses existing cache; sidecar fragments
     already have embeddings re-embedded on demand via the
     fragments endpoint).
  3. Call `attractor_manager.basin_manager.find_nearest_basin(emb)`
     → assign to existing basin if within threshold, else create new.
  4. Call `attractor_manager.connection_network.add_node(...)`.
  5. Update watermark.
- Bounded concurrency (4-8 in-flight) to avoid LLM rate-limit
  spikes when the embedder is network-backed.
- Backpressure: if the worker is more than N=1000 fragments behind,
  log warn so operators see lag.

**New endpoints:**

- `GET /api/v1/substrate/consolidation` → returns
  `{ total_fragments, consolidated_count, lag, current_basin_count }`

**Acceptance criteria:**

- [ ] After server restart with N=25k WAL records, worker
      consolidates 100% within 30 minutes (or shorter on local
      embedder).
- [ ] Basin count climbs as work progresses; visible at the new
      observability endpoint.
- [ ] ServicesSink (live cc_hooks) writes a fragment id into the
      worker's queue immediately, but does not block on its
      processing.
- [ ] Restart mid-consolidation → resume from watermark, no
      duplicate work.

**Files touched (estimate):**
`src/services/consolidation.rs` (new), `src/services/mod.rs`,
`src/api/tools.rs` (queue enqueue on store), `src/ingest/claude_code/sink.rs`
(queue enqueue), `src/bin/contextnest.rs` (spawn worker at startup).

---

### Phase 2 — Decay applied at retrieve time

**Goal:** Older fragments score lower at retrieve. The "your memory
forgets stale stuff" promise becomes real.

**Build:**

- In `src/api/tools.rs::retrieve`, after computing cosine
  similarity, apply a decay multiplier:
  ```
  decayed_similarity = base_similarity × exp(-age_days / half_life)
  ```
  where `age_days = (now - metadata.ts) / 1 day` and
  `half_life` is a config knob (default 60d).
- Each successful retrieve hit also bumps a per-fragment
  `last_accessed` timestamp in fragment_metadata. Future retrieves
  of "recently-recalled" fragments get a recency boost.
- Decay does NOT remove fragments — it just lowers their score.
  Hard-deletion remains a separate `/discard` operation.

**Config:**

- `CONTEXTNEST_DECAY_HALF_LIFE_DAYS=60` (env)
- Substrate page exposes the current value + a histogram of
  age-of-fragments-currently-served.

**Acceptance criteria:**

- [ ] A 90-day-old fragment with same content as a 1-day-old
      fragment scores ~70% of the new one.
- [ ] Retrieving a fragment bumps its `last_accessed`; next
      retrieve gives it a boost.
- [ ] Test: store fragment with `ts = 100 days ago`, retrieve
      with query that matches → similarity in response is
      explicitly lower than for an identical fragment with
      `ts = today`.

**Files touched (estimate):**
`src/api/tools.rs`, `src/config.rs`, `tests/retrieve_decay_test.rs` (new).

---

### Phase 3 — Real basins surface via `/api/v1/field/basins`

**Goal:** The `/field` viz stops showing project-derived basins and
starts showing actual attractor basins (when consolidation has
populated them).

**Build:**

- `/api/v1/field/basins` queries `attractor_manager.basin_manager`
  for live basins.
- Each basin response carries:
  - `id` (basin uuid)
  - `centroid` (already in 256-d; the field viz uses PCA to project)
  - `mass` (member count)
  - `member_fragment_ids` (capped at 500)
  - `dominant_kind` (kind histogram)
- When canonical basin count is zero (e.g., consolidation hasn't
  caught up), fall back to project-derived basins (current behavior)
  and label the response `source: "project"` so the frontend can
  show "consolidation in progress…".

**Acceptance criteria:**

- [ ] After Phase 1 has consolidated some fragments, the
      `/field` viz shows basin halos whose positions are the
      learned centroids, not project means.
- [ ] Basins that merge (Phase 1's merge_with logic) reflect that
      in the next field refresh.
- [ ] Sidebar count for the Phases nav shows real basin count.

**Files touched (estimate):**
`src/api/field.rs`, `web/src/routes/field.tsx` (handle new fields).

---

### Phase 4 — Basin-aware retrieval boost

**Goal:** When a query strongly matches fragment X, also surface X's
basin-siblings (cluster reinforcement).

**Build:**

- In `/api/v1/tools/retrieve`, after computing top-K by cosine:
  1. For the top hit, look up its basin via `attractor_manager`.
  2. Pull all OTHER fragments in that basin.
  3. Score them as `basin_similarity = top_hit_similarity × 0.7`
     (or configurable factor).
  4. Merge into the result set, re-sort, truncate to `top_k`.
- This is the "you've worked on this topic before" expansion — past
  fragments that don't match the query string but are
  semantically siblings of the top hit.

**Acceptance criteria:**

- [ ] Query that matches a single fragment in a 20-member basin
      now returns multiple basin members in the top-K.
- [ ] Query that matches across multiple basins still surfaces
      diverse hits, not just one cluster.
- [ ] Variance ratio in `/field`'s PCA improves (fewer outliers
      because basins have stronger cohesion in returned sets).

**Files touched (estimate):**
`src/api/tools.rs`, `tests/retrieve_basin_expansion_test.rs` (new).

---

### Phase 5 — ConnectionNetwork retrieval expansion

**Goal:** Top hits' learned graph neighbors get surfaced (1-hop
expansion). This is co-retrieval resonance applied at retrieve time.

**Build:**

- In retrieve, after Phase 4's basin expansion, query
  `connection_network.retrieve_memories` with the top hit as
  seed. Get the strongest 1-hop neighbors.
- Score them as `connection_similarity = top_hit_similarity × edge_weight`.
- Merge and re-sort as in Phase 4.

**Acceptance criteria:**

- [ ] Hover-neighbor highlighting in `/field` (the existing
      cosine-nearest-5 interaction) gains a complementary
      "connection-nearest" mode showing learned graph adjacency
      instead of pure embedding similarity.
- [ ] /retrieve returns 1-hop neighbors of the top hit when they
      exceed an edge-weight threshold.

**Files touched (estimate):**
`src/api/tools.rs`, `src/memory/attractors/memory_attractor_manager.rs`
(may need a new helper for "neighbors of fragment X with min weight").

---

### Phase 6 — Reconstruction as automatic context

**Goal:** The ReconstructionProtocol stops being a manual endpoint
and becomes part of retrieve when the query asks for a chain.

**Build:**

- Pattern-detect when a `/retrieve` query looks like a "chain"
  question: "context of X", "what led to X", "how did I arrive at
  X", "history of X". For now, a simple keyword filter is fine.
- When detected, ALSO invoke `reconstruction_protocol.reconstruct_memory`
  with the embedded query and return its result alongside the
  regular hits, under a new top-level `reconstruction` field.
- MCP tool `cn_reconstruct(query)` exposes the same path for
  Claude to call explicitly.

**Acceptance criteria:**

- [ ] Query "what was the context around the auth decision" returns
      both regular hits AND a chain reconstruction in the response.
- [ ] Chain reconstruction visibly orders the contributing
      fragments by their position in the inferred sequence.

**Files touched (estimate):**
`src/api/tools.rs`, `src/memory/attractors/reconstruction_protocol.rs`
(may need a minor public wrapper).

---

### Phase 7 — Substrate health + honest positioning

**Goal:** Make the substrate's actual behavior visible, and update
documentation to match.

**Build:**

- New `/api/v1/substrate/health` endpoint returns:
  ```json
  {
    "fragments": { "total": N, "consolidated": M, "lag": N-M },
    "basins": { "count": B, "avg_mass": ... },
    "connections": { "edges": E, "avg_degree": ... },
    "decay": { "half_life_days": 60, "median_fragment_age_days": 14 }
  }
  ```
- Dashboard's `/substrate` page renders this as a status board
  alongside the existing kind histogram.
- README's tagline and architecture.md updated to describe the
  ACTUAL runtime behavior. Drop "neural-field attractor
  consolidation" claims that have no code path; keep claims that
  pass a grep audit.
- Add a `docs/architecture-honest.md` style doc explaining the
  lazy-consolidation model so future contributors don't
  re-introduce the same gap.

**Acceptance criteria:**

- [ ] `make cn-curl-health` (a new Makefile target) returns the
      full substrate health snapshot.
- [ ] README's tagline survives a grep audit — every claim is
      reachable from an HTTP entry point.
- [ ] Dashboard substrate page shows live basin/connection/decay
      stats so operators can see the substrate consolidating.

**Files touched (estimate):**
`src/api/health.rs` (new), `src/api/simple.rs`, `web/src/routes/substrate.tsx`,
`README.md`, `docs/architecture.md`.

---

## Sequencing notes

- **Phase 1 unblocks Phases 3, 4, 5, 6.** They all read data the
  consolidation worker populates.
- **Phase 2 is independent and shippable any time.** Recommended to
  ship 2 first as the smallest-change demo of "real substrate
  behavior" before the bigger consolidation work.
- **Phase 7 is best shipped last** so the README catches up to
  reality.

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Basin formation produces too many small basins (noisy) | Med | Phase 1's merge_with logic kicks in at threshold; if basins still fragment, tune merge threshold via config |
| Consolidation lag grows unbounded under heavy ingest | Low | Backpressure log at 1000-behind, surface lag in /substrate/health; operator can pause ingest temporarily |
| Decay penalty too aggressive → fresh-but-stale-feeling experience | Med | Configurable half-life; expose in dashboard so users can tune |
| Reconstruction quality is poor (LLM-free reconstruction is rough) | Med | Phase 6 is the riskiest; ship behind a feature flag, allow disable |
| Migration: existing substrates have no consolidation watermark | Low | First start with no watermark → consolidate from 0; tracked as one-time cost |

## What success looks like

A user opens `/substrate`. They see:

```
Fragments      25,079 (consolidated: 25,079 · lag: 0)
Basins         147 active · avg mass 170
Connections    8,431 edges · avg degree 34
Decay          half-life 60d · median fragment age 14d
```

They open `/field` and see real basin centroids, not project averages.
They run a `/retrieve` query and the top-K includes fragments they
didn't word-match — basin siblings and graph neighbors brought in by
the attractor pipeline. Older fragments are visibly de-ranked
relative to fresh ones with the same content.

The README's tagline survives the grep audit.

## Appendix — verification methodology

To verify each phase delivers what it promises, grep for callers of
the dormant APIs from outside `src/memory/attractors/`:

```bash
# Should be > 0 after Phase 4:
grep -rn "find_nearest_basin\|find_attractor_basins" src/api/ src/services/

# Should be > 0 after Phase 5:
grep -rn "connection_network.retrieve_memories" src/api/ src/services/

# Should be > 0 after Phase 2:
grep -rn "decay_factor\|apply_decay\|last_accessed" src/api/tools.rs

# Should be > 0 after Phase 6:
grep -rn "reconstruction_protocol\." src/api/tools.rs
```

When all four greps come up with hits, the substrate has earned its
name.
