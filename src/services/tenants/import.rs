//! Offline, explicit migration into the legacy operator tenant. The live WAL
//! is never modified and project paths never become tenant identities.
use super::{database::Database, types::*, Error, Result};
use crate::services::wal::WalRecord;
use rusqlite::params;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub input_records: usize,
    pub sessions: usize,
    pub largest_session: usize,
    pub skipped_operator_cache_records: usize,
    pub applied: bool,
    pub tenant: &'static str,
}

fn records(path: &Path) -> Result<impl Iterator<Item = Result<WalRecord>>> {
    let input = File::open(path).map_err(|e| Error::Internal(e.to_string()))?;
    Ok(BufReader::new(input).lines().filter_map(|line| match line {
        Ok(line) if line.trim().is_empty() => None,
        Ok(line) => Some(serde_json::from_str(&line).map_err(Into::into)),
        Err(e) => Some(Err(Error::Internal(e.to_string()))),
    }))
}
fn id(raw: &str) -> String {
    if valid_id(raw) {
        raw.into()
    } else {
        format!("legacy-{}", hex::encode(Sha256::digest(raw.as_bytes())))
    }
}

pub fn migrate_copy(
    input: &Path,
    output: &Path,
    embedding_space: &str,
    apply: bool,
) -> Result<ImportReport> {
    let source = input
        .canonicalize()
        .map_err(|e| Error::Internal(e.to_string()))?;
    let default = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .map(|p| p.join(".contextnest/wal.jsonl"));
    let configured = std::env::var_os("CONTEXTNEST_WAL_PATH").map(std::path::PathBuf::from);
    for live in [default, configured].into_iter().flatten() {
        if live.canonicalize().ok().as_ref() == Some(&source) {
            return Err(Error::Invalid(
                "provide a separate WAL copy, not the live WAL",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let (Ok(a), Ok(b)) = (source.metadata(), live.metadata()) {
                if a.dev() == b.dev() && a.ino() == b.ino() {
                    return Err(Error::Invalid(
                        "WAL copy must not be a hard link to the live WAL",
                    ));
                }
            }
        }
    }
    let mut counts = BTreeMap::<String, usize>::new();
    let mut kinds = BTreeSet::new();
    let mut skipped = 0;
    for record in records(&source)? {
        match record? {
            WalRecord::Store {
                session_id,
                content,
                metadata,
                ..
            } => {
                if content.len() > 65_536 {
                    return Err(Error::Invalid(
                        "legacy content exceeds the migration limit; split it explicitly",
                    ));
                }
                *counts.entry(id(&session_id)).or_default() += 1;
                kinds.insert(
                    metadata
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or("conversation-turn")
                        .to_owned(),
                );
            }
            _ => skipped += 1,
        }
    }
    let mut report = ImportReport {
        input_records: counts.values().sum(),
        sessions: counts.len(),
        largest_session: counts.values().copied().max().unwrap_or(0),
        skipped_operator_cache_records: skipped,
        applied: false,
        tenant: "legacy-operator",
    };
    if report.largest_session > 10_000 || report.sessions > 10_000 {
        return Err(Error::Invalid("legacy scopes exceed supported limits; partition ownership explicitly before migration"));
    }
    if !apply {
        return Ok(report);
    }
    if output.exists() {
        return Err(Error::Conflict("migration output must be a new directory"));
    }
    if embedding_space.is_empty() {
        return Err(Error::Invalid("embedding space is required for apply"));
    }
    super::private_directory(output)?;
    let policy = TenantPolicy {
        embedding_space: embedding_space.into(),
        max_sessions: 10_000,
        max_records_per_session: 10_000,
        max_content_bytes: 65_536,
        kinds: kinds.into_iter().collect(),
        ..Default::default()
    };
    let mut db = Database::open(&output.join("legacy-operator.sqlite"), &policy)?;
    let tx = db.connection.transaction()?;
    let now = super::database::now();
    for record in records(&source)? {
        if let WalRecord::Store {
            fragment_id,
            session_id,
            content,
            importance,
            metadata,
        } = record?
        {
            let session = id(&session_id);
            let event = id(&fragment_id);
            tx.execute("INSERT OR IGNORE INTO sessions(id,generation,status,expires_at,snapshot) VALUES(?1,1,'active',?2,?3)",
                params![session,now+i64::from(policy.retention_days)*86400,serde_json::to_string(&crate::memory::attractors::memory_attractor_manager::CanonicalSnapshot::default())?])?;
            let mut metadata: BTreeMap<_, _> = metadata
                .into_iter()
                .filter(|(k, _)| {
                    !k.starts_with("_cn_")
                        && !["tenant_id", "session_id", "session_ids"].contains(&k.as_str())
                })
                .collect();
            metadata.insert("legacy_fragment_id".into(), fragment_id.into());
            metadata.insert("legacy_session_id".into(), session_id.into());
            let request = StoreRequest {
                source_event_id: event.clone(),
                revision: 1,
                content: content.clone(),
                importance,
                metadata: metadata.clone(),
            };
            let fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&request)?));
            let stored = StoredRecord {
                fragment_id: hex::encode(Sha256::digest(format!(
                    "legacy-operator\0{session}\0{event}"
                ))),
                source_event_id: event.clone(),
                revision: 1,
                content,
                importance,
                metadata,
                created_at: now,
            };
            let existing: Option<String> = tx
                .query_row(
                    "SELECT fingerprint FROM records WHERE session=?1 AND event=?2",
                    params![session, event],
                    |r| r.get(0),
                )
                .optional()?;
            if existing.as_ref().is_some_and(|old| old != &fingerprint) {
                return Err(Error::Conflict(
                    "legacy event identity has conflicting payloads",
                ));
            }
            tx.execute("INSERT OR IGNORE INTO records(session,event,revision,payload,fingerprint,state) VALUES(?1,?2,1,?3,?4,'pending')",
                params![session,event,serde_json::to_string(&stored)?,fingerprint])?;
        }
    }
    tx.commit()?;
    let config = serde_json::json!({"data_dir":output,"operator_token_env":"CONTEXTNEST_OPERATOR_TOKEN","tenants":[{"id":"legacy-operator","token_env":"CONTEXTNEST_LEGACY_APP_TOKEN","policy":policy}]});
    std::fs::write(
        output.join("registration.example.json"),
        serde_json::to_vec_pretty(&config)?,
    )
    .map_err(|e| Error::Internal(e.to_string()))?;
    report.applied = true;
    Ok(report)
}
use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copied_wal_migration_is_explicit_and_never_assigns_named_tenant_ownership() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("wal.copy.jsonl");
        let output = dir.path().join("migrated");
        let record = WalRecord::Store {
            fragment_id: "legacy-fragment".into(),
            session_id: "coding-session".into(),
            content: "coding memory".into(),
            importance: 0.5,
            metadata: std::collections::HashMap::from([
                ("project_cwd".into(), "/tmp/example-app".into()),
                ("_cn_consolidated".into(), true.into()),
            ]),
        };
        std::fs::write(
            &source,
            format!("{}\n", serde_json::to_string(&record).unwrap()),
        )
        .unwrap();
        let dry = migrate_copy(&source, &output, "local-test-space", false).unwrap();
        assert!(!dry.applied);
        assert!(!output.exists());
        let report = migrate_copy(&source, &output, "local-test-space", true).unwrap();
        assert!(report.applied);
        assert_eq!(report.tenant, "legacy-operator");
        assert!(!output.join("example-app.sqlite").exists());
        let db = rusqlite::Connection::open(output.join("legacy-operator.sqlite")).unwrap();
        let state: String = db
            .query_row("SELECT state FROM records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(state, "pending");
        assert!(migrate_copy(&source, &output, "local-test-space", true).is_err());
    }
}
