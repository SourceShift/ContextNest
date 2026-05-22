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
use contextnest::cli::{Cli, Commands, IngestSource};
use contextnest::config::Config;
use contextnest::inbox::{render_json, render_text, InboxItem};
use contextnest::ingest::claude_code::{
    discover_sessions, ingest_session_file, parse_since, DryRunSink, HttpSink, Sink, SinkReport,
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
    init_logging(cli.verbose);
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
        } => inbox(project, urgency, substrate, session_id, json).await,
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
///      and derive `cc-<8char>` substrate session_ids from each UUID.
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
        // Derive substrate session ids: cc-<8char> of each Claude Code UUID.
        // Dedup in case multiple .jsonl files share a UUID prefix (unlikely
        // but defensive).
        let mut seen = HashSet::new();
        discovered
            .iter()
            .filter_map(|s| {
                if s.session_uuid.is_empty() {
                    return None;
                }
                let cn_id = format!("cc-{}", s.session_uuid);
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

    if json_mode {
        println!("{}", render_json(&all_items)?);
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
    }
}

/// Install the four real-time hooks (SessionStart, UserPromptSubmit,
/// Stop, TaskCompleted) into the user-level Claude settings AND into
/// every explicitly-named project's local settings. Each target file is
/// backed up before write; the merge is idempotent (existing entries
/// detected by their `cc/hook/` URL substring are skipped).
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
        println!(
            "        + {:<22} -> {}/api/v1/cc/hook/{}",
            ev,
            substrate.trim_end_matches('/'),
            cc_hook_path_segment(ev)
        );
    }
    Ok(HookInstallOutcome::Wrote)
}

/// Merge ContextNest hook entries into a settings JSON value. Returns
/// `(updated_value, events_appended)`. Pure function so it can be
/// unit-tested without touching disk. Entries are detected by URL
/// substring (`/api/v1/cc/hook/`) so a re-run after a substrate URL
/// change still appends — that's intentional, so a user pointing
/// at a new substrate gets a fresh entry next to the old one.
fn merge_cc_hook_entries(existing: &Value, substrate: &str) -> (Value, Vec<&'static str>) {
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

    let mut added: Vec<&'static str> = Vec::new();
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

        let already_present = arr.iter().any(|entry| {
            entry
                .get("hooks")
                .and_then(Value::as_array)
                .map(|hooks| {
                    hooks.iter().any(|h| {
                        h.get("command")
                            .and_then(Value::as_str)
                            .is_some_and(|c| c.contains(&url))
                    })
                })
                .unwrap_or(false)
        });
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
        added.push(event_name);
    }

    (root, added)
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

    // Pick the sink based on --dry-run.
    if dry_run {
        let sink = DryRunSink::new();
        let total = process_sessions(&sessions, &sink).await?;
        let by_kind = sink.captured_by_kind().await;
        report_summary(total, &by_kind, true);
    } else {
        let sink = HttpSink::new(&substrate);
        println!("Pushing to substrate at {}", substrate);
        let total = process_sessions(&sessions, &sink).await?;
        report_summary(total, &std::collections::HashMap::new(), false);
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
        entries.sort_by_key(|(k, _)| k.clone());
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

/// Start the HTTP server (seven-tool memory API + health/status).
async fn serve(bind_override: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting ContextNest server");

    let config = load_configuration().await?;
    tracing::info!("Configuration loaded successfully");

    let bind_address = bind_override
        .unwrap_or_else(|| format!("{}:{}", config.api.rest.bind_address, config.api.rest.port));

    let services = ContextNestServices::new(config).await?;
    tracing::info!("Core services initialized successfully");

    // WAL bootstrap: replay any persisted records BEFORE opening the writer.
    // The writer is intentionally `None` during replay so that
    // `store_with_id` (called from the replay loop) does not re-log records
    // to disk. Once replay finishes we open the writer in append mode and
    // set it in the OnceCell; from that point onward every successful
    // `store` HTTP call appends a fresh record.
    if let Some(wal_path) = wal_path_from_env() {
        bootstrap_wal(&services, &wal_path).await?;
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
    format!(
        r#"F=$(mktemp /tmp/cnhk-XXXXXX); cat > "$F"; (curl -s -m 10 --retry 3 --retry-connrefused --retry-delay 1 -X POST {url} -H "content-type: application/json" --data-binary @"$F" >/dev/null 2>&1; rm -f "$F") &"#,
    )
}

#[cfg(test)]
mod render_hook_command_tests {
    use super::render_hook_command;

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
}

fn init_logging(verbose: bool) {
    let default_filter = if verbose {
        "contextnest=debug,tower_http=debug"
    } else {
        "contextnest=info,tower_http=info"
    };

    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default_filter.into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .try_init();
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

    // One-shot migration: old WAL records carry session_id = `cc-<first-8>`,
    // new code emits `cc-<full-uuid>`. Rewrite in-place using each
    // record's metadata.src_session as the source of truth, then
    // atomically replace the on-disk WAL so the next restart is a no-op.
    // Idempotent — long-form records pass through untouched.
    let (records, mig_report) =
        contextnest::services::wal::migrate_short_session_ids(path, records)?;
    if mig_report.migrated > 0 || mig_report.skipped_no_src_session > 0 {
        tracing::info!(
            migrated = mig_report.migrated,
            skipped = mig_report.skipped_no_src_session,
            wal_path = %path.display(),
            "session_id migration: WAL rewritten to canonical cc-<full-uuid>",
        );
    }

    let start = std::time::Instant::now();
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

    // Project records into the tuple shape `restore_sidecars_bulk` wants.
    // Importance is dropped — sidecars-only doesn't store it (canonical
    // fragments do, and those are skipped in this mode).
    let projected: Vec<_> = records
        .into_iter()
        .map(|r| match r {
            WalRecord::Store {
                fragment_id,
                session_id,
                content,
                importance: _,
                metadata,
            } => (fragment_id, session_id, content, metadata),
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
