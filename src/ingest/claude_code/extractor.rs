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
use crate::error::ContextNestResult;
use crate::services::embedding::EmbeddingService;

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
    /// Session-level domain bucket from the z-insight block's top-level
    /// `domain` field (one of frontend|backend|research|ai-ml|infra|ops|
    /// tooling|tests|docs|data|design|other). Aggregated to ONE record
    /// per session, holding the LATEST non-empty value seen across the
    /// turn stream. `metadata.progress` carries the latest non-empty
    /// `progress` value (starting|in-progress|blocked|wrapping-up|idle|
    /// done) and `metadata.topics` carries the deduped union of every
    /// `topics[]` array the session emitted. Used by downstream routers
    /// (e.g. the z-dashboard categorizer) so they can fetch session
    /// metadata from CN instead of trusting their own local insight
    /// store.
    Domain,
    /// One item from `z-insight.read_context[]`: files/docs/transcripts the
    /// assistant inspected before deciding. Complements `FilesTouched`, which
    /// records mutations only.
    ReadContext,
    /// One item from `z-insight.verification[]`: command/manual/dry-run
    /// evidence and pass/fail/block status.
    Verification,
    /// One structured pointer from `z-insight.evidence_refs[]`.
    EvidenceRef,
    /// One settled decision from `z-insight.decisions[]`. Distinct from
    /// `Decision`, which means "awaiting the user's decision".
    DecisionMade,
    /// One error/recovery pattern from `z-insight.failures[]`.
    Failure,
    /// One compact instruction candidate from
    /// `z-insight.prompt_directives[]`.
    PromptDirective,
    /// One possibly-stale premise from `z-insight.assumptions[]`.
    Assumption,
    /// One non-feature artifact from `z-insight.artifacts[]`.
    Artifact,
    /// One candidate for later promotion into durable preference/rule/gotcha
    /// memory from `z-insight.memory_candidates[]`.
    MemoryCandidate,
    /// One high-consequence risk constraint from `z-insight.risk_flags[]`.
    RiskFlag,
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
            Self::Domain => "domain",
            Self::ReadContext => "read_context",
            Self::Verification => "verification",
            Self::EvidenceRef => "evidence_ref",
            Self::DecisionMade => "decision_made",
            Self::Failure => "failure",
            Self::PromptDirective => "prompt_directive",
            Self::Assumption => "assumption",
            Self::Artifact => "artifact",
            Self::MemoryCandidate => "memory_candidate",
            Self::RiskFlag => "risk_flag",
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
            // Domain is durable session-level metadata — one aggregate
            // record per session, used as the primary axis by the
            // z-dashboard categorizer. Same tier as SessionTitle/GoalPhase.
            Self::Domain => 0.85,
            // Context that grounded a turn matters, but it is not itself
            // an instruction. Keep below learnings and touched files.
            Self::ReadContext => 0.65,
            // Verification is operationally important. Failed/blocked cases
            // get tuned per-record below; this default covers passed checks.
            Self::Verification => 0.75,
            // Evidence refs are mostly metadata anchors. They should be
            // retained, but rarely outrank the memory they support.
            Self::EvidenceRef => 0.60,
            // Settled decisions should outrank ordinary facts so future
            // prompts do not reopen already-resolved architecture choices.
            Self::DecisionMade => 0.90,
            // Failure/recovery traces are high-signal for trajectory
            // analysis and anti-pattern extraction.
            Self::Failure => 0.85,
            // Prompt directives are candidate L3 prompt memory.
            Self::PromptDirective => 0.95,
            // Assumptions are useful but intentionally lower confidence and
            // should usually be revalidated.
            Self::Assumption => 0.55,
            // Artifacts are durable outputs, but less important than shipped
            // features.
            Self::Artifact => 0.70,
            // Promotion candidates sit between facts and directives until
            // confirmed by repetition or user approval.
            Self::MemoryCandidate => 0.80,
            // Risk flags are high-consequence constraints for future prompt
            // capsules.
            Self::RiskFlag => 0.90,
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
/// session id is the bare UUID — the older `cc-<uuid>` and
/// `cc-<first-8>` forms are migrated to the bare-UUID shape at WAL
/// replay time (see `bootstrap_wal` in `bin/contextnest.rs`).
pub fn extract_memories(
    events: &[RawEvent],
    session_uuid: &str,
    project_cwd: &str,
) -> Vec<MemoryRecord> {
    let cn_session_id = session_uuid.to_string();
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

    // Session-level domain aggregation. We track:
    //   * latest_domain    — the LAST non-empty `block.domain` seen. The
    //                         agent's self-tag drifts as a session pivots
    //                         (frontend → infra → backend); the most-recent
    //                         answer is the most useful one for routing.
    //   * latest_progress  — same logic for `block.progress`.
    //   * latest_domain_ts — timestamp of the block that set latest_domain.
    //                         Used as the Domain record's `ts` so downstream
    //                         consumers can recency-sort.
    //   * topics_union     — every `topics[]` element ever emitted by the
    //                         session, deduped. Union (not last-seen) because
    //                         topics are typically stable cumulative tags
    //                         — "auth", "testing", "rust" — that all
    //                         describe what the session has been about.
    let mut latest_domain: Option<String> = None;
    let mut latest_progress: Option<String> = None;
    let mut latest_domain_ts: Option<String> = None;
    let mut topics_union: HashSet<String> = HashSet::new();

    // Provenance pre-pass: pair Bash invocations with their results so the
    // self-reported `verification[]` claims can be grounded (or contradicted)
    // against what actually ran. See `verification_provenance`. Built once
    // over the full stream because tool_result parts live in `user` events,
    // outside the assistant-only z-insight walk below.
    let bash_index = build_bash_outcome_index(events);

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
                    &bash_index,
                    &mut out,
                );
                // Track latest domain/progress + union topics for the
                // session-level Domain record emitted after the walk.
                if let Some(dom) = block.get("domain").and_then(Value::as_str) {
                    if !dom.is_empty() {
                        latest_domain = Some(dom.to_string());
                        latest_domain_ts = ev.timestamp.clone();
                    }
                }
                if let Some(prog) = block.get("progress").and_then(Value::as_str) {
                    if !prog.is_empty() {
                        latest_progress = Some(prog.to_string());
                    }
                }
                if let Some(topics) = block.get("topics").and_then(Value::as_array) {
                    for t in topics {
                        if let Some(s) = t.as_str() {
                            let trimmed = s.trim();
                            if !trimmed.is_empty() {
                                topics_union.insert(trimmed.to_string());
                            }
                        }
                    }
                }
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

    // 8. domain — one aggregate record per session, holding the latest
    // self-reported `domain` plus latest `progress` and the union of all
    // `topics[]` seen. Skipped entirely when the session never emitted a
    // domain (e.g. a pure terminal-source ingest). Drives downstream
    // routers that need session-level metadata without parsing every
    // memory record themselves.
    if let Some(domain_text) = latest_domain {
        let mut rec = MemoryRecord::new(MemoryKind::Domain, domain_text, cn_session_id.clone());
        rec = annotate_session_meta(rec, session_uuid, project_cwd, latest_domain_ts.as_deref());
        if let Some(prog) = latest_progress {
            rec = rec.with_meta("progress", Value::String(prog));
        }
        if !topics_union.is_empty() {
            let mut topics: Vec<String> = topics_union.into_iter().collect();
            topics.sort();
            // Cap at 12 — matches z-insight's max(8) plus a buffer for
            // sessions that span enough turns to enumerate a few extras.
            // Keeps the metadata sidecar lean even on long sessions.
            if topics.len() > 12 {
                topics.truncate(12);
            }
            let topics_value: Vec<Value> = topics.into_iter().map(Value::String).collect();
            rec = rec.with_meta("topics", Value::Array(topics_value));
        }
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
#[allow(clippy::too_many_arguments)]
fn extract_block_memories(
    block: &Value,
    ts: Option<&str>,
    session_uuid: &str,
    project_cwd: &str,
    cn_session_id: &str,
    bash_index: &[BashRun],
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

    extract_structured_array(
        block,
        "read_context",
        MemoryKind::ReadContext,
        &["salient", "reason", "path"],
        ts,
        session_uuid,
        project_cwd,
        cn_session_id,
        out,
    );
    // Verification gets a dedicated path (not the generic helper) because it
    // is the one z-insight kind whose claims we can ground against real Bash
    // output. Adds a `provenance` tier: observed / contradicted / partial /
    // claimed. See `verification_provenance`.
    extract_verifications(
        block,
        bash_index,
        ts,
        session_uuid,
        project_cwd,
        cn_session_id,
        out,
    );
    extract_structured_array(
        block,
        "evidence_refs",
        MemoryKind::EvidenceRef,
        &["claim", "ref"],
        ts,
        session_uuid,
        project_cwd,
        cn_session_id,
        out,
    );
    extract_structured_array(
        block,
        "decisions",
        MemoryKind::DecisionMade,
        &["decision", "text"],
        ts,
        session_uuid,
        project_cwd,
        cn_session_id,
        out,
    );
    extract_structured_array(
        block,
        "failures",
        MemoryKind::Failure,
        &["symptom", "root_cause", "recovery"],
        ts,
        session_uuid,
        project_cwd,
        cn_session_id,
        out,
    );
    extract_structured_array(
        block,
        "prompt_directives",
        MemoryKind::PromptDirective,
        &["directive", "trigger"],
        ts,
        session_uuid,
        project_cwd,
        cn_session_id,
        out,
    );
    extract_structured_array(
        block,
        "assumptions",
        MemoryKind::Assumption,
        &["assumption", "basis"],
        ts,
        session_uuid,
        project_cwd,
        cn_session_id,
        out,
    );
    extract_structured_array(
        block,
        "artifacts",
        MemoryKind::Artifact,
        &["path", "purpose"],
        ts,
        session_uuid,
        project_cwd,
        cn_session_id,
        out,
    );
    extract_structured_array(
        block,
        "memory_candidates",
        MemoryKind::MemoryCandidate,
        &["text"],
        ts,
        session_uuid,
        project_cwd,
        cn_session_id,
        out,
    );
    extract_structured_array(
        block,
        "risk_flags",
        MemoryKind::RiskFlag,
        &["risk", "mitigation"],
        ts,
        session_uuid,
        project_cwd,
        cn_session_id,
        out,
    );

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

#[allow(clippy::too_many_arguments)]
fn extract_structured_array(
    block: &Value,
    field: &str,
    kind: MemoryKind,
    text_keys: &[&str],
    ts: Option<&str>,
    session_uuid: &str,
    project_cwd: &str,
    cn_session_id: &str,
    out: &mut Vec<MemoryRecord>,
) {
    let Some(items) = block.get(field).and_then(Value::as_array) else {
        return;
    };

    for item in items {
        let Some(text) = structured_item_text(item, text_keys) else {
            continue;
        };
        let mut rec = MemoryRecord::new(kind, text, cn_session_id.to_string());
        rec.importance = structured_item_importance(kind, item);
        rec = annotate_session_meta(rec, session_uuid, project_cwd, ts);
        rec = copy_structured_item_metadata(rec, item);
        rec = rec.with_meta("zinsight_field", Value::String(field.to_string()));
        out.push(rec);
    }
}

fn structured_item_text(item: &Value, text_keys: &[&str]) -> Option<String> {
    if let Some(s) = item.as_str() {
        let trimmed = s.trim();
        return (!trimmed.is_empty()).then(|| trimmed.to_string());
    }

    let obj = item.as_object()?;
    for key in text_keys {
        if let Some(value) = obj.get(*key).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn structured_item_importance(kind: MemoryKind, item: &Value) -> f32 {
    match kind {
        MemoryKind::Verification => match item.get("status").and_then(Value::as_str) {
            Some("failed") | Some("blocked") => 0.85,
            Some("not_run") => 0.70,
            _ => kind.default_importance(),
        },
        MemoryKind::RiskFlag => match item.get("severity").and_then(Value::as_str) {
            Some("critical") | Some("high") => 0.95,
            Some("low") => 0.80,
            _ => kind.default_importance(),
        },
        MemoryKind::PromptDirective => match item.get("confidence").and_then(Value::as_str) {
            Some("low") => 0.80,
            Some("medium") => 0.90,
            _ => kind.default_importance(),
        },
        _ => kind.default_importance(),
    }
}

fn copy_structured_item_metadata(mut rec: MemoryRecord, item: &Value) -> MemoryRecord {
    let Some(obj) = item.as_object() else {
        return rec;
    };
    for (key, value) in obj {
        if value.is_null() {
            continue;
        }
        let meta_key = if key == "kind" { "item_kind" } else { key };
        rec.metadata.insert(meta_key.to_string(), value.clone());
    }
    rec
}

// ── Verification provenance (z-insight grounding) ───────────────────────────
//
// The `<z-insight>` block is the agent's self-report about its own turn, with
// no guarantee it matches what happened. `verification[]` is the highest-stakes
// kind: a turn can *claim* "tests pass" while the real tool output said they
// failed. Borrowing HarnessBridge's rule (a claim only stands if a real
// trajectory span backs it, else default to the safe state), we cross-check
// each verification claim against the actual Bash output and tag a provenance
// tier in metadata. This is a PROTOTYPE: the classifier is a cheap heuristic,
// not a shell-output parser — ambiguous output stays `Unknown` → `partial`.

/// Outcome of a Bash invocation inferred from its raw `tool_result` text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BashOutcome {
    Pass,
    Fail,
    Unknown,
}

/// One Bash command run paired with the outcome inferred from its result.
#[derive(Debug, Clone)]
pub(crate) struct BashRun {
    pub command: String,
    pub outcome: BashOutcome,
}

/// Pre-pass over the full event stream: pair each `tool_use{name:"Bash"}`
/// (carries `input.command` + an `id`) with its `tool_result` (lands in the
/// following `user` message under `tool_use_id`).
pub(crate) fn build_bash_outcome_index(events: &[RawEvent]) -> Vec<BashRun> {
    let mut pending: HashMap<String, String> = HashMap::new(); // id -> command
    let mut runs: Vec<BashRun> = Vec::new();

    for ev in events {
        let Some(msg) = &ev.message else { continue };
        let Some(parts) = msg.content.as_array() else {
            continue;
        };
        for part in parts {
            match part.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    if part.get("name").and_then(Value::as_str) == Some("Bash") {
                        if let (Some(id), Some(cmd)) = (
                            part.get("id").and_then(Value::as_str),
                            part.get("input")
                                .and_then(|i| i.get("command"))
                                .and_then(Value::as_str),
                        ) {
                            pending.insert(id.to_string(), cmd.to_string());
                        }
                    }
                }
                Some("tool_result") => {
                    if let Some(id) = part.get("tool_use_id").and_then(Value::as_str) {
                        if let Some(cmd) = pending.remove(id) {
                            runs.push(BashRun {
                                command: cmd,
                                outcome: classify_bash_output(&tool_result_text(part)),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
    runs
}

/// `tool_result.content` is either a plain string or an array of
/// `{type:"text", text:...}` parts. Flatten both to one string.
fn tool_result_text(part: &Value) -> String {
    match part.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Heuristic classification of Bash result text. Strong success markers are
/// checked first so `0 failed` / `test result: ok` aren't snagged by the
/// failure scan; then failure markers; then weak success. Anything else is
/// `Unknown` (we'd rather under-claim than guess).
fn classify_bash_output(text: &str) -> BashOutcome {
    let t = text.to_lowercase();
    const PASS_STRONG: &[&str] = &[
        "test result: ok",
        "0 failed",
        "0 errors",
        "build succeeded",
        "all tests passed",
    ];
    const FAIL: &[&str] = &[
        "error[",
        "error:",
        "panicked",
        "test result: failed",
        "failures:",
        "build failed",
        "fatal:",
        "command not found",
        "no such file",
        "traceback (most recent",
        "assertionerror",
        "exception",
    ];
    const PASS_WEAK: &[&str] = &[
        "passed",
        "succeeded",
        "finished `release`",
        "finished `dev`",
    ];

    if PASS_STRONG.iter().any(|m| t.contains(m)) {
        return BashOutcome::Pass;
    }
    if FAIL.iter().any(|m| t.contains(m)) {
        return BashOutcome::Fail;
    }
    if PASS_WEAK.iter().any(|m| t.contains(m)) {
        return BashOutcome::Pass;
    }
    BashOutcome::Unknown
}

/// Cross-check one `verification[]` item against the Bash outcome index.
///   `observed`     — a matching real run confirms the claimed status
///   `contradicted` — claim says pass but the matching run failed (money case)
///   `partial`      — a matching run exists but its outcome is Unknown
///   `claimed`      — no matching real run; the claim stands on self-report only
fn verification_provenance(item: &Value, index: &[BashRun]) -> &'static str {
    // No command to ground against ⇒ pure self-report.
    let Some(cmd) = item
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return "claimed";
    };
    let Some(run) = index.iter().find(|run| commands_match(&run.command, cmd)) else {
        return "claimed";
    };

    let claims_pass = match item.get("status").and_then(Value::as_str) {
        Some(s) => matches!(
            s.to_lowercase().as_str(),
            "pass" | "passed" | "ok" | "success" | "succeeded"
        ),
        // A verification entry with no explicit failure status reads as an
        // implicit "this checked out" — treat as a pass-claim for grounding.
        None => true,
    };

    match (claims_pass, run.outcome) {
        (true, BashOutcome::Fail) => "contradicted",
        (_, BashOutcome::Unknown) => "partial",
        _ => "observed",
    }
}

/// Fuzzy command match: trimmed substring either direction. The z-insight
/// `command` is often an abbreviation of the real invocation (`cargo test` vs
/// `cargo test --test seven_tools_api`), so containment beats equality.
fn commands_match(real: &str, claimed: &str) -> bool {
    let real = real.trim();
    let claimed = claimed.trim();
    !claimed.is_empty() && (real.contains(claimed) || claimed.contains(real))
}

/// Verification extraction with provenance grounding. Mirrors
/// `extract_structured_array` but adds the `provenance` tier.
#[allow(clippy::too_many_arguments)]
fn extract_verifications(
    block: &Value,
    bash_index: &[BashRun],
    ts: Option<&str>,
    session_uuid: &str,
    project_cwd: &str,
    cn_session_id: &str,
    out: &mut Vec<MemoryRecord>,
) {
    let Some(items) = block.get("verification").and_then(Value::as_array) else {
        return;
    };
    for item in items {
        let Some(text) = structured_item_text(item, &["summary", "command"]) else {
            continue;
        };
        let mut rec = MemoryRecord::new(MemoryKind::Verification, text, cn_session_id.to_string());
        rec.importance = structured_item_importance(MemoryKind::Verification, item);
        rec = annotate_session_meta(rec, session_uuid, project_cwd, ts);
        rec = copy_structured_item_metadata(rec, item);
        rec = rec.with_meta("zinsight_field", Value::String("verification".to_string()));
        rec = rec.with_meta(
            "provenance",
            Value::String(verification_provenance(item, bash_index).to_string()),
        );
        out.push(rec);
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

/// Cosine threshold for merging consecutive `goal_phase` records by
/// embedding similarity. Calibrated for normalized 768-d embeddings —
/// 0.85 catches "same intent, different wording" without collapsing
/// genuine intent pivots (see `E-embedding-clustering.md` analysis).
pub(crate) const EMBEDDING_CLUSTER_COSINE_THRESHOLD: f32 = 0.85;

/// Re-cluster consecutive `goal_phase` records by embedding cosine
/// similarity. Walks the existing record list in order; when two
/// consecutive `goal_phase` entries' embeddings clear
/// [`EMBEDDING_CLUSTER_COSINE_THRESHOLD`] they collapse into one —
/// representative text becomes the longer of the two, `start_ts` /
/// `end_ts` cover the union, and `turn_span` accumulates.
///
/// Non-goal_phase records pass through unchanged. When the embedder
/// fails on any text, that record is treated as "not similar to its
/// neighbour" — degraded mode falls back to the existing
/// token-overlap result from [`cluster_goal_phases`].
///
/// Why post-processing instead of inline in [`extract_memories`]:
/// extract_memories is synchronous and used by tests with no
/// EmbeddingService in scope. Doing the embedding pass here keeps
/// extract_memories simple and lets callers (CLI batch, cc-hooks
/// receiver) opt in based on whether they have an embedder available.
pub async fn refine_goal_phases_by_embedding(
    records: Vec<MemoryRecord>,
    embedder: &EmbeddingService,
) -> ContextNestResult<Vec<MemoryRecord>> {
    let mut goal_idxs: Vec<usize> = records
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == MemoryKind::GoalPhase)
        .map(|(i, _)| i)
        .collect();
    if goal_idxs.len() < 2 {
        return Ok(records);
    }

    // Sort goal indexes by start_ts so consecutive comparisons reflect
    // session-time order, not extractor emission order (mostly the
    // same, but defensive against future emit-order changes).
    goal_idxs.sort_by(|&a, &b| {
        let ta = records[a]
            .metadata
            .get("start_ts")
            .or_else(|| records[a].metadata.get("ts"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let tb = records[b]
            .metadata
            .get("start_ts")
            .or_else(|| records[b].metadata.get("ts"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        ta.cmp(tb)
    });

    // Embed each goal_phase's text once. Failure on any single
    // embedding falls back to "no merge for that pair" — we want this
    // refinement to be additive, never to drop records.
    let mut embeddings: Vec<Option<Vec<f32>>> = Vec::with_capacity(goal_idxs.len());
    for &idx in &goal_idxs {
        let embedded = embedder.generate_embedding(&records[idx].text).await.ok();
        embeddings.push(embedded);
    }

    // Walk consecutive pairs in start_ts order. For each adjacent
    // (i, i+1) whose cosine clears the threshold, record a merge
    // intent. After the pass, materialise the merges in one rebuild.
    let mut merge_target: Vec<usize> = (0..goal_idxs.len()).collect();
    for w in 0..goal_idxs.len().saturating_sub(1) {
        let (Some(a), Some(b)) = (&embeddings[w], &embeddings[w + 1]) else {
            continue;
        };
        if cosine(a, b) >= EMBEDDING_CLUSTER_COSINE_THRESHOLD {
            merge_target[w + 1] = merge_target[w];
        }
    }

    // Build the new record list: pass non-goal records through; merge
    // goal records per merge_target.
    let mut out: Vec<MemoryRecord> = Vec::with_capacity(records.len());
    let mut by_root: HashMap<usize, MemoryRecord> = HashMap::new();
    let mut root_order: Vec<usize> = Vec::new();
    let goal_set: HashSet<usize> = goal_idxs.iter().copied().collect();

    for (i, rec) in records.into_iter().enumerate() {
        if !goal_set.contains(&i) {
            out.push(rec);
            continue;
        }
        let pos = goal_idxs.iter().position(|&g| g == i).unwrap();
        let root_pos = merge_target[pos];
        let root_idx = goal_idxs[root_pos];
        match by_root.get_mut(&root_idx) {
            None => {
                by_root.insert(root_idx, rec);
                root_order.push(root_idx);
            }
            Some(existing) => {
                merge_into(existing, rec);
            }
        }
    }
    for root_idx in root_order {
        if let Some(rec) = by_root.remove(&root_idx) {
            out.push(rec);
        }
    }
    Ok(out)
}

fn merge_into(target: &mut MemoryRecord, other: MemoryRecord) {
    // Longer text wins as representative (carries more detail).
    if other.text.len() > target.text.len() {
        target.text = other.text;
    }
    // start_ts = earlier; end_ts = later.
    if let (Some(other_start), target_start) = (
        other.metadata.get("start_ts").and_then(|v| v.as_str()),
        target
            .metadata
            .get("start_ts")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
    ) {
        if target_start.is_empty() || other_start < target_start {
            target.metadata.insert(
                "start_ts".to_string(),
                Value::String(other_start.to_string()),
            );
        }
    }
    if let (Some(other_end), target_end) = (
        other.metadata.get("end_ts").and_then(|v| v.as_str()),
        target
            .metadata
            .get("end_ts")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
    ) {
        if target_end.is_empty() || other_end > target_end {
            target
                .metadata
                .insert("end_ts".to_string(), Value::String(other_end.to_string()));
        }
    }
    // Sum turn_span counts.
    let target_turns = target
        .metadata
        .get("turn_span")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);
    let other_turns = other
        .metadata
        .get("turn_span")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);
    target.metadata.insert(
        "turn_span".to_string(),
        Value::Number(serde_json::Number::from(target_turns + other_turns)),
    );
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
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

    // ── Verification-provenance prototype helpers + tests ───────────────────

    fn ev_bash_call(id: &str, command: &str, ts: &str) -> RawEvent {
        let part = serde_json::json!({
            "type": "tool_use", "id": id, "name": "Bash",
            "input": {"command": command}
        });
        let msg = serde_json::json!({"role": "assistant", "content": [part]});
        let ev = serde_json::json!({"type": "assistant", "timestamp": ts, "message": msg});
        serde_json::from_value(ev).unwrap()
    }

    fn ev_bash_result(id: &str, output: &str, ts: &str) -> RawEvent {
        let part = serde_json::json!({
            "type": "tool_result", "tool_use_id": id, "content": output
        });
        let msg = serde_json::json!({"role": "user", "content": [part]});
        let ev = serde_json::json!({"type": "user", "timestamp": ts, "sessionId": "sess-1", "message": msg});
        serde_json::from_value(ev).unwrap()
    }

    fn zinsight_verify(command: &str, status: &str, ts: &str) -> RawEvent {
        let payload = serde_json::json!({
            "goal": "x",
            "verification": [{"summary": "ran the suite", "command": command, "status": status}]
        });
        ev_assistant_with_text(&format!("<z-insight>{payload}</z-insight>"), ts)
    }

    fn verification_provenances(events: &[RawEvent]) -> Vec<String> {
        extract_memories(events, "sess-1", "/work")
            .into_iter()
            .filter(|r| r.kind == MemoryKind::Verification)
            .map(|r| {
                r.metadata
                    .get("provenance")
                    .and_then(Value::as_str)
                    .unwrap_or("<none>")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn verification_contradicted_when_bash_failed() {
        // Agent claims the suite passed, but the real run failed.
        let events = vec![
            ev_bash_call(
                "toolu_1",
                "cargo test --test seven_tools_api",
                "2026-01-01T00:00:00Z",
            ),
            ev_bash_result(
                "toolu_1",
                "running 5 tests\nerror[E0277]: mismatch\ntest result: FAILED. 2 passed; 3 failed",
                "2026-01-01T00:00:01Z",
            ),
            zinsight_verify("cargo test", "passed", "2026-01-01T00:00:02Z"),
        ];
        assert_eq!(verification_provenances(&events), vec!["contradicted"]);
    }

    #[test]
    fn verification_observed_when_bash_passed() {
        let events = vec![
            ev_bash_call("toolu_2", "cargo test", "2026-01-01T00:00:00Z"),
            ev_bash_result(
                "toolu_2",
                "running 5 tests\ntest result: ok. 5 passed; 0 failed",
                "2026-01-01T00:00:01Z",
            ),
            zinsight_verify("cargo test", "passed", "2026-01-01T00:00:02Z"),
        ];
        assert_eq!(verification_provenances(&events), vec!["observed"]);
    }

    #[test]
    fn verification_claimed_when_no_matching_run() {
        // No Bash run at all — the claim stands only on self-report.
        let events = vec![zinsight_verify(
            "cargo test",
            "passed",
            "2026-01-01T00:00:02Z",
        )];
        assert_eq!(verification_provenances(&events), vec!["claimed"]);
    }

    #[test]
    fn verification_partial_when_outcome_unknown() {
        // A matching run exists but its output isn't classifiable.
        let events = vec![
            ev_bash_call("toolu_3", "./scripts/smoke.sh", "2026-01-01T00:00:00Z"),
            ev_bash_result("toolu_3", "doing stuff... done", "2026-01-01T00:00:01Z"),
            zinsight_verify("./scripts/smoke.sh", "passed", "2026-01-01T00:00:02Z"),
        ];
        assert_eq!(verification_provenances(&events), vec!["partial"]);
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

        // CN session id is the bare Claude Code UUID — no prefix, no truncation.
        assert_eq!(decision.session_id_cn, "sess-1-very-long-uuid");
        for r in &recs {
            assert!(
                !r.session_id_cn.starts_with("cc-"),
                "session id should not carry the legacy cc- prefix"
            );
            assert_eq!(r.session_id_cn, "sess-1-very-long-uuid");
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
    fn extracts_trajectory_signal_arrays_from_zinsight() {
        let z_block = r#"<z-insight>
{
  "domain":"backend",
  "goal":"Improve Claude ingest trajectory memory",
  "current_state":"Implementing optional signal arrays",
  "read_context":[
    {
      "path":"docs/z-insight-schema.md",
      "kind":"doc",
      "reason":"Understand current schema",
      "salient":"Existing parser already accepts unknown fields",
      "refs":["docs/z-insight-schema.md:70"]
    }
  ],
  "verification":[
    {
      "kind":"dry_run",
      "command":"cargo run -- ingest claude-code --dry-run",
      "status":"passed",
      "summary":"Dry-run extracted 86 memories",
      "counts":{"memories":86,"failures":0}
    },
    {
      "kind":"curl",
      "command":"curl http://localhost:28080/api/v1/sessions/x/summary",
      "status":"failed",
      "summary":"Server stopped accepting connections"
    }
  ],
  "evidence_refs":[
    {"kind":"file","ref":"src/ingest/claude_code/extractor.rs:214","claim":"Extractor walks assistant turns"}
  ],
  "decisions":[
    {"decision":"Use optional z-insight arrays first","made_by":"assistant","scope":"project"}
  ],
  "failures":[
    {"symptom":"summary curl failed","root_cause":"server unavailable","recovery":"retry after substrate restart","status":"open"}
  ],
  "prompt_directives":[
    {"trigger":"When changing ingest schema","directive":"Run dry-run ingest before storage changes","scope":"project","confidence":"high"}
  ],
  "assumptions":[
    {"assumption":"CLI success should be rechecked through API","basis":"HTTP summary failed later","valid_until":"next runtime check"}
  ],
  "artifacts":[
    {"kind":"doc","path":"docs/roadmap/v0.4-z-insight-trajectory-signals.md","purpose":"Store implementation plan","status":"created"}
  ],
  "memory_candidates":[
    {"kind":"workflow","text":"For ContextNest ingest work, run dry-run ingest before changing extractor schema","scope":"project","confidence":"medium"}
  ],
  "risk_flags":[
    {"risk":"Live WAL migration can rewrite user data","severity":"high","mitigation":"Require backup before rewrite","scope":"project"}
  ]
}
</z-insight>"#;

        let events = vec![ev_assistant_with_text(z_block, "t1")];
        let recs = extract_memories(&events, "sess-trajectory", "/work");

        for kind in [
            MemoryKind::ReadContext,
            MemoryKind::EvidenceRef,
            MemoryKind::DecisionMade,
            MemoryKind::Failure,
            MemoryKind::PromptDirective,
            MemoryKind::Assumption,
            MemoryKind::Artifact,
            MemoryKind::MemoryCandidate,
            MemoryKind::RiskFlag,
        ] {
            assert!(
                recs.iter().any(|r| r.kind == kind),
                "expected {:?} memory",
                kind
            );
        }

        let verifications: Vec<_> = recs
            .iter()
            .filter(|r| r.kind == MemoryKind::Verification)
            .collect();
        assert_eq!(verifications.len(), 2);
        assert!(verifications.iter().any(|r| {
            r.text == "Dry-run extracted 86 memories"
                && r.metadata.get("status").and_then(Value::as_str) == Some("passed")
                && r.importance == 0.75
        }));
        assert!(verifications.iter().any(|r| {
            r.text == "Server stopped accepting connections"
                && r.metadata.get("status").and_then(Value::as_str) == Some("failed")
                && r.importance == 0.85
        }));

        let read_context = recs
            .iter()
            .find(|r| r.kind == MemoryKind::ReadContext)
            .expect("read_context memory");
        assert_eq!(
            read_context.metadata.get("kind").and_then(Value::as_str),
            Some("read_context")
        );
        assert_eq!(
            read_context
                .metadata
                .get("item_kind")
                .and_then(Value::as_str),
            Some("doc")
        );
        assert_eq!(
            read_context
                .metadata
                .get("zinsight_field")
                .and_then(Value::as_str),
            Some("read_context")
        );

        let risk = recs
            .iter()
            .find(|r| r.kind == MemoryKind::RiskFlag)
            .expect("risk flag memory");
        assert_eq!(risk.importance, 0.95);
        assert_eq!(
            risk.metadata.get("severity").and_then(Value::as_str),
            Some("high")
        );
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
        // CN session id mirrors the raw uuid byte-for-byte (no prefix).
        assert_eq!(recs[0].session_id_cn, "abc");
    }

    fn goal_phase_record(text: &str, ts: &str) -> MemoryRecord {
        let mut rec =
            MemoryRecord::new(MemoryKind::GoalPhase, text.to_string(), "sess".to_string());
        rec.metadata
            .insert("start_ts".to_string(), Value::String(ts.to_string()));
        rec.metadata
            .insert("end_ts".to_string(), Value::String(ts.to_string()));
        rec.metadata.insert(
            "turn_span".to_string(),
            Value::Number(serde_json::Number::from(1u64)),
        );
        rec
    }

    /// With the deterministic mock embedder, identical input yields
    /// identical embeddings → cosine 1.0 → merge. Two GoalPhase records
    /// with the same text collapse to one with summed turn_span.
    #[tokio::test]
    async fn refine_merges_identical_goal_texts() {
        use crate::config::EmbeddingServicesConfig;
        use crate::services::embedding::EmbeddingService;
        let emb = EmbeddingService::new(EmbeddingServicesConfig::default())
            .expect("mock embedder builds");
        let records = vec![
            goal_phase_record("Implement feature X", "2026-05-28T09:00:00Z"),
            goal_phase_record("Implement feature X", "2026-05-28T09:10:00Z"),
        ];
        let refined = refine_goal_phases_by_embedding(records, &emb)
            .await
            .unwrap();
        let goals: Vec<_> = refined
            .iter()
            .filter(|r| r.kind == MemoryKind::GoalPhase)
            .collect();
        assert_eq!(
            goals.len(),
            1,
            "identical-text goals must merge; got {} goals",
            goals.len()
        );
        let span = goals[0]
            .metadata
            .get("turn_span")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        assert_eq!(span, 2, "merged turn_span should sum, got {}", span);
        let start = goals[0]
            .metadata
            .get("start_ts")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let end = goals[0]
            .metadata
            .get("end_ts")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_eq!(start, "2026-05-28T09:00:00Z", "earliest start_ts wins");
        assert_eq!(end, "2026-05-28T09:10:00Z", "latest end_ts wins");
    }

    /// With a single GoalPhase the function is a no-op (no pair to
    /// compare). Verifies the early-return path.
    #[tokio::test]
    async fn refine_single_goal_is_noop() {
        use crate::config::EmbeddingServicesConfig;
        use crate::services::embedding::EmbeddingService;
        let emb = EmbeddingService::new(EmbeddingServicesConfig::default())
            .expect("mock embedder builds");
        let records = vec![goal_phase_record("Single goal", "2026-05-28T09:00:00Z")];
        let refined = refine_goal_phases_by_embedding(records, &emb)
            .await
            .unwrap();
        assert_eq!(refined.len(), 1);
        assert_eq!(refined[0].kind, MemoryKind::GoalPhase);
    }

    /// Non-goal records (learnings, todos, etc.) pass through unchanged
    /// regardless of clustering — refinement only touches GoalPhase.
    #[tokio::test]
    async fn refine_preserves_non_goal_records() {
        use crate::config::EmbeddingServicesConfig;
        use crate::services::embedding::EmbeddingService;
        let emb = EmbeddingService::new(EmbeddingServicesConfig::default())
            .expect("mock embedder builds");
        let mut learn = MemoryRecord::new(
            MemoryKind::Learning,
            "Learned something".to_string(),
            "sess".to_string(),
        );
        learn.metadata.insert(
            "ts".to_string(),
            Value::String("2026-05-28T09:05:00Z".into()),
        );
        let records = vec![
            goal_phase_record("Goal A", "2026-05-28T09:00:00Z"),
            learn,
            goal_phase_record("Goal A", "2026-05-28T09:10:00Z"),
        ];
        let refined = refine_goal_phases_by_embedding(records, &emb)
            .await
            .unwrap();
        let learnings = refined
            .iter()
            .filter(|r| r.kind == MemoryKind::Learning)
            .count();
        assert_eq!(learnings, 1, "Learning record must pass through");
    }

    #[test]
    fn cosine_handles_degenerate_inputs() {
        assert_eq!(cosine(&[], &[]), 0.0);
        assert_eq!(cosine(&[1.0, 2.0], &[1.0]), 0.0, "mismatched length → 0");
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0, "zero vector → 0");
        let v = vec![1.0, 0.0, 0.0];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6, "self-cosine ≈ 1");
    }
}
