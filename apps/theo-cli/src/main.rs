#[allow(dead_code)]
mod extract;
#[allow(dead_code)]
mod pipeline;

use std::path::Path;
use std::time::Instant;

use pipeline::{Pipeline, PipelineConfig};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("context") => cmd_context(&args[2..]),
        Some("impact") => cmd_impact(&args[2..]),
        Some("stats") => cmd_stats(&args[2..]),
        Some("--version") | Some("-V") => println!("theo-code v0.1.0"),
        _ => print_usage(),
    }
}

fn print_usage() {
    eprintln!("theo-code v0.1.0 — GRAPHCTX context engine");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  theo-code context <repo-path> <query>   Assemble context for a task");
    eprintln!("  theo-code impact <repo-path> <file>     Analyze impact of editing a file");
    eprintln!("  theo-code stats <repo-path>             Show graph statistics");
    eprintln!("  theo-code --version                     Show version");
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
