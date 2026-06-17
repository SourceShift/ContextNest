# Epic — Cross-fleet agent coordination (advisory lease plane)

**Status:** Proposal + Phase 1 MVP in progress. ~6–8 days across 5 phases.
Each phase independently shippable; no big-bang merge.

**Owner:** TBA.

**Last updated:** 2026-06-17.

## The problem

Multiple **master** Claude sessions each spawn **mini-ork** orchestration
teams. Each mini-ork runs many agents — heterogeneous (Claude, Codex),
different LLMs — and those agents edit the **same codebase**. The hazard
is the classic concurrency bug, lifted into the agent world:

```
  time →
  agent A (high prio):   read foo.rs ───── edit foo.rs ───── write ✔
  agent B (low prio):         read foo.rs ──── edit ──── write ✘ (clobbers A)
                                   └─ B built on a state A was about to change
```

This is **lost update / stale-read-then-write**. B needs to *wait* until
A's higher-priority edit to the overlapping scope is done — otherwise A's
work is silently overwritten, or B wastes a turn building on soon-stale
code. We need a robust plane where any agent (master or sub-agent) can ask
**"is it my turn to touch path P?"** and get a *priority-correct* answer,
and re-poll at the right time rather than busy-spinning.

## Why the agents can't solve this themselves

`arXiv:2606.07845` ("GRPO Does Not Close the Multi-Agent Coordination
Gap", cs.MA, Jun 2026) measured exactly this. They put LLMs on the
**dining philosophers problem** — the textbook shared-resource contention
model (philosophers = agents, forks = shared files). Findings:

- Frontier models reach only **0.45–0.87 mean reward** at 5 agents; a
  weaker model collapses to **0.13**.
- Fine-tuning the gap away **fails**: GRPO gives Welch's t-test
  **p = 0.66** (no significant improvement).
- Degenerate failure mode: some models "coordinate" by **doing nothing**
  (zero work) to avoid deadlock.

**Conclusion:** LLMs cannot reliably self-negotiate access to a shared
resource, and you cannot train it into them. Coordination must be
**external and authoritative** — an arbiter the agents query, not a
politeness protocol they are trusted to honour. In the dining-philosophers
literature the deadlock-free fix is precisely a **central arbitrator**
("the waiter") or a resource hierarchy. That arbiter is this epic.

## Why ContextNest is the right host

`arXiv:2606.14445` ("tap: A File-Based Protocol for Heterogeneous LLM
Agent Collaboration", cs.SE, Jun 2026) is the closest prior art — it lets
**Claude + Codex collaborate on one shared codebase** from separate
runtimes. But `tap` is a **message-passing protocol** (how agents *talk*).
It is not a concurrency-control layer (who may *write what, when, by what
priority*). That gap is what we fill.

ContextNest is already the one substrate every agent shares (single
localhost endpoint, shared WAL across worktrees). Three existing
primitives line up:

```
  EXISTING (today)                    →  ROLE IN COORDINATION
  ────────────────────────────────────────────────────────────────
  files_touched index +               →  "who TOUCHED what" (past).
  GET /api/v1/sessions/by-file            Leases are the forward version:
                                          "who's ABOUT to touch what".
  POST /api/v1/cc/pretool gate        →  the ENFORCEMENT hook. Every agent
  (shipped, PR #155)                      already POSTs here before a tool runs.
  active-state / U_t buckets +        →  leases are another active-state
  ScheduleWakeup                          bucket; backoff timing reuses wakeup.
```

Because the substrate is a shared central coordinator, we get the simple,
correct **central lock-manager** model (the Chubby/ZooKeeper role) without
paying the distributed-consensus tax of Ricart–Agrawala / Maekawa /
token-ring mutual exclusion.

## The core abstraction: a scoped, priority-aware lease

```
  ┌─ master-1 ─┐   ┌─ master-2 ─┐     each agent, before Edit/Write:
  │ ork│ork    │   │ ork        │
  │ a a a      │   │ a a        │   1. POST /coord/lease {paths, mode, prio, ttl}
  └──────┬─────┘   └─────┬──────┘   2. ← granted → proceed
         │               │             ← queued  → {holder, reason, eta} → back off
         └───────┬───────┘          3. DELETE /coord/lease/{id} when done
                 ▼
        ContextNest lease registry  conflict = path-set overlap AND ≥1 writer
        (single serialization pt,            (read/read is compatible)
         TTL-swept, in-memory)
```

Four mechanisms, each with a literature anchor:

1. **Leases, not locks** (Gray & Cheriton 1989). Every grant has a TTL; a
   crashed/abandoned agent's lease *expires* rather than deadlocking the
   fleet. Long edits `renew` (heartbeat). This is the single most
   important property — a lock that self-heals on holder failure.
2. **Conflict = write-set intersection** (optimistic concurrency control;
   readers-writers, Courtois 1971). Two leases conflict iff their
   normalized path/glob sets overlap **and** at least one is `write`.
   Read/read is compatible.
3. **Priority queue + priority inheritance** (Sha/Rajkumar/Lehoczky
   1990). A lower-or-equal-priority requester *waits*; a higher-priority
   requester jumps the queue but **non-preemptively** (never interrupt a
   half-applied multi-file edit). While a high-prio waiter blocks on a
   low-prio holder, the holder *inherits* the waiter's priority so a
   medium-prio third agent can't starve it — the textbook fix for
   priority inversion.
4. **Deadlock = wait-for cycle** (wound-wait / wait-die, Rosenkrantz
   1978). Because the registry is central, a cycle is a cheap local graph
   walk; break it by aborting the lower-priority requester.

"Query at a proper time" = the lease response carries a suggested re-poll
time (= holder's TTL-remaining), so the waiter sleeps that long via
`ScheduleWakeup` and re-queries. Lease + adaptive backoff, no busy-spin.

---

## Phased plan

| Phase | Slice | Ships | Effort |
|---|---|---|---|
| 1 | [Advisory lease registry (MVP)](#phase-1--advisory-lease-registry-mvp) | acquire/release/renew/list, overlap+write conflict, priority queue, lazy TTL, gate consult | ~2 days |
| 2 | [Priority inheritance + deadlock detection](#phase-2--priority-inheritance--deadlock-detection) | wait-for graph, cycle break, inheritance | ~1.5 days |
| 3 | [Deny-mode enforcement](#phase-3--deny-mode-enforcement) | opt-in hard block via PreToolUse `deny` | ~1 day |
| 4 | [Persistence + observability](#phase-4--persistence--observability) | contention audit log, dashboard panel, metrics | ~1.5 days |
| 5 | [Fine-grained scope](#phase-5--fine-grained-scope) | symbol/region leases, glob matching | ~1.5 days |

### Sequencing

Phase 1 is the critical path — everything else layers on the registry.
Phase 2 makes it *fair* (no starvation/deadlock). Phase 3 makes it
*enforceable* for scopes where advisory isn't safe enough — and
`arXiv:2606.07845`'s "do nothing to avoid deadlock" failure mode is the
evidence that advisory-only eventually needs a hard backstop. Phases 4–5
are quality/observability and can reorder by need.

---

## Phase 1 — Advisory lease registry (MVP)

**What.** An in-memory lease registry on `ContextNestServices`
(`coord_leases`), plus a `coord` sub-router:

```
POST   /api/v1/coord/lease            acquire
DELETE /api/v1/coord/lease/{id}       release
PUT    /api/v1/coord/lease/{id}/renew heartbeat (extend TTL)
GET    /api/v1/coord/leases[?path=]   inspect contention
```

`POST /coord/lease` body:
`{ agent_id, fleet_id?, paths:[...], mode:"write"|"read", priority:int,
ttl_secs?, reason? }`. Response is either:
- `{ status:"granted", lease_id, expires_at }`, or
- `{ status:"queued", blocked_by:[{lease_id, agent_id, priority, reason}],
  retry_after_secs, position }`.

**Conflict predicate.** Normalize paths; two leases conflict iff path-sets
overlap (prefix/equality for MVP) **and** at least one is `write`.
read/read never conflicts.

**Priority.** Higher integer = higher priority. A request is *granted*
when no conflicting lease is **held**; otherwise *queued*, sorted by
priority then request time. `retry_after_secs` = min TTL-remaining among
blockers.

**TTL.** Lazy expiry — every registry read first drops expired leases.
(No background worker needed for MVP; in-memory is correct because leases
are live coordination state, not durable memory — a server restart resets
the fleet's relationship anyway.)

**Gate integration.** Extend `POST /api/v1/cc/pretool`: when the request
carries a target path (`tool_input.file_path`), consult the registry; if a
conflicting higher-or-equal-priority lease is held by *another* agent,
append a `WAIT` advisory to `additionalContext` (still
`permissionDecision:"allow"` — warn-only contract preserved).

### Phase 1 — live smoke test

Start the server (`make cn-serve` → `127.0.0.1:28080`). The flow is
two simulated agents contending for one file:

```bash
BASE=http://127.0.0.1:28080

# Agent A (high prio) acquires a write lease on foo.rs.
curl -s -X POST "$BASE/api/v1/coord/lease" -H 'content-type: application/json' -d '{
  "agent_id":"A","fleet_id":"ork-1","paths":["src/foo.rs"],
  "mode":"write","priority":10,"ttl_secs":120,"reason":"refactor API"
}' | jq    # expect status:"granted", a lease_id, expires_at

# Agent B (low prio) requests the SAME file → must be queued behind A.
curl -s -X POST "$BASE/api/v1/coord/lease" -H 'content-type: application/json' -d '{
  "agent_id":"B","fleet_id":"ork-2","paths":["src/foo.rs"],
  "mode":"write","priority":5,"ttl_secs":120,"reason":"add helper"
}' | jq    # expect status:"queued", blocked_by:[A], retry_after_secs ≈ TTL-remaining

# A different file → no conflict, granted immediately.
curl -s -X POST "$BASE/api/v1/coord/lease" -H 'content-type: application/json' -d '{
  "agent_id":"C","paths":["src/bar.rs"],"mode":"write","priority":1
}' | jq    # expect status:"granted"

# Two readers on the same file → both granted (read/read compatible).
curl -s -X POST "$BASE/api/v1/coord/lease" -H 'content-type: application/json' \
  -d '{"agent_id":"R1","paths":["src/baz.rs"],"mode":"read","priority":1}' | jq '.status'
curl -s -X POST "$BASE/api/v1/coord/lease" -H 'content-type: application/json' \
  -d '{"agent_id":"R2","paths":["src/baz.rs"],"mode":"read","priority":1}' | jq '.status'
# expect "granted" "granted"

# Inspect contention on foo.rs.
curl -s "$BASE/api/v1/coord/leases?path=src/foo.rs" | jq

# A releases → B's next request is granted.
A_ID=$(...)    # lease_id from A's grant
curl -s -X DELETE "$BASE/api/v1/coord/lease/$A_ID" | jq '.status'   # "released"
# re-POST B → now status:"granted"

# The pretool gate now surfaces the lease as a WAIT advisory:
curl -s -X POST "$BASE/api/v1/cc/pretool" -H 'content-type: application/json' -d '{
  "session_id":"B","tool_name":"Edit","tool_input":{"file_path":"src/foo.rs"}
}' | jq '.hookSpecificOutput.additionalContext'
# expect text naming the holder + ETA while A holds the lease

# TTL self-heal: acquire with ttl_secs:2, wait 3s, re-request as another
# agent → granted (the abandoned lease expired, no manual release).
```

**Pass criteria:** granted/queued decisions match the priority + overlap
rules; read/read coexists; release frees the queue; an expired lease
auto-frees; the gate advisory names the blocking holder.

---

## Phase 2 — Priority inheritance + deadlock detection

**What.** (1) When a high-prio agent waits on a low-prio holder, bump the
holder's *effective* priority to the waiter's for the wait duration
(prevents a medium-prio agent jumping the queue and starving the
high-prio waiter). (2) Maintain a wait-for graph; on each `queued`
decision, walk for a cycle (A→B→A); break it by telling the
lower-priority requester to abort/retry (wound-wait).

### Phase 2 — live smoke test

```bash
# Priority inversion scenario:
#  - low-prio L holds foo.rs
#  - high-prio H requests foo.rs (waits on L → L inherits H's priority)
#  - medium-prio M requests foo.rs → must queue BEHIND L (not ahead of H)
# Assert: GET /coord/leases?path=src/foo.rs shows L with effective_priority
# = H's priority, and M's queue position is after H.

# Deadlock scenario:
#  - A holds X, B holds Y
#  - A requests Y (queued), B requests X (would close the cycle)
# Assert: B's POST returns status:"abort" with reason:"deadlock", and
# GET /coord/leases shows no wait-for cycle remains.
```

**Pass criteria:** no starvation of the high-prio waiter; the cycle is
detected and broken deterministically (lower priority loses).

---

## Phase 3 — Deny-mode enforcement

**What.** An opt-in **strict** mode for designated path scopes. For those
scopes the PreToolUse gate returns `permissionDecision:"deny"` (Claude
Code *does* honour a hard block) instead of a warn — so a contended edit
is actually prevented, not just discouraged. Default stays warn-only to
honour the fire-and-forget cc-hooks contract; strict mode is per-scope
opt-in (e.g. a `strict:true` flag on the lease, or a configured
glob list of always-strict paths like migrations / WAL schema).

### Phase 3 — live smoke test

```bash
# A holds a strict write lease on src/migrations/*.
# B's pretool check on src/migrations/0007.sql must return
# permissionDecision:"deny" (hard block), not "allow".
curl -s -X POST "$BASE/api/v1/cc/pretool" -H 'content-type: application/json' -d '{
  "session_id":"B","tool_name":"Edit",
  "tool_input":{"file_path":"src/migrations/0007.sql"}
}' | jq '.hookSpecificOutput.permissionDecision'   # expect "deny"

# A non-strict scope still returns "allow" + warn.
```

**Pass criteria:** strict scopes block; non-strict scopes warn; the
default posture is unchanged for everyone who doesn't opt in.

---

## Phase 4 — Persistence + observability

**What.** A bounded **contention audit log** (who waited on whom, how
long, who aborted) + a dashboard panel + Prometheus-style counters
(`coord_leases_held`, `coord_queue_depth`, `coord_deadlocks_broken`,
`coord_ttl_expirations`). The lease *state* stays in-memory (ephemeral by
design); only the *audit trail* persists, for post-hoc "why did my edit
stall" debugging.

### Phase 4 — live smoke test

```bash
# After running the Phase 1/2 scenarios:
curl -s "$BASE/api/v1/coord/metrics" | jq
# expect non-zero queue_depth peak, ttl_expirations, deadlocks_broken
curl -s "$BASE/api/v1/coord/audit?since=10m" | jq '.[0]'
# expect a record: {waiter, holder, scope, waited_secs, outcome}
# Dashboard: open web/ → /coordination route shows live contention graph.
```

**Pass criteria:** metrics increment under contention; audit records each
wait/abort; the dashboard renders the live wait-for graph.

---

## Phase 5 — Fine-grained scope

**What.** File-level leases serialize two agents editing *different
functions in one file* unnecessarily. Add (a) glob scopes
(`src/api/**`), and (b) optional symbol/region scopes
(`src/foo.rs#fn parse`) so non-overlapping symbol edits proceed in
parallel. Conflict predicate extends to symbol-set intersection within a
file.

### Phase 5 — live smoke test

```bash
# Two agents lease disjoint symbols in the same file → both granted.
curl -s -X POST "$BASE/api/v1/coord/lease" -d '{"agent_id":"A","paths":["src/foo.rs#fn parse"],"mode":"write","priority":1}' | jq '.status'
curl -s -X POST "$BASE/api/v1/coord/lease" -d '{"agent_id":"B","paths":["src/foo.rs#fn render"],"mode":"write","priority":1}' | jq '.status'
# expect "granted" "granted"
# Same symbol → second queues.
# Glob: lease src/api/** then request src/api/coord.rs → conflict.
```

**Pass criteria:** disjoint symbols coexist; same symbol or overlapping
glob conflicts.

---

## Out of scope

- Cross-machine coordination (multiple hosts). Today's model assumes one
  localhost substrate per fleet. Multi-host would need the distributed
  mutual-exclusion algorithms this design deliberately avoids.
- Automatic edit *merging* (CRDT/OT). Leases prevent concurrent writes;
  they don't merge them. Conflict resolution stays the agent's job.
- Trusting agents to *honour* advisory warnings in Phase 1 — that's the
  cooperating-fleet assumption; Phase 3 is the answer when it doesn't hold.

## Versioning

Ships as part of v0.2.x as phases land. Independent of the v0.3 LLM-proxy
milestone and the training-flywheel epic.

## Evidence base

- `arXiv:2606.07845` — GRPO Does Not Close the Multi-Agent Coordination
  Gap (dining philosophers; LLMs can't self-coordinate, RL can't fix it).
- `arXiv:2606.14445` — tap: A File-Based Protocol for Heterogeneous LLM
  Agent Collaboration (Claude+Codex shared codebase; message protocol, no
  concurrency control — the gap this epic fills).
- Classical anchors: Gray & Cheriton (leases, 1989); Sha/Rajkumar/Lehoczky
  (priority inheritance, 1990); Rosenkrantz et al. (wound-wait/wait-die,
  1978); Courtois et al. (readers-writers, 1971); Burrows (Chubby, 2006).
