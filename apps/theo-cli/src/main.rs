mod cmd;
mod cmd_skill;
mod config;
mod dashboard;
mod dashboard_agents;
mod init;
mod mcp_admin;
mod input;
mod json_output;
mod memory_lint;
mod permission;
mod pilot;
mod prompt_override;
mod render;
mod renderer;
mod runtime_features;
mod status_line;
mod subagent_admin;
mod tui;
mod tty;

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::cmd::*;
use crate::mcp_admin::McpCmd;
use crate::subagent_admin::{AgentsCmd, CheckpointsCmd, SubagentCmd};

// ---------------------------------------------------------------------------
// CLI definition (Clap derive)
// ---------------------------------------------------------------------------

/// Theo — autonomous coding agent
///
/// Run without arguments to start the interactive TUI.
/// Run with a prompt to execute a single task and exit.
///
/// Examples:
///   theo                          Start interactive TUI
///   theo "fix the bug in auth"    Execute task and exit
///   theo init                     Initialize project
///   theo pilot "implement X"      Autonomous loop
///   theo memory lint              Memory-subsystem lint
#[derive(Parser)]
#[command(name = "theo", version = "0.1.0")]
struct Cli {
    /// Project directory
    #[arg(long, global = true, default_value = ".")]
    repo: PathBuf,

    /// LLM provider (auto-detected if omitted)
    #[arg(long, global = true)]
    provider: Option<String>,

    /// Model name override
    #[arg(long, global = true)]
    model: Option<String>,

    /// Maximum iterations
    #[arg(long, global = true)]
    max_iter: Option<usize>,

    /// Agent mode (headless only — interactive mode uses `/mode` slash command)
    #[arg(long, global = true, value_parser = ["agent", "plan", "ask"])]
    mode: Option<String>,

    /// Headless mode for benchmarks/CI: read prompt from args (or stdin),
    /// emit a single JSON result line on stdout, no banners/REPL/streaming.
    /// Exit code 0 = success, 1 = failure. -p is an alias matching Claude Code.
    #[arg(short = 'p', long, global = true)]
    headless: bool,

    /// Sampling temperature (0.0 = deterministic). Overrides THEO_TEMPERATURE env var
    /// and .theo/config.toml. Required for reproducible benchmarks.
    #[arg(long, global = true)]
    temperature: Option<f32>,

    /// Random seed for LLM sampling (provider-dependent). Aids reproducibility
    /// when combined with temperature=0.0.
    #[arg(long, global = true)]
    seed: Option<u64>,

    /// Phase 13: enable hot-reload of `.theo/agents/` and `~/.theo/agents/`.
    /// When set, modifications to project agent specs are detected via
    /// filesystem watcher (debounce 500ms) and trigger registry re-load.
    /// Modified specs require re-approval via S3 manifest.
    #[arg(long, global = true)]
    watch_agents: bool,

    /// Phase 9: enable automatic checkpoint snapshots before file mutations
    /// (write/edit/apply_patch/bash). Shadow git repo at
    /// ~/.theo/checkpoints/{sha16}/. Use `theo checkpoints restore` to revert.
    #[arg(long, global = true)]
    enable_checkpoints: bool,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Task to execute (opens TUI if omitted, ignored when using subcommands)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    prompt: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize project — creates .theo/theo.md with AI analysis
    Init,

    /// Interactive REPL or single-shot task execution (same as default)
    Agent {
        /// Task to execute (opens REPL if omitted)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        prompt: Vec<String>,
    },

    /// Autonomous loop until promise is fulfilled
    Pilot {
        /// Maximum pilot loops
        #[arg(long)]
        calls: Option<usize>,

        /// Max loops per hour (rate limit)
        #[arg(long)]
        rate: Option<usize>,

        /// Definition of Done — criteria for success
        #[arg(long)]
        complete: Option<String>,

        /// Promise to fulfill (reads .theo/PROMPT.md if omitted)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        promise: Vec<String>,
    },

    /// Assemble context for a task using GRAPHCTX
    Context {
        /// Repository path
        repo_path: PathBuf,

        /// Query to search for
        query: Vec<String>,
    },

    /// Analyze impact of editing a file
    Impact {
        /// Repository path
        repo_path: PathBuf,

        /// File to analyze
        file: String,
    },

    /// Show graph statistics for a repository
    Stats {
        /// Repository path
        repo_path: PathBuf,
    },

    /// Memory subsystem utilities (lint, inspect).
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },

    /// Authenticate with a provider (OAuth device flow or API key).
    ///
    /// `theo login`              → OpenAI OAuth device flow (default).
    /// `theo login --key <K>`    → Persist `K` as an API key.
    /// `theo login --server <U>` → Generic RFC 8628 device flow.
    Login {
        /// API key (`sk-...` or `sess-...`). Skips OAuth entirely.
        #[arg(long)]
        key: Option<String>,

        /// Custom RFC 8628 device-flow server URL.
        #[arg(long)]
        server: Option<String>,

        /// Do not auto-open a browser (headless/SSH sessions).
        #[arg(long)]
        no_browser: bool,
    },

    /// Remove saved credentials (OpenAI provider).
    Logout,

    /// Start the observability dashboard HTTP server.
    ///
    /// Serves the built Theo UI bundle and exposes the observability API
    /// (`/api/list_runs`, `/api/run/:id/trajectory`, ...) so remote operators
    /// can view trajectories via `ssh -L <port>:localhost:<port>`.
    Dashboard {
        /// TCP port (default: 5173).
        #[arg(long, default_value_t = 5173)]
        port: u16,

        /// Override path to the built UI bundle. Defaults to an autodetect
        /// that looks for `apps/theo-ui/dist` or `<exe>/dashboard-dist`.
        #[arg(long)]
        static_dir: Option<PathBuf>,
    },

    /// Manage persisted sub-agent runs (Phase 10).
    Subagent {
        #[command(subcommand)]
        action: subagent_admin::SubagentCmd,
    },

    /// Manage workdir checkpoints (shadow git repos, Phase 9).
    Checkpoints {
        #[command(subcommand)]
        action: subagent_admin::CheckpointsCmd,
    },

    /// Manage project agents approval (Phase 2 / S3 manifest).
    Agents {
        #[command(subcommand)]
        action: subagent_admin::AgentsCmd,
    },

    /// Manage MCP discovery cache (Phase 21 / sota-gaps-followup).
    Mcp {
        #[command(subcommand)]
        action: mcp_admin::McpCmd,
    },

    /// T9.1 — Skill catalog: list / view / delete user-installed skills.
    ///
    /// Skills live under `$THEO_HOME/skills/<name>/SKILL.md` (default
    /// `$THEO_HOME = ~/.theo`). Each skill is a directory with a
    /// `SKILL.md` (frontmatter + body) plus optional `references/`,
    /// `templates/`, `assets/`, `scripts/` subdirs.
    Skill {
        #[command(subcommand)]
        action: SkillCmd,
    },

    /// T16.1 / D16 — Trajectory export tooling.
    ///
    /// Reads `.theo/trajectories/*.jsonl` rating envelopes (T16.1
    /// wire format) and writes a JSONL file ready for downstream
    /// RLHF tooling (axolotl / trl / similar). Theo does NOT train
    /// inside; this is export-only per ADR D16.
    Trajectory {
        #[command(subcommand)]
        action: TrajectoryCmd,
    },
}

/// T9.1 — Subcommands for `theo skill`.
#[derive(Subcommand)]
enum SkillCmd {
    /// List metadata for every installed skill (tier 1 — fast).
    List {
        /// Output format: `text` (default) or `json`.
        #[arg(long)]
        format: Option<String>,
    },

    /// View the full body + linked files of one skill (tier 2).
    View {
        /// Skill name (matches the directory under `skills/`).
        name: String,
    },

    /// Delete an installed skill (removes its directory under
    /// `$THEO_HOME/skills/<name>/`). NOT a network operation —
    /// purely local filesystem cleanup.
    Delete {
        /// Skill name to remove.
        name: String,

        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

/// T16.1 / D16 — Subcommands for `theo trajectory`.
#[derive(Subcommand)]
enum TrajectoryCmd {
    /// Export rating envelopes to a JSONL file consumable by
    /// downstream RLHF tooling. Reads
    /// `<repo>/.theo/trajectories/*.jsonl` and writes filtered
    /// records to `--out`.
    ///
    /// Filters mirror `RatingFilter`:
    ///   `all` (default) — every rating envelope
    ///   `positive`      — rating > 0
    ///   `negative`      — rating < 0
    ///   `<n>`           — exact rating value (e.g. `1`, `-1`)
    ExportRlhf {
        /// Output JSONL path. Created if missing; overwritten if
        /// present.
        #[arg(long)]
        out: PathBuf,

        /// Rating filter. Default: `all`.
        #[arg(long, default_value = "all")]
        filter: String,
    },
}

#[derive(Subcommand)]
enum MemoryAction {
    /// Run health-check lint over the memory mount.
    Lint {
        /// Output format (text|json).
        #[arg(long)]
        format: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let mut cli = Cli::parse();
    match cli.command.take() {
        Some(Commands::Init) => cmd_init(cli.repo),
        Some(Commands::Agent { prompt }) => dispatch_agent(cli, prompt),
        Some(Commands::Pilot { calls, rate, complete, promise }) => {
            cmd_pilot(promise, cli.repo, cli.provider, cli.model, calls, rate, complete);
        }
        Some(Commands::Context { repo_path, query }) => cmd_context(&repo_path, &query.join(" ")),
        Some(Commands::Impact { repo_path, file }) => cmd_impact(&repo_path, &file),
        Some(Commands::Stats { repo_path }) => cmd_stats(&repo_path),
        Some(Commands::Login { key, server, no_browser }) => {
            std::process::exit(cmd_login(key, server, no_browser));
        }
        Some(Commands::Logout) => std::process::exit(cmd_logout()),
        Some(Commands::Dashboard { port, static_dir }) => {
            cmd_dashboard(cli.repo, port, static_dir);
        }
        Some(Commands::Memory { action }) => dispatch_memory(action),
        Some(Commands::Subagent { action }) => dispatch_subagent(action, cli.repo),
        Some(Commands::Checkpoints { action }) => dispatch_checkpoints(action, cli.repo),
        Some(Commands::Agents { action }) => dispatch_agents(action, cli.repo),
        Some(Commands::Mcp { action }) => dispatch_mcp(action, cli.repo),
        Some(Commands::Skill { action }) => std::process::exit(dispatch_skill(action)),
        Some(Commands::Trajectory { action }) => {
            std::process::exit(dispatch_trajectory(action, &cli.repo));
        }
        None => dispatch_default(cli),
    }
}

fn dispatch_agent(cli: Cli, prompt: Vec<String>) {
    if cli.headless {
        cmd_headless(
            prompt,
            cli.repo,
            cli.provider,
            cli.model,
            cli.max_iter,
            cli.mode,
            cli.temperature,
            cli.seed,
        );
        return;
    }
    // Build injections from CLI flags so subagent integrations are
    // active even on the explicit `theo agent` subcommand.
    let features = runtime_features::RuntimeFeatures::from_flags(
        cli.watch_agents,
        cli.enable_checkpoints,
        &cli.repo,
    );
    features.print_status();
    let injections = build_injections(&features, &cli.repo);
    let _runtime_features = features;
    cmd_agent(prompt, cli.repo, cli.provider, cli.model, cli.max_iter, injections);
}

fn dispatch_memory(action: MemoryAction) {
    match action {
        MemoryAction::Lint { format } => {
            let fmt = memory_lint::LintFormat::from_str_opt(format.as_deref());
            // Stub inputs — real collection belongs to a follow-up
            // that reads hash manifest, journal timestamps, and
            // retrieval metrics. The subcommand surface lands here
            // so downstream plumbing has a stable entry point.
            let inputs = theo_application::use_cases::memory_lint::LintInputs {
                seconds_since_last_compile: 0,
                lessons: Vec::new(),
                orphan_episode_ids: Vec::new(),
                broken_link_pages: Vec::new(),
                recall_p50_ms: 0.0,
                recall_p95_ms: 0.0,
            };
            let code = memory_lint::run(inputs, fmt);
            std::process::exit(code);
        }
    }
}

fn dispatch_subagent(action: SubagentCmd, project: PathBuf) {
    if let Err(e) = subagent_admin::handle_subagent(action, &project) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn dispatch_checkpoints(action: CheckpointsCmd, workdir: PathBuf) {
    if let Err(e) = subagent_admin::handle_checkpoints(action, &workdir) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn dispatch_agents(action: AgentsCmd, project: PathBuf) {
    if let Err(e) = subagent_admin::handle_agents(action, &project) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn dispatch_mcp(action: McpCmd, project: PathBuf) {
    // Process-local cache: every CLI invocation starts fresh.
    // Operator workflow: run `theo mcp discover` once before the
    // first `theo agent` to warm the cache for that session.
    let cache = std::sync::Arc::new(theo_application::facade::mcp::DiscoveryCache::new());
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: failed to create tokio runtime: {}", e);
            std::process::exit(1);
        }
    };
    if let Err(e) = rt.block_on(mcp_admin::handle_mcp(action, &project, cache)) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn dispatch_skill(action: SkillCmd) -> i32 {
    match action {
        SkillCmd::List { format } => {
            let fmt = cmd_skill::ListFormat::from_str_opt(format.as_deref());
            cmd_skill::handle_list(fmt)
        }
        SkillCmd::View { name } => cmd_skill::handle_view(&name),
        SkillCmd::Delete { name, yes } => cmd_skill::handle_delete(&name, yes),
    }
}

fn dispatch_trajectory(action: TrajectoryCmd, repo: &Path) -> i32 {
    match action {
        TrajectoryCmd::ExportRlhf { out, filter } => {
            cmd_trajectory_export_rlhf(repo, &out, &filter)
        }
    }
}

fn dispatch_default(cli: Cli) {
    // Phase 9 + 13: activate runtime features per CLI flags.
    // Held in scope so watcher / checkpoint live for the session.
    let features = runtime_features::RuntimeFeatures::from_flags(
        cli.watch_agents,
        cli.enable_checkpoints,
        &cli.repo,
    );
    features.print_status();
    // Build SubagentInjections from active features so the runtime
    // (AgentLoop → AgentRunEngine → SubAgentManager) sees them.
    let injections = build_injections(&features, &cli.repo);
    // Keep watcher alive for the session.
    let _runtime_features = features;
    if cli.headless {
        cmd_headless(
            cli.prompt,
            cli.repo,
            cli.provider,
            cli.model,
            cli.max_iter,
            cli.mode,
            cli.temperature,
            cli.seed,
        );
        return;
    }
    // Default: TUI (interactive or one-shot with trailing prompt).
    cmd_agent(cli.prompt, cli.repo, cli.provider, cli.model, cli.max_iter, injections);
}

/// Translate CLI runtime features into `SubagentInjections`. Always
/// includes a builtin registry. Adds checkpoint when --enable-checkpoints.
/// When `--watch-agents` is active, uses the live snapshot from
/// `RuntimeFeatures.reloadable` so reloads from the watcher are picked up
/// each time `delegate_task` resolves a sub-agent name.
fn build_injections(
    features: &runtime_features::RuntimeFeatures,
    project_dir: &Path,
) -> theo_application::use_cases::run_agent_session::SubagentInjections {
    use std::sync::Arc;
    let mut inj = theo_application::use_cases::run_agent_session::SubagentInjections::default();

    // Registry: when --watch-agents is active, expose the LIVE reloadable
    // (so AgentRunEngine reads a fresh snapshot per delegate_task call).
    // Otherwise inject a static registry built once with builtins + project.
    if let Some(rel) = &features.reloadable {
        inj.reloadable = Some(rel.clone());
        // Also seed a static snapshot as fallback.
        inj.registry = Some(Arc::new(rel.snapshot()));
    } else {
        // T3.3 — runtime types via the theo-application::cli_runtime façade.
        let mut reg = theo_application::cli_runtime::SubAgentRegistry::with_builtins();
        let _ = reg.load_all(
            Some(project_dir),
            None,
            theo_application::cli_runtime::ApprovalMode::TrustAll,
        );
        inj.registry = Some(Arc::new(reg));
    }

    // Persistent run store under <project>/.theo/subagent/
    let store = theo_application::cli_runtime::FileSubagentRunStore::new(
        project_dir.join(".theo").join("subagent"),
    );
    inj.run_store = Some(Arc::new(store));

    // Cancellation tree (always — Ctrl+C propagation).
    inj.cancellation = Some(Arc::new(
        theo_application::cli_runtime::CancellationTree::new(),
    ));

    // Phase 9: checkpoint manager (only when --enable-checkpoints).
    if let Some(cp) = &features.checkpoint {
        inj.checkpoint = Some(cp.clone());
    }

    // Phase 17 (sota-gaps): MCP discovery cache. Always-on; the cache stays
    // empty when no MCP registry is configured, so the cost is zero.
    inj.mcp_discovery = Some(Arc::new(
        theo_application::facade::mcp::DiscoveryCache::new(),
    ));

    // Phase 18 + 23 (sota-gaps): handoff guardrail chain with built-in
    // defaults + any declarative entries from .theo/handoff_guardrails.toml.
    inj.handoff_guardrails = Some(Arc::new(
        theo_application::use_cases::guardrail_loader::load_project_guardrails(
            project_dir,
        ),
    ));

    // Phase 27 follow-up (sota-gaps-followup gap #4): wire the
    // AutomaticModelRouter from .theo/config.toml so routing decisions
    // are recorded. Recorder writes to stderr when THEO_DEBUG_ROUTING=1
    // — full MetricsCollector integration is a future improvement.
    let recorder: Option<theo_application::facade::llm::routing::RoutingMetricsRecorder> =
        if std::env::var("THEO_DEBUG_ROUTING").is_ok() {
            Some(Arc::new(|task_type, tier, model_id| {
                eprintln!(
                    "[theo:router] task_type={} tier={} model={}",
                    task_type, tier, model_id
                );
            }))
        } else {
            None
        };
    inj.router = theo_application::use_cases::router_loader::load_router(
        project_dir,
        recorder,
    );

    inj
}


// ---------------------------------------------------------------------------
// Command handlers — extracted to apps/theo-cli/src/cmd.rs
// (T5.3 of docs/plans/god-files-2026-07-23-plan.md, ADR D6).
// ---------------------------------------------------------------------------
