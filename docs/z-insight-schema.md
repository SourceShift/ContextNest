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

  "read_context":      [],
  "verification":      [],
  "evidence_refs":     [],
  "decisions":         [],
  "failures":          [],
  "prompt_directives": [],
  "assumptions":       [],
  "artifacts":         [],
  "memory_candidates": [],
  "risk_flags":        [],

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

### Sparse trajectory emission policy

The trajectory arrays default to `[]`. Emit entries only when a turn crosses
one of these gates:

- **Decision gate:** a choice was settled that should prevent future agents
  from reopening the same question.
- **Verification gate:** a command, test, dry-run, curl probe, or manual check
  materially changed confidence in the work.
- **Failure/recovery gate:** a non-obvious failed attempt, blocked command,
  wrong hypothesis, or recovery path should not be repeated.
- **Risk gate:** the turn touched security, privacy, data loss, shared infra,
  WAL/schema migration, secrets, or other high-consequence state.
- **Promotion gate:** the turn produced a reusable instruction, repeated
  pattern, durable workflow, or high-consequence gotcha.
- **Artifact gate:** a durable file/report/fixture/patch was produced and is
  likely worth finding by path later.

Hard caps per turn:

| Field | Max entries |
|---|---:|
| `read_context` | 3 |
| `verification` | 3 |
| `evidence_refs` | 5 |
| `decisions` | 2 |
| `failures` | 2 |
| `prompt_directives` | 1 |
| `assumptions` | 2 |
| `artifacts` | 3 |
| `memory_candidates` | 2 |
| `risk_flags` | 2 |

These caps are prompt-level guidance, not a storage limit. They keep per-turn
token cost and memory noise bounded while still preserving high-signal
trajectory evidence for later phase/session aggregation.

| Field | Type | Required? | Purpose |
|---|---|---|---|
| `current_task` | string | optional | **What I'm doing THIS turn** specifically. Distinct from `goal` (which is session-level + stable). `goal` answers "where are we headed?"; `current_task` answers "where am I right now?" |
| `epic_files` | string[] | optional | Files this turn touched or is about to touch. Lets a future "what was Claude editing in session X?" query work via metadata filter, no transcript scan needed. |
| `awaiting_decision` | boolean | optional (defaults false) | Machine-readable flag: `true` means the assistant has produced output that requires the user to confirm/decide before continuing. Surfaces in the attention inbox. |
| `decision` | string | optional | When `awaiting_decision == true`, the **actual question** the user needs to answer. One sentence ideally. |
| `blockers` | string[] | optional | Concrete blockers preventing progress. Distinct from `progress: "blocked"` (which is just a flag) — these are the actual reasons. Each blocker should be one short string. |
| `requires_user_action` | object[] | optional | **Imperative steps the USER must do** (reload a page, click a button, run a command, confirm an outcome). Distinct from `tasks[]` which is the assistant's own work. Each entry: `{step, action, reason, urgency}` where `urgency` ∈ `"now"`/`"soon"`/`"later"`. See semantics below. |
| `delivered_features` | object[] | optional | **Features this turn shipped**, named in the assistant's own words. Lets the substrate answer "which session added the query-overlay mode" without grepping commits or PRs. Each entry: `{feature, files?, refs?, layer?}` — see [`delivered_features[]` shape](#delivered_features-shape) below. Higher-signal than walking `tool_use` because the agent names the feature; the per-file `tool_use` index in the ingester answers "which session touched X.tsx" complementarily. |
| `read_context` | object[] | optional | Files, docs, transcripts, or external sources the assistant inspected before acting. Distinct from `files_touched`, which records only mutations. |
| `verification` | object[] | optional | Commands, curl probes, manual checks, dry-runs, or tests with status and summary. Used to distinguish verified work from plans and assumptions. |
| `evidence_refs` | object[] | optional | Structured pointers supporting claims in the block: file anchors, commands, PRs, commits, URLs, transcript turns, or logs. |
| `decisions` | object[] | optional | Settled decisions already made. Distinct from `decision` + `awaiting_decision`, which means the user still needs to decide. |
| `failures` | object[] | optional | Error, failed-command, permission-denial, or recovery traces. Used for future trajectory analysis and anti-pattern extraction. |
| `prompt_directives` | object[] | optional | Compact, scoped candidate instructions that a future prompt compiler may inject. Should include trigger, directive, scope, confidence, and evidence when possible. |
| `assumptions` | object[] | optional | Premises that shaped the work and may go stale. Should carry basis and validity/staleness hints. |
| `artifacts` | object[] | optional | Docs, reports, plans, research notes, todos, or patches produced this turn that are not shipped product features. |
| `memory_candidates` | object[] | optional | Candidate long-term preferences, rules, gotchas, workflows, or anti-patterns that need later promotion. |
| `risk_flags` | object[] | optional | Security, privacy, data-loss, migration, or high-consequence constraints that future prompts should surface early. |

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

### `delivered_features[]` shape

```jsonc
{
  "feature":     "query-overlay mode for /field viz",   // required: short feature name
  "files":       ["web/src/routes/field.tsx",          // optional: which files implement it
                  "web/src/styles.css"],
  "refs":        ["PR #39", "src/api/tools.rs:retrieve"], // optional: outside-of-files pointers
  "layer":       "frontend",                            // optional: frontend|backend|infra|docs|tests|other
  "how_to_test": "open /field, paste 'auth bcrypt' into the query bar, verify dimming",  // optional: replayable recipe
  "defs":        ["fn basin_aware_expand", "fn fragmentRadius"]                          // optional: symbol names
}
```

The ingester emits one `MemoryKind::Feature` record per entry, with
`metadata.kind == "feature"`, `metadata.files`, `metadata.refs`,
`metadata.layer`, `metadata.how_to_test`, `metadata.defs`, and
`metadata.src_session` set. The feature name itself goes into the
fragment's text so semantic retrieve hits it.

**Why this isn't just a `top_jobs[]` bullet:** `top_jobs` is
freeform "I did X" prose. `delivered_features` is structured, with
file pointers + layer + refs + test recipe + symbol defs, suitable for
`/api/v1/sessions/by-feature?q=…` to answer "which session added
feature X" deterministically AND
`/api/v1/features?since=24h&layer=backend` to answer "what shipped
today, and how do I test it?" without grepping commits.

### `how_to_test` conventions

Free-form. The agent picks the shape that fits the feature:
- Shell command: `cargo test --test foo`
- Curl one-liner: `curl http://localhost:28080/api/v1/features?since=24h | jq`
- Manual recipe: `Open /field, paste 'auth' into query bar, verify dimming`

A future migration may tag the field with a kind (`"shell"`,
`"curl"`, `"manual"`); free-form is the simplest thing that lets
the daily-test loop work today.

### `defs[]` conventions

Symbol names the agent says implement the feature. Pick the shortest
unambiguous form:
- Rust: `fn retrieve`, `struct BasinSnapshot`, `impl MemoryAttractorManager`
- TypeScript: `function QueryResultsPanel`, `type RetrieveHit`
- Used by future "what code defines feature X" queries; not yet
  consumed by an endpoint but stored so it's there when needed.

### `verification[]` shape

```jsonc
{
  "kind": "shell",                 // shell|curl|manual|test|dry_run
  "command": "cargo test --lib",   // optional for manual checks
  "status": "passed",              // passed|failed|blocked|not_run
  "summary": "All extractor tests passed",
  "counts": {"tests": 12, "failures": 0}
}
```

Failed and blocked verification records are stored with higher importance than
passed records because future sessions need to see unresolved checks early.

### `read_context[]` shape

```jsonc
{
  "path": "src/ingest/claude_code/extractor.rs",
  "kind": "source",                 // doc|source|test|config|transcript|external
  "reason": "Match the existing MemoryKind pattern",
  "salient": "metadata.kind is the routing key",
  "refs": ["src/ingest/claude_code/extractor.rs:78"]
}
```

Use this for meaningful grounding context, not every file skimmed. The
ingester stores item-level `kind` as `metadata.item_kind` so it does not
overwrite the memory router in `metadata.kind`.

### `decisions[]` shape

```jsonc
{
  "decision": "Use optional z-insight arrays before changing storage schema",
  "made_by": "assistant",
  "alternatives": ["free-form facts only", "new SQL tables first"],
  "rationale": "Existing parser can accept optional fields safely",
  "reversibility": "two_way",
  "scope": "project"
}
```

Use `decisions[]` only for settled choices. Use `awaiting_decision` +
`decision` when the user still needs to choose.

### `prompt_directives[]` shape

```jsonc
{
  "trigger": "When modifying Claude ingest schema",
  "directive": "Run a dry-run ingest before changing storage behavior",
  "scope": "project",
  "confidence": "high",
  "evidence": ["session abc123"]
}
```

These are candidate prompt-capsule lines. Keep them short, scoped, and
evidence-linked so bad directives can be audited and demoted later.

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
| `read_context[]` (per item) | `read_context` | item fields, with item-level `kind` stored as `item_kind` |
| `verification[]` (per item) | `verification` | item fields, with failed/blocked status assigned higher importance |
| `evidence_refs[]` (per item) | `evidence_ref` | item fields, with item-level `kind` stored as `item_kind` |
| `decisions[]` (per item) | `decision_made` | item fields |
| `failures[]` (per item) | `failure` | item fields |
| `prompt_directives[]` (per item) | `prompt_directive` | item fields, with confidence influencing importance |
| `assumptions[]` (per item) | `assumption` | item fields |
| `artifacts[]` (per item) | `artifact` | item fields, with item-level `kind` stored as `item_kind` |
| `memory_candidates[]` (per item) | `memory_candidate` | item fields, with item-level `kind` stored as `item_kind` |
| `risk_flags[]` (per item) | `risk_flag` | item fields, with high/critical severity assigned higher importance |
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
