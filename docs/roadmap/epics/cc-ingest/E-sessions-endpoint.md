# Epic — Sessions endpoint

**Path:** `/api/v1/sessions/*`. The query surface that turns stored
memories into answers.

**Depends on:** MVP foundation PR (this branch).

**Estimate:** ~2 days.

## What

Four HTTP endpoints under `/api/v1/sessions/`:

| Endpoint | Returns | Purpose |
|---|---|---|
| `GET /` | List of session summaries (id, project, last_state, latest goal_phase, ts) | "What was I working on?" |
| `GET /:short_id` | Session detail: every memory grouped by kind | Drill-down |
| `GET /attention` | Sessions with the latest memory carrying `awaiting_decision: true`, non-empty `requires_user_action`, or `progress: "blocked"` | The inbox query |
| `POST /find` | NL query → ranked sessions by embedding cosine over goal_phase + session_title | "Find the session where I worked on X" |

Query params for the list: `?project=X&since=7d&limit=20`.

## Why

The MVP lands the data (memories with kind/awaiting_decision/etc.
metadata) but no query path that aggregates per-session. Without this
epic, the user has to construct `/api/v1/tools/retrieve` calls with
metadata_filter manually for every question.

## Files touched

| File | Change |
|---|---|
| `src/api/sessions.rs` | New module — 4 handlers |
| `src/api/mod.rs` | Mount routes under `/api/v1/sessions` |
| `src/services/mod.rs` | Maybe add `list_sessions()` helper that walks the SessionIndex + metadata sidecar |
| `tests/sessions_endpoint_test.rs` | Integration tests for all 4 endpoints against an in-process substrate |
| `docs/usage.md` | Document the new endpoints with curl examples |

## Implementation sketch

Each handler is a thin wrapper over the existing seven-tool API +
metadata filtering (from MVP). The `find` handler additionally calls
`services.embedding.generate_embedding(query)` and ranks stored
`session_title` + `goal_phase` memories by cosine similarity.

The attention endpoint is `metadata_filter: {awaiting_decision: true} OR
metadata_filter: {kind: "blocker"} OR metadata_filter: {kind:
"user_action"}` unioned and grouped by session.

## Success criteria

- `curl /api/v1/sessions/attention` returns the inbox for the current
  project in <100ms.
- `curl -X POST /api/v1/sessions/find -d '{"query": "..."}'` returns
  ranked matches with relevance scores.
- The session detail endpoint groups by kind in a stable order that
  prioritises actionable items (decisions + user_actions first).

## What's NOT in scope

- Pagination beyond a simple `limit` param. Cursor-based pagination
  lands when someone has 1000+ sessions and complains.
- Real-time updates (WebSocket / SSE). The hook receiver epic owns
  that.
- Session deletion / archival.
