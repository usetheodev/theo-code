#[allow(dead_code)]
mod extract;
mod init;
#[allow(dead_code)]
mod pipeline;
mod commands;
mod pilot;
mod renderer;
mod repl;

use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Parser, Subcommand};
use pipeline::{Pipeline, PipelineConfig};

// ---------------------------------------------------------------------------
// CLI definition (Clap derive)
// ---------------------------------------------------------------------------

/// Theo — autonomous coding agent
///
/// Run without arguments to start the interactive REPL.
/// Run with a prompt to execute a single task and exit.
///
/// Examples:
///   theo                          Start interactive REPL
///   theo "fix the bug in auth"    Execute task and exit
///   theo --mode plan "design X"   Plan mode single-shot
///   theo init                     Initialize project
///   theo pilot "implement X"      Autonomous loop
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

    /// Agent mode
    #[arg(long, global = true, value_parser = ["agent", "plan", "ask"])]
    mode: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Task to execute (opens REPL if omitted, ignored when using subcommands)
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
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Init) => cmd_init(cli.repo),
        Some(Commands::Agent { prompt }) => {
            cmd_agent(prompt, cli.repo, cli.provider, cli.model, cli.max_iter, cli.mode);
        }
        Some(Commands::Pilot { calls, rate, complete, promise }) => {
            cmd_pilot(promise, cli.repo, cli.provider, cli.model, calls, rate, complete);
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
        None => {
            // Default: agent mode. REPL if no prompt, single-shot if prompt given.
            cmd_agent(cli.prompt, cli.repo, cli.provider, cli.model, cli.max_iter, cli.mode);
        }
    }
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

fn cmd_init(repo: PathBuf) {
    let project_dir = resolve_dir(repo);

    eprintln!("\x1b[1mtheo init\x1b[0m — initializing project");

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let (config, _provider) = resolve_agent_config(None, None, None).await;
        match init::run_init_with_agent(&project_dir, config).await {
            Ok(true) => eprintln!("\n\x1b[32m✓ Project initialized.\x1b[0m Review .theo/theo.md and edit if needed."),
            Ok(false) => eprintln!("\n\x1b[33m⚠ Already initialized.\x1b[0m"),
            Err(e) => {
                eprintln!("\n\x1b[31m✗ Error:\x1b[0m {e}");
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
    mode: Option<String>,
) {
    let project_dir = resolve_dir(repo);

    let inline_prompt = if !prompt.is_empty() {
        Some(prompt.join(" "))
    } else {
        None
    };

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let (config, provider_name) = resolve_agent_config(
            provider_id.as_deref(),
            model.as_deref(),
            max_iter,
        ).await;

        let mut repl = repl::Repl::new(config, project_dir, provider_name);
        if let Some(ref mode_str) = mode {
            if let Some(m) = theo_agent_runtime::config::AgentMode::from_str(mode_str) {
                repl = repl.with_mode(m);
            } else {
                eprintln!("Unknown mode: {}. Use: agent, plan, ask", mode_str);
                std::process::exit(1);
            }
        }

        if let Some(prompt) = inline_prompt {
            repl.execute_single(&prompt).await;
        } else {
            repl.run().await;
        }
    });
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
        let (config, _provider_name) = resolve_agent_config(
            provider_id.as_deref(),
            model.as_deref(),
            None,
        ).await;

        let mut pilot_config = theo_agent_runtime::pilot::PilotConfig::load(&project_dir);
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
                        eprintln!("[cache] Loaded graph + clusters from {}", cache_dir.display());
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
                let (g, gi, cl, co) = build_fresh(&mut pipeline, repo_path, &cache_dir, &cache_path);
                graph_ms = g; git_ms = gi; cluster_ms = cl; communities = co;
            }
        }
    } else {
        let (g, gi, cl, co) = build_fresh(&mut pipeline, repo_path, &cache_dir, &cache_path);
        graph_ms = g; git_ms = gi; cluster_ms = cl; communities = co;
    };

    let t = Instant::now();
    let context = pipeline.assemble_context_with_code(query, repo_path);
    let search_ms = t.elapsed().as_millis();
    let total_ms = total_start.elapsed().as_millis();

    println!("=== GRAPHCTX Context Assembly ===");
    println!();
    println!("Query: {}", query);
    println!("Repo:  {}", repo_path.display());
    println!("Cache: {}", if cache_exists { "HIT" } else { "MISS (built fresh)" });
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
    println!("--- Context ({}/{} tokens, {} items) ---", context.total_tokens, context.budget_tokens, context.items.len());
    println!();
    for (i, item) in context.items.iter().take(5).enumerate() {
        println!("--- Item {} [{} tok, score={:.3}] ---", i + 1, item.token_count, item.score);
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
    let (files, _) = extract::extract_repo(repo_path);
    pipeline.build_graph(&files);
    let _ = pipeline.add_git_cochanges(repo_path);
    pipeline.cluster();

    let report = pipeline.impact_analysis(file_path);

    println!("=== Impact Analysis ===");
    println!();
    println!("File: {}", report.edited_file);
    println!("BFS depth: {}", report.bfs_depth);
    println!();
    println!("Affected communities ({}):", report.affected_communities.len());
    for c in &report.affected_communities { println!("  - {}", c); }
    println!();
    println!("Tests covering edit ({}):", report.tests_covering_edit.len());
    for t in &report.tests_covering_edit { println!("  - {}", t); }
    println!();
    println!("Co-change candidates ({}):", report.co_change_candidates.len());
    for c in &report.co_change_candidates { println!("  - {}", c); }
    println!();
    println!("Risk alerts ({}):", report.risk_alerts.len());
    for a in &report.risk_alerts { println!("  ⚠ {}", a); }
}

fn cmd_stats(repo_path: &Path) {
    let t = Instant::now();
    let mut pipeline = Pipeline::with_defaults();
    let (files, ext_stats) = extract::extract_repo(repo_path);
    let stats = pipeline.build_graph(&files);
    let _ = pipeline.add_git_cochanges(repo_path);
    pipeline.cluster();
    let ms = t.elapsed().as_millis();

    println!("=== GRAPHCTX Stats ===");
    println!();
    println!("Repo:        {}", repo_path.display());
    println!("Files parsed: {}/{}", ext_stats.files_parsed, ext_stats.files_found);
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
) -> (u128, u128, u128, Vec<theo_engine_graph::cluster::Community>) {
    let t = Instant::now();
    let (files, _) = extract::extract_repo(repo_path);
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
            eprintln!("[cache] Saved graph + clusters + summaries to {}", cache_dir.display());
        }
    }

    (graph_ms, git_ms, cluster_ms, communities)
}

async fn resolve_agent_config(
    provider_id: Option<&str>,
    model: Option<&str>,
    max_iter: Option<usize>,
) -> (theo_agent_runtime::AgentConfig, String) {
    use theo_infra_llm::provider::registry::create_default_registry as create_provider_registry;

    let mut config = theo_agent_runtime::AgentConfig::default();
    let mut provider_name = "default".to_string();

    let mut api_key: Option<String> = None;
    let mut oauth_applied = false;

    if provider_id.is_none() {
        let auth = theo_infra_auth::OpenAIAuth::with_default_store();
        if let Ok(Some(tokens)) = auth.get_tokens() {
            if !tokens.is_expired() {
                api_key = Some(tokens.access_token.clone());
                oauth_applied = true;
            }
        }
    }

    let registry = create_provider_registry();

    if let Some(pid) = provider_id {
        if let Some(spec) = registry.get(pid) {
            config.base_url = spec.base_url.to_string();
            config.endpoint_override = Some(spec.endpoint_url());
            config.api_key = api_key.or_else(|| {
                spec.api_key_env_var().and_then(|var| std::env::var(var).ok())
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

            let auth = theo_infra_auth::OpenAIAuth::with_default_store();
            if let Ok(Some(tokens)) = auth.get_tokens() {
                if let Some(ref account_id) = tokens.account_id {
                    config.extra_headers.insert(
                        "ChatGPT-Account-Id".to_string(),
                        account_id.clone(),
                    );
                }
            }
        }
    } else if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        if let Some(spec) = registry.get("openai") {
            config.base_url = spec.base_url.to_string();
            config.endpoint_override = Some(spec.endpoint_url());
            config.api_key = Some(key);
            provider_name = "OpenAI".to_string();
        }
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
