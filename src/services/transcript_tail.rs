//! Bounded append-only JSONL tailing. A checkpoint advances only through a
//! complete newline; the caller commits it after durable ingest acceptance.
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const CHUNK_BYTES: u64 = 4 * 1024 * 1024;
const FINGERPRINT_BYTES: usize = 128;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Checkpoint {
    pub offset: u64,
    pub path: String,
    pub identity: u64,
    pub length: u64,
    pub modified_ns: u128,
    pub fingerprint: Vec<u8>,
}
pub struct Tail {
    pub bytes: Vec<u8>,
    pub checkpoint: Checkpoint,
    pub bytes_read: usize,
}

pub async fn read(path: &Path, previous: &Checkpoint) -> Result<Tail, String> {
    let metadata = tokio::fs::metadata(path).await.map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("transcript is not a regular file".into());
    }
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        metadata.ino() ^ metadata.dev().rotate_left(32)
    };
    #[cfg(not(unix))]
    let identity = 0;
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path_string = path.to_string_lossy().into_owned();
    let mut next = previous.clone();
    if previous.path == path_string
        && previous.identity == identity
        && previous.length == metadata.len()
        && previous.modified_ns == modified_ns
    {
        return Ok(Tail {
            bytes: Vec::new(),
            checkpoint: next,
            bytes_read: 0,
        });
    }
    if previous.path != path_string
        || previous.identity != identity
        || metadata.len() < previous.offset
    {
        next = Checkpoint::default();
    }
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| e.to_string())?;
    let mut bytes_read = 0;
    if next.offset > 0 && !next.fingerprint.is_empty() {
        file.seek(std::io::SeekFrom::Start(
            next.offset - next.fingerprint.len() as u64,
        ))
        .await
        .map_err(|e| e.to_string())?;
        let mut actual = vec![0; next.fingerprint.len()];
        file.read_exact(&mut actual)
            .await
            .map_err(|e| e.to_string())?;
        bytes_read += actual.len();
        if actual != next.fingerprint {
            next.offset = 0;
            next.fingerprint.clear();
        }
    }
    file.seek(std::io::SeekFrom::Start(next.offset))
        .await
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(CHUNK_BYTES)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    bytes_read += bytes.len();
    let complete = bytes
        .iter()
        .rposition(|b| *b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    if complete == 0 && bytes.len() as u64 == CHUNK_BYTES {
        return Err("JSONL record exceeds the 4 MiB tail limit".into());
    }
    bytes.truncate(complete);
    next.offset += complete as u64;
    if !bytes.is_empty() {
        next.fingerprint = bytes[bytes.len().saturating_sub(FINGERPRINT_BYTES)..].to_vec();
    }
    next.path = path_string;
    next.identity = identity;
    next.modified_ns = modified_ns;
    // If another full chunk remains, do not mark the file as unchanged.
    next.length = if next.offset + CHUNK_BYTES < metadata.len() || bytes_read as u64 >= CHUNK_BYTES
    {
        next.offset
    } else {
        metadata.len()
    };
    Ok(Tail {
        bytes,
        checkpoint: next,
        bytes_read,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn append_partial_unchanged_and_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        tokio::fs::write(&path, b"{\"a\":1}\n{\"b\":")
            .await
            .unwrap();
        let first = read(&path, &Checkpoint::default()).await.unwrap();
        assert_eq!(first.bytes, b"{\"a\":1}\n");
        assert_eq!(first.checkpoint.offset, 8);
        let unchanged = read(&path, &first.checkpoint).await.unwrap();
        assert_eq!(unchanged.bytes_read, 0);
        tokio::fs::write(&path, b"{\"a\":1}\n{\"b\":2}\n")
            .await
            .unwrap();
        let second = read(&path, &first.checkpoint).await.unwrap();
        assert_eq!(second.bytes, b"{\"b\":2}\n");
        tokio::fs::write(&path, b"{\"x\":9}\n{\"q\":3}\n{\"z\":4}\n")
            .await
            .unwrap();
        let replacement = read(&path, &second.checkpoint).await.unwrap();
        assert!(replacement.bytes.starts_with(b"{\"x\""));
    }
}
