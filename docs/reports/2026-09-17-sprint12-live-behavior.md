# Sprint 1 + T5 live-workload behavior on the researcher substrate

Date: 2026-09-17. Substrate: ~195k fragments accumulated in `~/.contextnest/wal.jsonl` over ~4 months.

The v0.1.0 CPU report (`docs/reports/2026-09-08-cn-serve-cpu.md`) measured the pre-repair binary's consolidation hotspot. This follow-up records what the v0.1.1+ substrate does under the same real workload with the T1 recurrence gate turned on.

## What was measured

- Binary: `target/release/contextnest` built from `main` at commit 08ed584 (v0.1.3).
- Environment: `CONTEXTNEST_CONSOLIDATION_RECURRENCE_MIN_COUNT=5`, `CONTEXTNEST_RETRIEVE_FREQUENCY_WEIGHT=0.15`.
- Uptime at measurement start: ~11 minutes.
- Substrate size at measurement: 195,794 fragments, 112,148 basins, 4,364,529 edges.
- Sample: 12 × 5-second CPU snapshots via `ps -p <PID> -o pcpu=`.

## Results

| Metric | Value |
|---|---|
| CPU samples (12 × 5s) | 0.0, 56.2, 20.4, 0.0, 0.0, 54.9, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0 |
| Median CPU | 0.0% of one core |
| Mean CPU | ~11% of one core |
| Peak CPU | 56.2% |
| Idle samples | 7 of 12 |
| Consolidated since restart (Done) | 125 |
| Deferred to subconscious | 732 |
| Failures | 0 |
| Queue depth at end | 16 |
| Lag | 20 |
| RSS | 2.37 GB |

## Observation

The workload pattern remains bursty — background consolidation is not continuously pegged. What changed post-Sprint-1 is the ratio of work performed to work deferred: **732 / (732 + 125) ≈ 85%** of new fragments were deferred to the subconscious store rather than consolidated. Those fragments still have sidecar text and are retrievable at similarity 0 via the sidecar fallback; they never invoked the embedder round-trip or the basin/graph pipeline. The recurrence gate is doing exactly what the RecMem paper (arXiv:2605.16045) predicted: it filters out sparse / solo fragments and only consolidates patterns that recur.

The CPU peaks (54.9%, 56.2%) are individual consolidations that passed the gate — those look identical to the pre-repair binary because the per-fragment pipeline itself has not changed. What changed is the population of consolidations: only ~15% of the arrival rate makes it through.

## What this does NOT establish

- A production CPU-percentage guarantee. Hard CPU caps require OS-level process controls (cgroups, nice, ionice); the substrate's controls are workload-shaping, not resource-cap.
- A latency improvement for `retrieve`. The retrieve path is unchanged aside from the T5 frequency-of-use multiplier, which adds a fixed-cost log2 computation per hit — negligible.
- A quality improvement in retrieval results. The T1 gate defers basin + graph work but preserves sidecar retrievability. Deferred fragments score at similarity 0 in retrieve, so they show up only when nothing better matches. Whether this is net-positive depends on the caller's tolerance for a longer tail of near-empty responses on sparse sessions.

## Follow-up questions

1. What is the reconsideration rate? When a peer arrives that clears a deferred sibling's marker, does the sibling generally pass the gate on retry or stay stuck? Instrument this: add a `reconsidered_and_passed` counter to `ConsolidationMetrics`.
2. What happens to retrieval quality on sessions where >85% of fragments are deferred? A/B test on a hold-out session with gate on vs off, same query set.
3. Does the peer-cosine floor (`CONNECTION_SIMILARITY_THRESHOLD=0.7`) need to be different for the T1 gate than for connection-network attachment? Currently reused — one knob, two purposes.

## Diagnostic pointers

- `GET /api/v1/substrate/consolidation` — includes `deferred_subconscious` field since v0.1.2.
- `GET /api/v1/substrate/replay?dry_run=true` — since v0.1.1, reports WAL record counts + unique fragment ids. Diagnostic escape hatch when the canonical checkpoint desyncs.
- `GET /api/v1/substrate/health` — unchanged shape, still the single source for basin/edge counts.
