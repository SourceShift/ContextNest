# Epic — Real-time hook receiver

**Path:** `POST /api/v1/cc/hook/<event>`. Mirrors z-dashboard's
`localhost:7048` daemon pattern.

**Depends on:** MVP foundation PR.

**Estimate:** ~2 days.

## What

A new endpoint that Claude Code hooks fire-and-forget into. Returns
204 in <50ms. Does the actual ingest work async in a `tokio::spawn`
task.

Events handled:

| Event | Behaviour |
|---|---|
| `session_start` | Register session in in-memory tracker, no memory writes yet |
| `user_prompt_submit` | **The real-time path.** Tail the `.jsonl` from last byte offset, extract any new z-insight blocks, push as memories |
| `stop` | Final tail + run phase-clustering pass over the full session's z-insight history, write `goal_phase` + `session_title` memories |
| `task_completed` | Store the completed task as an `accomplishment` memory |
| `subagent_stop` | Log only |

Plus a `contextnest ingest claude-code --install-hooks` subcommand
that writes the 5 hook entries into `~/.claude/settings.json` (with a
`.bak` backup of the previous file).

## Why

The MVP gives you batch ingest — you run it once, sessions get
indexed. The real-time path means the moment Claude emits an
`awaiting_decision: true`, the inbox sees it. No "run the ingester
every hour" cron.

## Files touched

| File | Change |
|---|---|
| `src/api/cc_hooks.rs` | New module — endpoint + `SessionTracker` struct (in-memory `HashMap<session_id, byte_offset>`) |
| `src/api/mod.rs` | Mount route |
| `src/ingest/claude_code/mod.rs` | Add `ingest_session_file_since(path, byte_offset)` helper |
| `src/cli/mod.rs` + `src/bin/contextnest.rs` | `--install-hooks` flag |
| `docs/ingest/claude-code-hooks.md` | One-page user-facing install + troubleshooting |
| `tests/cc_hooks_endpoint_test.rs` | 204 response in <50ms, idempotent byte-offset tracking, no double-counting |

## Implementation sketch

```rust
pub async fn hook_handler(
    State(s): State<AppState>,
    Path(event): Path<String>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    // Ack immediately, work async.
    let tracker = s.session_tracker.clone();
    let services = s.services.clone();
    tokio::spawn(async move {
        if let Err(e) = process_hook(event, payload, tracker, services).await {
            tracing::warn!("hook processing failed: {}", e);
        }
    });
    StatusCode::NO_CONTENT
}
```

The tracker is `Arc<RwLock<HashMap<String, u64>>>` keyed by session_id.
Restart loses in-memory state — the substrate's per-fragment hash
dedup absorbs the resulting one-time re-ingest.

## Settings.json snippet (what `--install-hooks` writes)

```json
{
  "hooks": {
    "SessionStart":      [{"hooks": [{"type":"command","command":"curl -s -m 1 -X POST ${CONTEXTNEST_HOOK_URL:-http://localhost:8080}/api/v1/cc/hook/session_start -H 'content-type: application/json' --data-binary @- &"}]}],
    "UserPromptSubmit":  [{"hooks": [{"type":"command","command":"curl -s -m 1 -X POST ${CONTEXTNEST_HOOK_URL:-http://localhost:8080}/api/v1/cc/hook/user_prompt_submit -H 'content-type: application/json' --data-binary @- &"}]}],
    "Stop":              [{"hooks": [{"type":"command","command":"curl -s -m 1 -X POST ${CONTEXTNEST_HOOK_URL:-http://localhost:8080}/api/v1/cc/hook/stop -H 'content-type: application/json' --data-binary @- &"}]}],
    "TaskCompleted":     [{"hooks": [{"type":"command","command":"curl -s -m 1 -X POST ${CONTEXTNEST_HOOK_URL:-http://localhost:8080}/api/v1/cc/hook/task_completed -H 'content-type: application/json' --data-binary @- &"}]}]
  }
}
```

The `${CONTEXTNEST_HOOK_URL:-http://localhost:8080}` pattern lets the
user override without re-editing settings.json (e.g.
`CONTEXTNEST_HOOK_URL=http://localhost:28080`).

## Success criteria

- `POST /api/v1/cc/hook/user_prompt_submit` returns 204 in <50ms
  (measured at p95 over 1000 requests).
- After `--install-hooks` writes settings.json, the next session's
  z-insight blocks appear in `/api/v1/sessions/attention` within ~1s
  of the assistant turn completing.
- Idempotent: posting the same hook event twice doesn't double-store
  memories.
- Daemon unreachable: hook command's curl `-m 1` absorbs the timeout
  without breaking Claude.

## What's NOT in scope

- Disk persistence of byte offsets (in-memory is enough for v0.2).
  Add `--persist-offsets <path>` in a follow-up if real-world testing
  shows it matters.
- Queue-on-unreachable fallback (write to `~/.contextnest/hook-queue.jsonl`
  when substrate is down). The hook command already absorbs the
  failure; users who need durability can add this in a follow-up.
- Authentication on the hook endpoint. localhost-only by default;
  remote deployments expose it via reverse-proxy auth.
