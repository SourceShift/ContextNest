# Application tenants and session memory

ContextNest v2 isolates memory by authenticated application and session. LibWit
uses tenant `libwit`; its persisted interview ID selects the session. Another
application or interview cannot participate in that session's retrieval, graph,
basins, jobs or caches. Coding-agent transcripts remain in the operator store.

Implementation: `src/api/tenants.rs`, `src/services/tenants/`, and LibWit's
`electron/services/memory/`. See the [design](roadmap/epics/tenant-session-memory.md)
and [implementation audit](reports/2026-09-08-tenant-session-implementation-audit.md).
This is a local, trusted application integration. A distributed application needs
its own authenticated backend to hold application credentials.

## Activate the prepared changes

The implementation worktrees are:

- ContextNest: `/Volumes/docker-ssd/Migration/Development/ContextNest-tenant-session-cpu`
- LibWit: `/Volumes/docker-ssd/ps/libwit-contextnest-session-memory`

Private configuration has been prepared under `~/.contextnest/tenant-auth/`.
It has not been activated. The LibWit file contains its application credential;
operator credentials are only in the server/header files. These files stay
outside Git, with mode 600. To prepare another installation, run
`make cn-tenant-config`; it refuses to replace an existing credential set.

When ready, stop the existing ContextNest process in its owning terminal, then
run the following yourself from the ContextNest implementation worktree:

```bash
source ~/.contextnest/tenant-auth/server.env
make cn-serve
```

`cn-serve` always asks Cargo to build the current release source before launch.
It preserves the configured embedding provider; review `config.toml` when
moving to a different worktree. `make cn-run-existing` explicitly skips building.
Do not run two processes against the same checkpoint or tenant database.

Reinstall generated Claude Code hooks with the updated binary so they reference
the operator header file. This command backs up existing settings and updates
ContextNest-generated commands in place:

```bash
./target/release/contextnest ingest claude-code --install-hooks --substrate http://localhost:28080
make cn-curl-health
```

Hooks, the CLI ingestion sink, and `scripts/operator-curl.sh` read
`CONTEXTNEST_OPERATOR_HEADERS`. For local port 28080, they also recognize the
default `~/.contextnest/tenant-auth/operator.headers` path. Custom endpoints
require an explicit header-file configuration. Custom handwritten hooks need
the same header support. Other operator clients, including dashboards and MCP
clients, must send this operator credential when tenant mode is enabled.

In the shell used to start the LibWit implementation worktree:

```bash
source ~/.contextnest/tenant-auth/libwit.env
# Start LibWit using your usual development command from this worktree.
```

Both server and Electron launches remain operator-controlled. The old running
binary does not gain these changes until it is restarted from this worktree.

## Authorization and API

Set `CONTEXTNEST_TENANTS_FILE` to a registration file such as
`config/tenants.example.json`. Each tenant has its own credential, policy and
SQLite database. The operator credential must be different from every tenant
credential. Tenant mode rejects missing/incorrect credentials for every legacy
data route. Anonymous `/api/health` and `/api/status` expose readiness/build
identity without memory counts. With no registration file, legacy-only mode
retains its original local API behavior.

| Request | Credential and result |
|---|---|
| `POST /api/v2/sessions` | App bearer; `{session_id, mode: "create"}` returns a signed session capability. Existing IDs require explicit `mode: "resume"`. |
| `POST /api/v2/memory/store` | Session bearer; stable `source_event_id`, positive `revision`, `content`, optional `importance`/`metadata`. |
| `POST /api/v2/memory/retrieve` | Session bearer; `{query, top_k}`. Returns ready records from that session only. |
| `GET /api/v2/memory/fragments` | Session bearer; ready records from that session only. |
| `GET /api/v2/memory/health` | Session bearer; scalar indexing counts and work metrics for that session. |
| `POST /api/v2/memory/discard` | Session bearer; `{source_event_id}` removes that event's content and canonical references. |
| `POST /api/v2/session/close` | Invalidates the capability and preserves history for explicit resume. |
| `POST /api/v2/session/reset` | Erases session records, advances generation, and requires a new capability via resume. |
| `POST /api/v2/session/delete` | Erases session records and retains a tombstone. That ID cannot be reopened; create a new session. |

Memory routes infer ownership from the verified capability. They reject body
fields such as `tenant_id`, `session_id` or `session_ids`. Knowing another
session ID or fragment ID grants no authority. Legacy tools such as summarize,
reconstruct, field, inbox, hooks and exports require the separate operator
credential and continue to operate on the legacy operator store. They are not
application-tenant APIs and cannot search the new tenant databases.

## Acceptance, retry and recovery

`202 Accepted` means the record and pending indexing work committed to SQLite
with WAL journaling and `synchronous=FULL`. It does not mean the record is ready
for associative recall. An identical delivery of the same event/revision is
idempotent. A different payload at the same revision returns 409; a higher
revision replaces the event and schedules recomputation. A discarded event
cannot be resurrected by another delivery under the same event ID.

One shared scheduler rotates tenants and chooses the least recently served
session within each tenant. A session lease serializes canonical mutations.
Completion rechecks session generation, lease, record revision and canonical
version before committing vectors, graph/basins and readiness together. A
reset, deletion, revision or concurrent discard makes obsolete work fail that
commit check. Transient failures back off with jitter; five unsuccessful
attempts become a visible failed record. Invalid input does not keep retrying.

Ready vectors and canonical snapshots survive restart. Policy changes require
an increased policy version; they invalidate capabilities and rebuild affected
session state. Unchanged embedding spaces reuse persisted vectors. A changed
embedding-space identity requires re-embedding. The configured pipeline version
must be supported by this binary. Retention is a fixed deadline from session
creation, not a sliding deadline; expired data becomes inaccessible immediately
and is cleaned by periodic maintenance.

## Defaults and CPU controls

| Limit | Default |
|---|---:|
| Registered tenants | maximum 32 |
| Sessions per tenant, excluding deleted/expired sessions | 1,000 |
| Records per session | 2,000 |
| Pending/processing records per tenant | 4,096 |
| Content per record | 16 KiB |
| Session retention | 90 days |
| v2 concurrent requests | 32 |
| Concurrent stores/recalls per tenant | 8 |
| Shared application embedding requests | 4 |
| Shared CPU workers | 2 |
| Cached canonical views per tenant | 2 sessions / 64 MiB |
| Graph auto-connections per new record | 32 |

`TenantPolicy` controls retention, record/content/work limits, allowed kinds,
embedding-space identity, pipeline version and graph/basin thresholds. The
initial LibWit kind is `conversation-turn`. Hook/file ingestion is operator-only;
applications cannot ask v2 to read host paths.

The exact scorer uses cached norms and a bounded top-K heap. It preserves exact
scores within the selected session with deterministic ties and rejects invalid
vectors. No approximate index is enabled. Legacy graph search also uses cached
norms/top-K and an incident-edge index; its graph remains global to the operator.
Legacy basin attachment still performs an exact scan over operator basins.

`CONTEXTNEST_CPU_WORKERS` bounds blocking compute (default 2). Legacy
consolidation pauses after every successful batch, with a proportional pause
controlled by `CONTEXTNEST_CONSOLIDATION_DUTY_PERCENT` (default 20) and the
500 ms base interval. This is a soft processing budget, not a hard operating
system CPU limit. Application jobs have a 100 ms successful-job pause.

Operator health caches scalar statistics for five seconds and age distributions
for sixty seconds, with mutation invalidation. It exposes cumulative
`process_cpu_seconds`; compare two samples and their collection times to derive
CPU consumption. Session health separates `processing_ms`, `embedding_ms` and
`queue_wait_ms` totals for completed jobs; queue wait has one-second resolution.
Session metrics do not expose process totals or foreign-session counts.

## Legacy WAL and optional migration

Ordinary startup keeps the existing JSONL operator WAL and adds
`wal.canonical.sqlite` beside it. Completion and canonical state persist together;
transcript offsets and retry budgets survive restart. The first updated startup
can still have a backlog because the old binary did not persist canonical state.

The independent SQLite import tool is optional. It accepts a **copy** of a WAL
and a new output directory, defaults to dry run, and assigns everything to the
explicit `legacy-operator` tenant. It never assigns `project_cwd` transcripts to
LibWit. Cache-only WAL events are reported and skipped. No live migration was
performed by this task.

```bash
cargo run --example migrate_operator_wal -- \
  --wal-copy /absolute/path/to/copied-wal.jsonl \
  --output /absolute/path/to/new-import
```

After reviewing counts and ownership, apply to that new output directory with
`--apply --embedding-space <configured-space-identity>`. Review the generated
registration example before loading the imported tenant. Keep the original WAL
and backup. The prepared LibWit database starts empty on first activation.

## Verification and live acceptance

Local checks are listed in the implementation audit. The offline scorer example
can be replayed with `cargo run --example scoped_exact_benchmark`; its synthetic
timings are not a production CPU claim.

After activation, create interview A and archive a distinctive old turn. Create
interview B and confirm A's turn is absent from its recall and local fallback.
Resume A and confirm its archive is available. Close/reset A while a recall is
pending and confirm the late result does not appear in B. Check scoped failed
counts, LibWit dropped-store counters, operator build identity, and CPU under the
same workload as the original report. Remote embedding latency may exceed the
300 ms recall budget; LibWit then uses its local session fallback.
