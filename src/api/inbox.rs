//! `GET /api/v1/inbox` — cross-session "what does Claude need from me?" feed.
//!
//! This endpoint replaces the old dashboard fan-out (one
//! `/api/v1/sessions` call followed by `2 × N` `/api/v1/tools/retrieve`
//! calls per polling tick) with a single server-side scan. The cost
//! profile drops from `O(S)` HTTP round-trips + `O(S × F)` lock
//! acquisitions per tick to a single request that walks `fragment_metadata`
//! once and looks up active session affinity from a single
//! `SessionIndex::active_fragments_session_map` snapshot.
//!
//! ## Inclusion rules
//!
//! A fragment is surfaced in the inbox iff:
//!
//! - it is currently **active** in `SessionIndex` (soft-deleted IDs are
//!   skipped — same visibility rule `retrieve` uses), **and**
//! - its stored metadata has `kind == "user_action"`, **or**
//! - its stored metadata has `kind == "decision"` AND
//!   `awaiting_decision == true`.
//!
//! These two filters mirror the previous frontend behaviour in
//! `web/src/hooks/useInbox.ts` so the dashboard semantics are unchanged.
//!
//! ## Response shape
//!
//! Returns a flat list of `InboxHit` items. The shape is intentionally
//! close to `RetrieveHit` so the frontend mapper does not need to be
//! rewritten — the only addition is the top-level `session_id` field,
//! and `similarity` is omitted because no query is run.
//!
//! Items are returned **sorted by metadata `ts` descending** (newest
//! first), with the empty/missing-`ts` case sorting last. The frontend
//! still applies its urgency-bucketed sort on top, so this server-side
//! order is a defensive default rather than a load-bearing contract.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::services::ContextNestServices;

/// Per-item content preview cap when rendering Markdown. Mirrors the
/// equivalent limit in `/features?format=markdown` and the prompt-context
/// capsule renderer; keeps the body bounded even when an agent emits a
/// long user_action description.
const INBOX_MD_CONTENT_LIMIT: usize = 280;

/// Hard cap on items returned regardless of `?limit=`. Bounds the payload
/// for both JSON and Markdown formats so a runaway inbox can't dominate
/// a dashboard polling tick.
const INBOX_MAX_LIMIT: usize = 500;

#[derive(Debug, Deserialize)]
pub struct InboxQuery {
    /// Substring match on the item's `project_cwd` metadata. Case-sensitive
    /// (mirrors other endpoints' project filters).
    pub project: Option<String>,
    /// One of `now` / `soon` / `later` — restrict to items carrying that
    /// urgency. Case-insensitive. Unknown values are tolerated (just match
    /// nothing). Items without an `urgency` field are excluded when this
    /// filter is set.
    pub urgency: Option<String>,
    /// Restrict to one inbox-eligible kind (`user_action` / `todo` /
    /// `decision`). Unknown kinds match nothing.
    pub kind: Option<String>,
    /// Max items returned. Default unbounded (subject to `INBOX_MAX_LIMIT`).
    pub limit: Option<usize>,
    /// Response format. `json` (default) returns `InboxResponse`;
    /// `markdown` returns `text/markdown` grouped by urgency. Unknown
    /// values fall through to JSON.
    pub format: Option<String>,
}

/// One item in the dashboard inbox feed.
///
/// Mirrors [`crate::api::tools::RetrieveHit`] field-by-field except:
/// - `session_id` is added at the top level (the inbox is cross-session
///   by definition, so callers need to know where each item came from)
/// - `similarity` is omitted (no query, no scoring)
#[derive(Debug, Serialize)]
pub struct InboxHit {
    pub id: String,
    pub session_id: String,
    pub content: String,
    pub importance: f32,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct InboxResponse {
    pub items: Vec<InboxHit>,
}

/// `GET /api/v1/inbox`
///
/// See module docs for inclusion rules and ordering. Acquires three read
/// locks (fragment_metadata, session_index.active, fragment_texts) and
/// holds none of them across an `await` that could touch a writer.
///
/// Optional query params: `project` (substring on project_cwd),
/// `urgency` (`now`/`soon`/`later`), `kind` (one of the three eligible
/// kinds), `limit` (capped at `INBOX_MAX_LIMIT`), and `format`
/// (`json` default, `markdown` for a prose digest grouped by urgency).
/// All filters are independent — the result is the conjunction.
pub async fn list_inbox(
    State(services): State<ContextNestServices>,
    Query(q): Query<InboxQuery>,
) -> Result<axum::response::Response, StatusCode> {
    // Snapshot active-fragment → session affinity in one lock acquisition.
    // Soft-deleted fragments are intentionally excluded — they would still
    // resolve via `find_session` (reverse map) but they are not visible to
    // `retrieve`, so they should not be visible to the inbox either.
    let frag_to_session = services.session_index.active_fragments_session_map().await;
    let want_project = q.project.as_deref();
    let want_urgency = q.urgency.as_deref().map(str::to_lowercase);
    let want_kind = q.kind.as_deref();

    if frag_to_session.is_empty() {
        return finalize_inbox(Vec::new(), &q);
    }

    // Walk metadata under a single read lock. For each entry, apply the
    // inclusion rules; only inbox-eligible fragments survive into the
    // intermediate vector.
    let metadata = services.fragment_metadata.read().await;
    let mut eligible: Vec<(String, String, HashMap<String, serde_json::Value>)> = Vec::new();

    for (frag_id, meta) in metadata.iter() {
        if !is_inbox_eligible(meta) {
            continue;
        }
        if let Some(want) = want_kind {
            match meta.get("kind").and_then(|v| v.as_str()) {
                Some(k) if k == want => {}
                _ => continue,
            }
        }
        if let Some(want) = want_project {
            match meta.get("project_cwd").and_then(|v| v.as_str()) {
                Some(p) if p.contains(want) => {}
                _ => continue,
            }
        }
        if let Some(want) = want_urgency.as_deref() {
            match meta.get("urgency").and_then(|v| v.as_str()) {
                Some(u) if u.eq_ignore_ascii_case(want) => {}
                _ => continue,
            }
        }
        let Some(session_id) = frag_to_session.get(frag_id) else {
            // Metadata exists but the fragment is not active — likely
            // soft-deleted. Skip.
            continue;
        };
        eligible.push((frag_id.clone(), session_id.clone(), meta.clone()));
    }
    drop(metadata);

    if eligible.is_empty() {
        return finalize_inbox(Vec::new(), &q);
    }

    // Single bulk text lookup for survivors only.
    let texts = services.fragment_texts.read().await;

    // Importance lives on the canonical fragment, not in the metadata
    // sidecar. The substrate's `MemoryAttractorManager::get_fragment` is
    // the source of truth, but calling it N times here would re-introduce
    // the per-fragment lock acquisition we just eliminated. Importance is
    // a UI nice-to-have for the inbox (the frontend doesn't actually sort
    // by it), so default to a neutral 0.5 and accept the staleness rather
    // than pay the cost. A future iteration could mirror importance into
    // the metadata sidecar at store time if any dashboard view starts to
    // depend on it.
    let mut items: Vec<InboxHit> = eligible
        .into_iter()
        .map(|(id, session_id, meta)| {
            let content = texts.get(&id).cloned().unwrap_or_default();
            InboxHit {
                id,
                session_id,
                content,
                importance: 0.5,
                metadata: meta,
            }
        })
        .collect();
    drop(texts);

    // Sort by `ts` descending (newest first). Missing/empty ts sorts last
    // so stale entries never crowd out fresh ones in the default view.
    items.sort_by(|a, b| {
        let a_ts = a.metadata.get("ts").and_then(|v| v.as_str()).unwrap_or("");
        let b_ts = b.metadata.get("ts").and_then(|v| v.as_str()).unwrap_or("");
        match (a_ts.is_empty(), b_ts.is_empty()) {
            (true, true) => a.id.cmp(&b.id),
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => b_ts.cmp(a_ts).then_with(|| a.id.cmp(&b.id)),
        }
    });

    // View-layer dedup: long-running agents re-emit the same actionable
    // item across many turns while waiting for the user (e.g. researcher
    // autopilot's "Pick A/B/C" survives 7 turns until answered). The
    // substrate ingests each emission as a distinct fragment because
    // urgency/step/reason can drift across turns — losing that history
    // at ingest would be lossy. But the dashboard's attention queue only
    // needs "is this still pending?" once per (session, content, kind).
    //
    // Strategy: dedup by `(session_id, kind, content)`. Because we've
    // already sorted by ts desc, the FIRST occurrence of any key is the
    // newest — keep it, drop the rest. Cross-session duplicates are
    // intentionally preserved: "Run wrangler login" pending in two
    // different sessions is two distinct signals.
    let mut seen: std::collections::HashSet<(String, String, String)> =
        std::collections::HashSet::new();
    items.retain(|hit| {
        let kind = hit
            .metadata
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let key = (hit.session_id.clone(), kind, hit.content.clone());
        seen.insert(key)
    });

    finalize_inbox(items, &q)
}

/// Branch on `format` and build the Response (JSON or Markdown), applying
/// the `limit` cap. Extracted so the early-return paths (empty session
/// index / empty eligible set) share the response-building logic.
fn finalize_inbox(
    mut items: Vec<InboxHit>,
    q: &InboxQuery,
) -> Result<axum::response::Response, StatusCode> {
    let limit = q
        .limit
        .map(|n| n.min(INBOX_MAX_LIMIT))
        .unwrap_or(INBOX_MAX_LIMIT);
    items.truncate(limit);

    use axum::response::IntoResponse;
    let format_md = matches!(q.format.as_deref(), Some("markdown") | Some("md"));

    if format_md {
        let md = render_inbox_markdown(
            q.project.as_deref(),
            q.urgency.as_deref(),
            q.kind.as_deref(),
            &items,
        );
        axum::response::Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/markdown; charset=utf-8")
            .body(axum::body::Body::from(md))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    } else {
        Ok(Json(InboxResponse { items }).into_response())
    }
}

/// Render the inbox as Markdown grouped by urgency: `now` first, then
/// `soon`, `later`, and an "Unspecified" bucket for items with no urgency
/// metadata. Within each bucket items are newest-first (the parent caller
/// already sorted by `ts` desc). Designed for paste-into-prompt and
/// shell-pipe-into-pbcopy workflows mirroring `/features?format=markdown`.
fn render_inbox_markdown(
    project: Option<&str>,
    urgency: Option<&str>,
    kind: Option<&str>,
    items: &[InboxHit],
) -> String {
    let mut md = String::with_capacity(2 * 1024 + items.len() * 200);
    md.push_str("# Attention Inbox\n\n");
    md.push('_');
    let mut filters: Vec<String> = vec![format!("{} items", items.len())];
    if let Some(p) = project {
        filters.push(format!("project ~ `{p}`"));
    }
    if let Some(u) = urgency {
        filters.push(format!("urgency `{u}`"));
    }
    if let Some(k) = kind {
        filters.push(format!("kind `{k}`"));
    }
    md.push_str(&filters.join(" · "));
    md.push_str("_\n\n");

    if items.is_empty() {
        md.push_str("_(no items matched the filters)_\n");
        return md;
    }

    // Bucket by urgency. Stable iteration order on the ordered list of
    // buckets — `now` first because it's literally what blocks the user.
    let buckets: &[(&str, &str)] = &[
        ("now", "Now"),
        ("soon", "Soon"),
        ("later", "Later"),
        ("", "Unspecified"),
    ];
    let mut grouped: HashMap<String, Vec<&InboxHit>> = HashMap::new();
    for hit in items {
        let key = hit
            .metadata
            .get("urgency")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let normalized = match key.as_str() {
            "now" | "soon" | "later" => key,
            _ => "".to_string(),
        };
        grouped.entry(normalized).or_default().push(hit);
    }

    for (bucket_key, heading) in buckets {
        let Some(group) = grouped.get(*bucket_key) else {
            continue;
        };
        if group.is_empty() {
            continue;
        }
        md.push_str(&format!("## {heading} ({count})\n\n", count = group.len()));
        for hit in group {
            let kind_str = hit
                .metadata
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ts = hit
                .metadata
                .get("ts")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let project_cwd = hit
                .metadata
                .get("project_cwd")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let session_short: String = hit.session_id.chars().take(8).collect();
            let preview = truncate_md(&hit.content, INBOX_MD_CONTENT_LIMIT);
            md.push_str(&format!("- {preview}\n"));
            let mut meta_parts: Vec<String> = Vec::new();
            if !kind_str.is_empty() {
                meta_parts.push(format!("kind `{kind_str}`"));
            }
            meta_parts.push(format!("session `{session_short}`"));
            if !project_cwd.is_empty() {
                meta_parts.push(format!("project `{project_cwd}`"));
            }
            if !ts.is_empty() {
                meta_parts.push(format!("ts {ts}"));
            }
            md.push_str(&format!("  _{}_\n", meta_parts.join(" · ")));
        }
        md.push('\n');
    }
    md
}

/// Truncate `s` to at most `limit` chars (not bytes), collapse newlines to
/// spaces, append ellipsis when truncated. Local mirror of the same helper
/// in `sessions.rs` + `prompt_context.rs`; not yet worth a cross-module
/// extraction at three call sites.
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

/// Decide which memory kinds belong in the user's attention queue.
///
/// Three eligible kinds — keep the frontend filter in
/// `web/src/hooks/useInbox.ts` and the CLI renderer in
/// `src/inbox/mod.rs` in sync with this list:
///
/// - **`user_action`** — explicit `<user_action>` tag from a Claude
///   session. Always inbox-worthy.
/// - **`todo`** — extracted task / open item. Empirically these read as
///   "the user needs to do X" even without a tag (e.g. researcher
///   autopilot emits "User picks: ship / iterate / suspend"). A `todo`
///   carrying `task_status == "completed"` is dropped: the work is
///   already done, so it has no place in an attention queue. (The
///   ingest pipeline re-emits the same `(session_id, kind, content)`
///   triple with the new `task_status`, and the view-layer dedupe at
///   the bottom of `list_inbox` keeps the freshest copy — so once a
///   task flips to completed, this filter retires its inbox row.)
/// - **`decision`** — only when `awaiting_decision: true`, since
///   already-resolved decisions are historical, not actionable.
///
/// Any other kind (accomplishment, learning, state, goal_phase, …) is
/// historical / contextual and excluded.
pub(crate) fn is_inbox_eligible(meta: &HashMap<String, serde_json::Value>) -> bool {
    match meta.get("kind").and_then(|v| v.as_str()) {
        Some("user_action") => true,
        Some("todo") => {
            // Drop completed todos; everything else (pending,
            // in_progress, failed, or missing task_status entirely)
            // stays inbox-eligible. `failed` is deliberately kept —
            // regressions are valuable attention signal.
            !matches!(
                meta.get("task_status").and_then(|v| v.as_str()),
                Some("completed")
            )
        }
        Some("decision") => meta
            .get("awaiting_decision")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        _ => false,
    }
}

/// Build the inbox router. Mounted alongside the tools / cc_hooks /
/// sessions routers in `create_simple_app`.
pub fn create_inbox_router() -> Router<ContextNestServices> {
    Router::new().route("/api/v1/inbox", get(list_inbox))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta(kv: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
        kv.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn user_action_is_eligible() {
        assert!(is_inbox_eligible(&meta(&[("kind", json!("user_action"))])));
    }

    #[test]
    fn todo_is_eligible() {
        // researcher-style ingest emits `todo` for "user needs to do X"
        // records that would otherwise be tagged `user_action` explicitly.
        assert!(is_inbox_eligible(&meta(&[("kind", json!("todo"))])));
    }

    #[test]
    fn todo_pending_is_eligible() {
        assert!(is_inbox_eligible(&meta(&[
            ("kind", json!("todo")),
            ("task_status", json!("pending")),
        ])));
    }

    #[test]
    fn todo_in_progress_is_eligible() {
        assert!(is_inbox_eligible(&meta(&[
            ("kind", json!("todo")),
            ("task_status", json!("in_progress")),
        ])));
    }

    #[test]
    fn todo_failed_is_eligible() {
        // Regressions are useful attention signal — keep them visible.
        assert!(is_inbox_eligible(&meta(&[
            ("kind", json!("todo")),
            ("task_status", json!("failed")),
        ])));
    }

    #[test]
    fn todo_completed_is_ineligible() {
        // Regression: a completed task should not occupy an inbox slot.
        // This is the bug that caused "Add BE route POST
        // /api/publisher-style-synthesis/:jobId" to keep showing 14
        // minutes after the assistant marked task #22 done.
        assert!(!is_inbox_eligible(&meta(&[
            ("kind", json!("todo")),
            ("task_status", json!("completed")),
        ])));
    }

    #[test]
    fn decision_awaiting_is_eligible() {
        assert!(is_inbox_eligible(&meta(&[
            ("kind", json!("decision")),
            ("awaiting_decision", json!(true)),
        ])));
    }

    #[test]
    fn decision_not_awaiting_is_ineligible() {
        assert!(!is_inbox_eligible(&meta(&[
            ("kind", json!("decision")),
            ("awaiting_decision", json!(false)),
        ])));
    }

    #[test]
    fn decision_without_awaiting_flag_is_ineligible() {
        assert!(!is_inbox_eligible(&meta(&[("kind", json!("decision"))])));
    }

    #[test]
    fn unknown_kind_is_ineligible() {
        assert!(!is_inbox_eligible(&meta(&[("kind", json!("note"))])));
    }

    #[test]
    fn missing_kind_is_ineligible() {
        assert!(!is_inbox_eligible(&HashMap::new()));
    }

    fn hit(content: &str, urgency: Option<&str>, kind: &str, session: &str) -> InboxHit {
        let mut m: HashMap<String, serde_json::Value> = HashMap::new();
        m.insert("kind".to_string(), json!(kind));
        if let Some(u) = urgency {
            m.insert("urgency".to_string(), json!(u));
        }
        m.insert("ts".to_string(), json!("2026-05-31T10:00:00Z"));
        m.insert("project_cwd".to_string(), json!("/repo/x"));
        InboxHit {
            id: format!("frag-{content}"),
            session_id: session.to_string(),
            content: content.to_string(),
            importance: 0.5,
            metadata: m,
        }
    }

    #[test]
    fn markdown_empty_renders_explanation() {
        let md = render_inbox_markdown(None, None, None, &[]);
        assert!(md.starts_with("# Attention Inbox\n"));
        assert!(md.contains("0 items"));
        assert!(md.contains("_(no items matched the filters)_"));
    }

    #[test]
    fn markdown_groups_by_urgency_in_priority_order() {
        // Mix of urgencies + a missing one. Buckets must render in
        // `now → soon → later → Unspecified` regardless of insertion order.
        let items = vec![
            hit(
                "Pick a deployment target",
                Some("later"),
                "decision",
                "sess-A1234567",
            ),
            hit(
                "Run wrangler login",
                Some("now"),
                "user_action",
                "sess-B7654321",
            ),
            hit("Set GITHUB_TOKEN", None, "todo", "sess-C0000000"),
            hit(
                "Resume the cluster",
                Some("soon"),
                "user_action",
                "sess-D9999999",
            ),
        ];
        let md = render_inbox_markdown(None, None, None, &items);

        // All four buckets present.
        let now_pos = md.find("## Now (1)").expect("now heading");
        let soon_pos = md.find("## Soon (1)").expect("soon heading");
        let later_pos = md.find("## Later (1)").expect("later heading");
        let unspec_pos = md.find("## Unspecified (1)").expect("unspecified heading");
        // Priority order strictly respected.
        assert!(now_pos < soon_pos);
        assert!(soon_pos < later_pos);
        assert!(later_pos < unspec_pos);
        // Each item carries kind/session/project metadata line.
        assert!(md.contains("Run wrangler login"));
        assert!(md.contains("kind `user_action`"));
        assert!(md.contains("session `sess-B76`"));
        assert!(md.contains("project `/repo/x`"));
    }

    #[test]
    fn markdown_header_surfaces_active_filters() {
        let items = vec![hit("x", Some("now"), "user_action", "sess-12345678")];
        let md = render_inbox_markdown(Some("/repo/x"), Some("now"), Some("user_action"), &items);
        // Filters appear in the meta line so a paste-into-prompt audience
        // can see what scope the digest is over.
        assert!(md.contains("project ~ `/repo/x`"));
        assert!(md.contains("urgency `now`"));
        assert!(md.contains("kind `user_action`"));
    }

    #[test]
    fn truncate_md_caps_chars_not_bytes_and_collapses_newlines() {
        assert_eq!(truncate_md("hello", 10), "hello");
        assert_eq!(truncate_md("a\nb\rc", 10), "a b c");
        let long_emoji = "🦀".repeat(300);
        let out = truncate_md(&long_emoji, 240);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), 241);
    }
}
