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

    /// Query the substrate's `prompt-context` surface — the deterministic
    /// L1 / L1.5 read layer over trajectory atoms (decisions, failures,
    /// verifications, risks, ...). Pairs cleanly with shell pipes:
    ///
    ///   contextnest prompt-context capsule --query auth | pbcopy
    ///   contextnest prompt-context capsule --since 7d > .claude/ctx.md
    #[command(name = "prompt-context")]
    PromptContext {
        #[command(subcommand)]
        action: PromptContextCommands,
    },

    /// List features the substrate has seen agents ship — across every
    /// session — and render as Markdown (default) or JSON. The "what did
    /// I miss while away" workflow:
    ///
    ///   contextnest features --since 24h | pbcopy
    ///   contextnest features --since 7d --layer backend
    ///   contextnest features --project ContextNest --json | jq
    Features {
        /// Age window (default `24h`). Format: `<n>{m,h,d}`.
        #[arg(long)]
        since: Option<String>,

        /// Layer filter (case-insensitive: backend / frontend / infra /
        /// docs / tests / other). Matches the `layer` field on the
        /// agent's `delivered_features[]` z-insight entries.
        #[arg(long)]
        layer: Option<String>,

        /// Substring match on `project_cwd`.
        #[arg(long)]
        project: Option<String>,

        /// Switch from Markdown stdout (default — pipe into pbcopy /
        /// redirect to file / paste into prompt) to raw JSON (suitable
        /// for `jq`).
        #[arg(long)]
        json: bool,

        /// Substrate base URL. Falls back to `$CONTEXTNEST_URL`, then
        /// `http://localhost:8080`.
        #[arg(long)]
        url: Option<String>,
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

/// `prompt-context` subcommand modes. Currently exposes only `capsule`
/// because the JSON `/atoms` and `/clusters` surfaces are already easy to
/// consume via `curl` and `jq`; the Markdown capsule is the one that
/// genuinely benefits from a CLI shortcut (paste-into-prompt workflow).
#[derive(Subcommand)]
pub enum PromptContextCommands {
    /// Print a Markdown prompt-context capsule to stdout. Body shape and
    /// kind ordering match `GET /api/v1/prompt-context/capsule`. Stdout
    /// carries the Markdown; logs and errors go to stderr — so a pipe
    /// into `pbcopy` or redirect into a file gives you ONLY the capsule.
    ///
    /// Examples:
    ///   contextnest prompt-context capsule
    ///   contextnest prompt-context capsule --query auth --since 14d
    ///   contextnest prompt-context capsule --project ContextNest > ctx.md
    Capsule {
        /// Case-insensitive substring filter on cluster normalized text.
        #[arg(long)]
        query: Option<String>,

        /// Substring match on the project_cwd metadata.
        #[arg(long)]
        project: Option<String>,

        /// Scope to a single src_session UUID.
        #[arg(long)]
        session_id: Option<String>,

        /// Age window suffix (`30d`, `24h`, `90m`). Default: `30d`.
        #[arg(long)]
        since: Option<String>,

        /// Drop clusters whose total count is below this. Default: 2.
        /// Set to 1 to include solo-occurrence atoms.
        #[arg(long)]
        min_count: Option<usize>,

        /// Cap clusters listed per kind. Default: 5, max: 25.
        #[arg(long)]
        max_per_kind: Option<usize>,

        /// When set, additionally merge clusters whose representative
        /// embeddings clear cosine ≥ 0.85 (paraphrase dedup). The capsule
        /// body's header line gains a `· semantic merge ON` annotation so
        /// you can confirm the flag took effect. Gracefully degrades to
        /// deterministic-only when fragment embeddings aren't yet
        /// hydrated; never errors.
        #[arg(long)]
        semantic: bool,

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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// `contextnest prompt-context capsule` with no flags must parse and
    /// route to the Capsule variant with every option `None`.
    #[test]
    fn prompt_context_capsule_parses_with_no_flags() {
        let cli =
            Cli::try_parse_from(["contextnest", "prompt-context", "capsule"]).expect("must parse");
        match cli.command {
            Commands::PromptContext {
                action:
                    PromptContextCommands::Capsule {
                        query,
                        project,
                        session_id,
                        since,
                        min_count,
                        max_per_kind,
                        semantic,
                        url,
                    },
            } => {
                assert!(query.is_none());
                assert!(project.is_none());
                assert!(session_id.is_none());
                assert!(since.is_none());
                assert!(min_count.is_none());
                assert!(max_per_kind.is_none());
                assert!(!semantic, "semantic flag must default to false");
                assert!(url.is_none());
            }
            _ => panic!("expected Commands::PromptContext{{Capsule{{..}}}}"),
        }
    }

    /// All flags carry through to the parsed variant.
    #[test]
    fn prompt_context_capsule_parses_with_all_flags() {
        let cli = Cli::try_parse_from([
            "contextnest",
            "prompt-context",
            "capsule",
            "--query",
            "auth",
            "--project",
            "ContextNest",
            "--session-id",
            "sid-xyz",
            "--since",
            "14d",
            "--min-count",
            "3",
            "--max-per-kind",
            "7",
            "--semantic",
            "--url",
            "http://localhost:28080",
        ])
        .expect("must parse");
        match cli.command {
            Commands::PromptContext {
                action:
                    PromptContextCommands::Capsule {
                        query,
                        project,
                        session_id,
                        since,
                        min_count,
                        max_per_kind,
                        semantic,
                        url,
                    },
            } => {
                assert_eq!(query.as_deref(), Some("auth"));
                assert_eq!(project.as_deref(), Some("ContextNest"));
                assert_eq!(session_id.as_deref(), Some("sid-xyz"));
                assert_eq!(since.as_deref(), Some("14d"));
                assert_eq!(min_count, Some(3));
                assert_eq!(max_per_kind, Some(7));
                assert!(semantic, "--semantic flag must parse as true");
                assert_eq!(url.as_deref(), Some("http://localhost:28080"));
            }
            _ => panic!("expected Commands::PromptContext{{Capsule{{..}}}}"),
        }
    }

    /// `contextnest features` with no flags must parse to the Features
    /// variant with json defaulting to false (Markdown is the default).
    #[test]
    fn features_parses_with_no_flags() {
        let cli = Cli::try_parse_from(["contextnest", "features"]).expect("must parse");
        match cli.command {
            Commands::Features {
                since,
                layer,
                project,
                json,
                url,
            } => {
                assert!(since.is_none());
                assert!(layer.is_none());
                assert!(project.is_none());
                assert!(!json, "json must default to false (Markdown is default)");
                assert!(url.is_none());
            }
            _ => panic!("expected Commands::Features{{..}}"),
        }
    }

    /// All flags + the `--json` toggle threads through correctly.
    #[test]
    fn features_parses_with_all_flags() {
        let cli = Cli::try_parse_from([
            "contextnest",
            "features",
            "--since",
            "7d",
            "--layer",
            "backend",
            "--project",
            "ContextNest",
            "--json",
            "--url",
            "http://localhost:28080",
        ])
        .expect("must parse");
        match cli.command {
            Commands::Features {
                since,
                layer,
                project,
                json,
                url,
            } => {
                assert_eq!(since.as_deref(), Some("7d"));
                assert_eq!(layer.as_deref(), Some("backend"));
                assert_eq!(project.as_deref(), Some("ContextNest"));
                assert!(json, "--json flag must parse as true");
                assert_eq!(url.as_deref(), Some("http://localhost:28080"));
            }
            _ => panic!("expected Commands::Features{{..}}"),
        }
    }
}
