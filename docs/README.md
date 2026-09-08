# ContextNest Documentation

Curated entry points for understanding and using ContextNest.

## Core reading

- [**Application tenant and session setup**](tenant-session-memory.md) — activation,
  operator authentication, LibWit sessions, CPU controls and durable recovery.

- [**architecture.md**](architecture.md) — how the substrate works under the
  seven-tool API. Mental model, sequence diagrams for each tool, why
  neural-field attractors beat a flat vector store, concurrency contract.
- [**usage.md**](usage.md) — practical how-to with copy-paste curl examples
  for every tool, end-to-end demo, integration recipes (Python, bash,
  Claude Code hooks), and a troubleshooting matrix.
- [**extensibility.md**](extensibility.md) — plug any LLM or embedding
  provider. Built-in support for Anthropic / OpenAI / Google plus any
  OpenAI-shaped endpoint (Ollama, LiteLLM, vLLM, OpenRouter, Together,
  Groq, Mistral) and three embedding providers (Ollama, Hugging Face,
  generic HTTP for Voyage / Cohere / Jina / Mistral / TEI). Custom
  provider trait for anything else.

## Where this is going

- [**Tenant and session memory repair design**](roadmap/epics/tenant-session-memory.md)
  — application tenants, isolated LibWit sessions, and a concrete repair for
  each diagnosed CPU issue; implementation and local verification are recorded in the audit.

- [**CPU investigation: `make cn-serve`**](reports/2026-09-08-cn-serve-cpu.md)
  — live profiling, graph-scan costs, transcript and health-polling overhead,
  and proposed fixes (2026-09-08).

- [**roadmap/**](roadmap/) — forward-looking design + sequencing. Strategic
  five-milestone plan (v0.2 → v1.0), shipped neural-field/runtime
  reconciliation notes, and concrete specs for upcoming milestones.

## Repository docs

- [../README.md](../README.md) — project overview, quickstart, badges
- [../CONTRIBUTING.md](../CONTRIBUTING.md) — canonical pipeline + how to
  add a new tool to the seven-tool API
- [../SECURITY.md](../SECURITY.md) — responsible-disclosure contact
- [../CHANGELOG.md](../CHANGELOG.md) — release notes

## Auto-generated API docs

```bash
cargo doc --no-deps --open
```

Public docs.rs build: <https://docs.rs/contextnest>
