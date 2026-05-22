# Architecture — what actually happens at runtime

This document complements `docs/architecture.md` (which describes the
*conceptual* design) by recording, in plain language, what every
runtime code path actually does. Future contributors should be able
to grep the repo against this doc and find every claim backed by a
caller.

## TL;DR

ContextNest is a **fast local vector store with metadata filter**
that *progressively* turns into a **neural-field attractor substrate**
in the background. Live ingest is just sidecar writes — basins,
connection-network nodes, and decay never affect the hot path. A
background worker lazily replays each fragment through the
canonical attractor pipeline, populating the substrate over time.
Retrieve consults the populated substrate for scoring boosts.

This split exists because the canonical pipeline (`process_memories`)
makes an embedder round-trip per fragment, which at 25k+ fragments
turns a WAL replay or a Claude-Code-hook ingest into hours of
latency. We can't pay that on the hot path. So we don't.

## Lifecycle of one cc_hooks fragment

```
Claude Code Stop hook
  → POST /api/v1/cc/hooks
  → ServicesSink::store
      ├─ fragment_texts.insert(id, text)             [sync]
      ├─ fragment_metadata.insert(id, meta)          [sync]
      ├─ session_index.add(session_id, id)           [sync]
      ├─ consolidation_queue.enqueue(id)             [sync, dedup]
      └─ wal.append(WalRecord::Store)                [best-effort]
  ← 204 No Content                                   [< 5ms typically]

... time passes (worker tick interval, default 500ms) ...

Consolidation worker
  → drain_batch(32 ids)
  → for each id, in parallel up to 4:
      ├─ generate_embedding(text)                    [~250ms on DeepInfra]
      ├─ build MemoryFragment with that embedding
      ├─ process_memories(req) with conservative options:
      │     enable_attractor_creation = true
      │     enable_connections = true
      │     enable_reconstruction = false
      │     enable_gap_filling = false
      │   Inside process_memories, for our single-fragment batch:
      │     Step 1   → basin_manager.create_basin(content, importance, ...)
      │                + add_fragment_to_basin(basin_id, fragment.id)
      │     Step 1.5 → connection_network.add_node(memory_node)
      │                which internally runs create_connections_for_node
      │                to auto-link similar existing nodes by similarity
      │     Step 1.6 → reconstruction_protocol.fragment_store.insert
      │     Step 2   → SKIPPED (enable_reconstruction = false)
      │     Step 3   → SKIPPED (single-fragment guard, fragments.len() > 1 false)
      └─ fragment_metadata[id]["_cn_consolidated"] = true
```

After consolidation, the fragment exists in **four** places:

| Sidecar / store | Purpose | Populated when |
|---|---|---|
| `fragment_texts` | source text | ingest |
| `fragment_metadata` | structured meta + `_cn_consolidated` flag | ingest + consolidation |
| `session_index` | session affinity | ingest |
| `reconstruction_protocol.fragment_store` | canonical `MemoryFragment` (embedding + importance) | consolidation |
| `basin_manager.basins[basin_id].associated_fragments` | basin membership | consolidation |
| `connection_network.graph.nodes[id]` | graph node + auto-formed edges | consolidation |

## Lifecycle of one retrieve query

```
POST /api/v1/tools/retrieve { query, top_k, session_id, metadata_filter? }
  → embed query
  → active_ids = session_index.list_active(session_id)
  → candidate_ids = active_ids filtered by metadata_filter
  → for each candidate, hydrate canonical MemoryFragment via
        attractor_manager.get_fragment(id)
    (sidecar fallback for ids the canonical store didn't have yet —
     e.g. fragment ingested 0.5s ago, worker hasn't reached it; we
     return content from sidecars at similarity = 0)
  → for each canonical hit:
        base_similarity = cosine(query_emb, fragment.embedding)
        decay = decay_multiplier(metadata)  ← Phase 2
        final_similarity = base × decay
  → sort by similarity desc
  ─── Phase 4 basin expansion ───
  → find which basin contains scored[0].id
  → for each basin sibling not already in scored AND in candidate_ids:
        append at similarity = top_sim × 0.7 (CONTEXTNEST_RETRIEVE_BASIN_BOOST)
  ─── Phase 5 connection expansion ───
  → list_neighbors(scored[0].id) → Vec<(neighbor_id, edge_weight)>
  → for each neighbor passing min_weight AND not already in scored
        AND in candidate_ids:
        append at similarity = top_sim × edge_weight × 0.5
        (CONTEXTNEST_RETRIEVE_CONNECTION_BOOST)
  → resort, truncate to top_k
  → bump last_accessed on each returned id           ← Phase 2 recency
  → update connection_log co-occurrence pairs        (separate from
                                                      ConnectionNetwork —
                                                      drives /field viz)
  ─── Phase 6 auto-reconstruction (if chain query) ───
  → if is_chain_query(query) && single_session && auto enabled:
        compute_reconstruction(query_emb, session_id, depth=5)
        → attached to response.reconstruction
  ← { hits: [...], reconstruction? }
```

## Knobs

All optional, env-overridable, sensible defaults:

| Env var | Default | Phase | Purpose |
|---|---|---|---|
| `CONTEXTNEST_DECAY_HALF_LIFE_DAYS` | 60 | 2 | Half-life for the age-based decay multiplier |
| `CONTEXTNEST_CONSOLIDATION_INTERVAL_MS` | 500 | 1 | Worker tick interval |
| `CONTEXTNEST_CONSOLIDATION_CONCURRENCY` | 4 | 1 | In-flight embedder calls |
| `CONTEXTNEST_CONSOLIDATION_BATCH_SIZE` | 32 | 1 | Max ids per tick |
| `CONTEXTNEST_CONSOLIDATION_ENABLED` | true | 1 | Master kill switch |
| `CONTEXTNEST_RETRIEVE_BASIN_BOOST` | 0.7 | 4 | Outer multiplier on basin expansion |
| `CONTEXTNEST_RETRIEVE_BASIN_MAX_EXPANSION` | 20 | 4 | Cap on basin siblings appended |
| `CONTEXTNEST_RETRIEVE_CONNECTION_BOOST` | 0.5 | 5 | Outer multiplier on graph expansion |
| `CONTEXTNEST_RETRIEVE_CONNECTION_MAX_EXPANSION` | 10 | 5 | Cap on graph neighbors appended |
| `CONTEXTNEST_RETRIEVE_CONNECTION_MIN_WEIGHT` | 0.1 | 5 | Floor on edge weight |
| `CONTEXTNEST_RETRIEVE_AUTO_RECONSTRUCT` | true | 6 | Auto-attach reconstruction on chain queries |
| `CONTEXTNEST_RETRIEVE_AUTO_RECONSTRUCT_DEPTH` | 5 | 6 | top-N fragments stitched in the chain |

## Observability

Two endpoints expose the substrate's internal state:

- `GET /api/v1/substrate/consolidation` — worker progress
  (queue, lag, success/fail totals, last-batch time).
- `GET /api/v1/substrate/health` — aggregate snapshot
  (fragments, basins, connections, decay). Operators look here to
  verify the attractor pipeline is actually running. If
  `basins.count == 0` on a populated substrate, the worker has
  failed to start or the embedder is down. If
  `connections.edges == 0` after lots of consolidations, the
  similarity-driven auto-connection threshold inside
  `ConnectionNetwork::add_node` may be set too high.

## Verification recipe — every tagline claim is grep-auditable

```bash
# "consolidates basins" — Phase 1
grep -rn "consolidation_queue.enqueue\|process_memories" src/api/ src/ingest/

# "decay" — Phase 2
grep -rn "decay_multiplier\|last_accessed" src/api/tools.rs

# "basin-aware retrieve" — Phase 4
grep -rn "basin_aware_expand\|list_basin_snapshots" src/api/

# "connection network" — Phase 5
grep -rn "connection_aware_expand\|list_neighbors\|neighbors_of" src/api/ src/memory/

# "auto reconstruction" — Phase 6
grep -rn "compute_reconstruction\|is_chain_query" src/api/

# "substrate health" — Phase 7
grep -rn "get_substrate_health\|SubstrateHealth" src/api/
```

Each grep should return at least one hit from outside
`src/memory/attractors/`. If any returns zero, the corresponding
claim has lost its caller and the README needs to be revised.

## What this isn't

- It's not a multi-machine distributed memory yet — everything
  lives in one process.
- It's not a long-horizon learning system; basins don't survive
  embedder model swaps, and there's no re-consolidation worker for
  that case.
- The reconstruction proxy is not the full canonical chain
  (`docs/00_COURSE/05_memory_systems/04_reconstructive_memory.md`).
  Steps 1, 1.5, 1.6 of `process_memories` run; Step 2 (full
  reconstruction) is invoked only on demand via Phase 6, and
  gap-filling is still 0.
- Sidecar-only hits (fragments visible to ingest but not yet
  consolidated) return at similarity 0. They sort to the bottom of
  the result set and never anchor a Phase 4 / 5 expansion.

If you find a tagline claim that fails the grep recipe, please open
an issue — the README has to track the runtime, not the other way
around.
