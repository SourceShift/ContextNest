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
use contextnest::ingest::claude_code::{
    discover_sessions, ingest_session_file, parse_since, DryRunSink, HttpSink, Sink, SinkReport,
};
use contextnest::services::ContextNestServices;
use std::path::PathBuf;
use std::process;
use std::time::SystemTime;
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
    }
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
