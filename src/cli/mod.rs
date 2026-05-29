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

    /// Ingest external session data into the substrate.
    Ingest {
        #[command(subcommand)]
        source: IngestSource,
    },

    /// Print an urgency-sorted action list for every session waiting on
    /// the user. Queries the substrate's `/api/v1/tools/retrieve` with
    /// metadata filters for `kind=user_action` and `kind=decision +
    /// awaiting_decision=true`, groups by session, renders to the
    /// terminal.
    ///
    /// Examples:
    ///   contextnest inbox
    ///   contextnest inbox --project ContextNest
    ///   contextnest inbox --urgency now
    ///   contextnest inbox --json
    Inbox {
        /// Substring-match the project cwd in metadata.project_cwd.
        /// Case-insensitive. Omit to include every project.
        #[arg(long)]
        project: Option<String>,

        /// Only show items with this urgency. One of: `now`, `soon`,
        /// `later`. Omit to show everything sorted with now first.
        #[arg(long)]
        urgency: Option<String>,

        /// Substrate base URL. Default: `http://localhost:8080`.
        #[arg(long, default_value = "http://localhost:8080")]
        substrate: String,

        /// Specific session_id to scope the inbox to. Omit to query
        /// every session the substrate knows about.
        #[arg(long)]
        session_id: Option<String>,

        /// Machine-readable JSON output instead of the terminal-styled
        /// list. Suitable for `jq` / scripts / piping into other tools.
        #[arg(long)]
        json: bool,
    },

    /// Run the Model Context Protocol server so MCP-speaking agents
    /// (Claude Code, Cursor, Zed, ...) can call ContextNest's memory
    /// tools natively instead of shelling out to `curl`.
    Mcp {
        #[command(subcommand)]
        action: McpCommands,
    },
}

/// MCP server modes. v0.x ships only the stdio transport (the standard
/// MCP subprocess transport invoked by the host agent).
#[derive(Subcommand)]
pub enum McpCommands {
    /// Serve MCP tools over stdio. The host agent spawns this as a
    /// subprocess and speaks newline-delimited JSON-RPC 2.0 over
    /// stdin/stdout.
    ///
    /// Example `~/.claude/settings.json` `mcpServers` entry:
    ///   "contextnest": {
    ///     "command": "contextnest",
    ///     "args": ["mcp", "serve"],
    ///     "env": { "CONTEXTNEST_URL": "http://localhost:28080" }
    ///   }
    Serve {
        /// Substrate base URL. Falls back to `$CONTEXTNEST_URL`, then
        /// `http://localhost:8080`.
        #[arg(long)]
        url: Option<String>,
    },
}

/// Sources the ingester can pull from. v0.2 phase 1 ships only the Claude
/// Code adapter; more adapters land as siblings here.
#[derive(Subcommand)]
pub enum IngestSource {
    /// Ingest Claude Code session transcripts from `~/.claude/projects/`.
    ///
    /// Examples:
    ///   contextnest ingest claude-code --project ContextNest --since 7d
    ///   contextnest ingest claude-code --session-id 4c998114 --dry-run
    ///   contextnest ingest claude-code --substrate http://localhost:28080 \
    ///       --project ContextNest
    ///
    /// `--dry-run` prints what WOULD be stored without hitting the
    /// substrate. Useful for sanity-checking extraction before mass-ingest.
    #[command(name = "claude-code")]
    ClaudeCode {
        /// Substring-match the project cwd (case-insensitive). Skipped
        /// when omitted — every project in the projects dir is ingested.
        #[arg(long)]
        project: Option<String>,

        /// Specific session UUID (or first-N-chars prefix). When set,
        /// `--project` and `--since` are ignored.
        #[arg(long)]
        session_id: Option<String>,

        /// Only sessions modified within this duration. Format: `<n>{s,m,h,d,w}`.
        /// Example: `7d` for 7 days, `24h` for 24 hours.
        #[arg(long)]
        since: Option<String>,

        /// Print records to stdout instead of POSTing. No network calls.
        #[arg(long)]
        dry_run: bool,

        /// Substrate base URL. Default: `http://localhost:8080`.
        #[arg(long, default_value = "http://localhost:8080")]
        substrate: String,

        /// Override the discovery root. Default: `~/.claude/projects/`.
        #[arg(long)]
        projects_dir: Option<PathBuf>,

        /// Append the four real-time hooks (SessionStart,
        /// UserPromptSubmit, Stop, TaskCompleted) to
        /// `~/.claude/settings.json` (user-level), pointing at
        /// `<substrate>/api/v1/cc/hook/*`. Backs up the existing settings
        /// file with a `.bak-<ts>` suffix before writing, and APPENDS —
        /// existing hooks (z-dashboard, claude-status-writer, etc.) stay
        /// in place. Re-running is idempotent: existing ContextNest hook
        /// entries are detected by their `cc/hook/` URL substring and
        /// skipped. When this flag is set, no ingest happens — the
        /// command exits after writing settings.
        #[arg(long)]
        install_hooks: bool,

        /// Project root path to ALSO install hooks into, as a sibling to
        /// the user-level install. Repeatable. For each path, hooks are
        /// appended to `<path>/.claude/settings.local.json` (created if
        /// missing). Use this when a project defines its own hooks for
        /// SessionStart/UserPromptSubmit/Stop/TaskCompleted that would
        /// otherwise shadow the user-level ContextNest entries.
        /// Example: --install-hooks --project-path ~/code/researcher
        ///                          --project-path ~/code/other-proj
        #[arg(long = "project-path", action = clap::ArgAction::Append)]
        project_paths: Vec<PathBuf>,
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
