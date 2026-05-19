//! Adapters that pull external session data into the substrate.
//!
//! Each adapter is a leaf-level submodule with the same three-layer shape:
//!
//! 1. **Parser** — domain-specific deserialization (e.g. `.jsonl` for
//!    Claude Code). Permissive: unknown fields land in a loose
//!    [`serde_json::Value`] rather than failing the parse, because external
//!    schemas drift between releases.
//! 2. **Extractor** — turns typed events into a `Vec<MemoryRecord>`. This
//!    is where the domain knowledge lives (z-insight block parsing,
//!    phase-clustering, todo dedup).
//! 3. **Sink** — pushes records into the substrate. Either via the HTTP
//!    seven-tool API (when running against a live substrate) or via direct
//!    in-process calls (when embedded as a library). A `DryRun` variant
//!    prints records instead of storing them.
//!
//! See [`claude_code`] for the canonical implementation.

pub mod claude_code;

pub use claude_code::{MemoryKind, MemoryRecord};
