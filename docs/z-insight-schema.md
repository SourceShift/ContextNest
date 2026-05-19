# `<z-insight>` block schema

The structured-telemetry contract between an LLM assistant (Claude
Code today, any agent in principle) and ContextNest's ingester.

## What is a `<z-insight>` block?

A JSON object wrapped in `<z-insight>` … `</z-insight>` tags emitted
at the **end of every assistant turn** as part of the assistant's
text content. The agent's response carries it verbatim; downstream
tools (the original z-dashboard daemon, ContextNest's ingester) parse
it out and store its fields.

The block is **structured memory**. The agent figures out once per
turn "what's the state, what did I do, what's pending, what do I need
from the user" and emits the answer in JSON. Consumers never have to
re-summarise free-form text.

## Canonical example

```jsonc
<z-insight>
{
  "domain":             "backend",
  "goal":               "Land Claude Code session ingest into ContextNest",
  "current_task":       "Wire metadata_filter into the retrieve handler",
  "progress":           "in-progress",
  "topics":             ["claude-code-ingest", "metadata-filter", "rust"],

  "top_jobs":           ["Drafted spec for /api/v1/sessions/* endpoints",
                         "Wrote 8 epic docs under docs/roadmap/epics/cc-ingest/"],
  "current_state":      "Foundation MVP scope locked; epic specs landed; starting implementation.",
  "facts":              ["FragmentSidecar widening is the smallest change unlocking metadata filtering"],

  "tasks": [
    {"id": "T-43", "subject": "Build ingester module — parser + extractor + sink", "status": "in_progress"},
    {"id": "T-44", "subject": "Add Ingest CLI subcommand",                         "status": "pending"}
  ],

  "epic_files": [
    "src/api/tools.rs",
    "src/ingest/claude_code/extractor.rs",
    "docs/roadmap/epics/cc-ingest/INDEX.md"
  ],

  "awaiting_decision": false,
  "decision":          null,
  "blockers":          [],

  "requires_user_action": [
    {
      "step":    1,
      "action":  "Reload the dashboard at http://localhost:5283 after the next push",
      "reason":  "Vite picks up the new providers screen",
      "urgency": "soon"
    },
    {
      "step":    2,
      "action":  "Approve --install-hooks behaviour (direct write with backup vs print snippet)",
      "reason":  "Step 5 of the MVP can't ship without this decision",
      "urgency": "now"
    }
  ]
}
</z-insight>
```

## Field reference

### Existing fields (already in the original z-insight protocol)

| Field | Type | Values | Required? | Purpose |
|---|---|---|---|---|
| `domain` | string | `frontend` \| `backend` \| `research` \| `ai-ml` \| `infra` \| `ops` \| `tooling` \| `tests` \| `docs` \| `data` \| `design` \| `other` | yes | Single-word categorisation for filtering |
| `goal` | string | one sentence | yes | **Session-level** main goal — stays stable across many turns within a phase. Future-oriented (what we're trying to accomplish), not past-tense (what we did). |
| `progress` | string | `starting` \| `in-progress` \| `blocked` \| `wrapping-up` \| `idle` \| `done` | yes | High-level progress state used by the attention surface |
| `topics` | string[] | 2–5 short noun phrases | optional | Faceted tags for retrieval |
| `top_jobs` | string[] | 1–5 short bullets | optional | What landed **this turn** specifically (not session-cumulative) |
| `current_state` | string | one sentence | yes | Where the work stands right now, including any blocker. Read this first when scanning many sessions. |
| `facts` | string[] | 0–5 short bullets | optional | Non-obvious learnings — the gold for future sessions. Decisions, gotchas, environment quirks, "do not do X" notes. |
| `tasks` | object[] | each `{id, subject, status}` | optional | **The assistant's own todos.** Status ∈ `pending`/`in_progress`/`completed`/`failed`/`deleted`. NOT for user-facing actions — that's `requires_user_action[]`. |

### New fields (ContextNest extensions — all optional, backward-compatible)

| Field | Type | Required? | Purpose |
|---|---|---|---|
| `current_task` | string | optional | **What I'm doing THIS turn** specifically. Distinct from `goal` (which is session-level + stable). `goal` answers "where are we headed?"; `current_task` answers "where am I right now?" |
| `epic_files` | string[] | optional | Files this turn touched or is about to touch. Lets a future "what was Claude editing in session X?" query work via metadata filter, no transcript scan needed. |
| `awaiting_decision` | boolean | optional (defaults false) | Machine-readable flag: `true` means the assistant has produced output that requires the user to confirm/decide before continuing. Surfaces in the attention inbox. |
| `decision` | string | optional | When `awaiting_decision == true`, the **actual question** the user needs to answer. One sentence ideally. |
| `blockers` | string[] | optional | Concrete blockers preventing progress. Distinct from `progress: "blocked"` (which is just a flag) — these are the actual reasons. Each blocker should be one short string. |
| `requires_user_action` | object[] | optional | **Imperative steps the USER must do** (reload a page, click a button, run a command, confirm an outcome). Distinct from `tasks[]` which is the assistant's own work. Each entry: `{step, action, reason, urgency}` where `urgency` ∈ `"now"`/`"soon"`/`"later"`. See semantics below. |

### `requires_user_action[]` shape

```jsonc
{
  "step":    1,                                                // ordinal in the list
  "action":  "Reload 127.0.0.1:5173/en/reader/<id>",            // the imperative, verbatim
  "reason":  "Vite serves the new bundle",                      // why this matters (short)
  "urgency": "now"                                              // see urgency semantics below
}
```

**Urgency semantics:**

| Value | Meaning |
|---|---|
| `now` | The user needs to do this before the next assistant turn / next interaction with this session. Inbox highlights these. |
| `soon` | The user needs to do this before the next time they open this session. Inbox includes these. |
| `later` | Backlog. Stored but not surfaced in the attention inbox. |

## What the ingester does with this

ContextNest's ingester (`src/ingest/claude_code/`) parses every
`<z-insight>` block out of every assistant turn in a session's
`.jsonl` and writes one or more memories per block:

| Block field | Stored as memory `kind` | Stored as metadata |
|---|---|---|
| `domain`, `goal`, `current_state` | `state` | `{progress, domain, ts, src_session}` |
| `goal` (clustered across turns) | `goal_phase` | `{start_ts, end_ts, turn_span, ts, src_session}` |
| `current_task` | `current_task` | `{ts, src_session}` (low importance — superseded every turn) |
| `top_jobs[]` (per item) | `accomplishment` | `{ts, src_session}` |
| `facts[]` (per item) | `learning` | `{ts, src_session}` |
| `tasks[]` (per item, deduped to final status per id/subject) | `todo` | `{task_id, task_status, ts, src_session}` |
| `epic_files[]` | embedded in the memory's metadata as `epic_files`, not its own memory | — |
| `awaiting_decision: true` + `decision` | `decision` | `{decision_text, awaiting_decision: true, ts, src_session}` |
| `blockers[]` (per item) | `blocker` | `{ts, src_session}` |
| `requires_user_action[]` (per item) | `user_action` | `{step, reason, urgency, ts, src_session}` |
| (whole session) | `session_title` (from `ai-title` events) | `{ts, src_session}` |

## How to start emitting the new fields

If you're a Claude Code user with the z-dashboard protocol already
configured in `~/.claude/CLAUDE.md`, just extend the block schema in
your CLAUDE.md to include the new optional fields. Old consumers
ignore them; new ones (the ContextNest ingester) consume them.

A copy-paste-friendly addition to your CLAUDE.md is in the project's
[`/docs/ingest/claude-code.md`](ingest/claude-code.md) (lands with the
foundation PR).

## Compatibility

- **Old emitters** (CLAUDE.md without the new fields): the ingester
  reads what's there, missing fields default sensibly (`blockers: []`,
  `awaiting_decision: false`, etc.). No memories of the new kinds get
  stored from old sessions, but everything else still works.
- **Old consumers** (z-dashboard daemon): ignores fields it doesn't
  recognise. No change to its behaviour.
- **New emitters with new consumers**: full feature set.
- **New emitters with old consumers**: works fine; the new fields are
  just dead weight in the JSON.

## See also

- [`docs/roadmap/v0.2-claude-code-ingest.md`](roadmap/v0.2-claude-code-ingest.md) — the design rationale
- [`docs/roadmap/epics/cc-ingest/INDEX.md`](roadmap/epics/cc-ingest/INDEX.md) — follow-up PRs
- [`src/ingest/claude_code/event.rs`](../src/ingest/claude_code/event.rs) — the parser (after foundation PR lands)
