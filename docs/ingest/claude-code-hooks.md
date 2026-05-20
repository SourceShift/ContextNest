# Real-time Claude Code hook receiver

ContextNest exposes a small set of HTTP endpoints under
`/api/v1/cc/hook/<event>` that Claude Code's hook framework can POST to
on every session event. The substrate then ingests the new turn's
`<z-insight>` block in milliseconds — so `contextnest inbox`,
`/api/v1/tools/retrieve`, and the dashboard always reflect Claude's
*current* state, not what was true at the last batch ingest.

```
   Claude Code session ──hook──> POST /api/v1/cc/hook/<event>
        (writes .jsonl)         │
                                ▼
                          204 No Content              ─── hook returns to Claude in ~1ms
                                │
                                └── tokio::spawn ──> tail .jsonl
                                                      from last byte offset
                                                      ↓
                                                    parse new events
                                                      ↓
                                                    extract memories
                                                      ↓
                                                    ServicesSink → substrate
```

The receiver is a thin wrapper over the same `extract_memories` +
`Sink::store_batch` pipeline the batch ingester (`contextnest ingest
claude-code`) uses. **One extraction code path, two transports.**

## Install the four hooks

```bash
contextnest ingest claude-code --install-hooks
# or, pointing at a non-default substrate URL:
contextnest ingest claude-code --install-hooks --substrate http://localhost:28080
```

The command:

1. Reads your existing `~/.claude/settings.json` (or `{}` if absent).
2. Backs it up to `~/.claude/settings.json.bak-<unix-ts>`.
3. **Appends** four hook entries to `hooks.SessionStart`,
   `hooks.UserPromptSubmit`, `hooks.Stop`, and `hooks.TaskCompleted`.
   Existing entries (e.g. `claude-status-writer`, `z-dashboard`) stay
   in place.
4. Idempotent: re-running detects existing ContextNest entries by their
   `/api/v1/cc/hook/` URL substring and skips them. Changing
   `--substrate` and re-running will append a new entry — that's
   intentional so a user pointing at a new substrate gets a fresh hook
   without losing the old one.

Each entry has the shape:

```json
{
  "hooks": [
    {
      "type": "command",
      "command": "curl -s -m 1 -X POST http://localhost:8080/api/v1/cc/hook/user_prompt_submit -H 'content-type: application/json' --data-binary @- &"
    }
  ]
}
```

The trailing `&` is what makes the hook fire-and-forget from Claude's
perspective. The 1-second curl timeout caps Claude's blocking time
even in pathological cases where the substrate hangs.

## Per-event semantics

| Event | Endpoint | What the receiver does |
|---|---|---|
| `SessionStart` | `/api/v1/cc/hook/session_start` | Register session in the in-memory `SessionTracker` at byte offset 0. No writes. |
| `UserPromptSubmit` | `/api/v1/cc/hook/user_prompt_submit` | Tail the `.jsonl` from the last byte offset, extract memories from new events, push via `ServicesSink`. The "real-time" path. |
| `Stop` | `/api/v1/cc/hook/stop` | Same tail-and-push behaviour. Acts as a safety net after the final turn of a group. |
| `TaskCompleted` | `/api/v1/cc/hook/task_completed` | Store one `accomplishment` memory built from the payload's `subject` / `task_id`. No transcript read. |
| `SubagentStop` | `/api/v1/cc/hook/subagent_stop` | Reserved. Logged only for now. |

Unknown event names return `404 Not Found` — a cheap signal that
something misspelled the hook URL during installation.

## Byte-offset tailing

The receiver keeps a `session_id → byte_offset` map in memory (per
`SessionTracker`). On each `user_prompt_submit` or `stop` it:

1. Reads the whole `.jsonl` file (small — sessions are typically a few
   MB at most).
2. Slices off the bytes already processed.
3. If the slice doesn't start at a line boundary (rare; only happens if
   the file was externally truncated), advances to the next `\n`.
4. Parses just that slice through `parse_session_string`.
5. Bumps the offset to the new full file length.

This means **the next hook for the same session only sees new content**.
Re-firing the same hook with no new bytes is a no-op.

### Restart behaviour

The tracker lives in process memory and is **wiped on substrate
restart**. The next hook for a session that existed before the restart
re-ingests from byte 0 — so you get one session's worth of memory
duplicates the first time around. This is intentional: persisting
offset state to disk is fragile (rename races, partial writes, file
permissions) and the substrate's own retrieval is already
duplicate-tolerant. If duplicates become a real problem, add content-
hash dedup at the `store` handler level rather than persistent offset
state.

## Failure modes

| Symptom | Cause | What happens |
|---|---|---|
| Hook returns 204, no memory appears | Substrate-side store error (e.g. embedding service down) | Logged at `warn`. Next hook re-tails from the bumped offset, so the failed batch is dropped — that's a substrate-level alarm, not a hook concern. |
| Tail reads but ingests zero memories | `.jsonl` chunk has no `<z-insight>` blocks (and no `ai-title`) | Normal — most turns don't carry z-insight. Offset still gets bumped. |
| 404 on a hook URL | Misspelled event in `settings.json` | Returns `404 Not Found`; hook stays installed but does nothing. Reinstall with the right name. |
| Substrate not running | curl times out after 1s | Hook silently fails; Claude's loop is unaffected. Next session start re-tails everything. |
| Settings file write fails | Permissions / disk full / parent missing | `--install-hooks` exits with a user-facing error before touching anything. The backup file is also written first, so a partial install is detectable from the timestamp. |

## How this composes with the rest of v0.2

```
            ┌─────────────────────────────────────────────────┐
            │  Claude Code session (writes .jsonl per turn)   │
            └─────────────────────────────────────────────────┘
                              │
                ┌─────────────┼─────────────┐
                │                           │
                ▼                           ▼
        BATCH:                       REAL-TIME (this PR):
   contextnest ingest             curl POST /api/v1/cc/hook/<event>
   claude-code                              │
        │                                   │
        └──────────────► extract_memories ◄─┘
                              │
                              ▼
                       ServicesSink / HttpSink
                              │
                              ▼
                ┌─────────────────────────────┐
                │  /api/v1/tools/store        │   (canonical write path)
                │  with metadata{kind, urgency,│
                │  awaiting_decision, ...}    │
                └─────────────────────────────┘
                              │
            ┌─────────────────┼─────────────────┐
            ▼                                   ▼
   /api/v1/tools/retrieve            contextnest inbox
   + metadata_filter                  (reads via retrieve)
```

The batch path (`contextnest ingest claude-code`) is still useful for
backfilling history. The hook receiver is what keeps the substrate
fresh on every turn going forward. They write through the same sink
abstraction and produce the same metadata shape, so any reader
(`contextnest inbox`, the dashboard, an MCP client) sees both equally.
