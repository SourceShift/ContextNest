# Epic — `contextnest inbox` CLI helper

**Depends on:** [E-sessions-endpoint.md](E-sessions-endpoint.md).

**Estimate:** ~1 day.

## What

A new CLI subcommand that prints a flat, urgency-sorted list of things
the user needs to act on across every Claude Code session.

```bash
contextnest inbox                      # all sessions, all projects
contextnest inbox --project ContextNest
contextnest inbox --urgency now        # only "now" items
contextnest inbox --json               # machine-readable, for piping
```

Sample output:

```
📋 ContextNest — what Claude needs from you

▸ session-d7abfbf5  · libwit · highlight rescue menu fixes
   urgency: now
   1. Reload 127.0.0.1:5173/en/reader/...   → Vite serves new bundle
   2. Click pulsing highlight (left-click)  → menu appears
   3. Stop waiting                          → fires BE /stop + local fail
   4. Remove highlight                      → deletes from DB + cache
   ❓ Confirm: does the menu open on left-click after reload?

▸ session-4c998114  · ContextNest · claude-code ingest design
   urgency: soon
   ❓ Confirm: --install-hooks should write directly with backup, or print snippet?
```

## Why

The user explicitly asked for this. They don't want to read walls of
text from each Claude session — they want one flat list, sorted by
urgency. The inbox is the killer terminal experience for the ingest
pipeline.

## Files touched

| File | Change |
|---|---|
| `src/cli/mod.rs` | Add `Inbox` variant to `Commands` enum |
| `src/bin/contextnest.rs` | Wire handler — calls `GET /api/v1/sessions/attention?include=user_actions,decisions`, renders grouped output |
| `tests/inbox_cli_test.rs` | Snapshot test against a fixture inbox response |
| `docs/ingest/inbox.md` | One-page user-facing doc |

## Implementation sketch

```rust
fn render_inbox(sessions: Vec<SessionAttention>) -> String {
    let by_urgency = group_and_sort(sessions);  // now > soon > later
    for entry in by_urgency {
        println!("▸ session-{} · {} · {}", entry.short_id, entry.project, entry.title);
        println!("   urgency: {}", entry.urgency);
        for (i, action) in entry.user_actions.iter().enumerate() {
            println!("   {}. {} → {}", i + 1, action.action, action.reason);
        }
        if let Some(decision) = entry.decision {
            println!("   ❓ Confirm: {}", decision);
        }
    }
}
```

`--json` mode bypasses the renderer and pipes the raw JSON for shell
scripting (`contextnest inbox --json | jq '.[] | select(.urgency =="now")'`).

## Success criteria

- Running `contextnest inbox` on a substrate with ingested sessions
  shows actionable items grouped by session, sorted by urgency, in
  <200ms.
- `contextnest inbox --json` returns parseable JSON that matches the
  structure documented in `docs/ingest/inbox.md`.
- Terminal output uses Unicode safely (degrades gracefully when
  `NO_COLOR=1` or piping to a non-TTY).

## What's NOT in scope

- Marking items done from the CLI (no `contextnest inbox done <id>`).
  The way you "complete" a user_action is by doing it — the next
  z-insight emission will reflect the new state.
- Watching mode (`contextnest inbox --watch` that re-renders every
  N seconds). Defer until anyone asks.
- Per-user notification (email / slack when new decisions arrive).
  Different epic.
