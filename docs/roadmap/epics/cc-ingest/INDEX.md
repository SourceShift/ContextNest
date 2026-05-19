# Claude Code session ingest — epic roadmap

The MVP foundation PR (this branch, `feat/claude-code-ingest`) lands the
parsing/extraction pipeline + CLI for batch ingest of Claude Code session
transcripts into ContextNest memory.

Everything below is a follow-up PR. Each has its own spec so it can be
picked up and scoped independently. They're listed in the order they
unlock the most user value per dollar of work.

## The MVP (already covered in this PR)

- `<z-insight>` schema extension documented (`docs/z-insight-schema.md`)
- `metadata_filter` on the `retrieve` HTTP handler
- Rust ingester module: parser + extractor (phase-clustering with 60%
  token overlap) + HTTP sink + dry-run mode
- CLI: `contextnest ingest claude-code [--project | --session-id | --since | --dry-run | --substrate]`
- Integration test against a fixture `.jsonl`

## Follow-up epics

| # | Epic | Why it matters | Effort |
|---|---|---|---|
| 1 | [E-sessions-endpoint.md](E-sessions-endpoint.md) | Surface `/api/v1/sessions/*` (list, detail, attention, find) — the consumer queries that turn stored memories into answers | ~2 days |
| 2 | [E-inbox-cli.md](E-inbox-cli.md) | `contextnest inbox` — the killer terminal experience. Reads the attention endpoint, renders grouped urgency-sorted action list | ~1 day |
| 3 | [E-hook-receiver.md](E-hook-receiver.md) | Real-time `/api/v1/cc/hook/<event>` so Claude hooks fire-and-forget. Replaces batch ingest with live updates per turn | ~2 days |
| 4 | [E-mcp-server.md](E-mcp-server.md) | MCP wrapper so Claude (and Cursor / Aider) can query the inbox natively, not via Bash + curl | ~2 days |
| 5 | [E-dashboard-sessions.md](E-dashboard-sessions.md) | Dashboard routes `/sessions` + `/attention` for humans who prefer a UI over the CLI | ~2 days |
| 6 | [E-embedding-clustering.md](E-embedding-clustering.md) | Replace phase-1 token-overlap clustering with embedding cosine similarity. Better semantic accuracy on goal-pivots | ~1 day |
| 7 | [E-privacy-filter.md](E-privacy-filter.md) | `.cn-ignore` opt-out per project + pattern-redactor for sensitive data in transcripts | ~1 day |

Total deferred work: ~11 days. Each is independently shippable; the
ordering above maximises user value per dollar.

## Sequencing notes

- **#1 (sessions endpoint) is the critical-path follow-up** — every
  downstream consumer (#2 inbox CLI, #3 hook receiver attention surface,
  #4 MCP server, #5 dashboard) depends on it for query patterns.
- **#3 (hook receiver) is independent of #1** structurally; could ship
  before. But the user can't validate that hooks work without the
  query surface from #1, so #1 → #3 in practice.
- **#4 (MCP server) and #5 (dashboard) are siblings** — same data, two
  surfaces. Pick the one that matches your workflow first.
- **#6 (embedding clustering) is purely an upgrade** — phase-1 token
  overlap works; ship #6 once we have feedback on which clusters
  feel wrong.
- **#7 (privacy filter) is on-demand** — until a user reports a
  concrete leak risk, the local-FS trust model is fine for solo dev.

## Versioning

These epics ship as part of v0.2.x patch releases of ContextNest as
they land. v0.3 (the LLM-proxy milestone) is unaffected and runs in
parallel.
