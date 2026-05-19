//! Claude Code session-transcript adapter.
//!
//! Reads `~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl` files,
//! extracts structured memories from the `<z-insight>` blocks (and
//! fallback paths for sessions without those), and produces
//! [`MemoryRecord`]s ready for any downstream sink.
//!
//! Phase 1 scope (this PR):
//!
//! - Parser: typed JSONL events with permissive deserialize ([`event`])
//! - Extractor: events → MemoryRecords with phase-clustering ([`extractor`])
//! - No CLI / no HTTP sink yet — those land in follow-up commits
//!
//! See [`docs/z-insight-schema.md`](../../../docs/z-insight-schema.md)
//! for the contract this code parses against.

pub mod event;
pub mod extractor;

pub use event::{extract_zinsight_blocks, parse_session_file, RawEvent, SessionMetadata};
pub use extractor::{extract_memories, MemoryKind, MemoryRecord};
