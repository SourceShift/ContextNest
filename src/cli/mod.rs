/// ContextNest CLI Tools
/// Command-line interface for ContextNest
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// ContextNest CLI
#[derive(Parser)]
#[command(name = "contextnest")]
#[command(about = "ContextNest CLI")]
#[command(version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Configuration file path
    #[arg(short, long, default_value = "contextnest.toml")]
    pub config: PathBuf,
}

/// Available CLI commands
/// `field`, `test`, `status` subcommand families were
/// removed — their handlers in `src/bin/contextnest.rs` printed
/// "not wired up in this build" and exited 0 silently, which broke CI
/// scripts that probed the exit code. The corresponding enum families
/// (`FieldCommands`, `TestCommands`, `StatusCommands`) were also deleted
/// to keep `--help` honest. When implementations land, re-introduce as
/// new feature-gated commands.
#[derive(Subcommand)]
pub enum Commands {
    /// Start the ContextNest HTTP server
    Serve {
        /// Bind address override (e.g. 0.0.0.0:7050). Falls back to config.
        #[arg(long)]
        bind: Option<String>,
    },
}

/// CLI execution result
pub type CliResult<T = ()> = Result<T, CliError>;

/// CLI error types
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Plugin error: {0}")]
    Plugin(String),

    #[error("Build error: {0}")]
    Build(String),

    #[error("Test error: {0}")]
    Test(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Command not implemented: {0}")]
    NotImplemented(String),

    #[error("Dialoguer error: {0}")]
    Dialoguer(#[from] dialoguer::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Protocol error: {0}")]
    Protocol(String),
}

/// Plugin types supported by the CLI
#[derive(Debug, Clone)]
pub enum PluginType {
    /// Domain plugin extension point (reserved for future integrations)
    Domain,
    /// Context provider plugin
    Context,
    /// Embedding strategy plugin
    Embedding,
    /// Analysis engine plugin
    Analysis,
    /// Custom plugin type
    Custom(String),
}

impl std::str::FromStr for PluginType {
    type Err = CliError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "domain" => Ok(PluginType::Domain),
            "context" => Ok(PluginType::Context),
            "embedding" => Ok(PluginType::Embedding),
            "analysis" => Ok(PluginType::Analysis),
            custom => Ok(PluginType::Custom(custom.to_string())),
        }
    }
}

/// Project types supported by the CLI
#[derive(Debug, Clone)]
pub enum ProjectType {
    /// Plugin project
    Plugin,
    /// Application project
    Application,
    /// Library project
    Library,
}

impl std::str::FromStr for ProjectType {
    type Err = CliError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "plugin" => Ok(ProjectType::Plugin),
            "application" | "app" => Ok(ProjectType::Application),
            "library" | "lib" => Ok(ProjectType::Library),
            _ => Err(CliError::Config(format!("Unknown project type: {}", s))),
        }
    }
}

/// Build targets
#[derive(Debug, Clone)]
pub enum BuildTarget {
    Debug,
    Release,
}

impl std::str::FromStr for BuildTarget {
    type Err = CliError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "debug" => Ok(BuildTarget::Debug),
            "release" => Ok(BuildTarget::Release),
            _ => Err(CliError::Config(format!("Unknown build target: {}", s))),
        }
    }
}
