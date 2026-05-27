use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

const VOLATILE_METADATA_KEYS: &[&str] =
    &["last_accessed", "_cn_consolidated", "_cn_consolidated_at"];

/// Build a deterministic fragment id for logical-memory writes.
///
/// Hook delivery and transcript sweeps can replay the same memory more than
/// once. The id must therefore be based on the memory's logical identity, not
/// on the delivery attempt. Read/consolidation markers are excluded because
/// they are generated after storage, but timestamps stay in the key: identical
/// text at different times can be valid separate memories and powers retrieve
/// decay semantics.
pub fn stable_fragment_id(
    session_id: &str,
    content: &str,
    metadata: &HashMap<String, Value>,
) -> String {
    let mut stable_meta = BTreeMap::new();
    for (key, value) in metadata {
        if VOLATILE_METADATA_KEYS.contains(&key.as_str()) {
            continue;
        }
        stable_meta.insert(key, canonical_value(value));
    }

    let payload = serde_json::json!({
        "session_id": session_id,
        "content": content.trim(),
        "metadata": stable_meta,
    });
    let bytes = serde_json::to_vec(&payload).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    format!("cn-{}", hex::encode(&digest[..16]))
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical_value).collect()),
        Value::Object(map) => {
            let ordered: BTreeMap<String, Value> = map
                .iter()
                .map(|(key, value)| (key.clone(), canonical_value(value)))
                .collect();
            serde_json::to_value(ordered).unwrap_or(Value::Null)
        }
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stable_fragment_id_ignores_read_and_consolidation_metadata() {
        let a = HashMap::from([
            ("kind".to_string(), json!("accomplishment")),
            ("source".to_string(), json!("TaskCompleted")),
            ("ts".to_string(), json!("2026-05-27T06:40:58.804Z")),
            ("last_accessed".to_string(), json!("later")),
            ("_cn_consolidated".to_string(), json!(true)),
        ]);
        let b = HashMap::from([
            ("kind".to_string(), json!("accomplishment")),
            ("source".to_string(), json!("TaskCompleted")),
            ("ts".to_string(), json!("2026-05-27T06:40:58.804Z")),
        ]);

        assert_eq!(
            stable_fragment_id("sess", "Completed: Token sanity check", &a),
            stable_fragment_id("sess", "Completed: Token sanity check", &b),
        );
    }

    #[test]
    fn stable_fragment_id_keeps_timestamps_in_identity() {
        let a = HashMap::from([
            ("kind".to_string(), json!("learning")),
            ("ts".to_string(), json!("2026-05-27T06:40:58.804Z")),
        ]);
        let b = HashMap::from([
            ("kind".to_string(), json!("learning")),
            ("ts".to_string(), json!("2026-05-27T06:46:11.998Z")),
        ]);

        assert_ne!(
            stable_fragment_id("sess", "same text", &a),
            stable_fragment_id("sess", "same text", &b),
        );
    }

    #[test]
    fn stable_fragment_id_keeps_kind_in_identity() {
        let a = HashMap::from([("kind".to_string(), json!("learning"))]);
        let b = HashMap::from([("kind".to_string(), json!("decision"))]);

        assert_ne!(
            stable_fragment_id("sess", "same text", &a),
            stable_fragment_id("sess", "same text", &b),
        );
    }
}
