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
    /// LLM proxy cache insert (v0.3 Phase 2 slice 2.5 + Phase 3
    /// slice 3.1 encryption). Persists a cached chat-completion
    /// response across restarts so the warm-up curve doesn't have to
    /// be re-paid on every binary deploy.
    ///
    /// Stored separately from `Store` because cache entries are NOT
    /// user memories — mixing them through `process_memories` would
    /// pollute the substrate's reconstruct / resonate queries with
    /// cache fragments. Cache replay rebuilds the in-memory map
    /// directly via `LlmCacheService::replay`.
    LlmCacheInsert {
        /// Exact-match prefix fields (replay rebuilds the in-memory
        /// HashMap key from these without needing to reverse the
        /// truncated SHA-256). These stay cleartext on disk because
        /// the HashMap lookup happens BEFORE decryption.
        project_id: String,
        model: String,
        temperature_bucket: u8,
        system_prompt_hash: [u8; 8],
        /// Unix timestamp in seconds of the original insert. Used to
        /// reconstruct entry age for TTL checks after replay.
        inserted_at_unix_secs: u64,
        /// Either plaintext (legacy) or AES-256-GCM-sealed embedding
        /// + response_json. See [`CachePayload`].
        payload: CachePayload,
    },
    /// LLM proxy cache hard-delete (v0.3 Phase 3 slice 3.3). The
    /// `DELETE /llm/v1/cache/entries/<fingerprint>` HTTP handler
    /// writes this record so the bucket doesn't return on restart.
    ///
    /// "Hard" means the bucket leaves both the in-memory store AND
    /// the WAL's effective state. The original `LlmCacheInsert`
    /// records stay on disk (we never rewrite the WAL mid-flight),
    /// but the replay path skips any bucket whose fingerprint has a
    /// corresponding `LlmCacheDiscard` record at a later
    /// position. Standard tombstone pattern; a future WAL compactor
    /// reclaims the bytes.
    ///
    /// Deletion is bucket-level by exact-prefix fingerprint —
    /// multiple entries within one bucket (same project + model +
    /// temperature + system_prompt) are deleted together. The GDPR
    /// + project-purge shapes are both bucket-shaped, so this
    /// covers v0.3's load-bearing cases. Per-entry addressing
    /// (embedding-hash suffix) lands as a follow-up if real usage
    /// surfaces the need.
    LlmCacheDiscard {
        /// 32-byte `ExactKeyPrefix::fingerprint()`. Stored as raw
        /// bytes so the replay loop doesn't re-decode per record.
        /// The HTTP handler accepts URL-safe base64 over the wire
        /// and decodes at the edge.
        prefix_fingerprint: [u8; 32],
        /// Unix timestamp of the delete operation. Lets the replay
        /// pipeline enforce strict insert-before-discard ordering
        /// when the WAL contains both for the same fingerprint.
        deleted_at_unix_secs: u64,
        /// Free-form audit reason from the `?reason=` query param.
        /// Stored so the WAL itself doubles as the audit trail
        /// without a separate log-shipping pipeline.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

/// Either a plaintext or AEAD-sealed envelope of the embedding +
/// response body for one cache entry. Discriminator field is `mode`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum CachePayload {
    /// Plaintext on disk. Default when the substrate has no
    /// encryption key configured. Equivalent to Phase 2 slice 2.5
    /// behaviour.
    Plaintext {
        embedding: Vec<f32>,
        /// Serialised `ChatCompletionsResponse` JSON.
        response_json: String,
    },
    /// AES-256-GCM ciphertext over a length-prefixed concatenation of
    /// the embedding (as little-endian f32 bytes) and the response
    /// JSON. AAD binds the ciphertext to the
    /// `ExactKeyPrefix::fingerprint` of the entry (project + model +
    /// temperature + system_prompt_hash) so a ciphertext lifted from
    /// one bucket can't be replayed into another. `nonce` is the GCM
    /// nonce; `ciphertext` is the sealed bytes including the 16-byte
    /// authentication tag.
    AesGcm {
        nonce: [u8; 12],
        ciphertext: Vec<u8>,
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

/// Result of [`migrate_legacy_session_ids`].
#[derive(Debug, Clone, Copy, Default)]
pub struct MigrationReport {
    /// Records whose `session_id` was rewritten to the canonical bare-UUID
    /// form. Covers both legacy shapes:
    /// - `cc-<full-uuid>` → `<full-uuid>` (drop prefix)
    /// - `cc-<first-8>` + `metadata.src_session` → `<full-uuid>` (drop
    ///   prefix and expand from oracle)
    pub migrated: usize,
    /// Short-form records (`cc-<first-8>`) that lacked a
    /// `metadata.src_session` oracle and were therefore left unchanged.
    /// These should be rare; manual recovery would mean grepping the
    /// original transcript for the full UUID.
    pub skipped_no_src_session: usize,
}

/// One-shot migration to the canonical bare-UUID `session_id`.
///
/// The substrate previously emitted `cc-<full-uuid>` (and earlier
/// `cc-<first-8-of-uuid>`). The `cc-` namespacing has been retired —
/// Claude Code session UUIDs are themselves globally unique, so the
/// extra namespace tag was redundant and confused the dashboard /
/// curl examples.
///
/// Detection per record:
/// - Bare UUID (no `cc-` prefix): pass through unchanged.
/// - `cc-` prefix with ≥36 chars after the prefix: strip prefix.
/// - `cc-` prefix with fewer than 36 chars (legacy short form): strip
///   prefix AND replace with `metadata.src_session` if present.
/// - `cc-` prefix without `src_session` oracle: leave as-is and bump
///   `skipped_no_src_session` — the operator can investigate.
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
pub fn migrate_legacy_session_ids(
    path: &Path,
    records: Vec<WalRecord>,
) -> std::io::Result<(Vec<WalRecord>, MigrationReport)> {
    const UUID_LEN: usize = 36;

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
                let stripped = session_id.strip_prefix("cc-");
                let new_session_id = match stripped {
                    None => {
                        // Already bare UUID — nothing to do.
                        session_id
                    }
                    Some(rest) if rest.len() >= UUID_LEN => {
                        // `cc-<full-uuid>` — just drop the prefix.
                        report.migrated += 1;
                        rest.to_string()
                    }
                    Some(_) => {
                        // `cc-<short-form>` — try the src_session oracle.
                        let full_uuid = metadata
                            .get("src_session")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string());
                        match full_uuid {
                            Some(uuid) => {
                                report.migrated += 1;
                                uuid
                            }
                            None => {
                                report.skipped_no_src_session += 1;
                                session_id
                            }
                        }
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
            // Non-`Store` variants pass through untouched — the session-id
            // migration only applies to user-memory records.
            other => out.push(other),
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
            let line = serde_json::to_string(r)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
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
            session_id: "test-sess".to_string(),
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
            other => panic!("expected Store, got {other:?}"),
        }
        match &records[2] {
            WalRecord::Store { fragment_id, .. } => assert_eq!(fragment_id, "c"),
            other => panic!("expected Store, got {other:?}"),
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
