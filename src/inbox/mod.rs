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

        // Only `user_action`, `todo`, `decision`, `ask`, `handoff` belong in
        // the inbox. Kept in sync with the backend endpoint
        // (`src/api/inbox.rs::is_inbox_eligible`) and the frontend filter
        // (`web/src/hooks/useInbox.ts`). Researcher-style autopilot data
        // contains `todo` entries that read as "user needs to pick X" —
        // semantically inbox-worthy even without an explicit user_action tag.
        //
        // `ask` and `handoff` added per idea 023 Gap G2 — agent-substrate
        // primitives that need cross-session visibility before oracle wires up.
        if kind != "user_action"
            && kind != "decision"
            && kind != "todo"
            && kind != "ask"
            && kind != "handoff"
        {
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

/// Per-item content preview cap when rendering Markdown. Bounds the body
/// even when an agent emits a multi-line user_action description.
const INBOX_MD_CONTENT_LIMIT: usize = 280;

/// Group items by urgency (now → soon → later → Unspecified), render as a
/// paste-ready Markdown digest. Shape is intentionally aligned with the
/// substrate's `/api/v1/inbox?format=markdown` body (PR #87) so an agent
/// reading either surface sees the same priority structure — the CLI
/// renders locally from the per-session aggregation, the substrate renders
/// from the global fragment_metadata walk, but both bucket the same way.
/// Pure — no IO.
pub fn render_markdown(items: &[InboxItem]) -> String {
    let mut md = String::with_capacity(2 * 1024 + items.len() * 180);
    md.push_str("# Attention Inbox\n\n");
    md.push_str(&format!("_{} items_\n\n", items.len()));

    if items.is_empty() {
        md.push_str("_(no items matched the filters)_\n");
        return md;
    }

    // Group by urgency. Items keep their natural CLI sort (per-session
    // aggregation upstream) inside each bucket because the buckets
    // dominate the visual hierarchy; finer order doesn't matter once
    // the priority is correct.
    let buckets: &[(&str, &str)] = &[
        ("now", "Now"),
        ("soon", "Soon"),
        ("later", "Later"),
        ("", "Unspecified"),
    ];
    let mut grouped: std::collections::HashMap<&str, Vec<&InboxItem>> =
        std::collections::HashMap::new();
    for item in items {
        let key = match item.urgency.as_str() {
            "now" | "soon" | "later" => item.urgency.as_str(),
            _ => "",
        };
        grouped.entry(key).or_default().push(item);
    }

    for (bucket_key, heading) in buckets {
        let Some(group) = grouped.get(*bucket_key) else {
            continue;
        };
        if group.is_empty() {
            continue;
        }
        md.push_str(&format!("## {heading} ({count})\n\n", count = group.len()));
        for item in group {
            let preview = truncate_md(&item.content, INBOX_MD_CONTENT_LIMIT);
            md.push_str(&format!("- {preview}\n"));
            let session_short: String = item.session_id.chars().take(8).collect();
            let mut meta_parts: Vec<String> = vec![format!("kind `{}`", item.kind)];
            meta_parts.push(format!("session `{session_short}`"));
            if !item.project_cwd.is_empty() {
                // Trim to last-2 path components for readability.
                let trimmed: Vec<&str> = item.project_cwd.rsplit('/').take(2).collect();
                let label = trimmed.into_iter().rev().collect::<Vec<_>>().join("/");
                meta_parts.push(format!("project `{label}`"));
            }
            if !item.timestamp.is_empty() {
                meta_parts.push(format!("ts {ts}", ts = item.timestamp));
            }
            if let Some(step) = item.step {
                meta_parts.push(format!("step {step}"));
            }
            if !item.reason.is_empty() {
                let reason = truncate_md(&item.reason, INBOX_MD_CONTENT_LIMIT);
                meta_parts.push(format!("reason: {reason}"));
            }
            md.push_str(&format!("  _{}_\n", meta_parts.join(" · ")));
        }
        md.push('\n');
    }

    md
}

/// Multibyte-safe char-count truncation that collapses newlines into
/// spaces so a multi-line item stays on one bullet. Mirrors the helper in
/// `src/api/inbox.rs` and `src/api/sessions.rs`; the three are independent
/// because each module's local data shape differs slightly. Worth a
/// cross-module extraction if a fourth caller appears.
fn truncate_md(s: &str, limit: usize) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if cleaned.chars().count() <= limit {
        return cleaned;
    }
    let head: String = cleaned.chars().take(limit).collect();
    format!("{head}…")
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

    #[test]
    fn render_markdown_empty_input_renders_explanation() {
        let md = render_markdown(&[]);
        assert!(md.starts_with("# Attention Inbox\n"));
        assert!(md.contains("0 items"));
        assert!(md.contains("_(no items matched the filters)_"));
    }

    #[test]
    fn render_markdown_buckets_by_urgency_in_priority_order() {
        // Four items spanning four urgency buckets including missing-urgency.
        let items: Vec<InboxItem> = vec![
            InboxItem::from_hit(&user_action_hit(
                "sess-A12345678",
                "/repo/x",
                "Free disk",
                "now",
                1,
                "low space",
                "2026-05-31T10:00:00Z",
            ))
            .unwrap(),
            InboxItem::from_hit(&user_action_hit(
                "sess-B12345678",
                "/repo/y",
                "Run wrangler login",
                "soon",
                2,
                "auth expires",
                "2026-05-31T11:00:00Z",
            ))
            .unwrap(),
            InboxItem::from_hit(&user_action_hit(
                "sess-C12345678",
                "/repo/z",
                "Pick a target",
                "later",
                3,
                "no rush",
                "2026-05-31T12:00:00Z",
            ))
            .unwrap(),
            // Synthesize an Unspecified item by setting urgency to empty.
            {
                let mut it = InboxItem::from_hit(&user_action_hit(
                    "sess-D12345678",
                    "/repo/w",
                    "Set token",
                    "now",
                    4,
                    "needed",
                    "2026-05-31T09:00:00Z",
                ))
                .unwrap();
                it.urgency = String::new();
                it
            },
        ];
        let md = render_markdown(&items);
        // All four headings present in priority order.
        let now_pos = md.find("## Now (1)").expect("now heading");
        let soon_pos = md.find("## Soon (1)").expect("soon heading");
        let later_pos = md.find("## Later (1)").expect("later heading");
        let unspec_pos = md.find("## Unspecified (1)").expect("unspec heading");
        assert!(now_pos < soon_pos);
        assert!(soon_pos < later_pos);
        assert!(later_pos < unspec_pos);
        // Per-item meta line carries kind, session-short, project (trimmed),
        // ts, step, reason.
        assert!(md.contains("Free disk"));
        assert!(md.contains("kind `user_action`"));
        assert!(md.contains("session `sess-A12`"));
        assert!(md.contains("project `repo/x`")); // last-2-components only
        assert!(md.contains("ts 2026-05-31T10:00:00Z"));
        assert!(md.contains("step 1"));
        assert!(md.contains("reason: low space"));
    }

    #[test]
    fn render_markdown_truncates_long_content_with_ellipsis() {
        let mut it = InboxItem::from_hit(&user_action_hit(
            "sess-E12345678",
            "/repo/w",
            "x",
            "now",
            1,
            "r",
            "t",
        ))
        .unwrap();
        it.content = "x".repeat(500);
        let md = render_markdown(std::slice::from_ref(&it));
        // First bullet line ("- ...") is truncated to 280 chars + ellipsis.
        let first_line = md
            .lines()
            .find(|l| l.starts_with("- "))
            .expect("bullet line");
        assert!(first_line.ends_with('…'));
        assert_eq!(first_line.chars().count(), 2 + 280 + 1); // "- " + 280 + "…"
    }
}
