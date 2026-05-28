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
  trajectories.
- [**v0.5-trajectory-substrate-aware-cards.md**](v0.5-trajectory-substrate-aware-cards.md)
  Four trajectory-card upgrades (basin badge, resonance strip, promotion
  clusters, heat-weighted sort) each grounded in a named substrate
  primitive from `docs/architecture.md`.

## Future placeholders

Specs for v0.4 (semantic storage), v0.5 (`RESONATE WITH` SQL
extension), and v1.0 (auth + functions + deploy + realtime) will land
here as each upstream milestone gates them.

## How to propose a change

- Open an issue tagged `roadmap` outlining the change and the
  hypothesis it validates.
- If accepted, draft a `vX.Y-<feature>.md` here following the format
  of [v0.3-llm-proxy.md](v0.3-llm-proxy.md): status, dependencies,
  TL;DR, user-facing surface, architecture (with mermaid diagram),
  security model, phases, success metrics, open questions.
- Reviewable artifact lands first; PRs that implement it land second.
