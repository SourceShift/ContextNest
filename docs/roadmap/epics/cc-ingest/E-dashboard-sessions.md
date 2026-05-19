# Epic — Dashboard `/sessions` + `/attention` routes

**Depends on:** [E-sessions-endpoint.md](E-sessions-endpoint.md) AND
the v0.2-ui dashboard scaffold (PR `feat/ui-v02-scaffold`).

**Estimate:** ~2 days.

## What

Two new routes in `web/src/routes/`:

| Route | Component | Renders |
|---|---|---|
| `/sessions` | `SessionsList` | All sessions grouped by project, with last activity + status pills |
| `/sessions/:id` | `SessionDetail` | One session: all memories grouped by kind, timeline of goal_phases |
| `/attention` | `Attention` | Inbox — what the user needs to act on across all sessions |
| `/sessions/find` | `Find` | NL search bar that hits `POST /api/v1/sessions/find` |

Plus extend the sidebar with two new navigation entries:
`📨 Attention` and `📚 Sessions`.

## Why

Some users prefer a clickable browseable UI over the CLI. The
dashboard already exists; this just adds two new screens that consume
the existing sessions endpoint.

## Files touched

| File | Change |
|---|---|
| `web/src/routes/sessions.tsx` | New: list + detail |
| `web/src/routes/attention.tsx` | New: inbox view |
| `web/src/routes/find.tsx` | New: NL search |
| `web/src/components/layout/sidebar.tsx` | Add two nav entries |
| `web/src/lib/api.ts` | Add typed clients for the sessions endpoints |
| `web/src/App.tsx` | Register routes |

## Implementation sketch

Each route is ~150 LOC of React + TanStack Query. The styling reuses
the existing `Card` / `Button` / shadcn-style primitives in
`web/src/components/ui/`. Polling at 5s for the attention view; manual
refresh for sessions list/detail.

The Attention view is the most visually-distinctive: each session card
shows its inbox items as a checklist, the decision question as a
highlighted callout, the urgency as a coloured strip down the left
edge of the card.

## Success criteria

- `/attention` renders the same data `contextnest inbox` shows in the
  terminal — same urgency sorting, same item-per-session grouping.
- `/sessions/find` returns ranked matches within 200ms of typing.
- Lighthouse Performance score on the new routes ≥85.
- Type errors at strict tsc: 0.

## What's NOT in scope

- Editing memories from the UI (no inline-edit of importance, no
  delete buttons). Memory mutation belongs in a different epic.
- Notifications when a new attention item arrives (no toast / no
  push). v0.3+ work.
- Cross-project session search with facets. Add when someone with
  20+ projects asks.
