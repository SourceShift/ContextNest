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

/// Result of [`migrate_short_session_ids`].
#[derive(Debug, Clone, Copy, Default)]
pub struct MigrationReport {
    /// Records whose `session_id` was rewritten from the old
    /// `cc-<first-8>` form to the canonical `cc-<full-uuid>` form.
    pub migrated: usize,
    /// Short-form records that lacked a `metadata.src_session` oracle
    /// and were therefore left unchanged.
    pub skipped_no_src_session: usize,
}

/// One-shot migration from the legacy `cc-<first-8-of-uuid>` short-form
/// session id to the canonical `cc-<full-uuid>` long form.
///
/// Detection: a record is migrated iff its `session_id` is shorter than
/// `cc-` + 36 chars AND starts with `cc-` AND carries a non-empty
/// `metadata.src_session`. Long-form records pass through untouched;
/// records without an `src_session` oracle (manual API stores, test
/// fixtures) are kept as-is.
///
/// If any records changed, the on-disk WAL at `path` is rewritten:
/// write `wal.new` (fsync), rename current `wal` → `wal.bak`, rename
/// `wal.new` → `wal`. The `.bak` is left in place as a recovery
/// breadcrumb. If `path` does not exist (cold start) the call is a
/// no-op that returns the input records unchanged.
///
/// Idempotency: rerunning the function after a successful migration
/// finds nothing to do and skips the rewrite — safe to call on every
/// boot.
pub fn migrate_short_session_ids(
    path: &Path,
    records: Vec<WalRecord>,
) -> std::io::Result<(Vec<WalRecord>, MigrationReport)> {
    const LONG_FORM_MIN_LEN: usize = "cc-".len() + 36;

    let mut report = MigrationReport::default();
    let mut out: Vec<WalRecord> = Vec::with_capacity(records.len());

    for rec in records {
        match rec {
            WalRecord::Store {
                fragment_id,
                session_id,
                content,
                importance,
                metadata,
            } => {
                if session_id.len() >= LONG_FORM_MIN_LEN || !session_id.starts_with("cc-") {
                    out.push(WalRecord::Store {
                        fragment_id,
                        session_id,
                        content,
                        importance,
                        metadata,
                    });
                    continue;
                }
                let full_uuid = metadata
                    .get("src_session")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let new_session_id = match full_uuid {
                    Some(uuid) if !uuid.is_empty() => {
                        report.migrated += 1;
                        format!("cc-{uuid}")
                    }
                    _ => {
                        report.skipped_no_src_session += 1;
                        session_id
                    }
                };
                out.push(WalRecord::Store {
                    fragment_id,
                    session_id: new_session_id,
                    content,
                    importance,
                    metadata,
                });
            }
        }
    }

    if report.migrated == 0 || !path.exists() {
        return Ok((out, report));
    }

    let new_path = path.with_extension("new");
    let bak_path = path.with_extension("bak");
    {
        let mut f = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&new_path)?;
        let mut buf = BufWriter::new(&mut f);
        for r in &out {
            let line = serde_json::to_string(r).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, e)
            })?;
            writeln!(buf, "{line}")?;
        }
        buf.flush()?;
        drop(buf);
        f.sync_all()?;
    }
    // Two-rename swap. A crash between these leaves `.bak` and `.new`
    // for manual recovery: `mv wal.new wal` (or restore `.bak` and
    // re-run; migration is idempotent).
    std::fs::rename(path, &bak_path)?;
    std::fs::rename(&new_path, path)?;
    Ok((out, report))
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
