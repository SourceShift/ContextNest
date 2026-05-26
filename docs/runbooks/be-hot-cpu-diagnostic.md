# Runbook — diagnosing ContextNest BE hot-CPU loop

**Last incident:** 2026-05-26 — PID 93445 stuck at 99% CPU for ~60 min, with
global `POST /api/v1/tools/retrieve` silently returning `{"hits":[]}`. Inbox
endpoint and explicit-session retrieve both kept working. Restart fixed it.
Bug class: **state drift between `session_index.active` and
`fragment_metadata` / `fragment_texts` sidecars in long-running BE.**

## Symptoms (recognise the bug)

1. `POST /api/v1/tools/retrieve` with `{"query":"…"}` (no `session_id`,
   no `session_ids`) returns `{"hits":[]}` even though the substrate
   clearly has data.
2. Same call with `session_ids: [<any one session>]` returns hits
   normally.
3. `GET /api/v1/inbox` still returns thousands of items.
4. `ps aux | grep contextnest` shows the BE at 80–99 % CPU sustained.
5. BE process uptime is hours, not minutes.

## Recovery (restore service in <30 s)

Kill the BE and restart it. The WAL replays all state coherently and the
bug goes away.

```bash
pkill -f 'contextnest serve'
make -C /Volumes/docker-ssd/Migration/Development/ContextNest cn-serve-dev
# (or `cargo run --profile fast --bin contextnest -- serve --bind 127.0.0.1:28080`)
```

## Diagnosis (capture data before restarting)

If you can spare 10–30 seconds before recovering, capture a CPU profile
so we can finally find the runaway loop:

```bash
# One-shot — samples NOW, regardless of CPU
./scripts/diagnose-hot-be.sh

# Watch mode — waits until CPU > 50 % for 3 consecutive seconds, then samples
./scripts/diagnose-hot-be.sh --watch
```

Output lands in `/tmp/cn-be-hot-<ISO>.sample`. Script also prints the top
non-idle frames inline.

## Suspect list (from observation, ranked by likelihood)

The 2026-05-26 baseline sample (on a *healthy* BE) showed all three of these
warm in the background. The bug is likely one of them going into a runaway:

| Suspect | Symbol to grep for in the sample | What "runaway" looks like |
|---|---|---|
| **Consolidation worker** | `services::consolidation::consolidate_one` | Same `consolidate_one` frame across many threads / many seconds |
| **ConnectionNetwork edge updates** | `connection_network::ConnectionNetwork::create_connection` or `MemoryGraph::update_metrics` | Long stacks of `add_node` / `create_connection` / `update_metrics` |
| **Attractor manager process_memories** | `attractors::memory_attractor_manager::MemoryAttractorManager::process_memories` | Long stacks of `process_memories` with no progress out |

```bash
# Confirm which one is the runaway:
grep -c consolidate_one /tmp/cn-be-hot-*.sample
grep -c create_connection /tmp/cn-be-hot-*.sample
grep -c process_memories /tmp/cn-be-hot-*.sample
```

Whichever count is dramatically higher than the others is the culprit.

## After capturing a profile — what's needed for the real fix

1. The `.sample` file (preserve it; it has the full stack trace).
2. The output of `curl -s http://localhost:28080/api/v1/sessions | jq '.sessions | length'` (how many sessions had accumulated).
3. The output of `curl -s http://localhost:28080/api/v1/inbox | jq '.items | length'` (inbox cardinality).
4. BE uptime: `ps -o etime= -p $(pgrep -f 'contextnest serve')`.
5. Approximate query rate over the last hour (hits per minute against `/api/v1/tools/retrieve`).

With (1)+(2)+(3), the runaway loop can be reproduced in a unit test and
fixed deterministically.

## Defensive additions (not yet implemented — track as follow-up issues)

- `/api/v1/debug/state-coherence` endpoint returning
  `(session_index_active_count, fragment_metadata_count, fragment_texts_count)`
  and `divergent: bool` when they differ by >10 %.
- Periodic self-check (every 5 min) that logs an `tracing::error!` when
  divergence is detected — turns silent drift into a visible alert.
- A "watchdog" task on the consolidation worker that aborts + retries if
  a single `consolidate_one` invocation runs > 60 s.

## Cross-references

- Initial diagnosis chat: 2026-05-26 session (PR #59 era)
- Suspect site #1: `src/services/consolidation.rs` (consolidate_one)
- Suspect site #2: `src/memory/attractors/connection_network.rs` (create_connection / update_metrics)
- Suspect site #3: `src/memory/attractors/memory_attractor_manager.rs` (process_memories)
- Insforge memory: `macos_sample_diagnose_stuck_bash` — the `sample <pid> N` pattern this script wraps
