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

- **MCP server (Phases 1 → 2 → 3)** — `contextnest mcp serve`
  advertises **12 tools** over stdio JSON-RPC: `cn_store`,
  `cn_retrieve`, `cn_summarize`, `cn_sessions_list`,
  `cn_session_summary`, `cn_session_trajectory`, `cn_inbox`,
  `cn_features`, `cn_prompt_context_atoms`, `cn_attention`,
  `cn_session_get`, `cn_session_find`. Closes
  [`epics/cc-ingest/E-mcp-server.md`](epics/cc-ingest/E-mcp-server.md).
- **Sessions endpoint epic** — `GET /sessions/attention` (per-session
  inbox-eligible aggregate), `GET /sessions/:id` (full grouped detail
  ordered actionable-first), `POST /sessions/find` (NL cosine search
  over goal_phase + session_title). Closes
  [`epics/cc-ingest/E-sessions-endpoint.md`](epics/cc-ingest/E-sessions-endpoint.md).
- **Prompt-context Phases 1a + 1b** —
  `GET /prompt-context/atoms` (deterministic L1 trajectory-atom index
  across every session) and `GET /prompt-context/clusters` (L1.5 dedup
  by normalized text; ~88% compression on a real 7030-atom corpus).
  Deterministic, no LLM — the floor future LLM-distilled capsule
  phases build on.

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
