# Agent coordination via ContextNest — design proposal

**Status:** Proposal. Research-grounded, not yet committed scope.
**Date:** 2026-06-06
**Audience:** ContextNest maintainers + the user (solo-founder running
parallel mini-orch dispatches across Claude + Codex agents).
**Pre-reads:** `docs/architecture.md` (CN substrate primitives),
`docs/roadmap/v0.2-to-v1.0.md` (milestone sequence),
`docs/development-workflow.md` (worktree-per-PR pattern).

## Problem statement

The user runs many Claude / Codex sessions concurrently. Some are
**master** sessions (live in the main project checkout); some are
**worker** sessions spawned by master mini-orch dispatches (live in
`*.agentflow/worktrees/<EPIC>-<TRACK>-<MODEL>/`). The substrate
already records who-touched-what after the fact (`/sessions/by-file`,
`delivered_features[]`), but **conflict surfaces only at integration
time** — a worker pushes, the master tries to merge, and only then
does it discover a peer worker touched the same file or the same
migration. Worse, two masters can independently dispatch overlapping
work because neither knows the other started.

The ask: a ContextNest feature that agents can **query at task start
and during work** to answer:

1. *Is what I'm about to do conflicting with what another agent is
   already doing?*
2. *Are there shared contracts I should negotiate first to reduce
   that conflict?*
3. *If a conflict is unavoidable, what's the best coordination
   pattern (defer, split scope, sequence)?*

This document surveys the literature on multi-agent coordination,
maps existing CN primitives to the requirements, and proposes a
concrete feature design.

## Literature survey

Five threads in the recent literature are directly relevant:

| Paper | Year | Anchor idea | Maps to CN as |
|---|---|---|---|
| [Smith — Contract Net Protocol](https://www.cs.ubc.ca/~kevinlb/teaching/cs532a%20-%202003-4/Papers/Smith-CNet.pdf) | 1980 | announce → bid → award → execute | inbox kinds: `propose_contract`, `accept_contract`, `reject_contract` |
| [Hayes-Roth — Blackboard](https://www.sciencedirect.com/science/article/abs/pii/0004370285900637) | 1985 | broadcast requests to shared space; agents volunteer | CN's basin-aware /retrieve is already a topical blackboard |
| [Semantic Consensus Framework (arXiv 2604.16339)](https://arxiv.org/abs/2604.16339) | 2026 | Process Context Layer + Semantic Intent Graph + Conflict Detection Engine + Consensus Resolution Protocol + Drift Monitor + Governance Integration | the closest published analog of what we'd build on CN |
| [Agent Contracts (arXiv 2601.08815)](https://arxiv.org/abs/2601.08815) | 2026 (COINE / AAMAS) | Contract Net adapted to resource-bounded LLM execution; "conservation laws ensuring delegated budgets respect parent constraints"; 90% token reduction, 525× lower variance | formalism for contract messages between sessions |
| [CAID — Asynchronous SE Agents (arXiv 2603.21489)](https://arxiv.org/abs/2603.21489) | 2026 | central manager + per-worker git worktree isolation + executable test-based verification; +26.7% PaperBench, +14.3% Commit0 | **already implemented** in the user's mini-orch setup |
| [CodeCRDT (arXiv 2510.18893)](https://arxiv.org/abs/2510.18893) | 2025 | CRDTs for lock-free coordination via observable shared state | alternative if the negotiation overhead proves too high |
| [Blackboard Multi-Agent System (arXiv 2510.01285)](https://arxiv.org/abs/2510.01285) | 2025 | central agent posts to blackboard, autonomous subordinates volunteer; 13–57% improvement on data-discovery success | model for the "conflict probe → who-can-help" surface |
| [LLM-X (arXiv 2605.11376)](https://arxiv.org/abs/2605.11376) | 2026 | scalable negotiation-oriented exchange + typed message bus | wire-shape reference for `POST /coordination/probe` |
| [AWCP (arXiv 2602.20493)](https://arxiv.org/abs/2602.20493) | 2026 | Workspace Delegation Protocol for deep-engagement collaboration across remote agents | scope-handoff semantics for master ↔ worker boundary |

The shared thesis across the 2026 cohort: **coordination is a
substrate concern, not an agent concern.** Each individual agent can
be terrific at the work it does; conflict emerges from missing
infrastructure between them. The papers differ on what to centralize
(blackboard vs contract bus vs intent graph) but all reject the
"every agent figures it out per-call" approach.

CN is already a substrate, and three of its existing primitives map
1:1 onto the Semantic Consensus + Agent Contracts patterns:

- **Session-intent embeddings** (PR #128, Option C) ≈ Semantic Intent
  Graph: every active session already has a vector representing
  "what this session is ABOUT" — the building block for proactive
  intent-overlap detection.
- **Inbox + `ask` / `handoff` kinds** (PR #120) ≈ Contract Net
  message types: cross-session communication already exists; we just
  need to extend the kind taxonomy with contract-net verbs.
- **Basins + connection network** (existing core) ≈ blackboard
  topical clusters: sessions already auto-cluster by topic; a fresh
  agent's basin co-membership is a free "you're in the same
  neighborhood as N other sessions" signal.

What's missing is the **proactive query surface**. CN exposes
"who touched X" *after* the fact. Coordination needs "who's about to
touch X" *before*. That's the gap this proposal closes.

## CN primitives → coordination questions

| Coordination question | Existing CN primitive that already answers it | Gap |
|---|---|---|
| Who touched file X historically? | `GET /sessions/by-file?path=X` | — |
| Who's working on intent semantically similar to mine? | `GET /sessions/by-intent?q=…` (PR #128) | — for non-live sessions; live `last_ts` filter not yet wired |
| Which session shipped feature X? | `GET /sessions/by-feature?q=X` | — |
| Which sessions cluster together topically? | `GET /field/basins`, `GET /connections` | — |
| Which session is the master orchestrator vs worker? | inferable from `project_cwd` substring `.agentflow/worktrees/` | Needs a typed `kind: master|worker` field for cheap filtering |
| **Is there an ACTIVE session about to touch the files I'm about to touch?** | none | **Layer 1 below** |
| **Is the intent of my pending task overlapping with an active session's?** | weak signal via by-intent on a live query, but no claim registry | **Layer 1 below** |
| **Are there negotiable contracts in flight that I should accept or counter-offer?** | none | **Layer 3 below** |
| **What's the current resource-claim state (ports / DB schema / env flags)?** | none | **Layer 2 below** |

## Proposed feature: Conflict Probe + Contract Net

Three layers, each shippable as an independent PR slice. Together
they form a coherent coordination substrate; individually each one
already pays back vs the status quo.

```mermaid
sequenceDiagram
    autonumber
    participant W as Worker session<br/>(about to start)
    participant CN as ContextNest
    participant M as Master session
    participant W2 as Peer worker session<br/>(already running)

    Note over W,CN: Layer 1 — register intent + claims
    W->>CN: POST /coordination/register<br/>{ session_id, intent_text, planned_files[], resources[] }
    CN-->>W: { claim_id, overlapping_sessions[], severity }

    Note over W,CN: Layer 2 — probe before each significant step
    W->>CN: POST /coordination/probe<br/>{ session_id, files_about_to_touch[], resource_about_to_claim }
    CN->>CN: compute file Jaccard,<br/>intent cosine, basin co-membership,<br/>resource overlap
    CN-->>W: conflicts[]: [<br/>  { with_session, severity, reason,<br/>    suggested_action: defer|negotiate|split|proceed }<br/>]

    Note over W,CN,W2: Layer 3 — negotiate contracts
    W->>CN: POST /coordination/contracts<br/>{ to_session: W2, scope, deadline }<br/>(persisted as inbox kind=propose_contract)
    CN->>W2: inbox surfaces the proposal
    W2->>CN: POST /coordination/contracts/{id}/respond<br/>{ accept | reject | counter }
    CN-->>W: terminal state + the agreed-on partition
```

### Layer 1 — intent + claim registration

**Endpoint:** `POST /api/v1/coordination/register`

```jsonc
{
  "session_id": "9b91d3f9-…",
  "role": "master" | "worker",            // typed master/worker flag
  "intent_text": "Land W1 workspace schema extensions …",
  "planned_files": [
    "server/database/migrations/2026*-ws-w1-*",
    "server/routes/workspace/*.ts",
    "shared/types/workspace.ts"
  ],
  "resources": [
    { "kind": "port",       "id": "5175" },
    { "kind": "db_schema",  "id": "workspace" },
    { "kind": "env_flag",   "id": "FEATURE_WORKSPACE_V1" },
    { "kind": "mini_orch",  "id": "EMBPLAN-A" }
  ],
  "expected_duration_minutes": 90,
  "parent_session_id": null               // set when a worker spawned by master
}
```

**Response:**

```jsonc
{
  "claim_id": "claim-<short>",
  "overlapping_sessions": [
    { "session_id": "...", "severity": "yellow",
      "overlap_reason": "intent_cosine=0.78 (above warn threshold 0.7)" }
  ],
  "tombstone_at_unix_secs": 1735128000     // auto-released after expected_duration + 2h grace
}
```

**Mechanics:**

- New in-memory `agent_claims` index (`HashMap<claim_id, Claim>`),
  WAL-backed via a new `WalRecord::CoordinationClaim` variant so
  claims survive restart but auto-tombstone after expiry.
- Intent embedding reuses the existing `session_intent_embeddings`
  cache (PR #128). When the caller hasn't called `/by-intent` yet,
  this endpoint warms its slot.
- The `overlapping_sessions[]` in the response is the immediate
  warning surface — `red` (file overlap) > `yellow` (intent only) >
  `green` (no overlap).
- Per Semantic Consensus's *Drift Monitor* (paper §3.5), claims
  whose intent vector drifts past a similarity threshold from their
  original registration get a `claim_drift` event in the inbox so
  the operator can see "this worker's actual work no longer matches
  what it claimed at registration time."

### Layer 2 — conflict probe

**Endpoint:** `POST /api/v1/coordination/probe`

The query an agent fires before each significant action:

```jsonc
{
  "session_id": "9b91d3f9-…",
  "files_about_to_touch": ["server/services/workspace/billing.ts"],
  "resource_about_to_claim": { "kind": "db_schema", "id": "billing" },
  "step_description": "Add Stripe webhook handler"
}
```

**Response — concrete grading + actionable suggestion:**

```jsonc
{
  "conflicts": [
    {
      "with_session": "4eec0f26-…",
      "severity": "red",
      "signals": {
        "file_jaccard": 1.0,                  // identical files claimed
        "intent_cosine": 0.92,
        "basin_overlap": ["workspace-billing", "stripe-integration"],
        "resource_overlap": ["db_schema:billing"]
      },
      "reason": "Both sessions claim db_schema=billing AND file overlap is 100%.",
      "suggested_action": "defer_until",
      "suggested_action_detail": {
        "wait_for_session": "4eec0f26-…",
        "wait_reason": "they have the active claim and longer expected_duration"
      }
    }
  ],
  "soft_conflicts": [
    {
      "with_session": "965090f9-…",
      "severity": "yellow",
      "signals": { "intent_cosine": 0.74, "file_jaccard": 0.0 },
      "reason": "Similar topic (workspace billing) but disjoint files.",
      "suggested_action": "negotiate_contract",
      "suggested_action_detail": {
        "propose_split": "you take routes/webhook; they keep services/billing"
      }
    }
  ],
  "no_conflicts_count": 3                   // sessions checked, no overlap
}
```

**Conflict signals** — each is a row that exists today in some form;
the probe just combines them:

| Signal | How computed | Cost |
|---|---|---|
| **File overlap (Jaccard)** | `intersect(planned_files, others.planned_files) / union(…)` | O(N_claims × avg_files), in-memory string match |
| **Intent cosine** | embed `step_description`, cosine against active session intent embeddings | 1 embed call + O(N_claims) sim — reuses Option-C cache + Option B's bounded fan-out (PR #131) |
| **Basin co-membership** | both sessions in same basin per existing `attractor_basin` | O(1) lookup against existing field |
| **Resource overlap** | typed-claim string match on `(kind, id)` tuples | O(N_claims) |

**Severity grading** (per Semantic Consensus, paper §4):

- 🔴 **red — hard conflict** — file Jaccard > 0 OR exact resource
  overlap. Two agents physically cannot both succeed.
- 🟡 **yellow — soft conflict** — intent cosine > 0.75 with no file
  or resource overlap. The agents are working on the same topic
  separately; coordination would reduce duplicated work.
- 🟢 **green — no conflict** — orthogonal scope; safe to proceed.

**Suggested actions** (drawn from CAID 2603.21489 + Agent Contracts
2601.08815):

- `proceed` — orthogonal; nothing to coordinate.
- `negotiate_contract` — open a contract proposal to the conflicting
  session; let the agents partition scope.
- `defer_until` — the other session has stronger claim (longer
  expected_duration, earlier `tombstone_at`); wait for their claim
  to release.
- `split_scope` — both can proceed if they accept a specific
  partition the substrate can suggest (e.g., "you take FE, they
  take BE").
- `escalate_to_human` — severity red with no clean partition; pause
  + add an `ask` to the master's inbox.

### Layer 3 — contract net messaging

Extends the existing `kind` taxonomy with four contract-net verbs:

| New `kind` | Semantics |
|---|---|
| `propose_contract` | Sender proposes "I do scope A, you do scope B, by deadline T." |
| `accept_contract` | Recipient accepts the proposed partition verbatim. |
| `reject_contract` | Recipient declines with a reason (no counter-offer). |
| `counter_contract` | Recipient proposes a modified partition (loops back to `propose_contract` semantics on the new shape). |
| `complete_contract` | Sender signals scope is done; other party can proceed with their half. |

**Endpoint:** `POST /api/v1/coordination/contracts` (new) +
`POST /api/v1/coordination/contracts/:id/respond`.

**Wire shape:**

```jsonc
// propose
{
  "contract_id": "ctr-<short>",
  "from_session": "9b91d3f9-…",
  "to_session":   "4eec0f26-…",
  "scope": {
    "files_claimed_by_sender":    ["server/routes/workspace/webhook.ts"],
    "files_claimed_by_recipient": ["server/services/billing/*"],
    "deadline_unix_secs": 1735128000
  },
  "rationale": "Webhook is FE-adjacent and small; billing service is BE-deep."
}

// respond
{ "accept": true }
// — or —
{ "counter": { "scope": { … } } }
// — or —
{ "reject": { "reason": "I'm already three commits into billing/webhook.ts" } }
```

**Persistence:** every contract message is a regular substrate
fragment with `metadata.kind = "propose_contract"` (etc.). That gets
the existing WAL durability + inbox + audit-log infrastructure
*for free*. No new persistence layer.

**Resource budget** (per Agent Contracts 2601.08815's "conservation
laws"): a master session that dispatches workers carries a
resource-budget claim; the workers' sum of claims must be ≤ the
master's. CN enforces this at registration time — a worker claim
that would push the parent's worker-sum over budget is rejected
with `409 Conflict` + suggested smaller scope.

### Layer 4 (deferred) — proactive watcher

Once Layers 1–3 are in production, a follow-up watcher loop can:

- Tail the `agent_claims` index and emit `claim_drift` to the inbox
  when a session's `delivered_features` strays from its registered
  intent (Drift Monitor pattern, Semantic Consensus §3.5).
- Surface "you've held this resource for 2× your declared
  expected_duration without a heartbeat" as a soft-warning to
  encourage timely release.

This isn't load-bearing for v1 — the operator can do the watching
in the dashboard. Worth queuing as v1.1 once real usage shows which
drift patterns are recurrent.

## Why this fits CN's substrate-as-memory thesis

The user's substrate-as-memory pitch (`docs/roadmap/v0.2-to-v1.0.md`)
is: every primitive is a memory; agents query it semantically. The
existing seven tools (`store / retrieve / update / discard /
summarize / reconstruct / resonate`) are all memory verbs.

**Conflict probe and contract net are coordination memories.** A
claim is a fragment with a known TTL. A contract is a fragment with
two `session_id`s and a `kind` enum. The substrate already knows how
to retrieve, summarize, and discard these shapes. The only new
primitive is **the proactive query that answers "is my future work
conflicting"** — and that's mostly a reuse of `by-intent` + a new
file-Jaccard scorer.

This means the slice doesn't expand the substrate's vocabulary —
it composes existing primitives into a new query shape, which is
exactly the substrate-as-memory thesis at work.

## Migration path / phasing

Recommended sequencing (each shippable independently):

1. **Phase 1 — claim registration only.** Just `POST /register`,
   reuse intent embedding cache, in-memory index + WAL variant.
   Operator sees claims in the dashboard `/sessions` page. Agents
   can opt in voluntarily; no breaking change.
2. **Phase 2 — probe endpoint.** Add the file-Jaccard scorer + the
   compound severity grading + suggested-action ranker. Agents start
   firing probes between their CN store calls.
3. **Phase 3 — contract messaging.** Extend the kind taxonomy +
   the inbox eligibility list + the two endpoints. The hard part is
   the agent-side training to *use* contracts when probe returns
   yellow — that lives in the CLAUDE.md / agent prompts, not the
   substrate.
4. **Phase 4 (deferred) — drift monitor + budget enforcement.**

This phasing also gives clean A/B opportunities: ship Phase 1, run
the smoke benchmark + the user's mini-orch workload for a week,
measure how often a probe would have caught a real conflict, then
decide whether Phase 2/3 lead time is worth it.

## Open questions

The proposal makes defensible defaults, but five questions deserve
explicit user input before implementation:

1. **TTL on claims.** Default proposal: `expected_duration + 2h
   grace`, then auto-release. Some sessions run for days (the
   workspace ladder is one). Should claims be heartbeat-renewable
   instead, with a missed-heartbeat default of "release after 24h"?
2. **Hard vs soft enforcement.** Should the substrate ever
   *refuse* a `register` that would create a red conflict, or just
   surface the warning and let the agent decide? Soft-only is more
   honest to the agent's autonomy; hard-enforcement is safer but
   risks blocking legitimate work.
3. **Master-vs-worker priority.** When two sessions both claim the
   same scope, who wins? Default proposal: longer
   `expected_duration` + earlier `registered_at` wins. Alternative:
   master sessions always trump workers (workers must escalate to
   their parent). Alternative 3: explicit per-claim priority field.
4. **Cross-project coordination.** Should a worker in
   `researcher/` see conflict warnings against a worker in
   `ContextNest/`? Default: no — `project_cwd` is the scope
   boundary. Some operators run shared infrastructure (DB) across
   projects; they'd want cross-project visibility.
5. **Privacy.** Sessions register what they're *about* to do; that
   text could contain confidential intent. Should `register`
   payloads inherit the LLM-cache redactor (PR #129) before WAL
   persistence? Default: yes — same redactor, same WAL, no new
   surface.

## What this proposal is NOT

- **Not a replacement for git.** Worktree isolation per CAID stays
  the authoritative conflict mechanism for file-level edits. The
  probe surfaces conflicts *earlier* so worktrees can be sized
  appropriately; it doesn't replace the merge step.
- **Not a scheduler.** CN doesn't run jobs or block agents.
  Probe returns recommendations; the agent and the operator decide.
- **Not a strict consistency layer.** Two agents working on
  overlapping files can still both proceed if they accept the
  conflict — the substrate logs the override and lets the human
  reconcile later via standard merge tools.
- **Not auth.** v0.3 has no auth model; this slice doesn't change
  that. Per-project tenancy lands with v0.2.

## Decision needed

This document is a proposal. Concrete next step is one of:

- **Approve as-is** → I'll scaffold Phase 1 (claim registration)
  in a follow-up PR slice. ~250 LOC + tests.
- **Approve with modifications** to the open questions above →
  I'll incorporate before scaffolding.
- **Defer** → park behind v0.3 closure + the MCP work
  (#12-#14 from the coverage epic) and revisit when concurrent
  agent count grows past current pain.

## References

- Smith, R. G. (1980). *The Contract Net Protocol: High-Level
  Communication and Control in a Distributed Problem Solver.*
- Hayes-Roth, B. (1985). *A blackboard architecture for control.*
- arXiv:2604.16339 — *Semantic Consensus: Process-Aware Conflict
  Detection and Resolution for Enterprise Multi-Agent LLM Systems.*
- arXiv:2601.08815 — *Agent Contracts: A Formal Framework for
  Resource-Bounded Autonomous AI Systems* (COINE / AAMAS 2026).
- arXiv:2603.21489 — *Effective Strategies for Asynchronous
  Software Engineering Agents* (CAID).
- arXiv:2510.18893 — *CodeCRDT: Observation-Driven Coordination
  for Multi-Agent LLM Code Generation.*
- arXiv:2510.01285 — *LLM-Based Multi-Agent Blackboard System for
  Information Discovery in Data Science.*
- arXiv:2507.01701 — *Exploring Advanced LLM Multi-Agent Systems
  Based on Blackboard Architecture.*
- arXiv:2605.11376 — *LLM-X: A Scalable Negotiation-Oriented
  Exchange for Communication Among Personal LLM Agents.*
- arXiv:2602.20493 — *AWCP: A Workspace Delegation Protocol for
  Deep-Engagement Collaboration across Remote Agents.*
- arXiv:2602.04418 — *SPEAR: An Engineering Case Study of
  Multi-Agent Coordination for Smart Contract Auditing.*
