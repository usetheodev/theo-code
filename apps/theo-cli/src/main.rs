mod config;
mod dashboard;
mod init;
mod input;
mod json_output;
mod memory_lint;
mod permission;
mod pilot;
mod render;
mod renderer;
mod status_line;
mod subagent_admin;
mod tui;
mod tty;

use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Parser, Subcommand};
use theo_application::use_cases::pipeline::{Pipeline, PipelineConfig};

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
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Init) => cmd_init(cli.repo),
        Some(Commands::Agent { prompt }) => {
            if cli.headless {
                cmd_headless(prompt, cli.repo, cli.provider, cli.model, cli.max_iter, cli.mode, cli.temperature, cli.seed);
                return;
            }
            cmd_agent(prompt, cli.repo, cli.provider, cli.model, cli.max_iter);
        }
        Some(Commands::Pilot {
            calls,
            rate,
            complete,
            promise,
        }) => {
            cmd_pilot(
                promise,
                cli.repo,
                cli.provider,
                cli.model,
                calls,
                rate,
                complete,
            );
        }
        Some(Commands::Context { repo_path, query }) => {
            cmd_context(&repo_path, &query.join(" "));
        }
        Some(Commands::Impact { repo_path, file }) => {
            cmd_impact(&repo_path, &file);
        }
        Some(Commands::Stats { repo_path }) => {
            cmd_stats(&repo_path);
        }
        Some(Commands::Login { key, server, no_browser }) => {
            std::process::exit(cmd_login(key, server, no_browser));
        }
        Some(Commands::Logout) => {
            std::process::exit(cmd_logout());
        }
        Some(Commands::Dashboard { port, static_dir }) => {
            cmd_dashboard(cli.repo, port, static_dir);
        }
        Some(Commands::Memory { action }) => match action {
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
        },
        Some(Commands::Subagent { action }) => {
            let project = cli.repo.clone();
            if let Err(e) = subagent_admin::handle_subagent(action, &project) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Checkpoints { action }) => {
            let workdir = cli.repo.clone();
            if let Err(e) = subagent_admin::handle_checkpoints(action, &workdir) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Agents { action }) => {
            let project = cli.repo.clone();
            if let Err(e) = subagent_admin::handle_agents(action, &project) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        None => {
            if cli.headless {
                cmd_headless(cli.prompt, cli.repo, cli.provider, cli.model, cli.max_iter, cli.mode, cli.temperature, cli.seed);
                return;
            }
            // Default: TUI (interactive or one-shot with trailing prompt).
            cmd_agent(cli.prompt, cli.repo, cli.provider, cli.model, cli.max_iter);
        }
    }
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

/// `theo login` — OAuth device flow, API-key paste, or custom server URL.
/// Returns the desired process exit code (0 = success, 1 = failure).
fn cmd_login(key: Option<String>, server: Option<String>, no_browser: bool) -> i32 {
    use theo_application::use_cases::auth;

    // Path 1: API key direct persistence.
    if let Some(raw) = key {
        let store = theo_application::facade::auth::AuthStore::open();
        match auth::save_api_key(&store, &raw) {
            Ok(_) => {
                eprintln!("✓ Saved API key: {}", auth::mask_key(raw.trim()));
                0
            }
            Err(e) => {
                eprintln!("✗ save failed: {e}");
                1
            }
        }
    } else if let Some(url) = server {
        eprintln!("✗ `--server {url}` is not yet wired in the headless CLI.");
        eprintln!("  Use the TUI `/login {url}` (Ctrl+C then run `theo`) for the generic RFC 8628 flow.");
        1
    } else {
        // Path 2: OpenAI OAuth device flow.
        let rt = match tokio::runtime::Runtime::new() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("✗ failed to create tokio runtime: {e}");
                return 1;
            }
        };
        rt.block_on(async { run_oauth_device_flow(no_browser).await })
    }
}

/// Run the OpenAI device-flow end-to-end, printing UX prompts to stderr.
async fn run_oauth_device_flow(no_browser: bool) -> i32 {
    let auth_client = theo_application::facade::auth::OpenAIAuth::with_default_store();
    eprintln!("Contacting OpenAI authorization server...");
    let code = match auth_client.start_device_flow().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("✗ device flow failed: {e}");
            return 1;
        }
    };
    eprintln!();
    eprintln!("─────────────────────────────────────");
    eprintln!("1. Open:  {}", code.verification_uri);
    eprintln!("2. Enter code:  {}", code.user_code);
    eprintln!("3. Authorize the Theo application.");
    eprintln!("─────────────────────────────────────");
    eprintln!();
    if !no_browser {
        let _ = open_browser(&code.verification_uri);
    }
    eprintln!("Waiting for authorization…");
    match auth_client.poll_device_flow(&code).await {
        Ok(_) => {
            eprintln!("✓ Authenticated with OpenAI. Tokens saved.");
            0
        }
        Err(e) => {
            eprintln!("✗ authorization failed: {e}");
            1
        }
    }
}

/// Best-effort browser opener for the device-flow URL. Linux uses
/// `xdg-open`, macOS uses `open`. Failures are silent.
fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(not(target_os = "linux"), not(target_os = "macos")))]
    let program = "true"; // noop on unsupported platforms
    std::process::Command::new(program)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

/// `theo logout` — clear saved OpenAI credentials.
fn cmd_logout() -> i32 {
    use theo_application::use_cases::auth;
    let store = theo_application::facade::auth::AuthStore::open();
    match auth::logout(&store) {
        Ok(true) => {
            eprintln!("✓ Logged out. Saved credentials cleared.");
            0
        }
        Ok(false) => {
            eprintln!("Nothing to log out of — no OpenAI credentials were saved.");
            0
        }
        Err(e) => {
            eprintln!("✗ logout failed: {e}");
            1
        }
    }
}

fn cmd_dashboard(repo: PathBuf, port: u16, static_dir: Option<PathBuf>) {
    let project_dir = resolve_dir(repo);
    let static_dir = static_dir.or_else(dashboard::find_default_static_dir);
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    if let Err(e) = rt.block_on(dashboard::serve(project_dir, port, static_dir)) {
        eprintln!("✗ dashboard failed: {e}");
        std::process::exit(1);
    }
}

fn cmd_init(repo: PathBuf) {
    let project_dir = resolve_dir(repo);
    let caps = tty::TtyCaps::detect().style_caps();
    use render::style::{bold, error, success, warn};

    eprintln!("{} — initializing project", bold("theo init", caps));

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let (config, _provider) = resolve_agent_config(None, None, None).await;
        match init::run_init_with_agent(&project_dir, config).await {
            Ok(true) => eprintln!(
                "\n{} Review .theo/theo.md and edit if needed.",
                success("✓ Project initialized.", caps)
            ),
            Ok(false) => eprintln!("\n{}", warn("⚠ Already initialized.", caps)),
            Err(e) => {
                eprintln!("\n{} {e}", error("✗ Error:", caps));
                std::process::exit(1);
            }
        }
    });
}

fn cmd_agent(
    prompt: Vec<String>,
    repo: PathBuf,
    provider_id: Option<String>,
    model: Option<String>,
    max_iter: Option<usize>,
) {
    let project_dir = resolve_dir(repo);

    let inline_prompt = if !prompt.is_empty() {
        Some(prompt.join(" "))
    } else {
        None
    };

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let (config, provider_name) =
            resolve_agent_config(provider_id.as_deref(), model.as_deref(), max_iter).await;

        if let Err(e) = tui::run(config, project_dir, provider_name, inline_prompt).await {
            eprintln!("TUI error: {e}");
            std::process::exit(1);
        }
    });
}

/// Headless mode for benchmarks/CI.
///
/// Reads prompt from CLI args (joined) or, if empty, from stdin.
/// Emits exactly one line of JSON on stdout with run metrics and exit code
/// 0 (success) or 1 (failure). All chrome (banners, streaming, REPL) is
/// suppressed; non-result diagnostics go to stderr.
///
/// Schema (single line, application/json):
/// ```json
/// {
///   "schema": "theo.headless.v1",
///   "success": bool,
///   "summary": str,
///   "iterations": u64,
///   "duration_ms": u64,
///   "tokens": {"input": u64, "output": u64, "total": u64},
///   "tools": {"total": u64, "success": u64},
///   "llm": {"calls": u64, "retries": u64},
///   "files_edited": [str],
///   "model": str,
///   "mode": str,
///   "provider": str
/// }
/// ```
fn cmd_headless(
    prompt: Vec<String>,
    repo: PathBuf,
    provider_id: Option<String>,
    model: Option<String>,
    max_iter: Option<usize>,
    mode: Option<String>,
    temperature: Option<f32>,
    _seed: Option<u64>,
) {
    use std::io::Read;
    use std::time::Instant;

    let project_dir = resolve_dir(repo);

    let task = if !prompt.is_empty() {
        prompt.join(" ")
    } else {
        let mut buf = String::new();
        if std::io::stdin().read_to_string(&mut buf).is_err() {
            emit_headless_error("failed to read stdin");
            std::process::exit(1);
        }
        buf.trim().to_string()
    };

    if task.is_empty() {
        emit_headless_error("empty prompt: pass via args or stdin");
        std::process::exit(1);
    }

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let (mut config, provider_name) =
            resolve_agent_config(provider_id.as_deref(), model.as_deref(), max_iter).await;

        // Apply project config + env var overrides (THEO_TEMPERATURE, THEO_MODEL, etc.)
        // Precedence: CLI flag > env var > .theo/config.toml > default
        let project_config = theo_application::facade::agent::project_config::ProjectConfig::load(&project_dir)
            .with_env_overrides();
        project_config.apply_to(&mut config);

        // CLI flags override everything (highest precedence)
        if let Some(t) = temperature {
            config.temperature = t;
        }

        let mode_str = mode.as_deref().unwrap_or("agent");
        let agent_mode = theo_application::facade::agent::AgentMode::from_str(mode_str)
            .unwrap_or(theo_application::facade::agent::AgentMode::Agent);
        config.mode = agent_mode;
        config.system_prompt = theo_application::facade::agent::system_prompt_for_mode(agent_mode);

        let model_name = config.model.clone();
        let temperature_actual = config.temperature;

        // In headless mode, trim the system prompt to reduce per-call token overhead.
        // Remove verbose sections that don't help a single-shot benchmark task.
        if config.system_prompt.contains("## Task Management") {
            let lean = config.system_prompt
                .lines()
                .filter(|l| {
                    // Remove verbose sections that waste tokens in benchmark mode
                    !l.contains("task_create") && !l.contains("task_update")
                        && !l.contains("subagent") && !l.contains("subagent_parallel")
                        && !l.contains("## Skills") && !l.contains("skill")
                        && !l.contains("## Self-Reflection") && !l.contains("`reflect`")
                        && !l.contains("## Memory") && !l.contains("`memory`")
                })
                .collect::<Vec<_>>()
                .join("\n");
            config.system_prompt = lean;
        }

        // Headless mode: use aggressive retry to survive rate limits
        config.aggressive_retry = true;

        // Phase 0 T0.2: attach the MemoryEngine if memory_enabled=true.
        // run_agent_session does this for the interactive path; headless
        // bypasses that wrapper, so we must attach here or every memory
        // hook stays at no-op despite THEO_MEMORY=1.
        theo_application::use_cases::memory_factory::attach_memory_to_config(
            &mut config,
            &project_dir,
        );

        let registry = theo_application::facade::tooling::create_default_registry();
        let agent = theo_application::facade::agent::AgentLoop::new(config, registry);

        let started = Instant::now();
        let mut result = agent
            .run_with_history(&task, &project_dir, vec![], None)
            .await;
        result.duration_ms = started.elapsed().as_millis() as u64;

        let json = serde_json::json!({
            "schema": "theo.headless.v2",
            "success": result.success,
            "summary": result.summary,
            "iterations": result.iterations_used,
            "duration_ms": result.duration_ms,
            "tokens": {
                "input": result.input_tokens,
                "output": result.output_tokens,
                "total": result.tokens_used,
            },
            "tools": {
                "total": result.tool_calls_total,
                "success": result.tool_calls_success,
            },
            "llm": {
                "calls": result.llm_calls,
                "retries": result.retries,
            },
            "files_edited": result.files_edited,
            "model": model_name,
            "mode": mode_str,
            "provider": provider_name,
            "environment": {
                "temperature_actual": temperature_actual,
                "theo_version": env!("CARGO_PKG_VERSION"),
            },
        });
        println!("{}", serde_json::to_string(&json).unwrap_or_default());

        std::process::exit(if result.success { 0 } else { 1 });
    });
}

fn emit_headless_error(msg: &str) {
    let json = serde_json::json!({
        "schema": "theo.headless.v1",
        "success": false,
        "error": msg,
    });
    println!("{}", serde_json::to_string(&json).unwrap_or_default());
}

fn cmd_pilot(
    promise_args: Vec<String>,
    repo: PathBuf,
    provider_id: Option<String>,
    model: Option<String>,
    max_calls: Option<usize>,
    rate: Option<usize>,
    complete: Option<String>,
) {
    let project_dir = resolve_dir(repo);

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let (config, _provider_name) =
            resolve_agent_config(provider_id.as_deref(), model.as_deref(), None).await;

        let mut pilot_config = theo_application::facade::agent::PilotConfig::load(&project_dir);
        if let Some(calls) = max_calls {
            pilot_config.max_total_calls = calls;
        }
        if let Some(r) = rate {
            pilot_config.max_loops_per_hour = r;
        }

        let promise = pilot::resolve_promise(&promise_args, &project_dir);
        let Some(promise) = promise else {
            eprintln!("Error: No promise provided.");
            eprintln!("Usage: theo-code pilot \"your promise here\"");
            eprintln!("   or: create .theo/PROMPT.md with the promise");
            std::process::exit(1);
        };

        let result = pilot::run_pilot(config, pilot_config, project_dir, promise, complete).await;
        std::process::exit(if result.success { 0 } else { 1 });
    });
}

fn cmd_context(repo_path: &Path, query: &str) {
    if query.is_empty() {
        eprintln!("Usage: theo-code context <repo-path> <query>");
        std::process::exit(1);
    }

    if !repo_path.is_dir() {
        eprintln!("Error: '{}' is not a directory", repo_path.display());
        std::process::exit(1);
    }

    let total_start = Instant::now();
    let cache_dir = repo_path.join(".theo-cache");
    let cache_path = cache_dir.join("graph.bin");
    let cache_exists = cache_path.exists();

    let mut pipeline = Pipeline::new(PipelineConfig {
        token_budget: 16_384,
        graph_cache_path: Some(cache_path.to_string_lossy().to_string()),
        ..PipelineConfig::default()
    });

    let (graph_ms, git_ms, cluster_ms, communities);

    let cluster_cache = cache_dir.join("clusters.bin");
    let clusters_exist = cluster_cache.exists();

    if cache_exists && clusters_exist {
        let t = Instant::now();
        match pipeline.load_graph(&cache_path.to_string_lossy()) {
            Ok(()) => {
                graph_ms = t.elapsed().as_millis();
                git_ms = 0u128;
                let t2 = Instant::now();
                match pipeline.load_clusters(&cluster_cache.to_string_lossy()) {
                    Ok(()) => {
                        communities = pipeline.communities().to_vec();
                        cluster_ms = t2.elapsed().as_millis();
                        eprintln!(
                            "[cache] Loaded graph + clusters from {}",
                            cache_dir.display()
                        );
                    }
                    Err(e) => {
                        eprintln!("[cache] Cluster load failed ({}), re-clustering...", e);
                        pipeline.cluster();
                        communities = pipeline.communities().to_vec();
                        cluster_ms = t2.elapsed().as_millis();
                    }
                }
            }
            Err(e) => {
                eprintln!("[cache] Failed to load ({}), rebuilding...", e);
                let (g, gi, cl, co) =
                    build_fresh(&mut pipeline, repo_path, &cache_dir, &cache_path);
                graph_ms = g;
                git_ms = gi;
                cluster_ms = cl;
                communities = co;
            }
        }
    } else {
        let (g, gi, cl, co) = build_fresh(&mut pipeline, repo_path, &cache_dir, &cache_path);
        graph_ms = g;
        git_ms = gi;
        cluster_ms = cl;
        communities = co;
    };

    let t = Instant::now();
    let context = pipeline.assemble_context_with_code(query, repo_path);
    let search_ms = t.elapsed().as_millis();
    let total_ms = total_start.elapsed().as_millis();

    println!("=== GRAPHCTX Context Assembly ===");
    println!();
    println!("Query: {}", query);
    println!("Repo:  {}", repo_path.display());
    println!(
        "Cache: {}",
        if cache_exists {
            "HIT"
        } else {
            "MISS (built fresh)"
        }
    );
    println!();
    println!("--- Graph ---");
    let graph = pipeline.graph();
    println!("  Nodes:      {}", graph.node_count());
    println!("  Edges:      {}", graph.edge_count());
    println!();
    println!("--- Communities ---");
    println!("  Detected:   {}", communities.len());
    for (i, c) in communities.iter().enumerate() {
        println!("  [{:2}] {} ({} members)", i, c.name, c.node_ids.len());
    }
    println!();
    println!(
        "--- Context ({}/{} tokens, {} items) ---",
        context.total_tokens,
        context.budget_tokens,
        context.items.len()
    );
    println!();
    for (i, item) in context.items.iter().take(5).enumerate() {
        println!(
            "--- Item {} [{} tok, score={:.3}] ---",
            i + 1,
            item.token_count,
            item.score
        );
        println!("{}", item.content);
        println!();
    }
    if context.items.len() > 5 {
        println!("  ... +{} more items", context.items.len() - 5);
    }
    println!();
    println!("--- Timing ---");
    println!("  Graph:    {}ms", graph_ms);
    println!("  Git:      {}ms", git_ms);
    println!("  Cluster:  {}ms", cluster_ms);
    println!("  Search:   {}ms", search_ms);
    println!("  Total:    {}ms", total_ms);
}

fn cmd_impact(repo_path: &Path, file_path: &str) {
    let mut pipeline = Pipeline::with_defaults();
    let (files, _) = theo_application::use_cases::extraction::extract_repo(repo_path);
    pipeline.build_graph(&files);
    let _ = pipeline.add_git_cochanges(repo_path);
    pipeline.cluster();

    let report = pipeline.impact_analysis(file_path);

    println!("=== Impact Analysis ===");
    println!();
    println!("File: {}", report.edited_file);
    println!("BFS depth: {}", report.bfs_depth);
    println!();
    println!(
        "Affected communities ({}):",
        report.affected_communities.len()
    );
    for c in &report.affected_communities {
        println!("  - {}", c);
    }
    println!();
    println!(
        "Tests covering edit ({}):",
        report.tests_covering_edit.len()
    );
    for t in &report.tests_covering_edit {
        println!("  - {}", t);
    }
    println!();
    println!(
        "Co-change candidates ({}):",
        report.co_change_candidates.len()
    );
    for c in &report.co_change_candidates {
        println!("  - {}", c);
    }
    println!();
    println!("Risk alerts ({}):", report.risk_alerts.len());
    for a in &report.risk_alerts {
        println!("  ⚠ {}", a);
    }
}

fn cmd_stats(repo_path: &Path) {
    let t = Instant::now();

    let cache_dir = repo_path.join(".theo-cache");
    let cache_path = cache_dir.join("graph.bin");
    let cluster_cache = cache_dir.join("clusters.bin");

    let mut pipeline = Pipeline::new(PipelineConfig {
        graph_cache_path: Some(cache_path.to_string_lossy().to_string()),
        ..PipelineConfig::default()
    });

    // Try loading from cache first
    if cache_path.exists() && cluster_cache.exists()
        && pipeline.load_graph(&cache_path.to_string_lossy()).is_ok()
            && pipeline
                .load_clusters(&cluster_cache.to_string_lossy())
                .is_ok()
            {
                // Cache hit — stats from cached graph
                let graph = pipeline.graph();
                let ms = t.elapsed().as_millis();
                println!("=== GRAPHCTX Stats ===");
                println!();
                println!("Repo:        {}", repo_path.display());
                println!("Graph nodes: {}", graph.node_count());
                println!("Graph edges: {}", graph.edge_count());
                println!("Communities: {}", pipeline.communities().len());
                println!("Time:        {}ms (cached)", ms);
                return;
            }

    // Cache miss — full build
    let (files, ext_stats) = theo_application::use_cases::extraction::extract_repo(repo_path);
    let stats = pipeline.build_graph(&files);
    let _ = pipeline.add_git_cochanges(repo_path);
    pipeline.cluster();

    // Save cache for next time
    let _ = std::fs::create_dir_all(&cache_dir);
    let _ = pipeline.save_graph(&cache_path.to_string_lossy());
    let _ = pipeline.save_clusters(&cluster_cache.to_string_lossy());

    let ms = t.elapsed().as_millis();

    println!("=== GRAPHCTX Stats ===");
    println!();
    println!("Repo:        {}", repo_path.display());
    println!(
        "Files parsed: {}/{}",
        ext_stats.files_parsed, ext_stats.files_found
    );
    println!("Symbols:     {}", ext_stats.symbols_extracted);
    println!("References:  {}", ext_stats.references_extracted);
    println!("Graph nodes: {}", stats.total_nodes());
    println!("Graph edges: {}", stats.total_edges());
    println!("Communities: {}", pipeline.communities().len());
    println!("Time:        {}ms", ms);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve project directory — "." means current dir.
fn resolve_dir(path: PathBuf) -> PathBuf {
    if path == Path::new(".") {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        path
    }
}

fn build_fresh(
    pipeline: &mut Pipeline,
    repo_path: &Path,
    cache_dir: &Path,
    cache_path: &Path,
) -> (
    u128,
    u128,
    u128,
    Vec<theo_application::use_cases::pipeline::Community>,
) {
    let t = Instant::now();
    let (files, _) = theo_application::use_cases::extraction::extract_repo(repo_path);
    pipeline.build_graph(&files);
    let graph_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let _ = pipeline.add_git_cochanges(repo_path);
    let git_ms = t.elapsed().as_millis();

    let t = Instant::now();
    pipeline.cluster();
    let communities = pipeline.communities().to_vec();
    let cluster_ms = t.elapsed().as_millis();

    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        eprintln!("[cache] Cannot create dir: {}", e);
    } else {
        if let Err(e) = pipeline.save_graph(&cache_path.to_string_lossy()) {
            eprintln!("[cache] Cannot save graph: {}", e);
        }
        let cluster_cache = cache_dir.join("clusters.bin");
        if let Err(e) = pipeline.save_clusters(&cluster_cache.to_string_lossy()) {
            eprintln!("[cache] Cannot save clusters: {}", e);
        }
        let summaries_cache = cache_dir.join("summaries.bin");
        if let Err(e) = pipeline.save_summaries(&summaries_cache.to_string_lossy()) {
            eprintln!("[cache] Cannot save summaries: {}", e);
        } else {
            eprintln!(
                "[cache] Saved graph + clusters + summaries to {}",
                cache_dir.display()
            );
        }
    }

    (graph_ms, git_ms, cluster_ms, communities)
}

async fn resolve_agent_config(
    provider_id: Option<&str>,
    model: Option<&str>,
    max_iter: Option<usize>,
) -> (theo_application::facade::agent::AgentConfig, String) {
    use theo_application::facade::llm::provider_registry::create_default_registry
        as create_provider_registry;

    let mut config = theo_application::facade::agent::AgentConfig::default();
    // Opt-in flag for the memory subsystem (G1–G10). Default stays `false`
    // for backward-compat (test_pre5_ac_1_memory_enabled_default_false).
    // Set `THEO_MEMORY=1` (or any non-empty value) to activate every hook.
    if std::env::var("THEO_MEMORY").map(|v| !v.is_empty()).unwrap_or(false) {
        config.memory_enabled = true;
        eprintln!("[theo] THEO_MEMORY=1 detected — memory subsystem active");
    }
    let mut provider_name = "default".to_string();

    let mut api_key: Option<String> = None;
    let mut oauth_applied = false;

    if provider_id.is_none() {
        let auth = theo_application::facade::auth::OpenAIAuth::with_default_store();
        if let Ok(Some(tokens)) = auth.get_tokens()
            && !tokens.is_expired() {
                api_key = Some(tokens.access_token.clone());
                oauth_applied = true;
            }
    }

    let registry = create_provider_registry();

    if let Some(pid) = provider_id {
        if let Some(spec) = registry.get(pid) {
            config.base_url = spec.base_url.to_string();
            config.endpoint_override = Some(spec.endpoint_url());
            config.api_key = api_key.or_else(|| {
                spec.api_key_env_var()
                    .and_then(|var| std::env::var(var).ok())
            });
            provider_name = spec.display_name.to_string();
        } else {
            eprintln!("Unknown provider: {pid}");
            std::process::exit(1);
        }
    } else if oauth_applied {
        if let Some(spec) = registry.get("chatgpt-codex") {
            config.base_url = spec.base_url.to_string();
            config.endpoint_override = Some(spec.endpoint_url());
            config.api_key = api_key;
            provider_name = spec.display_name.to_string();

            let auth = theo_application::facade::auth::OpenAIAuth::with_default_store();
            if let Ok(Some(tokens)) = auth.get_tokens()
                && let Some(ref account_id) = tokens.account_id {
                    config
                        .extra_headers
                        .insert("ChatGPT-Account-Id".to_string(), account_id.clone());
                }
        }
    } else if let Ok(key) = std::env::var("OPENAI_API_KEY")
        && let Some(spec) = registry.get("openai") {
            config.base_url = spec.base_url.to_string();
            config.endpoint_override = Some(spec.endpoint_url());
            config.api_key = Some(key);
            provider_name = "OpenAI".to_string();
        }

    if let Some(m) = model {
        config.model = m.to_string();
    } else if oauth_applied && config.model == "default" {
        config.model = "gpt-5.3-codex".to_string();
    }

    if let Some(n) = max_iter {
        config.max_iterations = n;
    }

    if config.reasoning_effort.is_none() {
        config.reasoning_effort = Some("medium".to_string());
    }

    (config, provider_name)
}
