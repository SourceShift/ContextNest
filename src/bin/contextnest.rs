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
use contextnest::cli::{Cli, Commands};
use contextnest::config::Config;
use contextnest::services::ContextNestServices;
use std::process;
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
    }
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
