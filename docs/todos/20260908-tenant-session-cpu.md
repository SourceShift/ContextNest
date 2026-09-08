# Apply CPU repairs and tenant/session isolation

Status: implementation complete; operator activation and live acceptance pending. Last worked on: 2026-09-08.

Requirements: [accepted design](../roadmap/epics/tenant-session-memory.md),
[CPU evidence](../reports/2026-09-08-cn-serve-cpu.md), and the repository development workflow.

- [x] Authenticated application tenants, session capabilities, and operator-only legacy routes.
- [x] Exact scoped graph/basin processing, validated vectors, bounded top-K, and cheap health.
- [x] Durable idempotent acceptance, bounded fair workers, cancellation/generation checks.
- [x] SQLite canonical state and completion transactions, restart/deletion/revision recovery.
- [x] Legacy CPU pacing, incremental serialized transcript tailing, duplicate protection, durable checkpoints.
- [x] LibWit session lifecycle integration, bounded abortable client, and local fallback preservation.
- [x] Source-aware make targets and build provenance.
- [x] Focused tests, canonical checks, and two requirements/documentation audits.

Live process restart and live-data migration are operator actions. Validation uses temporary stores and copied fixtures.
Approximate indexing is conditional on evidence of a large session exceeding the exact-search budget; it is not enabled by this task.

## Operator replay

Status: pending. Last worked on: 2026-09-08 (procedure prepared).

- [ ] Restart ContextNest and LibWit from the implementation worktrees using the
  [activation guide](../tenant-session-memory.md).
- [ ] Verify real session A/B isolation, resume and cancellation; collect live
  CPU/latency under concurrent traffic. Remaining parts: the operator replay.

Both implementation requirement passes and their evidence are recorded in
[the audit](../reports/2026-09-08-tenant-session-implementation-audit.md).
