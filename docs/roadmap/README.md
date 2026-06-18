# ContextNest Roadmap

Forward-looking design and sequencing documents. Each entry here is a
proposal, not committed scope. Concrete code work begins only when the
referenced epic spec is approved and tracked through CHANGELOG.

## Active

- [**v0.2-to-v1.0.md**](v0.2-to-v1.0.md) — five-milestone strategic
  roadmap from current v0.1.0 substrate to the v1.0 feature-complete
  agent-native backend. Read this first for direction.
- [**v0.3-llm-proxy.md**](v0.3-llm-proxy.md) — concrete spec for the
  self-caching LLM proxy. The cheapest first milestone after v0.2;
  proves the memory-as-substrate thesis applied to a single primitive.
- [**v0.4-z-insight-trajectory-signals.md**](v0.4-z-insight-trajectory-signals.md)
  Transcript-derived proposal for richer `z-insight` fields that let
  ContextNest build evidence-linked prompt capsules from long-term Claude
  trajectories. **Phases 1a + 1b shipped** — see "Recently shipped" below.
- [**v0.4-trajectory-prompt-context-implementation.md**](v0.4-trajectory-prompt-context-implementation.md)
  Implementation companion to the z-insight spec — the full L0–L3
  compression spectrum (passive trajectory index → distilled capsules
  → injection harness). Phases 1a + 1b shipped as a deterministic L1 +
  L1.5 read surface; later phases earn the LLM call once the
  deterministic floor proves itself.
- [**v0.5-trajectory-substrate-aware-cards.md**](v0.5-trajectory-substrate-aware-cards.md)
  Four trajectory-card upgrades (basin badge, resonance strip, promotion
  clusters, heat-weighted sort) each grounded in a named substrate
  primitive from `docs/architecture.md`.
## Recently shipped

Concrete code now in `main`. Listed here so contributors can see what
the substrate currently exposes without re-reading the commit log.

- **MCP server (Phases 1 → 4)** — `contextnest mcp serve` advertises
  **14 tools** over stdio JSON-RPC. Phases 1–3 (`cn_store`,
  `cn_retrieve`, `cn_summarize`, `cn_sessions_list`,
  `cn_session_summary`, `cn_session_trajectory`, `cn_inbox`,
  `cn_features`, `cn_prompt_context_atoms`, `cn_attention`,
  `cn_session_get`, `cn_session_find`) closed
  [`epics/cc-ingest/E-mcp-server.md`](epics/cc-ingest/E-mcp-server.md);
  Phase 4 added `cn_prompt_context_clusters` and
  `cn_prompt_context_capsule` after the corresponding HTTP endpoints
  landed.
- **Sessions endpoint epic** — `GET /sessions/attention` (per-session
  inbox-eligible aggregate), `GET /sessions/:id` (full grouped detail
  ordered actionable-first), `POST /sessions/find` (NL cosine search
  over goal_phase + session_title). Closes
  [`epics/cc-ingest/E-sessions-endpoint.md`](epics/cc-ingest/E-sessions-endpoint.md).
- **v0.4 prompt-context surface — Phases 1a → 1c → 2** —
  `GET /prompt-context/atoms` (deterministic L1 trajectory-atom index
  across every session), `GET /prompt-context/clusters` (L1.5 dedup by
  normalized text; ~88% compression on a real 7030-atom corpus),
  `GET /prompt-context/capsule` (L1.5 Markdown digest with kind-priority
  ordering — `Risks → Decisions → Failures → Verifications → ...` —
  for paste-into-prompt workflows), plus an opt-in
  `?semantic=true` paraphrase merge that reuses each fragment's
  already-stored embedding to collapse near-duplicates at cosine ≥ 0.85.
  Deterministic floor remains the default; semantic merge gracefully
  degrades to deterministic when fragments aren't yet hydrated.
- **Prose-shaped views across HTTP + MCP + CLI** — three substrate
  read endpoints now support `?format=markdown` with `text/markdown;
  charset=utf-8` bodies designed for paste-into-prompt and pipe-into-
  pbcopy workflows: `/prompt-context/capsule`, `/features` (`What did
  I miss while away`), and `/inbox` (`What needs my attention`, grouped
  by urgency). Each has a matching MCP tool parameter and a `contextnest
  <verb> [--markdown|--json]` CLI flag. See "Prose-shaped views" below.
- **Neural-field substrate runtime reconciliation** — the
  [`epics/neural-field-real.md`](epics/neural-field-real.md) roadmap is
  complete in code. The runtime now has a background consolidation worker,
  decay/recency scoring, real attractor basins at `/field/basins`,
  basin and connection-aware retrieval expansion, chain-query
  auto-reconstruction, `/api/v1/substrate/health`, and the
  `make cn-curl-health` operator shortcut. The grep-verification recipe
  lives in [`../architecture-honest.md`](../architecture-honest.md).

## Prose-shaped views convention

Three current-state read endpoints follow the same access-pattern
triangle so paste-into-prompt workflows compose identically regardless
of consumer type:

| Endpoint | HTTP | MCP tool | CLI |
|---|---|---|---|
| `/prompt-context/capsule` | `?format=markdown` (default md) | `cn_prompt_context_capsule` | `contextnest prompt-context capsule` |
| `/features` | `?format=markdown` (default json) | `cn_features` | `contextnest features` (default md, `--json` toggles) |
| `/inbox` | `?format=markdown` (default json) | `cn_inbox` | `contextnest inbox --markdown` (default terminal text) |

**Substrate rules** (new endpoints should follow these to stay consistent):

1. **HTTP** — accept `?format=markdown` (or `md`); return
   `text/markdown; charset=utf-8`. Unknown `format` values fall through
   to JSON. JSON contract must be unchanged when `format` is omitted.
2. **MCP** — advertise `format` in `inputSchema.properties`; forward it
   via `collect_query` so an agent passing
   `{"format": "markdown"}` gets the Markdown body verbatim through the
   shared `get()` helper's JSON-or-raw-text fallback. **Invariant test
   pattern**: every MCP tool that uses `collect_query` should ship a
   `<tool>_def_advertises_*` test asserting every key the handler
   forwards also appears in `inputSchema.properties` — without it, an
   MCP agent inspecting the schema can't discover the option even when
   the handler accepts it.
3. **CLI** — Markdown rendering lives in the CLI's renderer module
   alongside `render_text` / `render_json` (or wrapping the substrate's
   `?format=markdown` body directly when the CLI does no local
   aggregation). The CLI flag should be either `--markdown` (additive,
   default = legacy) or default Markdown with `--json` toggling
   (depending on whether prose or structure is the dominant workflow
   for that endpoint).
4. **Body shape** — group by the endpoint's natural priority axis:
   urgency for `/inbox`, kind-priority for `/capsule`, chronological-
   newest-first for `/features`. Each item carries a one-line meta
   tail with kind / session-short / project (last-2 components) / ts.
5. **Truncation** — multibyte-safe char-count truncation at ~240–280
   chars per item with an ellipsis. Local helper named `truncate_md`
   or `truncate_preview` per module; cross-module extraction is worth
   doing once a fourth caller appears (three exist today and the
   duplication cost is < the abstraction cost).

## Future placeholders

Specs for **v0.7 (semantic storage)**, **v0.8 (`RESONATE WITH` SQL
extension)**, and **v1.0 (auth + functions + deploy + realtime)** will
land here as each upstream milestone gates them. The version numbers
above were previously printed as v0.4 / v0.5 / v1.0 in this README;
bumped to avoid collision with the shipped v0.4 / v0.5 specs.

## How to propose a change

- Open an issue tagged `roadmap` outlining the change and the
  hypothesis it validates.
- If accepted, draft a `vX.Y-<feature>.md` here following the format
  of [v0.3-llm-proxy.md](v0.3-llm-proxy.md): status, dependencies,
  TL;DR, user-facing surface, architecture (with mermaid diagram),
  security model, phases, success metrics, open questions.
- Reviewable artifact lands first; PRs that implement it land second.
