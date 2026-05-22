//! Turn parsed Claude Code events into substrate-ready [`MemoryRecord`]s.
//!
//! The extractor's job: pull every signal worth remembering out of a
//! session's event stream and emit it as a flat list of records with
//! structured metadata. The downstream sink (HTTP / dry-run / in-process)
//! just shuttles records to storage — it does no domain reasoning.
//!
//! Phase 1 (this PR) uses **50% token-overlap** clustering for
//! `goal_phase` (see [`CLUSTER_SIMILARITY_THRESHOLD`]). Phase 2 (epic
//! `docs/roadmap/epics/cc-ingest/E-embedding-clustering.md`) swaps in
//! embedding cosine similarity via `EmbeddingService`.

use serde_json::Value;
use std::collections::{HashMap, HashSet};

use super::event::{extract_zinsight_blocks, RawEvent};

/// What kind of memory a record represents. Becomes
/// `metadata["kind"]` on the stored memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryKind {
    /// Session-level theme from `ai-title` events.
    SessionTitle,
    /// A clustered phase of consecutive z-insight goals sharing intent.
    GoalPhase,
    /// First few user turns of the session — raw context, NOT a goal.
    InitialPromptWindow,
    /// One bullet from `z-insight.top_jobs[]`.
    Accomplishment,
    /// One bullet from `z-insight.facts[]`.
    Learning,
    /// One task from `z-insight.tasks[]` (after dedup-to-final-status).
    Todo,
    /// One step from `z-insight.requires_user_action[]`.
    UserAction,
    /// `z-insight.decision` text, paired with `awaiting_decision: true`.
    Decision,
    /// One bullet from `z-insight.blockers[]`.
    Blocker,
    /// `z-insight.current_state` — one per turn, low-importance.
    State,
    /// `z-insight.current_task` — one per turn, low-importance.
    CurrentTask,
    /// `summary` events written when /clear fires.
    Summary,
    /// Aggregate list of files this session edited or created. One record
    /// per session; `metadata.files` carries the deduplicated path array.
    /// Lets downstream queries answer "which session touched X" without
    /// grepping raw transcripts. Captured from `tool_use` events with
    /// names `Edit` / `Write` / `MultiEdit` / `NotebookEdit` —
    /// read-only `Read` is intentionally NOT included because "looked at"
    /// is a different signal from "changed".
    FilesTouched,
    /// One feature/deliverable declared by the assistant in a
    /// `z-insight.delivered_features[]` entry. The assistant's own
    /// summary of what shipped this turn — higher-signal than walking
    /// raw `tool_use` calls, because feature naming is the agent's job
    /// not the substrate's. `metadata.files` carries the agent's
    /// optional `files` array (the files THE AGENT believes the
    /// feature lives in); `metadata.refs` carries any free-form
    /// pointers like commit hashes or PR numbers.
    Feature,
}

impl MemoryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionTitle => "session_title",
            Self::GoalPhase => "goal_phase",
            Self::InitialPromptWindow => "initial_prompt_window",
            Self::Accomplishment => "accomplishment",
            Self::Learning => "learning",
            Self::Todo => "todo",
            Self::UserAction => "user_action",
            Self::Decision => "decision",
            Self::Blocker => "blocker",
            Self::State => "state",
            Self::CurrentTask => "current_task",
            Self::Summary => "summary",
            Self::FilesTouched => "files_touched",
            Self::Feature => "feature",
        }
    }

    /// Default importance for memories of this kind. The sink may override
    /// per-record if downstream tuning requires.
    pub fn default_importance(&self) -> f32 {
        match self {
            Self::Summary => 0.95,
            Self::SessionTitle => 0.85,
            Self::GoalPhase => 0.85,
            Self::Learning => 0.80,
            Self::Decision => 0.85,
            Self::Accomplishment => 0.75,
            Self::Todo => 0.70,
            Self::UserAction => 0.80,
            Self::Blocker => 0.80,
            Self::CurrentTask => 0.55,
            Self::State => 0.50,
            Self::InitialPromptWindow => 0.45,
            // Files touched is durable structural data — it should survive
            // through the decay window so "which session edited X" answers
            // months later still work.
            Self::FilesTouched => 0.85,
            // Features are the highest-signal artefact a session can leave
            // behind — they're literally the answer to "what did this
            // session ship". Importance just below summary.
            Self::Feature => 0.90,
        }
    }
}

/// A single memory record ready to be pushed into the substrate.
#[derive(Debug, Clone)]
pub struct MemoryRecord {
    pub kind: MemoryKind,
    pub text: String,
    pub importance: f32,
    pub session_id_cn: String,
    pub metadata: HashMap<String, Value>,
}

impl MemoryRecord {
    pub fn new(kind: MemoryKind, text: String, session_id_cn: String) -> Self {
        Self {
            kind,
            text,
            importance: kind.default_importance(),
            session_id_cn,
            metadata: HashMap::new(),
        }
    }

    pub fn with_meta(mut self, key: &str, value: Value) -> Self {
        self.metadata.insert(key.to_string(), value);
        self
    }
}

/// Extract every memory worth storing from a parsed session's event stream.
///
/// `session_uuid` is the full Claude Code session UUID. `project_cwd` is
/// the reconstructed project path (empty string if unknown). The substrate
/// session id is `cc-<full-uuid>` — the `cc-` prefix tags the ingest
/// source (Claude Code) and the full UUID guarantees no aliasing across
/// sessions whose UUIDs happen to share their first 8 chars. The old
/// `cc-<first-8>` form is migrated to this canonical shape at WAL
/// replay time (see `bootstrap_wal` in `bin/contextnest.rs`).
pub fn extract_memories(
    events: &[RawEvent],
    session_uuid: &str,
    project_cwd: &str,
) -> Vec<MemoryRecord> {
    let cn_session_id = format!("cc-{session_uuid}");
    let mut out = Vec::new();

    // 1. session_title from the first non-empty ai-title.
    if let Some(title) = events
        .iter()
        .find_map(|e| e.ai_title.as_deref().filter(|s| !s.is_empty()))
    {
        let mut rec = MemoryRecord::new(
            MemoryKind::SessionTitle,
            title.to_string(),
            cn_session_id.clone(),
        );
        rec = annotate_session_meta(rec, session_uuid, project_cwd, None);
        out.push(rec);
    }

    // 2. initial_prompt_window — concatenate first 3 user messages.
    let initial_prompts: Vec<(&str, Option<&str>)> = events
        .iter()
        .filter(|e| e.event_type == "user")
        .filter_map(|e| {
            let msg = e.message.as_ref()?;
            let text = msg.content.as_str()?;
            Some((text, e.timestamp.as_deref()))
        })
        .take(3)
        .collect();
    if !initial_prompts.is_empty() {
        let combined: String = initial_prompts
            .iter()
            .enumerate()
            .map(|(i, (t, _))| format!("[user turn {}] {}", i + 1, t))
            .collect::<Vec<_>>()
            .join("\n\n");
        let ts = initial_prompts.first().and_then(|(_, ts)| *ts);
        let mut rec = MemoryRecord::new(
            MemoryKind::InitialPromptWindow,
            combined,
            cn_session_id.clone(),
        );
        rec = annotate_session_meta(rec, session_uuid, project_cwd, ts);
        out.push(rec);
    }

    // 3. Walk every assistant turn, extract every z-insight block from
    // every text content part. Each block produces several memories.
    let mut z_goal_stream: Vec<(String, Option<String>)> = Vec::new(); // (text, ts)
    let mut z_task_final: HashMap<String, (Value, Option<String>)> = HashMap::new();
    // ^ task dedup keyed by id-if-present-else-subject, holding the latest
    // status seen for that task.

    // Files-touched aggregation. We walk every assistant message part —
    // when we see a `tool_use` whose `name` is a file-mutating tool we
    // pull `input.file_path` into a session-level dedup set. Cheap
    // (constant per part), and lets the substrate answer "which session
    // edited X.tsx" without grepping raw transcripts later. See
    // `FILE_MUTATING_TOOLS` below for the inclusion list — `Read` is
    // intentionally excluded because "looked at" is a different signal
    // from "changed".
    let mut files_touched: HashSet<String> = HashSet::new();
    let mut first_file_ts: Option<String> = None;

    for ev in events {
        if ev.event_type != "assistant" {
            continue;
        }
        let Some(msg) = &ev.message else { continue };
        let Some(parts) = msg.content.as_array() else {
            continue;
        };
        for part in parts {
            let part_type = part.get("type").and_then(Value::as_str);

            // Branch 1 — tool_use part: capture file paths for any
            // mutation-shaped tool (Edit/Write/MultiEdit/NotebookEdit).
            // No allocation on the common case where the tool doesn't
            // mutate files.
            if part_type == Some("tool_use") {
                if let Some(name) = part.get("name").and_then(Value::as_str) {
                    if FILE_MUTATING_TOOLS.contains(&name) {
                        if let Some(input) = part.get("input") {
                            if let Some(path) = input.get("file_path").and_then(Value::as_str) {
                                if !path.is_empty() {
                                    if files_touched.insert(path.to_string())
                                        && first_file_ts.is_none()
                                    {
                                        first_file_ts = ev.timestamp.clone();
                                    }
                                }
                            }
                            // NotebookEdit + MultiEdit also have edits arrays
                            // that reference the same top-level file_path,
                            // so no second pass needed here.
                        }
                    }
                }
                continue;
            }

            // Branch 2 — text part: existing z-insight extraction +
            // (new) delivered_features extraction.
            if part_type != Some("text") {
                continue;
            }
            let Some(text) = part.get("text").and_then(Value::as_str) else {
                continue;
            };
            for block in extract_zinsight_blocks(text) {
                // delivered_features[] — one Feature record per entry.
                // Cheap: most blocks don't carry this field.
                if let Some(features) = block.get("delivered_features").and_then(Value::as_array) {
                    for feat in features {
                        let name = feat
                            .get("feature")
                            .or_else(|| feat.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim();
                        if name.is_empty() {
                            continue;
                        }
                        let mut rec = MemoryRecord::new(
                            MemoryKind::Feature,
                            name.to_string(),
                            cn_session_id.clone(),
                        );
                        rec = annotate_session_meta(
                            rec,
                            session_uuid,
                            project_cwd,
                            ev.timestamp.as_deref(),
                        );
                        if let Some(files_arr) = feat.get("files").and_then(Value::as_array) {
                            let filtered: Vec<Value> = files_arr
                                .iter()
                                .filter(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false))
                                .cloned()
                                .collect();
                            if !filtered.is_empty() {
                                rec = rec.with_meta("files", Value::Array(filtered));
                            }
                        }
                        if let Some(refs) = feat.get("refs").and_then(Value::as_array) {
                            if !refs.is_empty() {
                                rec = rec.with_meta("refs", Value::Array(refs.clone()));
                            }
                        }
                        if let Some(layer) = feat.get("layer").and_then(Value::as_str) {
                            if !layer.is_empty() {
                                rec = rec.with_meta("layer", Value::String(layer.to_string()));
                            }
                        }
                        // `how_to_test` — free-form recipe the agent
                        // believes will exercise the feature. Plain
                        // string so curl one-liners, cargo test
                        // commands, and "click bell icon then look
                        // for new row" instructions all fit without
                        // forcing the agent into a tagged-union
                        // schema decision at write time.
                        if let Some(how) = feat.get("how_to_test").and_then(Value::as_str) {
                            if !how.is_empty() {
                                rec = rec.with_meta("how_to_test", Value::String(how.to_string()));
                            }
                        }
                        // `defs` — symbol names the agent says
                        // implement the feature (e.g. `fn retrieve()`,
                        // `struct BasinSnapshot`). Free-form so the
                        // agent doesn't have to commit to a parsing
                        // convention up front. Empty array filtered to
                        // keep the metadata sidecar lean.
                        if let Some(defs_arr) = feat.get("defs").and_then(Value::as_array) {
                            let filtered: Vec<Value> = defs_arr
                                .iter()
                                .filter(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false))
                                .cloned()
                                .collect();
                            if !filtered.is_empty() {
                                rec = rec.with_meta("defs", Value::Array(filtered));
                            }
                        }
                        out.push(rec);
                    }
                }
                extract_block_memories(
                    &block,
                    ev.timestamp.as_deref(),
                    session_uuid,
                    project_cwd,
                    &cn_session_id,
                    &mut out,
                );
                // Stash goal + task data for post-processing.
                if let Some(goal) = block.get("goal").and_then(Value::as_str) {
                    if !goal.is_empty() {
                        z_goal_stream.push((goal.to_string(), ev.timestamp.clone()));
                    }
                }
                if let Some(tasks) = block.get("tasks").and_then(Value::as_array) {
                    for t in tasks {
                        let key = task_dedup_key(t);
                        if let Some(key) = key {
                            z_task_final.insert(key, (t.clone(), ev.timestamp.clone()));
                        }
                    }
                }
            }
        }
    }

    // 4. Phase-cluster the goal stream into goal_phase memories.
    for phase in cluster_goal_phases(&z_goal_stream) {
        let mut rec = MemoryRecord::new(MemoryKind::GoalPhase, phase.text, cn_session_id.clone());
        rec = annotate_session_meta(rec, session_uuid, project_cwd, phase.start_ts.as_deref());
        if let Some(end_ts) = &phase.end_ts {
            rec = rec.with_meta("end_ts", Value::String(end_ts.clone()));
        }
        rec = rec.with_meta(
            "turn_span",
            Value::Number(serde_json::Number::from(phase.turn_count as u64)),
        );
        out.push(rec);
    }

    // 5. Emit final-state todos.
    for (_, (task, ts)) in z_task_final {
        let subject = task
            .get("subject")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if subject.is_empty() {
            continue;
        }
        let status = task
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("pending");
        let mut rec = MemoryRecord::new(MemoryKind::Todo, subject, cn_session_id.clone());
        // Importance tuning for todos: completed slightly higher than pending,
        // failed slightly higher still (regressions are valuable signal).
        rec.importance = match status {
            "completed" => 0.75,
            "failed" => 0.80,
            "pending" | "in_progress" => 0.65,
            _ => 0.55,
        };
        rec = annotate_session_meta(rec, session_uuid, project_cwd, ts.as_deref());
        rec = rec.with_meta("task_status", Value::String(status.to_string()));
        if let Some(id) = task.get("id").and_then(Value::as_str) {
            if !id.is_empty() {
                rec = rec.with_meta("task_id", Value::String(id.to_string()));
            }
        }
        out.push(rec);
    }

    // 6. Summary events.
    for ev in events {
        if ev.event_type == "summary" {
            if let Some(text) = &ev.summary {
                if !text.is_empty() {
                    let mut rec =
                        MemoryRecord::new(MemoryKind::Summary, text.clone(), cn_session_id.clone());
                    rec = annotate_session_meta(
                        rec,
                        session_uuid,
                        project_cwd,
                        ev.timestamp.as_deref(),
                    );
                    out.push(rec);
                }
            }
        }
    }

    // 7. files_touched — one aggregate record per session. Skipped
    // entirely when no file-mutating tool ran (e.g. read-only research
    // sessions). The text is a human-readable summary so retrieve's
    // semantic match still has something to hit on; the structured
    // answer lives in `metadata.files`.
    if !files_touched.is_empty() {
        let mut files: Vec<String> = files_touched.into_iter().collect();
        files.sort();
        let preview = files.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
        let summary_text = if files.len() <= 8 {
            format!("session touched {} file(s): {}", files.len(), preview)
        } else {
            format!(
                "session touched {} file(s) including: {}…",
                files.len(),
                preview
            )
        };
        let mut rec = MemoryRecord::new(
            MemoryKind::FilesTouched,
            summary_text,
            cn_session_id.clone(),
        );
        rec = annotate_session_meta(rec, session_uuid, project_cwd, first_file_ts.as_deref());
        let files_value: Vec<Value> = files.into_iter().map(Value::String).collect();
        rec = rec.with_meta("files", Value::Array(files_value));
        out.push(rec);
    }

    out
}

/// Tool names whose presence in a `tool_use` part means "the agent
/// mutated a file." Read-only tools (e.g. `Read`, `Grep`, `Glob`) are
/// deliberately excluded — the substrate's job here is to record what
/// *changed*, not what was inspected.
const FILE_MUTATING_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// Pull memories from a single z-insight block. The goal/tasks streams are
/// collected in the caller for post-processing (clustering + dedup), so this
/// fn only emits the per-block kinds: state, current_task, accomplishment,
/// learning, decision, blocker, user_action.
#[allow(clippy::too_many_arguments)]
fn extract_block_memories(
    block: &Value,
    ts: Option<&str>,
    session_uuid: &str,
    project_cwd: &str,
    cn_session_id: &str,
    out: &mut Vec<MemoryRecord>,
) {
    let mk = |kind: MemoryKind, text: String| -> MemoryRecord {
        let r = MemoryRecord::new(kind, text, cn_session_id.to_string());
        annotate_session_meta(r, session_uuid, project_cwd, ts)
    };

    if let Some(state) = block.get("current_state").and_then(Value::as_str) {
        if !state.is_empty() {
            out.push(mk(MemoryKind::State, state.to_string()));
        }
    }

    if let Some(task) = block.get("current_task").and_then(Value::as_str) {
        if !task.is_empty() {
            out.push(mk(MemoryKind::CurrentTask, task.to_string()));
        }
    }

    if let Some(jobs) = block.get("top_jobs").and_then(Value::as_array) {
        for job in jobs {
            if let Some(text) = job.as_str() {
                if !text.is_empty() {
                    out.push(mk(MemoryKind::Accomplishment, text.to_string()));
                }
            }
        }
    }

    if let Some(facts) = block.get("facts").and_then(Value::as_array) {
        for fact in facts {
            if let Some(text) = fact.as_str() {
                if !text.is_empty() {
                    out.push(mk(MemoryKind::Learning, text.to_string()));
                }
            }
        }
    }

    let awaiting = block
        .get("awaiting_decision")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if awaiting {
        let decision_text = block
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if !decision_text.is_empty() {
            let mut rec = mk(MemoryKind::Decision, decision_text.clone());
            rec = rec.with_meta("awaiting_decision", Value::Bool(true));
            rec = rec.with_meta("decision_text", Value::String(decision_text));
            out.push(rec);
        }
    }

    if let Some(blockers) = block.get("blockers").and_then(Value::as_array) {
        for b in blockers {
            if let Some(text) = b.as_str() {
                if !text.is_empty() {
                    out.push(mk(MemoryKind::Blocker, text.to_string()));
                }
            }
        }
    }

    if let Some(actions) = block.get("requires_user_action").and_then(Value::as_array) {
        for a in actions {
            let Some(action) = a.get("action").and_then(Value::as_str) else {
                continue;
            };
            if action.is_empty() {
                continue;
            }
            let mut rec = mk(MemoryKind::UserAction, action.to_string());
            if let Some(reason) = a.get("reason").and_then(Value::as_str) {
                rec = rec.with_meta("reason", Value::String(reason.to_string()));
            }
            if let Some(urgency) = a.get("urgency").and_then(Value::as_str) {
                rec = rec.with_meta("urgency", Value::String(urgency.to_string()));
            }
            if let Some(step) = a.get("step").and_then(Value::as_u64) {
                rec = rec.with_meta("step", Value::Number(serde_json::Number::from(step)));
            }
            out.push(rec);
        }
    }

    // epic_files lands as metadata on the state record above when present,
    // but we also store it as a list under the block-level metadata of any
    // record that came from this block. Keep it simple: stash on the most
    // recent record emitted (the state record, when present).
    if let Some(files) = block.get("epic_files").and_then(Value::as_array) {
        let owned: Vec<Value> = files.iter().filter(|v| v.is_string()).cloned().collect();
        if !owned.is_empty() {
            if let Some(last) = out.last_mut() {
                last.metadata
                    .insert("epic_files".to_string(), Value::Array(owned));
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct GoalPhaseBuilt {
    pub text: String,
    pub start_ts: Option<String>,
    pub end_ts: Option<String>,
    pub turn_count: usize,
}

/// Similarity threshold for the phase-1 token-overlap clusterer. Goals
/// whose overlap meets or exceeds this value are treated as the same
/// goal phase. Empirically, 0.50 separates paraphrases of the same
/// intent (typically 50-70% overlap) from genuine goal pivots
/// (typically <30% overlap) in real Claude Code transcripts.
pub(crate) const CLUSTER_SIMILARITY_THRESHOLD: f64 = 0.50;

/// Cluster consecutive goal strings sharing intent into one phase per
/// cluster. Phase 1 algorithm: token overlap (case-insensitive,
/// stop-word-stripped) between the next goal and the cluster's
/// representative; merge when overlap ≥ [`CLUSTER_SIMILARITY_THRESHOLD`].
///
/// The representative text starts as the first goal seen and updates to
/// whichever goal in the cluster is longest (longest usually carries the
/// most detail).
pub(crate) fn cluster_goal_phases(goals: &[(String, Option<String>)]) -> Vec<GoalPhaseBuilt> {
    let mut clusters: Vec<GoalPhaseBuilt> = Vec::new();
    for (text, ts) in goals {
        let push_new = match clusters.last_mut() {
            None => true,
            Some(cur) => {
                let overlap = token_overlap_pct(&cur.text, text);
                if overlap >= CLUSTER_SIMILARITY_THRESHOLD {
                    cur.turn_count += 1;
                    cur.end_ts = ts.clone();
                    if text.len() > cur.text.len() {
                        cur.text = text.clone();
                    }
                    false
                } else {
                    true
                }
            }
        };
        if push_new {
            clusters.push(GoalPhaseBuilt {
                text: text.clone(),
                start_ts: ts.clone(),
                end_ts: ts.clone(),
                turn_count: 1,
            });
        }
    }
    clusters
}

/// Token-overlap similarity in [0.0, 1.0]. Lowercases both strings, strips
/// a short stop-word list, then computes |intersect| / |smaller|.
fn token_overlap_pct(a: &str, b: &str) -> f64 {
    let a_set = tokenize(a);
    let b_set = tokenize(b);
    if a_set.is_empty() || b_set.is_empty() {
        return 0.0;
    }
    let smaller = a_set.len().min(b_set.len()) as f64;
    let intersect = a_set.intersection(&b_set).count() as f64;
    intersect / smaller
}

fn tokenize(s: &str) -> HashSet<String> {
    const STOP: &[&str] = &[
        "the", "a", "an", "and", "or", "but", "of", "in", "on", "at", "to", "for", "with", "by",
        "from", "as", "is", "are", "was", "were", "be", "been", "being", "this", "that", "these",
        "those", "it", "its", "i", "we", "you", "they", "he", "she", "into",
    ];
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !STOP.contains(w))
        .map(|w| w.to_string())
        .collect()
}

fn task_dedup_key(task: &Value) -> Option<String> {
    if let Some(id) = task.get("id").and_then(Value::as_str) {
        if !id.is_empty() {
            return Some(format!("id:{}", id));
        }
    }
    let subject = task.get("subject").and_then(Value::as_str)?;
    if subject.is_empty() {
        return None;
    }
    Some(format!("subj:{}", subject.to_lowercase()))
}

fn annotate_session_meta(
    mut rec: MemoryRecord,
    session_uuid: &str,
    project_cwd: &str,
    ts: Option<&str>,
) -> MemoryRecord {
    rec.metadata.insert(
        "kind".to_string(),
        Value::String(rec.kind.as_str().to_string()),
    );
    rec.metadata.insert(
        "src_session".to_string(),
        Value::String(session_uuid.to_string()),
    );
    if !project_cwd.is_empty() {
        rec.metadata.insert(
            "project_cwd".to_string(),
            Value::String(project_cwd.to_string()),
        );
    }
    if let Some(ts) = ts {
        rec.metadata
            .insert("ts".to_string(), Value::String(ts.to_string()));
    }
    rec
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev_assistant_with_text(text: &str, ts: &str) -> RawEvent {
        let line = format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"role":"assistant","content":[{{"type":"text","text":{} }}]}}}}"#,
            ts,
            serde_json::to_string(text).unwrap()
        );
        serde_json::from_str(&line).unwrap()
    }

    fn ev_user(text: &str, ts: &str) -> RawEvent {
        let line = format!(
            r#"{{"type":"user","timestamp":"{}","sessionId":"sess-1","cwd":"/work","message":{{"role":"user","content":{}}}}}"#,
            ts,
            serde_json::to_string(text).unwrap()
        );
        serde_json::from_str(&line).unwrap()
    }

    fn ev_ai_title(title: &str) -> RawEvent {
        let line = format!(
            r#"{{"type":"ai-title","sessionId":"sess-1","aiTitle":{}}}"#,
            serde_json::to_string(title).unwrap()
        );
        serde_json::from_str(&line).unwrap()
    }

    #[test]
    fn token_overlap_similar_goals_merge() {
        // Same intent, different wording, sharing topic anchors
        // (audit, codebase, rust). Overlap should clear 0.50.
        let p = token_overlap_pct(
            "Audit the codebase for unfinished Rust implementations",
            "Surface all incomplete Rust impls in the codebase audit",
        );
        assert!(
            p >= CLUSTER_SIMILARITY_THRESHOLD,
            "expected >= {}, got {}",
            CLUSTER_SIMILARITY_THRESHOLD,
            p
        );
    }

    #[test]
    fn token_overlap_distinct_goals_split() {
        let p = token_overlap_pct("Audit the codebase", "Run the test suite");
        assert!(
            p < CLUSTER_SIMILARITY_THRESHOLD,
            "expected < {}, got {}",
            CLUSTER_SIMILARITY_THRESHOLD,
            p
        );
    }

    #[test]
    fn cluster_merges_similar_consecutive() {
        let goals: Vec<(String, Option<String>)> = vec![
            (
                "Audit codebase for unfinished Rust impls".to_string(),
                Some("t1".into()),
            ),
            (
                "Surface unfinished Rust implementations in the codebase audit".to_string(),
                Some("t2".into()),
            ),
            (
                "Audit codebase finding incomplete Rust code".to_string(),
                Some("t3".into()),
            ),
            ("Run the test suite".to_string(), Some("t4".into())),
        ];
        let phases = cluster_goal_phases(&goals);
        assert_eq!(phases.len(), 2);
        assert_eq!(phases[0].turn_count, 3);
        assert_eq!(phases[1].turn_count, 1);
        // Representative is the longest of the cluster
        assert!(phases[0].text.contains("Surface"));
    }

    #[test]
    fn extracts_full_memory_set_from_realistic_session() {
        let z_block = r#"<z-insight>
{
  "domain":"backend",
  "goal":"Audit codebase for unfinished impls",
  "current_task":"Scanning files",
  "current_state":"Found 12 stubs so far",
  "top_jobs":["Scanned 94 Rust files"],
  "facts":["RSA Marvin has no upstream fix"],
  "tasks":[
    {"id":"T-1","subject":"Draft E1","status":"pending"},
    {"id":"T-1","subject":"Draft E1","status":"in_progress"},
    {"id":"T-2","subject":"Draft E2","status":"completed"}
  ],
  "blockers":["Need user input on E2 scope"],
  "awaiting_decision":true,
  "decision":"Should E2 strip auth entirely or gate behind feature flag?",
  "requires_user_action":[
    {"step":1,"action":"Review the audit report","reason":"feedback before E2","urgency":"now"},
    {"step":2,"action":"Decide on E2 strategy","reason":"unblocks implementation","urgency":"soon"}
  ],
  "epic_files":["src/audit.rs","docs/audit.md"]
}
</z-insight>"#;

        let events = vec![
            ev_ai_title("Audit codebase"),
            ev_user("check the codebase for issues", "t0"),
            ev_assistant_with_text(z_block, "t1"),
        ];

        let recs = extract_memories(&events, "sess-1-very-long-uuid", "/work");

        let kinds: Vec<&str> = recs.iter().map(|r| r.kind.as_str()).collect();

        // Expected memory presences:
        assert!(kinds.contains(&"session_title"), "session_title");
        assert!(
            kinds.contains(&"initial_prompt_window"),
            "initial_prompt_window"
        );
        assert!(kinds.contains(&"goal_phase"), "goal_phase");
        assert!(kinds.contains(&"state"), "state");
        assert!(kinds.contains(&"current_task"), "current_task");
        assert!(kinds.contains(&"accomplishment"), "accomplishment");
        assert!(kinds.contains(&"learning"), "learning");
        assert!(kinds.contains(&"todo"), "todo");
        assert!(kinds.contains(&"blocker"), "blocker");
        assert!(kinds.contains(&"decision"), "decision");
        assert!(kinds.contains(&"user_action"), "user_action");

        // Todos deduped: T-1 was seen twice (pending then in_progress) and T-2 once.
        // Final state must keep T-1 once (in_progress) + T-2 once (completed).
        let todos: Vec<_> = recs.iter().filter(|r| r.kind == MemoryKind::Todo).collect();
        assert_eq!(todos.len(), 2, "Todos deduped to 2 (T-1 and T-2)");

        // user_action records have urgency metadata
        let actions: Vec<_> = recs
            .iter()
            .filter(|r| r.kind == MemoryKind::UserAction)
            .collect();
        assert_eq!(actions.len(), 2);
        let urgencies: Vec<&str> = actions
            .iter()
            .filter_map(|r| r.metadata.get("urgency"))
            .filter_map(|v| v.as_str())
            .collect();
        assert!(urgencies.contains(&"now"));
        assert!(urgencies.contains(&"soon"));

        // Decision record has awaiting_decision + decision_text metadata
        let decision = recs
            .iter()
            .find(|r| r.kind == MemoryKind::Decision)
            .expect("decision record present");
        assert_eq!(
            decision.metadata.get("awaiting_decision"),
            Some(&Value::Bool(true))
        );
        assert!(decision.metadata.contains_key("decision_text"));

        // CN session id is `cc-<full-uuid>` — no truncation, no aliasing.
        assert_eq!(decision.session_id_cn, "cc-sess-1-very-long-uuid");
        for r in &recs {
            assert!(
                r.session_id_cn.starts_with("cc-"),
                "session id starts with cc-"
            );
            // Suffix is the entire uuid, byte-for-byte.
            assert_eq!(&r.session_id_cn["cc-".len()..], "sess-1-very-long-uuid");
        }

        // Every memory carries kind + src_session metadata
        for r in &recs {
            assert!(r.metadata.contains_key("kind"), "kind for {:?}", r.kind);
            assert_eq!(
                r.metadata.get("src_session"),
                Some(&Value::String("sess-1-very-long-uuid".to_string()))
            );
        }
    }

    #[test]
    fn session_with_no_zinsight_still_extracts_initial_prompt() {
        let events = vec![
            ev_user("first thing", "t0"),
            ev_user("second thing", "t1"),
            ev_assistant_with_text("hi (no z-insight block)", "t2"),
        ];
        let recs = extract_memories(&events, "deadbeef", "/x");
        let prompt = recs
            .iter()
            .find(|r| r.kind == MemoryKind::InitialPromptWindow);
        assert!(prompt.is_some());
        let p = prompt.unwrap();
        assert!(p.text.contains("first thing"));
        assert!(p.text.contains("second thing"));
    }

    #[test]
    fn short_uuid_does_not_panic() {
        // edge case: shorter than 8 chars
        let recs = extract_memories(&[ev_ai_title("title here")], "abc", "");
        assert!(!recs.is_empty());
        // CN session id should be "cc-abc" (truncated at uuid length, not panicking)
        assert_eq!(recs[0].session_id_cn, "cc-abc");
    }
}
