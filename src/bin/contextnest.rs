//! ContextNest CLI + server entrypoint.
//! Single command in v0.1.0:
//! - `contextnest serve [--bind ADDR]` — start the HTTP server exposing the
//!   seven-tool memory API (`/api/v1/tools/*`), health, and status endpoints.
//! `field` / `test` / `status` subcommands were removed
//! after they were found to print "not wired up in this build" and exit 0
//! silently — breaking CI scripts that probed the exit code. Re-introduce
//! as feature-gated commands when implementations land.

use clap::Parser;
use contextnest::api::create_app;
use contextnest::cli::{
    CheckpointCommands, Cli, Commands, IngestSource, McpCommands, PromptContextCommands,
};
use contextnest::config::Config;
use contextnest::inbox::{render_json, render_markdown, render_text, InboxItem};
use contextnest::ingest::claude_code::{
    discover_codex_sessions, discover_sessions, ingest_session_file, parse_since,
    redactor::Redactor, sink::RedactingSink, DryRunSink, HttpSink, Sink, SinkReport,
};
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, SystemTime};
use tokio::fs;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    // `mcp serve` owns stdout as the JSON-RPC channel — logs MUST go to
    // stderr or they corrupt the protocol stream the host agent reads.
    let mcp_mode = matches!(cli.command, Commands::Mcp { .. });
    init_logging(cli.verbose, mcp_mode);
    if let Err(e) = run_command(cli).await {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

async fn run_command(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Serve { bind } => serve(bind).await,
        Commands::Ingest { source } => ingest(source).await,
        Commands::Inbox {
            project,
            urgency,
            substrate,
            session_id,
            json,
            markdown,
        } => inbox(project, urgency, substrate, session_id, json, markdown).await,
        Commands::Mcp { action } => mcp(action).await,
        Commands::PromptContext { action } => prompt_context(action).await,
        Commands::Features {
            since,
            layer,
            project,
            json,
            url,
        } => features(since, layer, project, json, url).await,
        Commands::Checkpoint {
            action: CheckpointCommands::Compact { from, into },
        } => checkpoint_compact(from, into).await,
    }
}

/// `contextnest checkpoint compact`: stream a canonical checkpoint into the
/// binary-vector format at a new path. Runs on a blocking thread; the
/// source is read-only and must not be in use.
async fn checkpoint_compact(
    from: PathBuf,
    into: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("compacting {} -> {}", from.display(), into.display());
    let started = std::time::Instant::now();
    let report = tokio::task::spawn_blocking(move || {
        contextnest::services::checkpoint::compact(&from, &into).map_err(|e| e.to_string())
    })
    .await??;
    let mib = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
    println!(
        "fragments={} basins={} nodes={} edges={}\nsource={:.0} MiB output={:.0} MiB ({:.1}s)",
        report.fragments,
        report.basins,
        report.nodes,
        report.edges,
        mib(report.source_bytes),
        mib(report.output_bytes),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// Dispatch the `features` subcommand. Calls `GET /api/v1/features` with
/// the requested filters; Markdown by default (server-rendered via
/// `?format=markdown`), JSON via `--json`. Markdown body streams to
/// stdout verbatim; JSON is pretty-printed for terminal readability and
/// `jq`-pipe ergonomics.
async fn features(
    since: Option<String>,
    layer: Option<String>,
    project: Option<String>,
    json_mode: bool,
    url: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let base = url
        .or_else(|| std::env::var("CONTEXTNEST_URL").ok())
        .unwrap_or_else(|| "http://localhost:8080".to_string());
    let endpoint = format!("{}/api/v1/features", base.trim_end_matches('/'));
    let mut query_pairs: Vec<(&str, String)> = Vec::new();
    if let Some(v) = since.as_deref() {
        query_pairs.push(("since", v.to_string()));
    }
    if let Some(v) = layer.as_deref() {
        query_pairs.push(("layer", v.to_string()));
    }
    if let Some(v) = project.as_deref() {
        query_pairs.push(("project", v.to_string()));
    }
    if !json_mode {
        query_pairs.push(("format", "markdown".to_string()));
    }
    let client = reqwest::Client::new();
    let resp = client
        .get(&endpoint)
        .query(&query_pairs)
        .send()
        .await
        .map_err(|e| format!("GET {endpoint} failed: {e}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("reading features body from {endpoint} failed: {e}"))?;
    if !status.is_success() {
        return Err(format!("substrate {endpoint} returned {status}: {body}").into());
    }
    if json_mode {
        // Pretty-print for terminal readability; trailing newline so
        // shell prompts render cleanly.
        match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or(body)),
            Err(_) => print!("{body}"),
        }
    } else {
        // Markdown body already carries trailing newlines.
        print!("{body}");
    }
    Ok(())
}

/// Dispatch the `mcp` subcommand. Resolves the substrate base URL from the
/// `--url` flag, then `$CONTEXTNEST_URL`, then the localhost default, and
/// runs the stdio server until the host agent closes stdin.
async fn mcp(action: McpCommands) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        McpCommands::Serve { url } => {
            let base = url
                .or_else(|| std::env::var("CONTEXTNEST_URL").ok())
                .unwrap_or_else(|| "http://localhost:8080".to_string());
            contextnest::mcp::McpServer::new(base).serve_stdio().await
        }
    }
}

/// Dispatch the `prompt-context` subcommand. Currently only `capsule` —
/// fetches `GET /api/v1/prompt-context/capsule` with the provided filters
/// and prints the Markdown body to stdout. Logs / errors are written to
/// stderr (init_logging is called WITHOUT log_to_stderr mode for this
/// command, but `eprintln!` still goes to stderr), so the user can pipe
/// stdout into `pbcopy` / a file without polluting the output.
async fn prompt_context(action: PromptContextCommands) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        PromptContextCommands::Capsule {
            query,
            project,
            session_id,
            since,
            min_count,
            max_per_kind,
            semantic,
            url,
        } => {
            let base = url
                .or_else(|| std::env::var("CONTEXTNEST_URL").ok())
                .unwrap_or_else(|| "http://localhost:8080".to_string());
            let endpoint = format!(
                "{}/api/v1/prompt-context/capsule",
                base.trim_end_matches('/')
            );
            let mut query_pairs: Vec<(&str, String)> = Vec::new();
            if let Some(v) = query.as_deref() {
                query_pairs.push(("query", v.to_string()));
            }
            if let Some(v) = project.as_deref() {
                query_pairs.push(("project", v.to_string()));
            }
            if let Some(v) = session_id.as_deref() {
                query_pairs.push(("session_id", v.to_string()));
            }
            if let Some(v) = since.as_deref() {
                query_pairs.push(("since", v.to_string()));
            }
            if let Some(v) = min_count {
                query_pairs.push(("min_count", v.to_string()));
            }
            if let Some(v) = max_per_kind {
                query_pairs.push(("max_per_kind", v.to_string()));
            }
            // Only forward `semantic` when set — substrate default is off.
            if semantic {
                query_pairs.push(("semantic", "true".to_string()));
            }
            let client = reqwest::Client::new();
            let resp = client
                .get(&endpoint)
                .query(&query_pairs)
                .send()
                .await
                .map_err(|e| format!("GET {endpoint} failed: {e}"))?;
            let status = resp.status();
            let body = resp
                .text()
                .await
                .map_err(|e| format!("reading capsule body from {endpoint} failed: {e}"))?;
            if !status.is_success() {
                return Err(format!("substrate {endpoint} returned {status}: {body}").into());
            }
            // Markdown body to stdout. Single newline already on the body
            // from the renderer; no extra `println!` newline needed.
            print!("{body}");
            Ok(())
        }
    }
}

/// Query the substrate for everything Claude is waiting on the user for,
/// across one or every known session, and render an urgency-sorted list.
///
/// Algorithm:
///
/// 1. Determine which session_ids to scan:
///    - If `--session-id` is set, use exactly that.
///    - Else discover Claude Code sessions on disk (same path as `ingest`)
///      and use each session's bare UUID as the substrate session_id.
/// 2. For each session, run TWO retrieve calls in parallel:
///    - `metadata_filter: {kind: "user_action"}` (optionally + urgency)
///    - `metadata_filter: {kind: "decision", awaiting_decision: true}`
/// 3. Parse hits via `InboxItem::from_hits`, aggregate, render.
async fn inbox(
    project: Option<String>,
    urgency: Option<String>,
    substrate: String,
    session_id: Option<String>,
    json_mode: bool,
    markdown_mode: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Validate --urgency early — bad input is a hard error so users
    // notice typos.
    if let Some(u) = urgency.as_deref() {
        match u {
            "now" | "soon" | "later" => {}
            _ => {
                return Err(
                    format!("Invalid --urgency '{}'. Use one of: now, soon, later.", u).into(),
                );
            }
        }
    }

    // Build the list of substrate session_ids to query.
    let session_ids: Vec<String> = if let Some(sid) = session_id {
        vec![sid]
    } else {
        // Discover on-disk Claude Code sessions and derive substrate ids.
        let projects_root = default_projects_dir();
        if !projects_root.exists() {
            return Err(format!(
                "No --session-id given and Claude Code projects directory not found at {}. \
                 Pass --session-id <id> to scope the inbox query.",
                projects_root.display()
            )
            .into());
        }
        let discovered = discover_sessions(&projects_root, project.as_deref(), None)?;
        // Derive substrate session ids: bare UUID of each Claude Code
        // session. Dedup in case multiple .jsonl files share a UUID
        // (unlikely but defensive).
        let mut seen = HashSet::new();
        discovered
            .iter()
            .filter_map(|s| {
                if s.session_uuid.is_empty() {
                    return None;
                }
                let cn_id = s.session_uuid.clone();
                if seen.insert(cn_id.clone()) {
                    Some(cn_id)
                } else {
                    None
                }
            })
            .collect()
    };

    if session_ids.is_empty() {
        if json_mode {
            println!("[]");
        } else {
            println!("📋 No sessions discovered. Run `contextnest ingest claude-code` first.");
        }
        return Ok(());
    }

    // For each session, query both user_actions and decisions. Run all
    // queries with bounded concurrency so big inboxes don't blow up.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let mut all_items: Vec<InboxItem> = Vec::new();

    for sid in &session_ids {
        // user_actions
        let mut filter = json!({"kind": "user_action"});
        if let Some(u) = urgency.as_deref() {
            filter["urgency"] = json!(u);
        }
        if let Some(items) = fetch_inbox_for(&client, &substrate, sid, &filter).await? {
            all_items.extend(items);
        }

        // decisions (only if not filtering by an urgency other than "now"
        // — decisions are always urgent so they'd be filtered out below
        // by anything except urgency=now or no urgency filter)
        if matches!(urgency.as_deref(), None | Some("now")) {
            let dec_filter = json!({"kind": "decision", "awaiting_decision": true});
            if let Some(items) = fetch_inbox_for(&client, &substrate, sid, &dec_filter).await? {
                all_items.extend(items);
            }
        }
    }

    // Dispatch the renderer. `--json` and `--markdown` are mutually
    // exclusive at the clap layer (conflicts_with), so at most one is
    // true; falling through to render_text is the terminal default.
    if json_mode {
        println!("{}", render_json(&all_items)?);
    } else if markdown_mode {
        // Markdown body carries its own trailing newlines; no extra
        // println! so the output is pipe-clean.
        print!("{}", render_markdown(&all_items));
    } else {
        print!("{}", render_text(&all_items));
    }
    Ok(())
}

/// One retrieve call for one session + one filter. Returns the parsed
/// InboxItems on success, `None` for "session has no fragments" (HTTP
/// 200 with empty hits). Errors bubble up as the caller's problem.
async fn fetch_inbox_for(
    client: &reqwest::Client,
    substrate: &str,
    session_id: &str,
    metadata_filter: &Value,
) -> Result<Option<Vec<InboxItem>>, Box<dyn std::error::Error>> {
    let url = format!("{}/api/v1/tools/retrieve", substrate.trim_end_matches('/'));
    let body = json!({
        "query": "inbox", // semantic content doesn't matter — filter does the work
        "top_k": 200,     // generous cap; the filter narrows the result set
        "session_id": session_id,
        "metadata_filter": metadata_filter,
    });
    let resp = client.post(&url).json(&body).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        return Err(format!(
            "inbox: /retrieve returned {} for session {}: {}",
            status, session_id, body_text
        )
        .into());
    }
    let parsed: Value = resp.json().await?;
    let hits = parsed
        .get("hits")
        .and_then(|h| h.as_array())
        .cloned()
        .unwrap_or_default();
    if hits.is_empty() {
        return Ok(None);
    }
    Ok(Some(InboxItem::from_hits(&hits)))
}

/// Dispatch the `ingest` subcommand. Each adapter is a sibling under
/// `IngestSource`; v0.2 phase 1 ships only the Claude Code adapter.
async fn ingest(source: IngestSource) -> Result<(), Box<dyn std::error::Error>> {
    match source {
        IngestSource::ClaudeCode {
            project,
            session_id,
            since,
            dry_run,
            substrate,
            projects_dir,
            install_hooks,
            project_paths,
        } => {
            if install_hooks {
                // --install-hooks is a one-shot configuration write —
                // it short-circuits the ingest path so the user doesn't
                // accidentally start a transcript scan they didn't ask
                // for. Errors here are user-facing (likely a bad path
                // or unreadable settings.json), so surface them clearly.
                install_cc_hooks(&substrate, &project_paths)?;
                return Ok(());
            }
            if !project_paths.is_empty() {
                return Err("--project-path requires --install-hooks (the flag is meaningful only when installing hooks).".into());
            }
            ingest_claude_code(project, session_id, since, dry_run, substrate, projects_dir).await
        }
        IngestSource::Codex {
            project,
            session_id,
            since,
            dry_run,
            substrate,
            sessions_dir,
        } => ingest_codex(project, session_id, since, dry_run, substrate, sessions_dir).await,
    }
}

/// Install the four real-time hooks (SessionStart, UserPromptSubmit,
/// Stop, TaskCompleted) into the user-level Claude settings AND into
/// every explicitly-named project's local settings. Each target file is
/// backed up before write; the merge is idempotent (existing entries
/// detected by their URL are retained; generated commands are upgraded).
///
/// Project paths are explicit — there is no filesystem scan. This is a
/// deliberate trust boundary: ContextNest never writes to a project's
/// settings file you haven't pointed it at.
fn install_cc_hooks(
    substrate: &str,
    project_paths: &[PathBuf],
) -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var_os("HOME")
        .ok_or("install-hooks: $HOME is not set; cannot locate ~/.claude/settings.json")?;
    let user_settings = PathBuf::from(home).join(".claude/settings.json");

    let mut targets: Vec<(String, PathBuf)> = Vec::with_capacity(1 + project_paths.len());
    targets.push(("user".into(), user_settings));
    for p in project_paths {
        let project_settings = p.join(".claude/settings.local.json");
        targets.push((format!("project {}", p.display()), project_settings));
    }

    let mut any_change = false;
    for (label, settings_path) in &targets {
        match install_to_target(substrate, label, settings_path)? {
            HookInstallOutcome::Wrote => {
                any_change = true;
            }
            HookInstallOutcome::AlreadyInstalled => {}
        }
    }

    if !any_change {
        println!("All ContextNest hooks already present in every target. Nothing to do.");
    }
    Ok(())
}

enum HookInstallOutcome {
    Wrote,
    AlreadyInstalled,
}

/// Append ContextNest hook entries to one settings file. Creates the
/// file (and its parent dir) if missing. Backs up any pre-existing
/// content with a `.bak-<unix-ts>` suffix before writing.
fn install_to_target(
    substrate: &str,
    label: &str,
    settings_path: &Path,
) -> Result<HookInstallOutcome, Box<dyn std::error::Error>> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let existing_text = match std::fs::read_to_string(settings_path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(e) => return Err(format!("read {}: {}", settings_path.display(), e).into()),
    };
    let existing: Value = serde_json::from_str(&existing_text)
        .map_err(|e| format!("parse {}: {}", settings_path.display(), e))?;

    let (updated, added) = merge_cc_hook_entries(&existing, substrate);

    if added.is_empty() {
        println!(
            "[{}] All four ContextNest hooks already present in {}. Skipping.",
            label,
            settings_path.display()
        );
        return Ok(HookInstallOutcome::AlreadyInstalled);
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup_path = settings_path.with_extension(format!(
        "{}.bak-{}",
        settings_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("json"),
        ts
    ));
    if settings_path.exists() {
        std::fs::copy(settings_path, &backup_path)
            .map_err(|e| format!("backup to {}: {}", backup_path.display(), e))?;
    }

    let pretty = serde_json::to_string_pretty(&updated)?;
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(settings_path, format!("{}\n", pretty))
        .map_err(|e| format!("write {}: {}", settings_path.display(), e))?;

    println!(
        "[{}] Installed ContextNest hooks into {}",
        label,
        settings_path.display()
    );
    if backup_path.exists() {
        println!(
            "        Previous file backed up to {}",
            backup_path.display()
        );
    }
    for ev in &added {
        if let Some(label) = ev.strip_suffix(" (concord turn)") {
            println!(
                "        + {:<22} -> {}/api/v1/coord/turn",
                label,
                substrate.trim_end_matches('/'),
            );
        } else if let Some(label) = ev.strip_suffix(" (concord precheck)") {
            println!(
                "        + {:<22} -> {}/api/v1/coord/precheck",
                label,
                substrate.trim_end_matches('/'),
            );
        } else if let Some(label) = ev.strip_suffix(" (concord footprints)") {
            println!(
                "        + {:<22} -> {}/api/v1/coord/footprints",
                label,
                substrate.trim_end_matches('/'),
            );
        } else {
            println!(
                "        + {:<22} -> {}/api/v1/cc/hook/{}",
                ev,
                substrate.trim_end_matches('/'),
                cc_hook_path_segment(ev)
            );
        }
    }
    Ok(HookInstallOutcome::Wrote)
}

/// Merge ContextNest hook entries into a settings JSON value. Returns
/// `(updated_value, events_appended)`. Pure function so it can be
/// unit-tested without touching disk. Entries are detected by URL
/// substring (`/api/v1/cc/hook/`) so a re-run after a substrate URL
/// change still appends — that's intentional, so a user pointing
/// at a new substrate gets a fresh entry next to the old one.
///
/// `concord = true` also appends a synchronous `/api/v1/coord/turn`
/// entry to SessionStart and UserPromptSubmit (one per event), a
/// synchronous `/api/v1/coord/precheck` entry under PreToolUse
/// (Edit-class tools only), and an async
/// `/api/v1/coord/footprints` entry under PostToolUse (Read +
/// Edit-class). Each is detected by the bare substring of its URL —
/// a re-run against a new substrate URL therefore still skips. The
/// turn hook delivers the principal's mailbox per prompt; the
/// precheck emits a per-Edit advisory; the footprints hook records
/// every read/write so a later precheck can compare.
fn merge_cc_hook_entries_with(
    existing: &Value,
    substrate: &str,
    concord: bool,
) -> (Value, Vec<String>) {
    const EVENTS: &[(&str, &str)] = &[
        ("SessionStart", "session_start"),
        ("UserPromptSubmit", "user_prompt_submit"),
        ("Stop", "stop"),
        ("TaskCompleted", "task_completed"),
    ];

    let substrate = substrate.trim_end_matches('/').to_string();
    let mut root = existing.clone();
    if !root.is_object() {
        root = json!({});
    }
    let root_obj = root.as_object_mut().expect("just ensured object");
    let hooks_entry = root_obj
        .entry("hooks".to_string())
        .or_insert_with(|| json!({}));
    if !hooks_entry.is_object() {
        *hooks_entry = json!({});
    }
    let hooks_obj = hooks_entry.as_object_mut().expect("just ensured object");

    let mut added: Vec<String> = Vec::new();
    for (event_name, path_seg) in EVENTS {
        let url = format!("{}/api/v1/cc/hook/{}", substrate, path_seg);
        // Drain stdin into a tempfile BEFORE backgrounding the curl.
        // The naive `curl --data-binary @- &` pattern races: bash
        // backgrounds curl before it has read stdin, the parent sh
        // exits, the pipe closes, curl reads zero bytes, the substrate
        // sees an empty POST and returns 400. This tempfile dance
        // guarantees the body is fully captured before the parent shell
        // returns control to Claude.
        let cmd = render_hook_command(&url);

        let entries = hooks_obj
            .entry((*event_name).to_string())
            .or_insert_with(|| json!([]));
        if !entries.is_array() {
            *entries = json!([]);
        }
        let arr = entries.as_array_mut().expect("just ensured array");

        let mut already_present = false;
        let mut upgraded = false;
        for entry in arr.iter_mut() {
            if let Some(hooks) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
                for hook in hooks {
                    if let Some(command) = hook.get_mut("command") {
                        if let Some(old) = command.as_str().filter(|c| c.contains(&url)) {
                            already_present = true;
                            // Only replace commands generated by this installer;
                            // custom operator commands retain their own behavior.
                            if old != cmd && old.starts_with("F=$(mktemp /tmp/cnhk-") {
                                *command = Value::String(cmd.clone());
                                upgraded = true;
                            }
                        }
                    }
                }
            }
        }
        if upgraded {
            added.push((*event_name).to_string());
        }
        if already_present {
            continue;
        }

        arr.push(json!({
            "hooks": [
                {
                    "type": "command",
                    "command": cmd
                }
            ]
        }));
        added.push((*event_name).to_string());
    }

    if concord {
        // Synchronous /api/v1/coord/turn entries on SessionStart and
        // UserPromptSubmit. Detect an existing entry by the bare
        // substring "/api/v1/coord/turn" (per the brief, this is
        // deliberately looser than the cc/hook/ detection above so a
        // re-run against a NEW substrate URL still skips).
        const SYNCHRONOUS_EVENTS: &[&str] = &["SessionStart", "UserPromptSubmit"];
        let turn_url = format!("{}/api/v1/coord/turn", substrate);
        let turn_cmd = render_concord_turn_command(&turn_url);
        for event_name in SYNCHRONOUS_EVENTS {
            let entries = hooks_obj
                .entry((*event_name).to_string())
                .or_insert_with(|| json!([]));
            if !entries.is_array() {
                *entries = json!([]);
            }
            let arr = entries.as_array_mut().expect("just ensured array");

            let mut already_present = false;
            for entry in arr.iter() {
                if let Some(hooks) = entry.get("hooks").and_then(Value::as_array) {
                    for hook in hooks {
                        if let Some(command) = hook.get("command").and_then(Value::as_str) {
                            if command.contains("/api/v1/coord/turn") {
                                already_present = true;
                                break;
                            }
                        }
                    }
                }
                if already_present {
                    break;
                }
            }
            if already_present {
                continue;
            }

            arr.push(json!({
                "hooks": [
                    {
                        "type": "command",
                        "command": turn_cmd.clone()
                    }
                ]
            }));
            // Distinct label so the install_to_target print loop can
            // print the right URL path (it does NOT match
            // cc_hook_path_segment's table).
            added.push(format!("{event_name} (concord turn)"));
        }

        // Synchronous PreToolUse /api/v1/coord/precheck entry on
        // Edit-class tools only. Same idempotency rule: skip if any
        // existing command contains "/api/v1/coord/precheck".
        let precheck_url = format!("{}/api/v1/coord/precheck", substrate);
        let precheck_cmd = render_concord_precheck_command(&precheck_url);
        let precheck_entry = json!({
            "matcher": "Edit|Write|MultiEdit|NotebookEdit",
            "hooks": [
                {
                    "type": "command",
                    "command": precheck_cmd,
                }
            ]
        });
        if !entry_already_present(hooks_obj, "PreToolUse", "/api/v1/coord/precheck") {
            let arr = hooks_obj
                .entry("PreToolUse".to_string())
                .or_insert_with(|| json!([]));
            if !arr.is_array() {
                *arr = json!([]);
            }
            arr.as_array_mut()
                .expect("just ensured array")
                .push(precheck_entry);
            added.push("PreToolUse (concord precheck)".to_string());
        }

        // Async PostToolUse /api/v1/coord/footprints entry on
        // Read + Edit-class tools. Same idempotency rule.
        let footprints_url = format!("{}/api/v1/coord/footprints", substrate);
        let footprints_cmd = render_concord_footprints_command(&footprints_url);
        let footprints_entry = json!({
            "matcher": "Read|Edit|Write|MultiEdit|NotebookEdit",
            "hooks": [
                {
                    "type": "command",
                    "command": footprints_cmd,
                }
            ]
        });
        if !entry_already_present(hooks_obj, "PostToolUse", "/api/v1/coord/footprints") {
            let arr = hooks_obj
                .entry("PostToolUse".to_string())
                .or_insert_with(|| json!([]));
            if !arr.is_array() {
                *arr = json!([]);
            }
            arr.as_array_mut()
                .expect("just ensured array")
                .push(footprints_entry);
            added.push("PostToolUse (concord footprints)".to_string());
        }
    }

    (root, added)
}

/// True iff any command under `event_name` in `hooks_obj` contains
/// `needle` (the URL substring). Used by the precheck/footprints
/// appenders to detect an existing entry before pushing a duplicate.
fn entry_already_present(
    hooks_obj: &serde_json::Map<String, Value>,
    event_name: &str,
    needle: &str,
) -> bool {
    let Some(arr) = hooks_obj.get(event_name).and_then(Value::as_array) else {
        return false;
    };
    for entry in arr {
        if let Some(hooks) = entry.get("hooks").and_then(Value::as_array) {
            for hook in hooks {
                if let Some(command) = hook.get("command").and_then(Value::as_str) {
                    if command.contains(needle) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Thin wrapper that reads `CONTEXTNEST_CONCORD_HOOKS` from the
/// environment. The env opt-out exists for users who run install-hooks
/// inside a containerised agent fleet that doesn't talk to Concord.
/// Tests and other callers should use `merge_cc_hook_entries_with`
/// directly so they don't have to reset the env.
fn merge_cc_hook_entries(existing: &Value, substrate: &str) -> (Value, Vec<String>) {
    merge_cc_hook_entries_with(existing, substrate, concord_hooks_enabled())
}

/// `CONTEXTNEST_CONCORD_HOOKS=0` opts out of the synchronous Concord hooks.
fn concord_hooks_enabled() -> bool {
    std::env::var("CONTEXTNEST_CONCORD_HOOKS").as_deref() != Ok("0")
}

fn cc_hook_path_segment(event_name: &str) -> &'static str {
    match event_name {
        "SessionStart" => "session_start",
        "UserPromptSubmit" => "user_prompt_submit",
        "Stop" => "stop",
        "TaskCompleted" => "task_completed",
        _ => "",
    }
}

/// Walk `~/.claude/projects/`, filter by the user's flags, push memories
/// from every matching session to the chosen sink.
async fn ingest_claude_code(
    project: Option<String>,
    session_id: Option<String>,
    since: Option<String>,
    dry_run: bool,
    substrate: String,
    projects_dir: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Resolve the projects discovery root. Default: ~/.claude/projects/.
    let projects_root = projects_dir.unwrap_or_else(default_projects_dir);
    if !projects_root.exists() {
        return Err(format!(
            "Claude Code projects directory not found: {}",
            projects_root.display()
        )
        .into());
    }

    // Translate --since "7d" into a SystemTime cutoff. Bad input is a
    // hard error so users notice typos.
    let since_cutoff = match since.as_deref() {
        None => None,
        Some(s) => match parse_since(s) {
            Some(dur) => Some(SystemTime::now() - dur),
            None => {
                return Err(format!(
                    "Invalid --since value '{}'. Use a number + unit, e.g. '7d', '24h', '30m'.",
                    s
                )
                .into());
            }
        },
    };

    // When --session-id is set, project + since filters are ignored
    // because the user wants exactly that one session.
    let sessions = if let Some(uuid_filter) = session_id.as_deref() {
        let all = discover_sessions(&projects_root, None, None)?;
        let want = uuid_filter.to_lowercase();
        all.into_iter()
            .filter(|s| s.session_uuid.to_lowercase().contains(&want))
            .collect()
    } else {
        discover_sessions(&projects_root, project.as_deref(), since_cutoff)?
    };

    if sessions.is_empty() {
        println!(
            "No matching sessions in {}. (project={:?}, since={:?}, session_id={:?})",
            projects_root.display(),
            project,
            since,
            session_id
        );
        return Ok(());
    }

    println!(
        "Discovered {} session(s) under {}",
        sessions.len(),
        projects_root.display()
    );
    for s in &sessions {
        let mb = s.size_bytes as f64 / 1_048_576.0;
        println!(
            "  • {}  ({:.2} MB)  project: {}",
            s.session_uuid, mb, s.project_cwd
        );
    }
    println!();

    // Pick the sink based on --dry-run, then wrap with the redactor so
    // sensitive data (API keys, SSNs, etc.) is scrubbed before storage.
    // User-supplied patterns from ~/.contextnest/redact.toml are merged
    // with the built-in defaults; absent config = defaults only.
    let redactor_config = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".contextnest")
        .join("redact.toml");
    let redactor = Redactor::from_config(&redactor_config);

    if dry_run {
        let dry = DryRunSink::new();
        let sink = RedactingSink::new(dry, redactor);
        let total = process_sessions(&sessions, &sink).await?;
        let skipped = sink.skipped_count();
        // Recover the inner DryRunSink to read captured_by_kind — the
        // wrapper consumes it, so we re-construct the dry-run table
        // from the SinkReport directly.
        report_summary(total, &std::collections::HashMap::new(), true);
        if skipped > 0 {
            println!(
                "  Privacy filter: {} record{} dropped (>75% redacted)",
                skipped,
                if skipped == 1 { "" } else { "s" }
            );
        }
    } else {
        let http = HttpSink::new(&substrate);
        let sink = RedactingSink::new(http, redactor);
        println!("Pushing to substrate at {}", substrate);
        let total = process_sessions(&sessions, &sink).await?;
        let skipped = sink.skipped_count();
        report_summary(total, &std::collections::HashMap::new(), false);
        if skipped > 0 {
            println!(
                "  Privacy filter: {} record{} dropped (>75% redacted)",
                skipped,
                if skipped == 1 { "" } else { "s" }
            );
        }
    }

    Ok(())
}

/// Walk `~/.codex/sessions/`, filter by the user's flags, push memories
/// from matching Codex transcripts to the chosen sink.
async fn ingest_codex(
    project: Option<String>,
    session_id: Option<String>,
    since: Option<String>,
    dry_run: bool,
    substrate: String,
    sessions_dir: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let sessions_root = sessions_dir.unwrap_or_else(default_codex_sessions_dir);
    if !sessions_root.exists() {
        return Err(format!(
            "Codex sessions directory not found: {}",
            sessions_root.display()
        )
        .into());
    }

    let since_cutoff = match since.as_deref() {
        None => None,
        Some(s) => match parse_since(s) {
            Some(dur) => Some(SystemTime::now() - dur),
            None => {
                return Err(format!(
                    "Invalid --since value '{}'. Use a number + unit, e.g. '7d', '24h', '30m'.",
                    s
                )
                .into());
            }
        },
    };

    let sessions = if let Some(uuid_filter) = session_id.as_deref() {
        let all = discover_codex_sessions(&sessions_root, None, None)?;
        let want = uuid_filter.to_lowercase();
        all.into_iter()
            .filter(|s| s.session_uuid.to_lowercase().contains(&want))
            .collect()
    } else {
        discover_codex_sessions(&sessions_root, project.as_deref(), since_cutoff)?
    };

    if sessions.is_empty() {
        println!(
            "No matching Codex sessions in {}. (project={:?}, since={:?}, session_id={:?})",
            sessions_root.display(),
            project,
            since,
            session_id
        );
        return Ok(());
    }

    println!(
        "Discovered {} Codex session(s) under {}",
        sessions.len(),
        sessions_root.display()
    );
    for s in &sessions {
        let mb = s.size_bytes as f64 / 1_048_576.0;
        println!(
            "  • {}  ({:.2} MB)  project: {}",
            s.session_uuid, mb, s.project_cwd
        );
    }
    println!();

    let redactor_config = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".contextnest")
        .join("redact.toml");
    let redactor = Redactor::from_config(&redactor_config);

    if dry_run {
        let dry = DryRunSink::new();
        let sink = RedactingSink::new(dry, redactor);
        let total = process_sessions(&sessions, &sink).await?;
        let skipped = sink.skipped_count();
        report_summary(total, &std::collections::HashMap::new(), true);
        if skipped > 0 {
            println!(
                "  Privacy filter: {} record{} dropped (>75% redacted)",
                skipped,
                if skipped == 1 { "" } else { "s" }
            );
        }
    } else {
        let http = HttpSink::new(&substrate);
        let sink = RedactingSink::new(http, redactor);
        println!("Pushing to substrate at {}", substrate);
        let total = process_sessions(&sessions, &sink).await?;
        let skipped = sink.skipped_count();
        report_summary(total, &std::collections::HashMap::new(), false);
        if skipped > 0 {
            println!(
                "  Privacy filter: {} record{} dropped (>75% redacted)",
                skipped,
                if skipped == 1 { "" } else { "s" }
            );
        }
    }

    Ok(())
}

/// Run `ingest_session_file` over a list of sessions and aggregate the
/// reports. Errors on individual sessions are logged and counted; we
/// don't abort the whole batch.
async fn process_sessions<S: Sink + ?Sized>(
    sessions: &[contextnest::ingest::claude_code::DiscoveredSession],
    sink: &S,
) -> Result<SinkReport, Box<dyn std::error::Error>> {
    let mut combined = SinkReport::default();
    for s in sessions {
        match ingest_session_file(s, sink).await {
            Ok(report) => {
                combined.success += report.success;
                combined.failed += report.failed;
                if combined.first_error.is_none() {
                    combined.first_error = report.first_error;
                }
                for (k, v) in report.by_kind {
                    *combined.by_kind.entry(k).or_insert(0) += v;
                }
            }
            Err(e) => {
                eprintln!("  ✗ session {}: {}", s.session_uuid, e);
                combined.failed += 1;
            }
        }
    }
    Ok(combined)
}

fn report_summary(
    report: SinkReport,
    captured_by_kind: &std::collections::HashMap<String, usize>,
    dry_run: bool,
) {
    println!();
    println!(
        "─── {} ───",
        if dry_run {
            "DRY RUN COMPLETE"
        } else {
            "INGEST COMPLETE"
        }
    );
    println!(
        "  Memories: {} success / {} fail",
        report.success, report.failed
    );
    let by_kind = if !report.by_kind.is_empty() {
        &report.by_kind
    } else {
        captured_by_kind
    };
    if !by_kind.is_empty() {
        println!("  Breakdown by kind:");
        let mut entries: Vec<_> = by_kind.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (kind, count) in entries {
            println!("    {:<22} {}", kind, count);
        }
    }
    if let Some(err) = &report.first_error {
        eprintln!("  First error: {}", err);
    }
}

fn default_projects_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".claude").join("projects")
}

fn default_codex_sessions_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".codex").join("sessions")
}

/// Start the HTTP server (seven-tool memory API + health/status).
async fn serve(bind_override: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting ContextNest server");

    // Start the resident-memory ceiling BEFORE any boot work: checkpoint
    // restore is where a legacy heap profile lands. `None` means the guard is
    // off (CONTEXTNEST_MAX_RSS_MB=0 or physical memory unknown).
    match contextnest::services::resource_monitor::spawn_guard() {
        Some(ceiling) => tracing::info!(
            ceiling_mib = ceiling / (1024 * 1024),
            "Memory guard armed (set CONTEXTNEST_MAX_RSS_MB=0 to disable)"
        ),
        None => tracing::info!("Memory guard disabled"),
    }

    let config = load_configuration().await?;
    tracing::info!("Configuration loaded successfully");

    let bind_address = bind_override
        .unwrap_or_else(|| format!("{}:{}", config.api.rest.bind_address, config.api.rest.port));

    let services = ContextNestServices::new(config).await?;
    tracing::info!("Core services initialized successfully");

    // Concord P0 durable layer: swap the in-memory default for a
    // file-backed store BEFORE any `services.clone()`. Resolution
    // order:
    //   1. `CONTEXTNEST_COORD_DB` — explicit path wins.
    //   2. The WAL path's sibling directory, derived via
    //      `wal_path_from_env().map(|w| w.with_file_name("coord.db"))`.
    //      This keeps the durable store next to the WAL under
    //      `~/.contextnest/`, shared across worktrees.
    // A resolved path that fails to open ABORTS boot — silent
    // degradation to in-memory would lose principals across restarts.
    let coord_path: Option<std::path::PathBuf> = match std::env::var_os("CONTEXTNEST_COORD_DB") {
        Some(p) if !p.is_empty() => Some(std::path::PathBuf::from(p)),
        _ => wal_path_from_env().map(|w| w.with_file_name("coord.db")),
    };
    let mut services = services;
    if let Some(path) = coord_path {
        let store = contextnest::services::coord_store::CoordStore::open(&path)
            .map_err(std::io::Error::other)?;
        services.coord_store = std::sync::Arc::new(store);
        tracing::info!(path = %path.display(), "Concord principals store: file-backed");
    } else {
        tracing::warn!(
            "Concord principals store: in-memory (set CONTEXTNEST_COORD_DB or CONTEXTNEST_WAL_PATH to persist)"
        );
    }

    // WAL bootstrap: replay any persisted records BEFORE opening the writer.
    // The writer is intentionally `None` during replay so that
    // `store_with_id` (called from the replay loop) does not re-log records
    // to disk. Once replay finishes we open the writer in append mode and
    // set it in the OnceCell; from that point onward every successful
    // `store` HTTP call appends a fresh record.
    if let Some(wal_path) = wal_path_from_env() {
        bootstrap_wal(&services, &wal_path).await?;
        contextnest::services::checkpoint::bootstrap(
            &services,
            &wal_path.with_extension("canonical.sqlite"),
        )
        .await
        .map_err(std::io::Error::other)?;
    } else {
        tracing::info!(
            "WAL disabled (set CONTEXTNEST_WAL_PATH to enable persistence across restarts)"
        );
    }

    // Phase 1 of the neural-field epic: spawn the background
    // consolidation worker. Runs AFTER WAL replay so its initial scan
    // picks up every restored sidecar id. Honors
    // CONTEXTNEST_CONSOLIDATION_* env knobs (see
    // `src/services/consolidation.rs` for defaults).
    {
        use contextnest::services::consolidation::{run_worker, ConsolidationConfig};
        let worker_services = services.clone();
        let queue = services.consolidation_queue.clone();
        let cfg = ConsolidationConfig::from_env();
        if cfg.enabled {
            tracing::info!(
                interval_ms = cfg.interval_ms,
                concurrency = cfg.concurrency,
                batch_size = cfg.batch_size,
                "Consolidation worker spawning"
            );
        } else {
            tracing::warn!(
                "Consolidation worker DISABLED via CONTEXTNEST_CONSOLIDATION_ENABLED=false — \
                 attractor pipeline will not run for cc_hooks / WAL-replay fragments"
            );
        }
        tokio::spawn(async move {
            run_worker(worker_services, queue, cfg).await;
        });
    }

    let app = create_app(services).await?;

    tracing::info!("Configured API endpoints:");
    tracing::info!("  Health check: GET  /api/health");
    tracing::info!("  Status:       GET  /api/status");
    tracing::info!("  Seven-tool memory API:");
    tracing::info!("    POST /api/v1/tools/store");
    tracing::info!("    POST /api/v1/tools/retrieve");
    tracing::info!("    POST /api/v1/tools/update");
    tracing::info!("    POST /api/v1/tools/summarize");
    tracing::info!("    POST /api/v1/tools/discard");
    tracing::info!("    POST /api/v1/tools/reconstruct");
    tracing::info!("    POST /api/v1/tools/resonate");
    tracing::info!("  Substrate observability:");
    tracing::info!("    GET  /api/v1/substrate/consolidation");

    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    tracing::info!("ContextNest server listening on {}", bind_address);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Load configuration from `$CONTEXTNEST_CONFIG` (or `config.toml`) or fall
/// back to `Config::default()`.
async fn load_configuration() -> Result<Config, Box<dyn std::error::Error>> {
    let config_path =
        std::env::var("CONTEXTNEST_CONFIG").unwrap_or_else(|_| "config.toml".to_string());

    if fs::metadata(&config_path).await.is_ok() {
        let config_content = fs::read_to_string(&config_path).await?;
        let config: Config = toml::from_str(&config_content)?;
        tracing::info!("Loaded configuration from {}", config_path);
        Ok(config)
    } else {
        tracing::info!(
            "Using default configuration (config file not found: {})",
            config_path
        );
        Ok(Config::default())
    }
}

/// Render the bash command body that Claude Code's hook system invokes
/// per event. The shape is locked in `~/.claude/settings.json` once
/// `install-hooks` runs, so any change here only takes effect on
/// **re-running** the install command.
///
/// Constraints — every one of these is a real bug we paid for once:
///
/// 1. `mktemp` template puts the `X` placeholders at the END. macOS
///    `mktemp` refuses templates like `/tmp/cnhk-XXXXXX.json` (X's
///    followed by a literal suffix) — it treats the whole thing as a
///    literal name, succeeds on the first call by creating the file,
///    and fails on every subsequent call with "File exists" returning
///    an empty path. `cat > ""` then errors silently and the body
///    never reaches the substrate. **Symptom: hooks appear to fire
///    (Claude sees a fast no-op) but the WAL never grows.**
/// 2. The body is drained into a tempfile BEFORE the backgrounded
///    curl runs. The naive `curl --data-binary @- &` pattern races:
///    bash backgrounds curl before it has read stdin, the parent sh
///    exits, the pipe closes, curl reads zero bytes, the substrate
///    sees an empty POST and returns 400. The tempfile dance
///    guarantees the body is fully captured before the parent shell
///    returns control to Claude.
/// 3. Curl is `-s -m 10 --retry 3 --retry-connrefused --retry-delay 1` +
///    redirected to `/dev/null 2>&1`. The previous `-m 1` (1-second
///    total budget, no retry) silently dropped payloads on the smallest
///    bit of contention or during a substrate restart, leaving the
///    in-memory `SessionTracker` offset frozen and an active session's
///    inbox stuck. With 10s + 3 retries on connection refused, the
///    delivery is reliable enough that the server-side sweeper (see
///    [`crate::api::cc_hooks::spawn_sweeper`]) is purely defence in
///    depth, not the load-bearing path.
/// 4. Trailing `&` detaches the curl from the hook's foreground call so
///    Claude Code's hook protocol gets its instant ack — Claude never
///    waits for the network, regardless of how long the retries take.
fn render_hook_command(url: &str) -> String {
    let default_headers = if url.starts_with("http://localhost:28080/")
        || url.starts_with("http://127.0.0.1:28080/")
    {
        "$HOME/.contextnest/tenant-auth/operator.headers"
    } else {
        ""
    };
    let url = url.replace('\'', "'\\''");
    format!(
        r#"F=$(mktemp /tmp/cnhk-XXXXXX); cat > "$F"; (H="${{CONTEXTNEST_OPERATOR_HEADERS:-{default_headers}}}"; set --; if [ -r "$H" ]; then set -- --header "@$H"; fi; curl "$@" -s -m 10 --retry 3 --retry-connrefused --retry-delay 1 -X POST '{url}' -H "content-type: application/json" --data-binary @"$F" >/dev/null 2>&1; rm -f "$F") &"#,
    )
}

/// Render the bash command body for the synchronous Concord turn hook.
///
/// Unlike the four async hooks this one MUST NOT be backgrounded —
/// Claude Code's hook protocol reads the synchronous endpoint's body
/// directly (via `hookSpecificOutput.additionalContext`), and a
/// backgrounded curl would race the hook return. Constraints:
///
/// - No `mktemp` / no subshell: stdout reaches Claude as the hook's
///   response, so anything that buffers or replaces stdout would
///   strip the JSON envelope Claude expects.
/// - `-m 2` short timeout: a hung synchronous curl must not freeze the
///   user's prompt. The hook is best-effort; retries are explicitly
///   NOT wanted here because a retry could deliver a stale mailbox.
/// - `|| true` after curl: any curl failure (network, timeout,
///   non-2xx) must not propagate as a hook exit, which Claude would
///   read as "hook errored, suppress stdout".
/// - X-Concord-* headers carry the principal/pid/pane/tty so the
///   server can bind the session without re-deriving them.
fn render_concord_turn_command(url: &str) -> String {
    render_concord_synchronous_command(url)
}

/// Render the bash command body for the synchronous PreToolUse
/// `/api/v1/coord/precheck` hook. Same body shape as the turn hook
/// (URL-generic, short timeout, `|| true`, X-Concord-* headers,
/// never backgrounded) — the only thing that changes between the
/// two callers is the URL.
fn render_concord_precheck_command(url: &str) -> String {
    render_concord_synchronous_command(url)
}

/// Shared body for the two synchronous Concord hooks (turn +
/// precheck). Pulled out so the two callers can't drift; see the
/// `render_concord_turn_command` doc comment for the constraints
/// (`-m 2`, `|| true`, X-Concord-* headers, no `mktemp`).
fn render_concord_synchronous_command(url: &str) -> String {
    let default_headers = if url.starts_with("http://localhost:28080/")
        || url.starts_with("http://127.0.0.1:28080/")
    {
        "$HOME/.contextnest/tenant-auth/operator.headers"
    } else {
        ""
    };
    let url = url.replace('\'', "'\\''");
    format!(
        r#"H="${{CONTEXTNEST_OPERATOR_HEADERS:-{default_headers}}}"; set --; if [ -r "$H" ]; then set -- --header "@$H"; fi; curl "$@" -sf -m 2 -X POST '{url}' -H 'content-type: application/json' -H "X-Concord-Principal: ${{CONCORD_PRINCIPAL:-}}" -H "X-Concord-Pane: ${{TMUX_PANE:-}}" -H "X-Concord-Tty: $(ps -o tty= -p $PPID 2>/dev/null | tr -d ' ')" -H "X-Concord-Pid: $PPID" --data-binary @- 2>/dev/null || true"#,
    )
}

/// Render the bash command body for the async PostToolUse
/// `/api/v1/coord/footprints` hook. Like the four ingest hooks
/// (render_hook_command) this is a fire-and-forget backgrounded
/// curl — Claude never waits for the substrate to acknowledge a
/// read/write footprint. The X-Concord-* headers let the server
/// bind `session_id` to a principal without re-deriving the
/// lineage id.
fn render_concord_footprints_command(url: &str) -> String {
    let default_headers = if url.starts_with("http://localhost:28080/")
        || url.starts_with("http://127.0.0.1:28080/")
    {
        "$HOME/.contextnest/tenant-auth/operator.headers"
    } else {
        ""
    };
    let url = url.replace('\'', "'\\''");
    format!(
        r#"F=$(mktemp /tmp/cnhk-XXXXXX); cat > "$F"; (H="${{CONTEXTNEST_OPERATOR_HEADERS:-{default_headers}}}"; set --; if [ -r "$H" ]; then set -- --header "@$H"; fi; curl "$@" -s -m 10 --retry 3 --retry-connrefused --retry-delay 1 -X POST '{url}' -H 'content-type: application/json' -H "X-Concord-Principal: ${{CONCORD_PRINCIPAL:-}}" -H "X-Concord-Pane: ${{TMUX_PANE:-}}" -H "X-Concord-Tty: $(ps -o tty= -p $PPID 2>/dev/null | tr -d ' ')" -H "X-Concord-Pid: $PPID" --data-binary @"$F" >/dev/null 2>&1; rm -f "$F") &"#,
    )
}

#[cfg(test)]
mod render_hook_command_tests {
    use super::render_hook_command;

    #[test]
    fn generated_hooks_upgrade_in_place_and_then_remain_idempotent() {
        let original = serde_json::json!({"hooks":{"Stop":[{"hooks":[{"type":"command","command":"F=$(mktemp /tmp/cnhk-XXXXXX); curl http://localhost:28080/api/v1/cc/hook/stop"}]}]}});
        let (updated, changes) = super::merge_cc_hook_entries(&original, "http://localhost:28080");
        assert!(changes.iter().any(|s| s == "Stop"));
        assert_eq!(updated["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert!(updated["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("CONTEXTNEST_OPERATOR_HEADERS"));
        let (same, changes) = super::merge_cc_hook_entries(&updated, "http://localhost:28080");
        assert_eq!(same, updated);
        assert!(changes.is_empty());
    }

    #[test]
    fn url_is_substituted_into_curl_target() {
        let cmd = render_hook_command("http://localhost:28080/api/v1/cc/hook/stop");
        assert!(cmd.contains("http://localhost:28080/api/v1/cc/hook/stop"));
        assert!(cmd.contains("-X POST"));
    }

    #[test]
    fn mktemp_template_does_not_have_suffix_after_placeholder() {
        // Regression: any `.json` (or other literal suffix) after the
        // `X` chars breaks macOS mktemp. This test fails loudly if
        // someone "fixes" the template by re-adding an extension.
        let cmd = render_hook_command("http://x.test/y");
        assert!(
            cmd.contains("mktemp /tmp/cnhk-XXXXXX)"),
            "mktemp template must end in X's, no trailing literal suffix; got: {cmd}",
        );
        assert!(
            !cmd.contains("XXXXXX.json"),
            "mktemp must not have a literal extension after the X placeholder",
        );
    }

    #[test]
    fn body_is_drained_into_tempfile_before_curl_runs() {
        let cmd = render_hook_command("http://x.test/y");
        // Order check: `cat >` must appear before `curl ... --data-binary @`
        // so the body is on disk before the network call happens.
        let cat_pos = cmd.find(r#"cat > "$F""#).expect("cat segment present");
        let curl_pos = cmd.find("curl").expect("curl segment present");
        assert!(
            cat_pos < curl_pos,
            "stdin drain must precede curl invocation"
        );
        assert!(cmd.contains(r#"--data-binary @"$F""#));
    }

    #[test]
    fn tempfile_is_cleaned_up_after_curl() {
        let cmd = render_hook_command("http://x.test/y");
        assert!(cmd.contains(r#"rm -f "$F""#));
    }

    #[test]
    fn hook_subshell_is_backgrounded() {
        let cmd = render_hook_command("http://x.test/y");
        assert!(
            cmd.trim_end().ends_with('&'),
            "trailing & required so Claude's hook protocol gets an instant ack",
        );
    }

    #[test]
    fn curl_has_realistic_timeout_and_connect_retry() {
        // Regression: `-m 1` (1-second total budget) silently dropped
        // payloads under any contention. The combo below survives
        // brief substrate restarts and OS-scheduling jitter without
        // ever blocking Claude (the call is backgrounded; see
        // `hook_subshell_is_backgrounded`).
        let cmd = render_hook_command("http://x.test/y");
        assert!(
            cmd.contains("-m 10"),
            "curl --max-time must be ≥10s; -m 1 was the cause of the inbox staleness bug. cmd: {cmd}",
        );
        assert!(
            cmd.contains("--retry 3"),
            "curl must retry on transient failure; cmd: {cmd}",
        );
        assert!(
            cmd.contains("--retry-connrefused"),
            "curl must retry across substrate-restart windows; cmd: {cmd}",
        );
        assert!(
            cmd.contains("--retry-delay 1"),
            "curl must pause between retries to give the substrate time to come up; cmd: {cmd}",
        );
        assert!(
            !cmd.contains("-m 1 "),
            "regression: -m 1 reintroduced — payloads will drop under any contention. cmd: {cmd}",
        );
    }

    // ─────────────────── merge_cc_hook_concord_tests ───────────────────
    //
    // These tests prove DoD 6: install-hooks appends the synchronous
    // /api/v1/coord/turn entry on SessionStart + UserPromptSubmit, is
    // idempotent on re-run, leaves the async entries byte-identical,
    // and respects the CONTEXTNEST_CONCORD_HOOKS=0 opt-out.
    //
    // Every test path below contains `merge_cc_hook` so the verifier
    // `cargo test --bin contextnest merge_cc_hook` finds them all.
    #[test]
    fn merge_cc_hook_fresh_settings_adds_turn_entries_and_async_entries() {
        let original = serde_json::json!({});
        let substrate = "http://localhost:28080";
        let (updated, added) = super::merge_cc_hook_entries_with(&original, substrate, true);

        for event in ["SessionStart", "UserPromptSubmit"] {
            let arr = updated["hooks"][event]
                .as_array()
                .expect("event array present");
            assert_eq!(
                arr.len(),
                2,
                "{event} must hold 2 entries (1 async + 1 turn), got: {arr:?}",
            );
            let turn_count = arr
                .iter()
                .filter(|e| {
                    e.get("hooks")
                        .and_then(|h| h.as_array())
                        .map(|hs| {
                            hs.iter().any(|h| {
                                h.get("command")
                                    .and_then(|c| c.as_str())
                                    .map(|c| c.contains("/api/v1/coord/turn"))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                })
                .count();
            assert_eq!(turn_count, 1, "{event}: exactly one turn entry");
        }
        for event in ["Stop", "TaskCompleted"] {
            let arr = updated["hooks"][event]
                .as_array()
                .expect("event array present");
            assert_eq!(arr.len(), 1, "{event} must hold 1 async entry");
            let cmd = arr[0]["hooks"][0]["command"].as_str().unwrap();
            assert!(
                !cmd.contains("/api/v1/coord/turn"),
                "{event} must not get a turn entry, got: {cmd}",
            );
        }

        assert!(
            added.iter().any(|s| s == "SessionStart (concord turn)"),
            "SessionStart turn label present"
        );
        assert!(
            added.iter().any(|s| s == "UserPromptSubmit (concord turn)"),
            "UserPromptSubmit turn label present"
        );
        assert!(
            added.iter().any(|s| s == "SessionStart"),
            "SessionStart async label present"
        );
        assert!(
            added.iter().any(|s| s == "UserPromptSubmit"),
            "UserPromptSubmit async label present"
        );
    }

    #[test]
    fn merge_cc_hook_rerun_is_idempotent_on_turn_entries() {
        let substrate = "http://localhost:28080";
        let (first, _) = super::merge_cc_hook_entries_with(&serde_json::json!({}), substrate, true);
        let (second, added) = super::merge_cc_hook_entries_with(&first, substrate, true);
        assert_eq!(
            second, first,
            "second merge must be a no-op when the substrate URL is unchanged"
        );
        assert!(added.is_empty(), "second merge must report no additions");
    }

    #[test]
    fn merge_cc_hook_async_entries_unchanged_by_concord_toggle() {
        // Same starting settings, same async command bytes regardless
        // of whether `concord=true` or `concord=false`.
        let original = serde_json::json!({});
        let substrate = "http://localhost:28080";
        let (with_concord, _) = super::merge_cc_hook_entries_with(&original, substrate, true);
        let (without_concord, _) = super::merge_cc_hook_entries_with(&original, substrate, false);

        for event in ["SessionStart", "UserPromptSubmit", "Stop", "TaskCompleted"] {
            let a = with_concord["hooks"][event].as_array().expect("a array");
            let b = without_concord["hooks"][event].as_array().expect("b array");
            // The async entry is the same entry (same index, same bytes).
            // The concord-true side has one MORE entry — the turn entry.
            // Compare only the entry at index 0 (the async entry).
            assert_eq!(
                serde_json::to_string(&a[0]).unwrap(),
                serde_json::to_string(&b[0]).unwrap(),
                "{event} async entry must be byte-identical with/without concord",
            );
        }
    }

    #[test]
    fn merge_cc_hook_pre_populated_async_kept_intact() {
        // A user's existing settings already have async entries at the
        // SAME substrate URL; the installer must upgrade them in place
        // rather than append a duplicate. Only the turn entry is
        // appended (per the brief, that's the new addition).
        let async_cmd_old = "F=$(mktemp /tmp/cnhk-XXXXXX); cat > \"$F\"; curl -s -m 10 --retry 3 --retry-connrefused --retry-delay 1 -X POST 'http://localhost:28080/api/v1/cc/hook/stop' --data-binary @\"$F\"";
        let original = serde_json::json!({
            "hooks": {
                "Stop": [
                    {"hooks": [{"type": "command", "command": async_cmd_old}]}
                ],
                "UserPromptSubmit": [
                    {"hooks": [{"type": "command", "command": "custom-user-hook"}]}
                ]
            }
        });
        let (updated, added) =
            super::merge_cc_hook_entries_with(&original, "http://localhost:28080", true);

        // Stop: same URL + mktemp prefix → upgraded in place. Still 1
        // entry, command bytes reflect the new operator-headers
        // template.
        let stop = updated["hooks"]["Stop"].as_array().expect("stop array");
        assert_eq!(stop.len(), 1, "upgraded in place, no duplicate");
        let stop_cmd = stop[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(
            stop_cmd.contains("CONTEXTNEST_OPERATOR_HEADERS"),
            "in-place upgrade must inject the operator-headers env var; got: {stop_cmd}",
        );
        assert_ne!(stop_cmd, async_cmd_old, "command bytes changed on upgrade");

        // UserPromptSubmit: custom hook (without our URL) is left
        // alone; the installer appends a NEW async entry plus the turn
        // entry → total 3 entries (custom + async + turn).
        let ups = updated["hooks"]["UserPromptSubmit"]
            .as_array()
            .expect("ups array");
        assert_eq!(ups.len(), 3);
        let cmds: Vec<&str> = ups
            .iter()
            .map(|e| e["hooks"][0]["command"].as_str().unwrap())
            .collect();
        assert!(cmds.contains(&"custom-user-hook"), "custom entry preserved");
        assert!(
            cmds.iter().any(|c| c.contains("/api/v1/coord/turn")),
            "turn entry appended"
        );
        assert!(
            cmds.iter()
                .any(|c| c.contains("/api/v1/cc/hook/user_prompt_submit")),
            "async entry appended"
        );

        // Added labels mention both new turn events and the Stop
        // upgrade.
        assert!(added.iter().any(|s| s == "UserPromptSubmit (concord turn)"));
        assert!(added.iter().any(|s| s == "SessionStart (concord turn)"));
        assert!(
            added.iter().any(|s| s == "Stop"),
            "Stop upgrade must be reported in `added`"
        );
    }

    #[test]
    fn install_path_wrapper_honours_concord_hooks_env_opt_out() {
        // install_to_target calls merge_cc_hook_entries (the env wrapper), so
        // CONTEXTNEST_CONCORD_HOOKS=0 must suppress the turn entries there.
        let substrate = "http://localhost:28080";
        let has_turn = |v: &serde_json::Value| {
            ["SessionStart", "UserPromptSubmit"].iter().any(|ev| {
                v["hooks"][*ev].as_array().is_some_and(|arr| {
                    arr.iter().any(|e| {
                        e["hooks"][0]["command"]
                            .as_str()
                            .is_some_and(|c| c.contains("/api/v1/coord/turn"))
                    })
                })
            })
        };
        std::env::set_var("CONTEXTNEST_CONCORD_HOOKS", "0");
        let (off, _) = super::merge_cc_hook_entries(&serde_json::json!({}), substrate);
        std::env::remove_var("CONTEXTNEST_CONCORD_HOOKS");
        let (on, _) = super::merge_cc_hook_entries(&serde_json::json!({}), substrate);
        assert!(!has_turn(&off), "opt-out must suppress coord/turn entries");
        assert!(has_turn(&on), "default install must add coord/turn entries");
    }

    #[test]
    fn merge_cc_hook_concord_false_adds_no_turn_entries() {
        let original = serde_json::json!({});
        let (updated, added) =
            super::merge_cc_hook_entries_with(&original, "http://localhost:28080", false);
        for event in ["SessionStart", "UserPromptSubmit"] {
            let arr = updated["hooks"][event].as_array().expect("event array");
            for entry in arr {
                let cmd = entry["hooks"][0]["command"].as_str().unwrap();
                assert!(
                    !cmd.contains("/api/v1/coord/turn"),
                    "{event} must NOT carry a turn entry when concord=false; cmd: {cmd}",
                );
            }
        }
        assert!(
            !added.iter().any(|s| s.contains("concord turn")),
            "added must not contain any concord turn label"
        );
    }

    #[test]
    fn merge_cc_hook_existing_custom_turn_command_is_not_duplicated() {
        // Operator-customised command already contains /api/v1/coord/turn.
        // The merge must NOT add a second turn entry.
        let custom_turn = "curl -X POST http://example/api/v1/coord/turn -H 'x: y'";
        let original = serde_json::json!({
            "hooks": {
                "SessionStart": [
                    {"hooks":[{"type":"command","command": custom_turn}]}
                ]
            }
        });
        let (updated, added) =
            super::merge_cc_hook_entries_with(&original, "http://localhost:28080", true);
        let arr = updated["hooks"]["SessionStart"].as_array().expect("array");
        // 1 turn (existing) + 1 async (added) = 2 total.
        assert_eq!(arr.len(), 2);
        let turn_count = arr
            .iter()
            .filter(|e| {
                e["hooks"][0]["command"]
                    .as_str()
                    .unwrap_or("")
                    .contains("/api/v1/coord/turn")
            })
            .count();
        assert_eq!(
            turn_count, 1,
            "operator's turn entry must not be duplicated"
        );
        assert!(
            !added.iter().any(|s| s == "SessionStart (concord turn)"),
            "no concord-turn label must be reported when one already exists"
        );
    }

    #[test]
    fn merge_cc_hook_rendered_command_is_synchronous_and_carries_headers() {
        let cmd = super::render_concord_turn_command("http://localhost:28080/api/v1/coord/turn");
        assert!(cmd.contains("-m 2"), "short timeout required; got: {cmd}");
        assert!(
            cmd.contains("|| true"),
            "must swallow curl errors; got: {cmd}"
        );
        assert!(
            cmd.contains("X-Concord-Pane: ${TMUX_PANE:-}"),
            "must forward TMUX_PANE; got: {cmd}",
        );
        assert!(
            cmd.contains("X-Concord-Pid: $PPID"),
            "must forward PPID; got: {cmd}",
        );
        assert!(
            cmd.contains("CONTEXTNEST_OPERATOR_HEADERS"),
            "must use the operator headers env var; got: {cmd}",
        );
        // Synchronous — must NOT end with '&'.
        assert!(
            !cmd.trim_end().ends_with('&'),
            "synchronous turn hook must not be backgrounded; got: {cmd}",
        );
        // Must NOT use a subshell or mktemp dance — those would swallow
        // the JSON body Claude Code reads.
        assert!(
            !cmd.contains("mktemp"),
            "no tempfile drain; stdout must reach Claude directly; got: {cmd}",
        );
        // localhost/28080 gets the tenant headers default.
        assert!(
            cmd.contains("$HOME/.contextnest/tenant-auth/operator.headers"),
            "localhost 28080 must use the default operator headers path; got: {cmd}",
        );

        // Off-default host: no operator headers env var default.
        let off_default =
            super::render_concord_turn_command("http://other-host:9000/api/v1/coord/turn");
        assert!(
            !off_default.contains("$HOME/.contextnest"),
            "off-default substrate must NOT inject tenant headers path; got: {off_default}",
        );
    }

    // ─────────────────── merge_cc_hook_p1_tests ───────────────────
    //
    // P1 DoD 8: install-hooks adds the synchronous PreToolUse
    // /api/v1/coord/precheck entry and the async PostToolUse
    // /api/v1/coord/footprints entry exactly once, never duplicates
    // an operator-custom entry with the same URL, leaves the P0b
    // async + turn entries byte-identical, and respects the
    // concord=false opt-out (no PreToolUse / PostToolUse keys are
    // created at all in that mode).
    //
    // Every test path below contains `merge_cc_hook` so the
    // verifier `cargo test --bin contextnest merge_cc_hook` finds
    // them all.

    #[test]
    fn merge_cc_hook_fresh_settings_adds_precheck_and_footprints_entries() {
        let original = serde_json::json!({});
        let substrate = "http://localhost:28080";
        let (updated, added) = super::merge_cc_hook_entries_with(&original, substrate, true);

        // PreToolUse: exactly one entry, with the Edit-class matcher.
        let pre = updated["hooks"]["PreToolUse"]
            .as_array()
            .expect("pre array");
        assert_eq!(
            pre.len(),
            1,
            "exactly one PreToolUse entry on fresh settings"
        );
        assert_eq!(
            pre[0]["matcher"].as_str(),
            Some("Edit|Write|MultiEdit|NotebookEdit"),
            "matcher is the Edit-class set",
        );
        let pre_cmd = pre[0]["hooks"][0]["command"].as_str().expect("cmd");
        assert!(pre_cmd.contains("/api/v1/coord/precheck"));
        assert!(pre_cmd.contains("-m 2"), "short timeout; got: {pre_cmd}");
        assert!(
            pre_cmd.contains("|| true"),
            "must swallow curl errors; got: {pre_cmd}"
        );
        assert!(pre_cmd.contains("X-Concord-Principal"));
        assert!(
            !pre_cmd.trim_end().ends_with('&'),
            "PreToolUse must not be backgrounded; got: {pre_cmd}",
        );

        // PostToolUse: exactly one entry, with the Read+Edit-class
        // matcher. Async → backgrounded, mktemp dance, m 10.
        let post = updated["hooks"]["PostToolUse"]
            .as_array()
            .expect("post array");
        assert_eq!(
            post.len(),
            1,
            "exactly one PostToolUse entry on fresh settings"
        );
        assert_eq!(
            post[0]["matcher"].as_str(),
            Some("Read|Edit|Write|MultiEdit|NotebookEdit"),
            "matcher is Read + Edit-class set",
        );
        let post_cmd = post[0]["hooks"][0]["command"].as_str().expect("cmd");
        assert!(post_cmd.contains("/api/v1/coord/footprints"));
        assert!(
            post_cmd.contains("mktemp /tmp/cnhk-"),
            "mktemp dance; got: {post_cmd}"
        );
        assert!(post_cmd.contains("X-Concord-Principal"));
        assert!(
            post_cmd.trim_end().ends_with('&'),
            "PostToolUse must be backgrounded; got: {post_cmd}",
        );

        // Added labels cover the two new events.
        assert!(
            added.iter().any(|s| s == "PreToolUse (concord precheck)"),
            "precheck label present"
        );
        assert!(
            added
                .iter()
                .any(|s| s == "PostToolUse (concord footprints)"),
            "footprints label present"
        );
    }

    #[test]
    fn merge_cc_hook_p1_rerun_is_idempotent() {
        let substrate = "http://localhost:28080";
        let (first, _) = super::merge_cc_hook_entries_with(&serde_json::json!({}), substrate, true);
        let (second, added) = super::merge_cc_hook_entries_with(&first, substrate, true);
        assert_eq!(second, first, "second merge is a no-op");
        assert!(
            !added
                .iter()
                .any(|s| s.contains("precheck") || s.contains("footprints")),
            "no precheck/footprints labels on a re-run; got: {added:?}"
        );
    }

    #[test]
    fn merge_cc_hook_p1_concord_false_omits_precheck_and_footprints() {
        let (updated, added) = super::merge_cc_hook_entries_with(
            &serde_json::json!({}),
            "http://localhost:28080",
            false,
        );
        // No PreToolUse / PostToolUse keys at all.
        assert!(
            updated["hooks"].get("PreToolUse").is_none(),
            "PreToolUse must not be created when concord=false"
        );
        assert!(
            updated["hooks"].get("PostToolUse").is_none(),
            "PostToolUse must not be created when concord=false"
        );
        assert!(
            !added
                .iter()
                .any(|s| s.contains("precheck") || s.contains("footprints")),
            "no precheck/footprints labels when concord=false"
        );
    }

    #[test]
    fn merge_cc_hook_p1_preserves_existing_custom_precheck_entry() {
        // Operator has a custom command that already contains
        // /api/v1/coord/precheck. The installer must NOT add a
        // second one.
        let custom = "curl -X POST http://example/api/v1/coord/precheck -H 'x: y'";
        let original = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks":[{"type":"command","command": custom}]}
                ]
            }
        });
        let (updated, added) =
            super::merge_cc_hook_entries_with(&original, "http://localhost:28080", true);
        let arr = updated["hooks"]["PreToolUse"].as_array().expect("array");
        // Only the operator's precheck entry; no second one.
        assert_eq!(
            arr.len(),
            1,
            "operator's precheck must be preserved, not duplicated"
        );
        assert_eq!(arr[0]["hooks"][0]["command"].as_str(), Some(custom));
        assert!(
            !added.iter().any(|s| s.contains("precheck")),
            "no precheck label when one already exists; got: {added:?}"
        );
    }

    #[test]
    fn merge_cc_hook_p1_appends_after_existing_custom_posttooluse_entry() {
        // Operator's custom PostToolUse entry stays at index 0,
        // byte-identical. The new concord entry is appended at
        // index 1.
        let custom = "echo custom-posttooluse";
        let original = serde_json::json!({
            "hooks": {
                "PostToolUse": [
                    {"matcher": "Bash", "hooks":[{"type":"command","command": custom}]}
                ]
            }
        });
        let (updated, added) =
            super::merge_cc_hook_entries_with(&original, "http://localhost:28080", true);
        let arr = updated["hooks"]["PostToolUse"].as_array().expect("array");
        assert_eq!(arr.len(), 2, "custom + new = 2 entries");
        assert_eq!(
            arr[0]["hooks"][0]["command"].as_str(),
            Some(custom),
            "operator's entry at index 0, byte-identical",
        );
        assert!(
            arr[1]["hooks"][0]["command"]
                .as_str()
                .is_some_and(|c| c.contains("/api/v1/coord/footprints")),
            "new footprints entry appended at index 1",
        );
        assert!(
            added
                .iter()
                .any(|s| s == "PostToolUse (concord footprints)"),
            "footprints label reported"
        );
    }

    #[test]
    fn merge_cc_hook_p1_async_and_turn_entries_byte_identical_with_concord() {
        // Independent baseline: concord=false renders ONLY the async ingest
        // entries, through a different code path. With P1 on, Stop and
        // TaskCompleted must equal it byte-for-byte, and SessionStart /
        // UserPromptSubmit must equal it once their single coord/turn entry
        // is removed.
        let substrate = "http://localhost:28080";
        let (with_p1, _) =
            super::merge_cc_hook_entries_with(&serde_json::json!({}), substrate, true);
        let (async_only, _) =
            super::merge_cc_hook_entries_with(&serde_json::json!({}), substrate, false);
        let is_turn = |e: &serde_json::Value| {
            e["hooks"][0]["command"]
                .as_str()
                .is_some_and(|c| c.contains("/api/v1/coord/turn"))
        };
        for event in ["SessionStart", "UserPromptSubmit", "Stop", "TaskCompleted"] {
            let with = with_p1["hooks"][event].as_array().expect("with array");
            let base = async_only["hooks"][event].as_array().expect("base array");
            let turn_entries = with.iter().filter(|e| is_turn(e)).count();
            let expected_turn = usize::from(matches!(event, "SessionStart" | "UserPromptSubmit"));
            assert_eq!(
                turn_entries, expected_turn,
                "{event}: coord/turn entry count"
            );
            let without_turn: Vec<&serde_json::Value> =
                with.iter().filter(|e| !is_turn(e)).collect();
            assert_eq!(
                serde_json::to_string(&without_turn).unwrap(),
                serde_json::to_string(&base.iter().collect::<Vec<_>>()).unwrap(),
                "{event}: async ingest entries must be byte-identical with and without Concord",
            );
        }
    }
}

fn init_logging(verbose: bool, log_to_stderr: bool) {
    let default_filter = if verbose {
        "contextnest=debug,tower_http=debug"
    } else {
        "contextnest=info,tower_http=info"
    };

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| default_filter.into());

    // In MCP stdio mode stdout is reserved for the JSON-RPC stream, so the
    // fmt layer must write to stderr; otherwise keep the default stdout.
    if log_to_stderr {
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(false)
                    .with_writer(std::io::stderr),
            )
            .try_init();
    } else {
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_target(false))
            .try_init();
    }
}

/// Resolve the WAL path from `CONTEXTNEST_WAL_PATH` or the default
/// `~/.contextnest/wal.jsonl`. Returns `None` when neither env nor `$HOME`
/// is set — in that case the server runs without persistence.
fn wal_path_from_env() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("CONTEXTNEST_WAL_PATH") {
        let p = PathBuf::from(explicit);
        if p.as_os_str().is_empty() {
            return None;
        }
        return Some(p);
    }
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home).join(".contextnest").join("wal.jsonl");
    Some(p)
}

/// Replay mode for the WAL on startup.
///
/// `Sidecars` (default) is the fast, practical path: drops every record
/// into the three sidecars in bulk and skips the canonical attractor
/// pipeline entirely. Replay throughput is HashMap-insert bound — 12k
/// records finish in well under a second. `/api/v1/inbox` and sessions
/// metadata work immediately; `/api/v1/tools/retrieve` returns empty
/// hits until the attractor store is repopulated by live writes.
///
/// `Full` runs each record through the real `store_with_id` pipeline,
/// which includes `process_memories` and (when LLM is enabled) a
/// blocking HTTP round-trip to OpenAI per fragment. Use only with
/// LLM disabled or for small WALs where you specifically need
/// canonical attractor state restored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalReplayMode {
    Sidecars,
    Full,
}

impl WalReplayMode {
    /// Resolve from `CONTEXTNEST_WAL_REPLAY_MODE` env. Unknown values
    /// fall back to the safe default with a warning rather than aborting
    /// the server — operators get a hint, the server still starts.
    fn from_env() -> Self {
        match std::env::var("CONTEXTNEST_WAL_REPLAY_MODE").ok().as_deref() {
            Some("full") => Self::Full,
            Some("sidecars") | None | Some("") => Self::Sidecars,
            Some(other) => {
                tracing::warn!(
                    mode = %other,
                    "Unknown CONTEXTNEST_WAL_REPLAY_MODE; defaulting to 'sidecars'",
                );
                Self::Sidecars
            }
        }
    }
}

/// Replay any existing WAL records into `services`, then open the WAL
/// file for append-only writes and install the writer into the services'
/// OnceCell. Subsequent successful `store` HTTP calls will append.
async fn bootstrap_wal(
    services: &contextnest::services::ContextNestServices,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use contextnest::services::wal::{Wal, WalRecord};

    let mode = WalReplayMode::from_env();
    let records = Wal::read_records(path)?;
    let total = records.len();

    if total == 0 {
        tracing::info!(
            wal_path = %path.display(),
            "WAL replay: no prior records (cold start)",
        );
    } else {
        tracing::info!(
            wal_path = %path.display(),
            records = total,
            mode = ?mode,
            "WAL replay: starting",
        );
    }

    // One-shot migration: old WAL records carry session_id =
    // `cc-<full-uuid>` (or even older `cc-<first-8>`); current code emits
    // the bare UUID. Rewrite in-place — strip the `cc-` prefix and, for
    // short-form records, expand via metadata.src_session. Atomically
    // replace the on-disk WAL so the next restart is a no-op. Idempotent —
    // already-bare records pass through untouched.
    let (records, mig_report) =
        contextnest::services::wal::migrate_legacy_session_ids(path, records)?;
    if mig_report.migrated > 0 || mig_report.skipped_no_src_session > 0 {
        tracing::info!(
            migrated = mig_report.migrated,
            skipped = mig_report.skipped_no_src_session,
            wal_path = %path.display(),
            "session_id migration: WAL rewritten to canonical bare-UUID form",
        );
    }

    let start = std::time::Instant::now();

    // Slice 2.5: pull out LLM-cache records and replay them into the
    // in-memory cache before dispatching the remaining records to the
    // substrate replay path. Cache replay is independent of
    // sidecars/full mode (and cheap — just rebuilds an in-memory map).
    let now_unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cache_replayed = services.llm_cache.replay(&records, now_unix_secs);
    if cache_replayed > 0 {
        tracing::info!(
            count = cache_replayed,
            "WAL replay: restored llm_cache entries"
        );
    }

    let (replayed, failed) = match mode {
        WalReplayMode::Sidecars => replay_sidecars(services, records).await,
        WalReplayMode::Full => replay_full(services, records).await,
    };

    if total > 0 {
        tracing::info!(
            replayed,
            failed,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "WAL replay: complete",
        );
    }

    // Open writer for live appends, install in OnceCell. After this point,
    // every store HTTP call writes through to disk before the response.
    let writer = Wal::open_for_append(path.to_path_buf())?;
    services
        .wal
        .set(writer)
        .map_err(|_| "WAL OnceCell already initialized".to_string())?;
    tracing::info!(wal_path = %path.display(), "WAL writer opened for append");

    Ok(())
}

/// Sidecars-only fast replay. Bulk-inserts into the three sidecars; does
/// not touch the canonical attractor manager. Returns (replayed, failed).
async fn replay_sidecars(
    services: &contextnest::services::ContextNestServices,
    records: Vec<contextnest::services::wal::WalRecord>,
) -> (usize, usize) {
    use contextnest::services::wal::WalRecord;

    // Project Store records into the tuple shape
    // `restore_sidecars_bulk` wants. Importance is dropped — sidecars-
    // only doesn't store it (canonical fragments do, and those are
    // skipped in this mode). Non-Store variants (e.g. LlmCacheInsert)
    // were already handled by `LlmCacheService::replay` earlier in
    // `bootstrap_wal`; filter them out here.
    let projected: Vec<_> = records
        .into_iter()
        .filter_map(|r| match r {
            WalRecord::Store {
                fragment_id,
                session_id,
                content,
                importance: _,
                metadata,
            } => Some((fragment_id, session_id, content, metadata)),
            _ => None,
        })
        .collect();

    let count = projected.len();
    contextnest::api::tools::restore_sidecars_bulk(services, projected).await;
    (count, 0)
}

/// Full replay — runs each record through the live `store_with_id`
/// pipeline. Slow when the LLM provider is enabled; only use for small
/// WALs or with LLM disabled. Emits a progress log every 100 records so
/// operators can see whether it's still making progress.
async fn replay_full(
    services: &contextnest::services::ContextNestServices,
    records: Vec<contextnest::services::wal::WalRecord>,
) -> (usize, usize) {
    use contextnest::services::wal::WalRecord;

    let total = records.len();
    let mut replayed = 0usize;
    let mut failed = 0usize;

    for (idx, record) in records.into_iter().enumerate() {
        match record {
            WalRecord::Store {
                fragment_id,
                session_id,
                content,
                importance,
                metadata,
            } => match contextnest::api::tools::store_with_id(
                services,
                &fragment_id,
                &session_id,
                &content,
                importance,
                metadata,
            )
            .await
            {
                Ok(()) => replayed += 1,
                Err(e) => {
                    failed += 1;
                    tracing::warn!(
                        fragment_id = %fragment_id,
                        error = %e,
                        "WAL replay: store_with_id failed; skipping record",
                    );
                }
            },
            // Cache-insert + cache-discard records were already
            // applied in bootstrap_wal via `LlmCacheService::replay`
            // (which handles tombstone-vs-insert ordering) before
            // this loop. Skip silently here.
            WalRecord::LlmCacheInsert { .. } | WalRecord::LlmCacheDiscard { .. } => {}
        }
        if (idx + 1) % 100 == 0 {
            tracing::info!(
                done = idx + 1,
                total,
                replayed,
                failed,
                "WAL replay: progress",
            );
        }
    }

    (replayed, failed)
}
