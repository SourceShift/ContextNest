# ContextNest Documentation

Curated entry points for understanding and using ContextNest.

## Core reading

- [**architecture.md**](architecture.md) — how the substrate works under the
  seven-tool API. Mental model, sequence diagrams for each tool, why
  neural-field attractors beat a flat vector store, concurrency contract.
- [**usage.md**](usage.md) — practical how-to with copy-paste curl examples
  for every tool, end-to-end demo, integration recipes (Python, bash,
  Claude Code hooks), and a troubleshooting matrix.

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
