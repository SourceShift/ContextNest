//! Write-ahead log for the substrate's mutating operations.
//!
//! The substrate's in-memory state (canonical fragments in
//! [`crate::memory::attractors::MemoryAttractorManager`] plus the three
//! sidecars: `fragment_texts`, `fragment_metadata`,
//! [`crate::services::session_index::SessionIndex`]) is fully ephemeral —
//! a process restart wipes everything. This WAL is the persistence layer:
//! every successful mutating API call appends a record here, and on
//! startup the records are replayed against a fresh
//! [`crate::services::ContextNestServices`] to reconstruct state.
//!
//! ## Design
//!
//! - **Append-only JSONL.** One record per line, no in-place updates,
//!   no log compaction (yet). Trivially `tail -f`-able, `jq`-able, and
//!   resyncable.
//! - **Flush-per-write.** Each `append` calls `flush()` so that an HTTP
//!   201 response from the substrate is backed by bytes on disk. Without
//!   the flush, an OS/process crash between the response and a periodic
//!   fsync would lose acknowledged writes — silently.
//! - **Best-effort durability over fancy invariants.** No checksums, no
//!   double-write barriers. If a single record is corrupted on disk,
//!   replay logs a warning and skips it — better to come up with N-1
//!   fragments than refuse to start.
//! - **Replay tolerates re-replay.** Records carry the original
//!   `fragment_id`, so replay re-uses the same IDs. The sidecar maps
//!   are idempotent on insert with the same key; the SessionIndex `add`
//!   is documented as idempotent + restore-from-soft-delete.
//!
//! ## What's covered (v0.1)
//!
//! Only `Store` records. `Update` and `Discard` are reserved variants
//! that the handlers don't yet emit. This is deliberate: the immediate
//! use case is "ingest backfill survives restart", which only exercises
//! `store`. Adding the other two operations is a follow-up — the file
//! format already supports it via the `op` tag.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// One WAL entry. Tagged externally on `op` so the file format is
/// self-describing and extensible.
///
/// Forward-compatibility note: new variants can be appended at any time;
/// older readers that don't recognise an `op` value will skip that record
/// via the `serde::Deserialize` failure path in [`Wal::read_records`]
/// (which logs a warning and continues). When you add a new variant,
/// keep existing variants' field set stable — replay must remain
/// deterministic across binary versions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WalRecord {
    /// A successful `POST /api/v1/tools/store`. Carries every input the
    /// handler needs to reproduce the same canonical fragment +
    /// sidecar entries.
    Store {
        fragment_id: String,
        session_id: String,
        content: String,
        importance: f32,
        #[serde(default)]
        metadata: HashMap<String, serde_json::Value>,
    },
}

/// Append-only WAL writer. Construct via [`Wal::open_for_append`] *after*
/// any replay has completed.
pub struct Wal {
    path: PathBuf,
    writer: Mutex<BufWriter<File>>,
}

impl std::fmt::Debug for Wal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wal").field("path", &self.path).finish()
    }
}

impl Wal {
    /// Open (or create) the WAL file at `path` for append-only writes.
    /// Parent directories are created if missing. Existing content is
    /// preserved — this is the writer side of the same file
    /// [`Self::read_records`] consumes.
    pub fn open_for_append(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            path,
            writer: Mutex::new(BufWriter::new(file)),
        })
    }

    /// Append one record + flush. Returns once bytes are buffered to the
    /// kernel — `flush()` does NOT call `fsync`, so a sudden power loss
    /// could still drop the last few records. Adding fsync-per-record
    /// would halve throughput on rotating disks; the current trade-off
    /// favours throughput because the substrate is a cache of source
    /// `~/.claude/projects/` JSONL files anyway.
    pub fn append(&self, record: &WalRecord) -> std::io::Result<()> {
        let line = serde_json::to_string(record)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut w = self.writer.lock().expect("wal writer mutex poisoned");
        writeln!(w, "{line}")?;
        w.flush()?;
        Ok(())
    }

    /// Path this writer is appending to. Useful for log lines on startup.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read every record from `path`, in file order. Returns `Ok(vec![])`
    /// when the file doesn't exist (cold start case). Malformed lines
    /// are logged at warn-level and skipped — we never refuse to come
    /// up because of one bad record.
    pub fn read_records(path: &Path) -> std::io::Result<Vec<WalRecord>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let f = File::open(path)?;
        let reader = BufReader::new(f);
        let mut out = Vec::new();
        for (idx, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<WalRecord>(&line) {
                Ok(rec) => out.push(rec),
                Err(e) => {
                    tracing::warn!(
                        line_no = idx + 1,
                        error = %e,
                        "wal: skipping malformed record"
                    );
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn sample_store(id: &str) -> WalRecord {
        WalRecord::Store {
            fragment_id: id.to_string(),
            session_id: "cc-test".to_string(),
            content: format!("content for {id}"),
            importance: 0.7,
            metadata: HashMap::from([
                ("kind".to_string(), json!("user_action")),
                ("urgency".to_string(), json!("now")),
            ]),
        }
    }

    #[test]
    fn append_then_read_roundtrips_records_in_order() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wal.jsonl");

        {
            let wal = Wal::open_for_append(path.clone()).unwrap();
            wal.append(&sample_store("a")).unwrap();
            wal.append(&sample_store("b")).unwrap();
            wal.append(&sample_store("c")).unwrap();
        }

        let records = Wal::read_records(&path).unwrap();
        assert_eq!(records.len(), 3);
        match &records[0] {
            WalRecord::Store { fragment_id, .. } => assert_eq!(fragment_id, "a"),
        }
        match &records[2] {
            WalRecord::Store { fragment_id, .. } => assert_eq!(fragment_id, "c"),
        }
    }

    #[test]
    fn read_missing_file_returns_empty_vec() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does-not-exist.jsonl");
        let records = Wal::read_records(&path).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn malformed_line_is_skipped_not_fatal() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wal.jsonl");

        // Hand-write a file with one good + one garbage line.
        std::fs::write(
            &path,
            "{\"op\":\"store\",\"fragment_id\":\"x\",\"session_id\":\"s\",\"content\":\"c\",\"importance\":0.5,\"metadata\":{}}\nNOT_JSON\n",
        )
        .unwrap();

        let records = Wal::read_records(&path).unwrap();
        assert_eq!(records.len(), 1, "garbage line must be skipped");
    }

    #[test]
    fn append_creates_parent_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("path").join("wal.jsonl");
        let wal = Wal::open_for_append(path.clone()).unwrap();
        wal.append(&sample_store("only")).unwrap();
        drop(wal);
        assert!(path.exists());
    }

    #[test]
    fn reopen_appends_rather_than_truncates() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wal.jsonl");

        {
            let wal = Wal::open_for_append(path.clone()).unwrap();
            wal.append(&sample_store("first")).unwrap();
        }
        {
            let wal = Wal::open_for_append(path.clone()).unwrap();
            wal.append(&sample_store("second")).unwrap();
        }

        let records = Wal::read_records(&path).unwrap();
        assert_eq!(records.len(), 2);
    }
}
