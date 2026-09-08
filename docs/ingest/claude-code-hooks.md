# Real-time Claude Code hook receiver

ContextNest accepts operator-authorized hook events at
`/api/v1/cc/hook/<event>` and tails only newly committed transcript bytes.
This ingestion path belongs to the operator store; it cannot write into an
application tenant. See the [tenant/session repair design](../roadmap/epics/tenant-session-memory.md).

## Install or upgrade

```bash
./target/release/contextnest ingest claude-code --install-hooks --substrate http://localhost:28080
```

The installer backs up existing settings and adds four generated commands:
SessionStart, UserPromptSubmit, Stop and TaskCompleted. Re-running upgrades its
own generated commands in place and preserves unrelated/custom hooks. An exact
second run is idempotent. Explicit project paths add the same commands to those
projects' local settings; no filesystem project scan occurs.

Generated commands drain stdin to a temporary file before backgrounding curl.
The request has a ten-second timeout and bounded connection retries. It loads
an operator header file by reference, so credentials do not appear in settings
or command arguments. For local port 28080 the default is
`~/.contextnest/tenant-auth/operator.headers`; other endpoints require
`CONTEXTNEST_OPERATOR_HEADERS`. Handwritten commands containing the same URL
are preserved and need their own header-file support.

## Event and admission behavior

| Event | Behavior |
|---|---|
| `session_start` | Registers session/path metadata without reading the transcript. |
| `user_prompt_submit`, `stop`, `subagent_stop` | Records the source path and attempts a bounded background tail. |
| `task_completed` | Stores an accomplishment through the same durable sink. |

The tracker admits at most four detached ingestion jobs. It returns 503 when
those slots are full, rather than creating unbounded waiting tasks. A recorded
transcript path remains available to the reconciliation sweep. A 204 is receipt
of a hook, not proof that every record has been durably ingested or indexed.
Unknown events return 404 after authorization.

## Incremental tailing and restart

1. Serialize each transcript and session so hooks and the sweep cannot overlap
   ingestion of the same byte range.
2. Check file identity, length and modification metadata before reading. An
   unchanged file causes zero content reads.
3. Seek to the committed offset and read at most 4 MiB, including a small
   checkpoint fingerprint to detect truncate-and-regrow replacements.
4. Parse through the last complete newline. Retain a partial final line for a
   later pass; handle truncation/replacement by starting a new source position.
5. Advance the committed offset only after extracted records reach the durable
   sink boundary. A failed batch leaves the offset available for retry.

With the normal WAL-enabled server, checkpoints persist beside canonical state
in `wal.canonical.sqlite`. Restart restores offsets; it does not intentionally
re-ingest whole transcripts. A replay after replacement still deduplicates
stable logical records and preserves existing completion metadata. The active
tracker is bounded to 2,048 entries and evicts inactive completed drains; durable
offsets permit later sessions to reattach safely. The reconciliation sweep can
be disabled through `CONTEXTNEST_CC_SWEEPER_ENABLED=false`.

## Failure behavior

| Signal | Meaning and recovery |
|---|---|
| 401 | Tenant mode requires the operator header. Application/session tokens cannot authorize hooks. |
| 503 | Ingestion admission is full; a later hook or sweep retries a recorded transcript. |
| 204 but no new memory | The accepted task may still be processing, or the bytes contain no extractable records. Check ingestion logs and consolidation status. |
| Sink failure | Offset does not advance beyond failed work; identical redelivery is safe. |
| Incomplete last line | Deferred until the next append supplies the newline. |
| Substrate unavailable | Background curl expires/retries within its configured bounds; the foreground Claude turn continues. |
| Settings write failure | Installation reports an error; a pre-existing settings file is backed up first. |

Batch CLI ingestion uses `HttpSink`, which reads the same operator header file
and rejects redirects. In-process hooks use `ServicesSink`: input WAL append is
synced before visibility, while expensive canonical processing runs later under
bounded admission. Application v2 durable acceptance is a separate SQLite path.
