#[allow(dead_code)]
mod extract;
#[allow(dead_code)]
mod pipeline;
mod commands;
mod pilot;
mod renderer;
mod repl;

use std::path::Path;
use std::time::Instant;

use pipeline::{Pipeline, PipelineConfig};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("agent") => cmd_agent(&args[2..]),
        Some("pilot") => cmd_pilot(&args[2..]),
        Some("context") => cmd_context(&args[2..]),
        Some("impact") => cmd_impact(&args[2..]),
        Some("stats") => cmd_stats(&args[2..]),
        Some("--version") | Some("-V") => println!("theo-code v0.1.0"),
        _ => print_usage(),
    }
}

fn print_usage() {
    eprintln!("theo-code v0.1.0 — Theo Code Agent");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  theo-code agent [--repo <path>] [--provider <id>] [--model <name>]");
    eprintln!("                                        Interactive agent REPL");
    eprintln!("  theo-code pilot <promise> [--complete <criteria>] [--calls <N>] [--rate <N>]");
    eprintln!("                                        Autonomous loop until promise fulfilled");
    eprintln!("  theo-code context <repo-path> <query>  Assemble context for a task");
    eprintln!("  theo-code impact <repo-path> <file>    Analyze impact of editing a file");
    eprintln!("  theo-code stats <repo-path>            Show graph statistics");
    eprintln!("  theo-code --version                    Show version");
}

fn cmd_agent(args: &[String]) {
    // Parse agent args
    let mut repo: Option<String> = None;
    let mut provider_id: Option<String> = None;
    let mut model: Option<String> = None;
    let mut max_iter: Option<usize> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--repo" => { repo = args.get(i + 1).cloned(); i += 2; }
            "--provider" => { provider_id = args.get(i + 1).cloned(); i += 2; }
            "--model" => { model = args.get(i + 1).cloned(); i += 2; }
            "--max-iter" => { max_iter = args.get(i + 1).and_then(|s| s.parse().ok()); i += 2; }
            _ => { i += 1; }
        }
    }

    // Default to current directory
    let project_dir = repo
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));

    if !project_dir.exists() {
        eprintln!("Error: directory does not exist: {}", project_dir.display());
        std::process::exit(1);
    }

    // Build agent config with provider resolution
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let (config, provider_name) = resolve_agent_config(
            provider_id.as_deref(),
            model.as_deref(),
            max_iter,
        ).await;

        let mut repl = repl::Repl::new(config, project_dir, provider_name);
        repl.run().await;
    });
}

fn cmd_pilot(args: &[String]) {
    let mut repo: Option<String> = None;
    let mut provider_id: Option<String> = None;
    let mut model: Option<String> = None;
    let mut max_calls: Option<usize> = None;
    let mut rate: Option<usize> = None;
    let mut complete: Option<String> = None;

    let mut i = 0;
    let mut positional: Vec<String> = Vec::new();
    while i < args.len() {
        match args[i].as_str() {
            "--repo" => { repo = args.get(i + 1).cloned(); i += 2; }
            "--provider" => { provider_id = args.get(i + 1).cloned(); i += 2; }
            "--model" => { model = args.get(i + 1).cloned(); i += 2; }
            "--calls" => { max_calls = args.get(i + 1).and_then(|s| s.parse().ok()); i += 2; }
            "--rate" => { rate = args.get(i + 1).and_then(|s| s.parse().ok()); i += 2; }
            "--complete" => { complete = args.get(i + 1).cloned(); i += 2; }
            other if !other.starts_with("--") => { positional.push(other.to_string()); i += 1; }
            _ => { i += 1; }
        }
    }

    let project_dir = repo
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));

    if !project_dir.exists() {
        eprintln!("Error: directory does not exist: {}", project_dir.display());
        std::process::exit(1);
    }

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let (config, _provider_name) = resolve_agent_config(
            provider_id.as_deref(),
            model.as_deref(),
            None,
        ).await;

        // Load pilot config from .theo/config.toml
        let mut pilot_config = theo_agent_runtime::pilot::PilotConfig::load(&project_dir);

        // CLI overrides
        if let Some(calls) = max_calls {
            pilot_config.max_total_calls = calls;
        }
        if let Some(r) = rate {
            pilot_config.max_loops_per_hour = r;
        }

        // Resolve promise
        let promise = pilot::resolve_promise(&positional, &project_dir);
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

async fn resolve_agent_config(
    provider_id: Option<&str>,
    model: Option<&str>,
    max_iter: Option<usize>,
) -> (theo_agent_runtime::AgentConfig, String) {
    use theo_infra_llm::provider::registry::create_default_registry as create_provider_registry;

    let mut config = theo_agent_runtime::AgentConfig::default();
    let mut provider_name = "default".to_string();

    // Try OAuth first (if no explicit provider)
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
        // Explicit --provider
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
        // Auto-detect: OAuth → chatgpt-codex
        if let Some(spec) = registry.get("chatgpt-codex") {
            config.base_url = spec.base_url.to_string();
            config.endpoint_override = Some(spec.endpoint_url());
            config.api_key = api_key;
            provider_name = spec.display_name.to_string();

            // Add ChatGPT-Account-Id header
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

    // Apply model override
    if let Some(m) = model {
        config.model = m.to_string();
    } else if oauth_applied && config.model == "default" {
        config.model = "gpt-5.3-codex".to_string();
    }

    if let Some(n) = max_iter {
        config.max_iterations = n;
    }

    // Default reasoning effort for capable models
    if config.reasoning_effort.is_none() {
        config.reasoning_effort = Some("medium".to_string());
    }

    (config, provider_name)
}

fn build_fresh(
    pipeline: &mut Pipeline,
    repo_path: &Path,
    cache_dir: &Path,
    cache_path: &Path,
) -> (u128, u128, u128, Vec<theo_engine_graph::cluster::Community>) {
    // Full extraction
    let t = Instant::now();
    let (files, _) = extract::extract_repo(repo_path);
    pipeline.build_graph(&files);
    let graph_ms = t.elapsed().as_millis();

    // Git co-changes
    let t = Instant::now();
    let _ = pipeline.add_git_cochanges(repo_path);
    let git_ms = t.elapsed().as_millis();

    // Cluster
    let t = Instant::now();
    pipeline.cluster();
    let communities = pipeline.communities().to_vec();
    let cluster_ms = t.elapsed().as_millis();

    // Save cache (graph + clusters)
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

fn cmd_context(args: &[String]) {
    if args.len() < 2 {
        eprintln!("Usage: theo-code context <repo-path> <query>");
        std::process::exit(1);
    }

    let repo_path = Path::new(&args[0]);
    let query = args[1..].join(" ");

    if !repo_path.is_dir() {
        eprintln!("Error: '{}' is not a directory", repo_path.display());
        std::process::exit(1);
    }

    let total_start = Instant::now();

    // Cache path: <repo>/.theo-cache/graph.bin
    let cache_dir = repo_path.join(".theo-cache");
    let cache_path = cache_dir.join("graph.bin");
    let cache_exists = cache_path.exists();

    // Build pipeline
    let mut pipeline = Pipeline::new(PipelineConfig {
        token_budget: 16_384,
        graph_cache_path: Some(cache_path.to_string_lossy().to_string()),
        ..PipelineConfig::default()
    });

    let (graph_ms, git_ms, cluster_ms, communities);

    let cluster_cache = cache_dir.join("clusters.bin");
    let clusters_exist = cluster_cache.exists();

    if cache_exists && clusters_exist {
        // Fast path: load cached graph + clusters
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

    // Assemble context with real source code
    let t = Instant::now();
    let context = pipeline.assemble_context_with_code(&query, repo_path);
    let search_ms = t.elapsed().as_millis();

    let total_ms = total_start.elapsed().as_millis();

    // Output
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
    // Show top 5 context items with full summary content
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

fn cmd_impact(args: &[String]) {
    if args.len() < 2 {
        eprintln!("Usage: theo-code impact <repo-path> <file>");
        std::process::exit(1);
    }

    let repo_path = Path::new(&args[0]);
    let file_path = &args[1];

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
    for c in &report.affected_communities {
        println!("  - {}", c);
    }
    println!();
    println!("Tests covering edit ({}):", report.tests_covering_edit.len());
    for t in &report.tests_covering_edit {
        println!("  - {}", t);
    }
    println!();
    println!("Co-change candidates ({}):", report.co_change_candidates.len());
    for c in &report.co_change_candidates {
        println!("  - {}", c);
    }
    println!();
    println!("Risk alerts ({}):", report.risk_alerts.len());
    for a in &report.risk_alerts {
        println!("  ⚠ {}", a);
    }
}

fn cmd_stats(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: theo-code stats <repo-path>");
        std::process::exit(1);
    }

    let repo_path = Path::new(&args[0]);
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
