# ContextNest Documentation

This directory is reserved for design notes and architecture write-ups that
augment the inline rustdoc on the public crate.

For the current v0.1.0 surface, see:

- [`../README.md`](../README.md) — quickstart, seven-tool API, LLM provider
  config, build + test commands.
- [`../CONTRIBUTING.md`](../CONTRIBUTING.md) — canonical pipeline (`store →
  retrieve → reconstruct`), how to add a tool, and CI gates.
- [`../SECURITY.md`](../SECURITY.md) — responsible-disclosure contact.
- [`../CHANGELOG.md`](../CHANGELOG.md) — release notes.

Auto-generated API docs (after `cargo doc --no-deps --open`) live in
`target/doc/contextnest/`; the public docs.rs build is at
<https://docs.rs/contextnest>.
