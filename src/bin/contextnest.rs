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
use std::path::PathBuf;
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
                let cn_id = format!("cc-{}", &s.session_uuid[..s.session_uuid.len().min(8)]);
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
        } => ingest_claude_code(project, session_id, since, dry_run, substrate, projects_dir).await,
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
            &s.session_uuid[..s.session_uuid.len().min(8)],
            mb,
            s.project_cwd
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
                eprintln!(
                    "  ✗ session {}: {}",
                    &s.session_uuid[..s.session_uuid.len().min(8)],
                    e
                );
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
