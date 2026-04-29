//! Single-cmd slice extracted from `cmd.rs` (T5.3.b of god-files-2026-07-23-plan.md, ADR D6).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use theo_application::use_cases::pipeline::{Pipeline, PipelineConfig};

use crate::*;
use super::helpers::*;

pub fn cmd_context(repo_path: &Path, query: &str) {
    validate_args_or_exit(repo_path, query);
    let total_start = Instant::now();
    let cache_dir = repo_path.join(".theo-cache");
    let cache_path = cache_dir.join("graph.bin");
    let cache_exists = cache_path.exists();
    let mut pipeline = Pipeline::new(PipelineConfig {
        token_budget: 16_384,
        graph_cache_path: Some(cache_path.to_string_lossy().to_string()),
        ..PipelineConfig::default()
    });
    let (graph_ms, git_ms, cluster_ms, communities) =
        load_or_build_graph(&mut pipeline, repo_path, &cache_dir, &cache_path);

    let t = Instant::now();
    let context = pipeline.assemble_context_with_code(query, repo_path);
    let search_ms = t.elapsed().as_millis();
    let total_ms = total_start.elapsed().as_millis();

    print_context_report(
        query,
        repo_path,
        cache_exists,
        &pipeline,
        &communities,
        &context,
        graph_ms,
        git_ms,
        cluster_ms,
        search_ms,
        total_ms,
    );
}

fn validate_args_or_exit(repo_path: &Path, query: &str) {
    if query.is_empty() {
        eprintln!("Usage: theo-code context <repo-path> <query>");
        std::process::exit(1);
    }
    if !repo_path.is_dir() {
        eprintln!("Error: '{}' is not a directory", repo_path.display());
        std::process::exit(1);
    }
}

fn load_or_build_graph(
    pipeline: &mut Pipeline,
    repo_path: &Path,
    cache_dir: &Path,
    cache_path: &Path,
) -> (u128, u128, u128, Vec<theo_application::use_cases::pipeline::Community>) {
    let cluster_cache = cache_dir.join("clusters.bin");
    let cache_exists = cache_path.exists();
    let clusters_exist = cluster_cache.exists();

    if cache_exists && clusters_exist {
        let t = Instant::now();
        match pipeline.load_graph(&cache_path.to_string_lossy()) {
            Ok(()) => {
                let graph_ms = t.elapsed().as_millis();
                let t2 = Instant::now();
                match pipeline.load_clusters(&cluster_cache.to_string_lossy()) {
                    Ok(()) => {
                        let communities = pipeline.communities().to_vec();
                        let cluster_ms = t2.elapsed().as_millis();
                        eprintln!(
                            "[cache] Loaded graph + clusters from {}",
                            cache_dir.display()
                        );
                        (graph_ms, 0u128, cluster_ms, communities)
                    }
                    Err(e) => {
                        eprintln!("[cache] Cluster load failed ({}), re-clustering...", e);
                        pipeline.cluster();
                        let communities = pipeline.communities().to_vec();
                        (graph_ms, 0u128, t2.elapsed().as_millis(), communities)
                    }
                }
            }
            Err(e) => {
                eprintln!("[cache] Failed to load ({}), rebuilding...", e);
                build_fresh(pipeline, repo_path, cache_dir, cache_path)
            }
        }
    } else {
        build_fresh(pipeline, repo_path, cache_dir, cache_path)
    }
}

#[allow(clippy::too_many_arguments)]
fn print_context_report(
    query: &str,
    repo_path: &Path,
    cache_exists: bool,
    pipeline: &Pipeline,
    communities: &[theo_application::use_cases::pipeline::Community],
    context: &theo_application::use_cases::pipeline::ContextPayload,
    graph_ms: u128,
    git_ms: u128,
    cluster_ms: u128,
    search_ms: u128,
    total_ms: u128,
) {
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

