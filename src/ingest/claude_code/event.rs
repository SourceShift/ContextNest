//! Typed event model for Claude Code `.jsonl` session transcripts.
//!
//! Permissive deserialize: fields we don't model land in `extra` rather than
//! failing the parse. Claude Code adds new event types between releases and
//! we must NOT break forward-compatible additions.

use crate::error::{ContextNestError, ContextNestResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// One event in a Claude Code session transcript. Top-level fields the
/// extractor cares about are modelled; everything else stays in `extra`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(rename = "sessionId", default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Present on `user` and `assistant` events. Role-dependent content
    /// shape (string for user, array of parts for assistant).
    #[serde(default)]
    pub message: Option<Message>,
    /// `ai-title` events carry the synthesised session title on a top-level
    /// `aiTitle` key.
    #[serde(rename = "aiTitle", default)]
    pub ai_title: Option<String>,
    /// `summary` events carry their text on a top-level `summary` key.
    #[serde(default)]
    pub summary: Option<String>,
    /// Any unmodelled fields. Keeps the parse resilient to schema drift.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// `message` field on `user` and `assistant` events.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    #[serde(default)]
    pub role: Option<String>,
    /// Either a plain string (user) or an array of typed content parts
    /// (assistant). Branch in the extractor.
    #[serde(default)]
    pub content: Value,
}

/// Session-level metadata recovered from the first events with each field
/// populated. Used when a downstream caller doesn't already know the
/// session uuid or project cwd.
#[derive(Debug, Clone, Default)]
pub struct SessionMetadata {
    pub session_uuid: Option<String>,
    pub cwd: Option<String>,
    pub first_timestamp: Option<String>,
    pub ai_title: Option<String>,
}

/// Parse a `.jsonl` file into typed events + a metadata summary.
///
/// Malformed lines are logged with `tracing::warn!` and skipped — we
/// never abort the whole ingest because of one bad event. Empty files
/// return `(vec![], default)`.
pub fn parse_session_file(path: &Path) -> ContextNestResult<(Vec<RawEvent>, SessionMetadata)> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        ContextNestError::Api(format!(
            "claude_code parser: cannot read {}: {}",
            path.display(),
            e
        ))
    })?;
    Ok(parse_session_string(&content))
}

/// Parse a `.jsonl` payload as a string. Convenience for tests + future
/// streaming code paths that don't read from a file.
pub fn parse_session_string(content: &str) -> (Vec<RawEvent>, SessionMetadata) {
    let mut events = Vec::new();
    let mut metadata = SessionMetadata::default();

    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<RawEvent>(line) {
            Ok(ev) => {
                if metadata.session_uuid.is_none() {
                    metadata.session_uuid.clone_from(&ev.session_id);
                }
                if metadata.cwd.is_none() {
                    metadata.cwd.clone_from(&ev.cwd);
                }
                if metadata.first_timestamp.is_none() {
                    metadata.first_timestamp.clone_from(&ev.timestamp);
                }
                if metadata.ai_title.is_none() {
                    metadata.ai_title.clone_from(&ev.ai_title);
                }
                events.push(ev);
            }
            Err(e) => {
                tracing::warn!(
                    "claude_code parser: malformed event at line {}: {}",
                    line_no + 1,
                    e
                );
            }
        }
    }

    (events, metadata)
}

/// Extract every `<z-insight>{...}</z-insight>` block from an assistant
/// text part. Malformed JSON blocks are silently skipped — we'd rather
/// lose telemetry than abort the ingest.
pub fn extract_zinsight_blocks(text: &str) -> Vec<Value> {
    const OPEN: &str = "<z-insight>";
    const CLOSE: &str = "</z-insight>";

    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(rel_start) = text[cursor..].find(OPEN) {
        let payload_start = cursor + rel_start + OPEN.len();
        let rest = &text[payload_start..];
        let Some(rel_end) = rest.find(CLOSE) else {
            break;
        };
        let payload = rest[..rel_end].trim();
        if let Ok(v) = serde_json::from_str::<Value>(payload) {
            out.push(v);
        }
        cursor = payload_start + rel_end + CLOSE.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_user_event() {
        let line = r#"{"type":"user","timestamp":"2026-01-01T00:00:00Z","sessionId":"abc","cwd":"/x","message":{"role":"user","content":"hello"}}"#;
        let ev: RawEvent = serde_json::from_str(line).unwrap();
        assert_eq!(ev.event_type, "user");
        assert_eq!(ev.session_id.as_deref(), Some("abc"));
        assert_eq!(ev.cwd.as_deref(), Some("/x"));
        let msg = ev.message.unwrap();
        assert_eq!(msg.role.as_deref(), Some("user"));
        assert_eq!(msg.content.as_str(), Some("hello"));
    }

    #[test]
    fn parses_an_assistant_event_with_array_content() {
        let line = r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"hi"},{"type":"tool_use","name":"Bash","input":{"command":"ls"}}]}}"#;
        let ev: RawEvent = serde_json::from_str(line).unwrap();
        assert_eq!(ev.event_type, "assistant");
        let msg = ev.message.unwrap();
        let arr = msg.content.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["type"], "text");
        assert_eq!(arr[1]["type"], "tool_use");
    }

    #[test]
    fn parses_an_ai_title_event() {
        let line =
            r#"{"type":"ai-title","aiTitle":"Review and fix incomplete code","sessionId":"abc"}"#;
        let ev: RawEvent = serde_json::from_str(line).unwrap();
        assert_eq!(ev.event_type, "ai-title");
        assert_eq!(
            ev.ai_title.as_deref(),
            Some("Review and fix incomplete code")
        );
    }

    #[test]
    fn parses_a_summary_event() {
        let line = r#"{"type":"summary","summary":"Fixed three CI failures and shipped v0.1.0","leafUuid":"x"}"#;
        let ev: RawEvent = serde_json::from_str(line).unwrap();
        assert_eq!(ev.event_type, "summary");
        assert_eq!(
            ev.summary.as_deref(),
            Some("Fixed three CI failures and shipped v0.1.0")
        );
    }

    #[test]
    fn unknown_event_type_does_not_error() {
        let line = r#"{"type":"some-future-event","whatever":42,"timestamp":"x"}"#;
        let ev: RawEvent = serde_json::from_str(line).unwrap();
        assert_eq!(ev.event_type, "some-future-event");
        assert!(ev.extra.contains_key("whatever"));
    }

    #[test]
    fn parse_session_string_recovers_metadata() {
        let content = concat!(
            r#"{"type":"user","sessionId":"sess-1","cwd":"/work","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"start"}}"#,
            "\n",
            r#"{"type":"ai-title","aiTitle":"my title","sessionId":"sess-1"}"#,
            "\n",
            r#"   "#, // empty/whitespace line, should skip
            "\n",
            r#"{garbage not json"#, // malformed, should skip with warn
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
        );
        let (events, meta) = parse_session_string(content);
        // 3 valid events; garbage + empty skipped
        assert_eq!(events.len(), 3);
        assert_eq!(meta.session_uuid.as_deref(), Some("sess-1"));
        assert_eq!(meta.cwd.as_deref(), Some("/work"));
        assert_eq!(
            meta.first_timestamp.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(meta.ai_title.as_deref(), Some("my title"));
    }

    #[test]
    fn extracts_zinsight_block() {
        let text = r#"some prose <z-insight>{"goal":"x","facts":["a","b"]}</z-insight> trailing"#;
        let blocks = extract_zinsight_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["goal"], "x");
        assert_eq!(blocks[0]["facts"][0], "a");
    }

    #[test]
    fn extracts_multiple_zinsight_blocks() {
        let text = r#"<z-insight>{"a":1}</z-insight> mid <z-insight>{"b":2}</z-insight>"#;
        let blocks = extract_zinsight_blocks(text);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["a"], 1);
        assert_eq!(blocks[1]["b"], 2);
    }

    #[test]
    fn malformed_zinsight_is_skipped() {
        let text = r#"<z-insight>not json here</z-insight> ok <z-insight>{"x":3}</z-insight>"#;
        let blocks = extract_zinsight_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["x"], 3);
    }

    #[test]
    fn unterminated_zinsight_does_not_hang() {
        let text = r#"<z-insight>{"goal":"x"   "#;
        let blocks = extract_zinsight_blocks(text);
        assert!(blocks.is_empty());
    }

    #[test]
    fn handles_realistic_extended_schema_block() {
        // Mirrors the shape from docs/z-insight-schema.md.
        let block = r#"<z-insight>
{
  "domain": "backend",
  "goal": "Land Claude Code session ingest",
  "current_task": "Wire metadata_filter",
  "progress": "in-progress",
  "topics": ["claude-code-ingest", "metadata-filter"],
  "top_jobs": ["Drafted spec", "Wrote 8 epic docs"],
  "current_state": "Foundation MVP scope locked",
  "facts": ["FragmentSidecar widening is the smallest change"],
  "tasks": [
    {"id":"T-43","subject":"Build ingester","status":"in_progress"}
  ],
  "epic_files": ["src/api/tools.rs"],
  "awaiting_decision": true,
  "decision": "--install-hooks behaviour?",
  "blockers": [],
  "requires_user_action": [
    {"step":1,"action":"Reload","reason":"Vite serves new bundle","urgency":"soon"}
  ]
}
</z-insight>"#;
        let blocks = extract_zinsight_blocks(block);
        assert_eq!(blocks.len(), 1);
        let b = &blocks[0];
        assert_eq!(b["domain"], "backend");
        assert_eq!(b["awaiting_decision"], true);
        assert_eq!(b["requires_user_action"][0]["urgency"], "soon");
        assert_eq!(b["tasks"][0]["status"], "in_progress");
        assert_eq!(b["epic_files"][0], "src/api/tools.rs");
    }
}
