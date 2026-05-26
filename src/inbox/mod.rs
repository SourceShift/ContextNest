//! Cross-session "what does Claude need from me?" inbox.
//!
//! The renderer is pure (no IO, no async): caller fetches `RetrieveHit`s
//! from the substrate via `/api/v1/tools/retrieve` with the right
//! `metadata_filter`, hands the raw JSON to [`InboxItem::from_hits`],
//! gets back a sorted list of items, and renders via [`render_text`] or
//! [`render_json`].
//!
//! The CLI wrapper in `src/bin/contextnest.rs` glues these together
//! with the HTTP calls and the session-discovery loop.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One thing the user is waiting on, surfaced from a stored memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InboxItem {
    /// Substrate session id — the bare Claude Code session UUID.
    pub session_id: String,
    /// Reconstructed project path. Empty when not stored.
    pub project_cwd: String,
    /// `user_action` or `decision`.
    pub kind: String,
    /// The action text or the decision question.
    pub content: String,
    /// `now` / `soon` / `later` for user_actions. `now` for decisions
    /// (they're always urgent by definition).
    pub urgency: String,
    /// Step ordinal for user_actions (e.g. 1, 2, 3 in a sequence).
    /// `None` for decisions.
    pub step: Option<u64>,
    /// Reason / why-it-matters string for user_actions. Empty for
    /// decisions.
    pub reason: String,
    /// ISO 8601 timestamp from the original event. Used for tiebreaking
    /// sort within the same urgency bucket.
    pub timestamp: String,
    /// Originating Claude Code session UUID. Since the substrate
    /// `session_id` is now the bare UUID, this is the same value;
    /// kept for backward-compat with the JSON payload.
    pub src_session_uuid: String,
}

impl InboxItem {
    /// Sort key: urgency rank (now=0, soon=1, later=2, unknown=3),
    /// then timestamp ascending so older items in the same urgency
    /// bucket bubble to the top.
    pub fn sort_key(&self) -> (u8, &str) {
        let rank = match self.urgency.as_str() {
            "now" => 0,
            "soon" => 1,
            "later" => 2,
            _ => 3,
        };
        (rank, self.timestamp.as_str())
    }

    /// Parse a JSON array of `RetrieveHit` shapes (as produced by the
    /// substrate's `/api/v1/tools/retrieve` handler) into InboxItems.
    /// Hits without the required metadata fields are silently skipped —
    /// we never abort on one malformed record.
    pub fn from_hits(hits: &[Value]) -> Vec<InboxItem> {
        hits.iter().filter_map(Self::from_hit).collect()
    }

    fn from_hit(hit: &Value) -> Option<InboxItem> {
        let metadata = hit.get("metadata")?.as_object()?;
        let kind = metadata.get("kind")?.as_str()?.to_string();

        // Only `user_action`, `todo`, and `decision` belong in the inbox.
        // Kept in sync with the backend endpoint
        // (`src/api/inbox.rs::is_inbox_eligible`) and the frontend filter
        // (`web/src/hooks/useInbox.ts`). Researcher-style autopilot data
        // contains `todo` entries that read as "user needs to pick X" —
        // semantically inbox-worthy even without an explicit user_action tag.
        if kind != "user_action" && kind != "decision" && kind != "todo" {
            return None;
        }

        let content = hit
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if content.is_empty() {
            return None;
        }

        let urgency = if kind == "decision" {
            // Decisions are always "now" — they block user response.
            "now".to_string()
        } else {
            metadata
                .get("urgency")
                .and_then(|v| v.as_str())
                .unwrap_or("soon")
                .to_string()
        };

        Some(InboxItem {
            session_id: hit
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| {
                    // Fallback — when the response shape doesn't carry
                    // session_id at the top, derive it from the
                    // src_session uuid. The substrate canonical form is
                    // the bare Claude Code session UUID.
                    metadata
                        .get("src_session")
                        .and_then(|v| v.as_str())
                        .map(|uuid| uuid.to_string())
                })
                .unwrap_or_else(|| "unknown".to_string()),
            project_cwd: metadata
                .get("project_cwd")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            kind,
            content,
            urgency,
            step: metadata.get("step").and_then(|v| v.as_u64()),
            reason: metadata
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            timestamp: metadata
                .get("ts")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            src_session_uuid: metadata
                .get("src_session")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }
}

/// Group items by session, sort by urgency-then-timestamp, render to a
/// terminal-friendly string. Pure — no IO.
pub fn render_text(items: &[InboxItem]) -> String {
    if items.is_empty() {
        return "📋 Inbox empty — Claude is not waiting on anything.\n".to_string();
    }

    // Group by session_id.
    let mut by_session: std::collections::BTreeMap<String, Vec<&InboxItem>> =
        std::collections::BTreeMap::new();
    for item in items {
        by_session
            .entry(item.session_id.clone())
            .or_default()
            .push(item);
    }

    // Within each session, sort by urgency (now > soon > later) then ts.
    for items in by_session.values_mut() {
        items.sort_by_key(|i| i.sort_key());
    }

    // Sort sessions by their highest-urgency item — surfaces sessions
    // with "now" actions before sessions whose actions are all "later".
    let mut sessions_ordered: Vec<(&String, &Vec<&InboxItem>)> = by_session.iter().collect();
    sessions_ordered.sort_by_key(|(_, items)| {
        items
            .iter()
            .map(|i| i.sort_key().0)
            .min()
            .unwrap_or(u8::MAX)
    });

    let mut out = String::new();
    out.push_str("📋 ContextNest — what Claude needs from you\n\n");

    for (session_id, items) in sessions_ordered {
        // Session header — pull project from the first item's metadata.
        let project = items
            .iter()
            .find(|i| !i.project_cwd.is_empty())
            .map(|i| i.project_cwd.as_str())
            .unwrap_or("(unknown project)");
        let project_label = if project.is_empty() {
            "(unknown project)".to_string()
        } else {
            // Trim to last-2 path components for readability.
            let trimmed: Vec<&str> = project.rsplit('/').take(2).collect();
            trimmed.into_iter().rev().collect::<Vec<_>>().join("/")
        };
        out.push_str(&format!("▸ session-{}  ·  {}\n", session_id, project_label));

        // Group user_actions vs decisions inside the session.
        let actions: Vec<&InboxItem> = items
            .iter()
            .filter(|i| i.kind == "user_action")
            .copied()
            .collect();
        let decisions: Vec<&InboxItem> = items
            .iter()
            .filter(|i| i.kind == "decision")
            .copied()
            .collect();

        if !actions.is_empty() {
            // Pick the highest urgency seen across the session for the
            // header line.
            let top_urgency = items
                .iter()
                .map(|i| i.urgency.as_str())
                .min_by_key(|u| match *u {
                    "now" => 0,
                    "soon" => 1,
                    "later" => 2,
                    _ => 3,
                })
                .unwrap_or("soon");
            out.push_str(&format!("   urgency: {}\n", top_urgency));

            for item in &actions {
                let step = item.step.map(|s| format!("{}. ", s)).unwrap_or_default();
                let reason = if item.reason.is_empty() {
                    String::new()
                } else {
                    format!("   → {}", item.reason)
                };
                out.push_str(&format!("   {}{}{}\n", step, item.content, reason));
            }
        }

        for item in &decisions {
            out.push_str(&format!("   ❓ Confirm: {}\n", item.content));
        }

        out.push('\n');
    }

    out
}

/// Machine-readable JSON output. Pure.
pub fn render_json(items: &[InboxItem]) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn user_action_hit(
        session: &str,
        project: &str,
        action: &str,
        urgency: &str,
        step: u64,
        reason: &str,
        ts: &str,
    ) -> Value {
        json!({
            "id": format!("f_{}", action),
            "content": action,
            "importance": 0.8,
            "similarity": 0.9,
            "session_id": session,
            "metadata": {
                "kind": "user_action",
                "src_session": format!("{}-fulluuid", session),
                "project_cwd": project,
                "ts": ts,
                "urgency": urgency,
                "step": step,
                "reason": reason,
            }
        })
    }

    fn decision_hit(session: &str, project: &str, question: &str, ts: &str) -> Value {
        json!({
            "id": format!("f_{}", question),
            "content": question,
            "importance": 0.85,
            "similarity": 0.9,
            "session_id": session,
            "metadata": {
                "kind": "decision",
                "src_session": format!("{}-fulluuid", session),
                "project_cwd": project,
                "ts": ts,
                "awaiting_decision": true,
                "decision_text": question,
            }
        })
    }

    #[test]
    fn parses_user_action_hit_into_item() {
        let hit = user_action_hit(
            "abc12345",
            "/work/ContextNest",
            "Reload page",
            "now",
            1,
            "picks up new bundle",
            "2026-05-20T10:00:00Z",
        );
        let items = InboxItem::from_hits(&[hit]);
        assert_eq!(items.len(), 1);
        let it = &items[0];
        assert_eq!(it.kind, "user_action");
        assert_eq!(it.urgency, "now");
        assert_eq!(it.step, Some(1));
        assert_eq!(it.content, "Reload page");
        assert_eq!(it.reason, "picks up new bundle");
        assert_eq!(it.project_cwd, "/work/ContextNest");
        assert_eq!(it.session_id, "abc12345");
    }

    #[test]
    fn parses_decision_hit_with_implicit_now_urgency() {
        let hit = decision_hit(
            "abc12345",
            "/work/X",
            "Does the menu open?",
            "2026-05-20T11:00:00Z",
        );
        let items = InboxItem::from_hits(&[hit]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "decision");
        assert_eq!(items[0].urgency, "now");
        assert_eq!(items[0].step, None);
    }

    #[test]
    fn ignores_non_inbox_kinds() {
        let learning = json!({
            "content": "interesting fact",
            "similarity": 0.9,
            "importance": 0.7,
            "metadata": {"kind": "learning"}
        });
        let accomplishment = json!({
            "content": "shipped X",
            "similarity": 0.9,
            "importance": 0.7,
            "metadata": {"kind": "accomplishment"}
        });
        let items = InboxItem::from_hits(&[learning, accomplishment]);
        assert!(
            items.is_empty(),
            "learning + accomplishment never enter inbox"
        );
    }

    #[test]
    fn drops_hits_with_no_kind() {
        let bad = json!({"content": "x", "metadata": {}});
        assert!(InboxItem::from_hits(&[bad]).is_empty());
    }

    #[test]
    fn sort_key_orders_now_before_soon_before_later() {
        let now = InboxItem {
            session_id: "s".into(),
            project_cwd: "".into(),
            kind: "user_action".into(),
            content: "x".into(),
            urgency: "now".into(),
            step: None,
            reason: "".into(),
            timestamp: "2026-05-20T01:00:00Z".into(),
            src_session_uuid: "".into(),
        };
        let later = InboxItem {
            urgency: "later".into(),
            timestamp: "2026-05-20T00:00:00Z".into(),
            ..now.clone()
        };
        let soon = InboxItem {
            urgency: "soon".into(),
            timestamp: "2026-05-20T00:00:00Z".into(),
            ..now.clone()
        };
        // now < soon < later even though `now`'s timestamp is later
        // (urgency dominates timestamp).
        assert!(now.sort_key() < soon.sort_key());
        assert!(soon.sort_key() < later.sort_key());
    }

    #[test]
    fn render_text_groups_by_session_and_shows_urgency() {
        let items = vec![
            InboxItem::from_hit(&user_action_hit(
                "aaa11111",
                "/work/A",
                "Click button",
                "now",
                1,
                "fires the thing",
                "2026-05-20T10:00:00Z",
            ))
            .unwrap(),
            InboxItem::from_hit(&decision_hit(
                "aaa11111",
                "/work/A",
                "Does it work?",
                "2026-05-20T10:05:00Z",
            ))
            .unwrap(),
            InboxItem::from_hit(&user_action_hit(
                "bbb22222",
                "/work/B",
                "Run cargo test",
                "later",
                1,
                "baseline",
                "2026-05-20T09:00:00Z",
            ))
            .unwrap(),
        ];

        let rendered = render_text(&items);
        assert!(rendered.contains("📋 ContextNest"));
        assert!(rendered.contains("session-aaa11111"));
        assert!(rendered.contains("session-bbb22222"));
        assert!(rendered.contains("Click button"));
        assert!(rendered.contains("→ fires the thing"));
        assert!(rendered.contains("❓ Confirm: Does it work?"));

        // The "now" session (aaa11111) should appear BEFORE the
        // "later" session (bbb22222) — urgency drives session order.
        let aaa = rendered.find("aaa11111").unwrap();
        let bbb = rendered.find("bbb22222").unwrap();
        assert!(
            aaa < bbb,
            "session with urgency=now should appear before session with urgency=later"
        );
    }

    #[test]
    fn render_text_empty_inbox_says_empty() {
        let s = render_text(&[]);
        assert!(s.contains("empty"));
    }

    #[test]
    fn render_json_is_valid_array() {
        let items =
            vec![
                InboxItem::from_hit(&user_action_hit("x-sess", "/p", "a", "now", 1, "r", "t"))
                    .unwrap(),
            ];
        let s = render_json(&items).unwrap();
        let parsed: Value = serde_json::from_str(&s).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
        assert_eq!(parsed[0]["content"], "a");
    }
}
