//! Claude Code session-transcript adapter.
//!
//! Reads `~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl` files,
//! extracts structured memories from the `<z-insight>` blocks (and
//! fallback paths for sessions without those), and pushes them to a
//! [`Sink`].
//!
//! Public entry points:
//!
//! - [`discover_sessions`] — walk a projects directory, return matching
//!   `.jsonl` paths filtered by `--project` substring and `--since` mtime.
//! - [`ingest_session_file`] — parse one `.jsonl`, extract memories, push
//!   them via a [`Sink`]. Returns a [`SinkReport`] with success/fail
//!   counts + per-kind tallies.
//! - [`extract_zinsight_blocks`], [`extract_memories`] — lower-level
//!   helpers for callers that want to do their own I/O (e.g. a future
//!   hook-receiver endpoint that reads from a byte offset rather than
//!   from a full file).
//!
//! See [`docs/z-insight-schema.md`](../../../docs/z-insight-schema.md)
//! for the schema this code parses against.

pub mod event;
pub mod extractor;
pub mod sink;

use crate::error::{ContextNestError, ContextNestResult};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub use event::{extract_zinsight_blocks, parse_session_file, RawEvent, SessionMetadata};
pub use extractor::{extract_memories, MemoryKind, MemoryRecord};
pub use sink::{DryRunSink, HttpSink, ServicesSink, Sink, SinkReport};

/// One discovered Claude Code session on disk.
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    /// Absolute path to the `.jsonl` file.
    pub jsonl_path: PathBuf,
    /// The decoded project cwd reconstructed from the directory name.
    /// e.g. `-Volumes-docker-ssd-Migration-Development-ContextNest` →
    /// `/Volumes/docker-ssd/Migration/Development/ContextNest`.
    pub project_cwd: String,
    /// The session UUID parsed from the filename.
    pub session_uuid: String,
    /// File last-modified timestamp. Drives the `--since` filter.
    pub modified: Option<SystemTime>,
    /// File size in bytes — printed during discovery for operator
    /// situational awareness.
    pub size_bytes: u64,
}

/// Walk a Claude Code projects directory (typically `~/.claude/projects/`)
/// and return every `<project-dir>/<uuid>.jsonl` it finds.
///
/// `project_filter` is a case-insensitive substring match against the
/// reconstructed project cwd. Pass `None` to include every project.
///
/// `since` filters by file mtime — only sessions modified at-or-after
/// this point are returned. Pass `None` to include everything.
///
/// Results are sorted newest-first by mtime (when available) so callers
/// that take the first N get the most recent work.
pub fn discover_sessions(
    projects_dir: &Path,
    project_filter: Option<&str>,
    since: Option<SystemTime>,
) -> ContextNestResult<Vec<DiscoveredSession>> {
    let project_filter_lc = project_filter.map(|s| s.to_lowercase());

    let entries = std::fs::read_dir(projects_dir).map_err(|e| {
        ContextNestError::Api(format!(
            "discover_sessions: cannot read projects dir {}: {}",
            projects_dir.display(),
            e
        ))
    })?;

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(s) => s,
            None => continue,
        };
        let project_cwd = decode_project_dir_name(dir_name);

        if let Some(f) = &project_filter_lc {
            if !project_cwd.to_lowercase().contains(f) {
                continue;
            }
        }

        let session_entries = match std::fs::read_dir(&path) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for sess in session_entries.flatten() {
            let sess_path = sess.path();
            if !sess_path.is_file() {
                continue;
            }
            let ext_ok = sess_path
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x == "jsonl")
                .unwrap_or(false);
            if !ext_ok {
                continue;
            }

            let metadata = match std::fs::metadata(&sess_path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let modified = metadata.modified().ok();

            if let Some(since_t) = since {
                if let Some(mt) = modified {
                    if mt < since_t {
                        continue;
                    }
                }
            }

            let session_uuid = sess_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            out.push(DiscoveredSession {
                jsonl_path: sess_path,
                project_cwd: project_cwd.clone(),
                session_uuid,
                modified,
                size_bytes: metadata.len(),
            });
        }
    }

    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(out)
}

/// Read one `.jsonl`, extract memories, push them via the given sink.
///
/// Returns a [`SinkReport`] with success/fail counts + a per-kind tally.
pub async fn ingest_session_file<S: Sink + ?Sized>(
    session: &DiscoveredSession,
    sink: &S,
) -> ContextNestResult<SinkReport> {
    let (events, metadata) = parse_session_file(&session.jsonl_path)?;

    let session_uuid = if !session.session_uuid.is_empty() {
        session.session_uuid.as_str()
    } else {
        metadata.session_uuid.as_deref().unwrap_or("unknown")
    };
    let project_cwd = if !session.project_cwd.is_empty() {
        session.project_cwd.as_str()
    } else {
        metadata.cwd.as_deref().unwrap_or("")
    };

    let records = extract_memories(&events, session_uuid, project_cwd);
    sink.store_batch(&records).await
}

/// Reconstruct a project cwd from its `~/.claude/projects/<name>` dir
/// name. Claude Code encodes the absolute path by replacing `/` with `-`
/// (and prefixing with `-`).
///
/// `-Volumes-docker-ssd-Migration-Development-ContextNest`
///   → `/Volumes/docker-ssd/Migration/Development/ContextNest`
///
/// The encoding is lossy — a real dash in a path component becomes
/// indistinguishable from the path separator after encoding. We accept
/// this; reconstructed paths are best-effort and only used for filter
/// matching, never as the canonical project identifier (the session
/// UUID is canonical).
pub fn decode_project_dir_name(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    let stripped = name.strip_prefix('-').unwrap_or(name);
    format!("/{}", stripped.replace('-', "/"))
}

/// Convenience: parse a human-friendly duration like "7d", "24h", "30m"
/// into a `Duration`. Used by the `--since` CLI flag. Returns None for
/// invalid input.
pub fn parse_since(input: &str) -> Option<std::time::Duration> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let (num_part, unit) = input.split_at(input.len() - 1);
    let n: u64 = num_part.parse().ok()?;
    let secs = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 60 * 60,
        "d" => n * 60 * 60 * 24,
        "w" => n * 60 * 60 * 24 * 7,
        _ => return None,
    };
    Some(std::time::Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_typical_macos_path_is_lossy_on_dashes() {
        // The encoding replaces `/` with `-` — dashes inside path
        // components (here: `docker-ssd`) are unrecoverable. This
        // matches the docstring's warning; reconstructed paths are
        // best-effort and only used for filter substring matching,
        // never as the canonical project identifier.
        let n = "-Volumes-docker-ssd-Migration-Development-ContextNest";
        let decoded = decode_project_dir_name(n);
        assert!(decoded.starts_with("/Volumes/"));
        assert!(decoded.contains("Migration"));
        assert!(decoded.contains("ContextNest"));
    }

    #[test]
    fn decode_short_path() {
        assert_eq!(decode_project_dir_name("-Users-admin"), "/Users/admin");
    }

    #[test]
    fn decode_empty_returns_empty() {
        assert_eq!(decode_project_dir_name(""), "");
    }

    #[test]
    fn parse_since_handles_common_units() {
        use std::time::Duration;
        assert_eq!(parse_since("7d"), Some(Duration::from_secs(7 * 86400)));
        assert_eq!(parse_since("24h"), Some(Duration::from_secs(24 * 3600)));
        assert_eq!(parse_since("30m"), Some(Duration::from_secs(30 * 60)));
        assert_eq!(parse_since("45s"), Some(Duration::from_secs(45)));
        assert_eq!(parse_since("2w"), Some(Duration::from_secs(2 * 7 * 86400)));
    }

    #[test]
    fn parse_since_rejects_bad_input() {
        assert_eq!(parse_since(""), None);
        assert_eq!(parse_since("abc"), None);
        assert_eq!(parse_since("7x"), None); // unknown unit
        assert_eq!(parse_since("d"), None); // no number
    }
}
