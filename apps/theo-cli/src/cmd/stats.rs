//! Single-cmd slice extracted from `cmd.rs` (T5.3.b of god-files-2026-07-23-plan.md, ADR D6).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use theo_application::use_cases::pipeline::{Pipeline, PipelineConfig};

use crate::*;
use super::helpers::*;

pub fn cmd_stats(repo_path: &Path) {
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
