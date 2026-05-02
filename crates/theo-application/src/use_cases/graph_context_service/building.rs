//! Single-purpose slice extracted from `graph_context_service.rs` (T4.5 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use theo_domain::graph_context::{
    ContextBlock, GraphContextError, GraphContextProvider, GraphContextResult,
};

use theo_engine_graph::bridge::{
    self, DataModelData, FileData, ImportData, ReferenceData, SymbolData,
};
use theo_engine_graph::cluster::{ClusterAlgorithm, ClusterResult, Community};
use theo_engine_graph::model::CodeGraph;

use theo_engine_parser::tree_sitter::{self as ts, SupportedLanguage};
use theo_engine_parser::types::FileExtraction;

use theo_engine_retrieval::assembly;
use theo_engine_retrieval::search::FileBm25;
#[cfg(not(feature = "tantivy-backend"))]
use theo_engine_retrieval::search::MultiSignalScorer;

#[cfg(feature = "tantivy-backend")]
use theo_engine_retrieval::tantivy_search::FileTantivyIndex;

#[cfg(feature = "dense-retrieval")]
use theo_engine_retrieval::embedding::cache::EmbeddingCache;
#[cfg(feature = "dense-retrieval")]
use theo_engine_retrieval::embedding::neural::NeuralEmbedder;
#[cfg(feature = "dense-retrieval")]
use theo_engine_retrieval::pipeline::retrieve_with_config;
// T8.1 — `CrossEncoderConfig` + `CrossEncoderReranker` are always
// compiled by the retrieval crate, but we only consume them on the
// dense-retrieval path because that's where the RRF candidate set
// originates.
#[cfg(feature = "dense-retrieval")]
use theo_engine_retrieval::reranker::{CrossEncoderConfig, CrossEncoderReranker};

/// T8.1 part 4 — Read `THEO_RERANKER_PRELOAD` from the environment.
/// Truthy (`1`, `true`, `yes`, `on`, case-insensitive) opts the
/// background graph build into preloading the cross-encoder model
/// — first session pays the ~200 MB download once; subsequent
/// queries get the +15 pt nDCG@10 SOTA gain immediately.
/// Falsy / unset = preload OFF (default; preserves cold-start speed
/// for users who don't query enough to amortize the download).
use super::*;


// ---------------------------------------------------------------------------
// Graph build pipeline (runs in spawn_blocking)
// ---------------------------------------------------------------------------

/// Full pipeline: walk → parse → convert → build_graph → cluster → scorer.
pub fn build_graph_from_project(project_dir: &Path) -> (CodeGraph, Vec<Community>) {
    // Step 1: Walk files and parse with tree-sitter.
    let file_data = parse_project_files(project_dir);

    // Step 2: Build code graph from FileData.
    let (mut graph, _stats) = bridge::build_graph(&file_data);

    // Step 3: Apply git co-change history (best-effort, max 500 commits, 50 files/commit).
    let _ = theo_engine_graph::git::populate_cochanges_from_git(project_dir, &mut graph, 500, 50);

    // Step 4: Cluster only (scorer built conditionally by caller).
    let communities = tokio_safe_cluster(&graph).communities;

    (graph, communities)
}

// EXCLUDED_DIRS imported from theo-domain::graph_context (source of truth).

/// Maximum files to parse. Prevents timeout on huge monorepos.
const MAX_FILES_TO_PARSE: usize = 500;

/// Detect the dominant language of the project from manifest files.
pub fn detect_project_language(project_dir: &Path) -> Option<&'static str> {
    if project_dir.join("Cargo.toml").exists() {
        Some("rs")
    } else if project_dir.join("go.mod").exists() || project_dir.join("go.work").exists() {
        Some("go")
    } else if project_dir.join("pyproject.toml").exists()
        || project_dir.join("requirements.txt").exists()
    {
        Some("py")
    } else if project_dir.join("package.json").exists() {
        Some("ts") // covers TS and JS projects
    } else {
        None
    }
}

/// Walk project, parse each file with tree-sitter, convert to FileData.
///
/// Prioritizes the project's primary language: if a Cargo.toml exists,
/// .rs files are parsed first, then other languages up to MAX_FILES_TO_PARSE.
/// Walk the project, partition primary vs secondary by language, and sample
/// up to `MAX_FILES_TO_PARSE` files (breadth + recency).
fn collect_files_to_parse(
    project_dir: &Path,
    primary_ext: Option<&str>,
) -> Vec<std::path::PathBuf> {
    let walker = build_project_walker(project_dir);
    let (primary, secondary) = partition_by_language(walker, primary_ext);

    let mut all_files = primary;
    all_files.extend(secondary);
    if all_files.len() <= MAX_FILES_TO_PARSE {
        return all_files;
    }
    sample_files_by_breadth_and_recency(all_files, project_dir)
}

fn build_project_walker(project_dir: &Path) -> ignore::Walk {
    let mut wb = ignore::WalkBuilder::new(project_dir);
    wb.hidden(true).git_ignore(true);
    let _ = wb.add_ignore(project_dir.join(".gitignore"));
    wb.add_custom_ignore_filename(".theoignore");
    wb.filter_entry(|entry| {
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            let name = entry.file_name().to_str().unwrap_or("");
            return !theo_domain::graph_context::EXCLUDED_DIRS.contains(&name);
        }
        true
    });
    wb.build()
}

fn partition_by_language(
    walker: ignore::Walk,
    primary_ext: Option<&str>,
) -> (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) {
    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    for entry in walker.flatten() {
        let path = entry.into_path();
        if !path.is_file() || ts::detect_language(&path).is_none() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if primary_ext.is_some_and(|pe| {
            ext == pe || (pe == "ts" && (ext == "tsx" || ext == "js" || ext == "jsx"))
        }) {
            primary.push(path);
        } else {
            secondary.push(path);
        }
    }
    (primary, secondary)
}

fn sample_files_by_breadth_and_recency(
    mut all_files: Vec<std::path::PathBuf>,
    project_dir: &Path,
) -> Vec<std::path::PathBuf> {
    // Step 1: 1 file per top-level dir (most recently modified).
    let mut by_dir: std::collections::HashMap<String, Vec<std::path::PathBuf>> =
        std::collections::HashMap::new();
    for path in &all_files {
        let dir = path
            .strip_prefix(project_dir)
            .unwrap_or(path)
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .unwrap_or_else(|| "root".to_string());
        by_dir.entry(dir).or_default().push(path.clone());
    }
    let mut selected: Vec<std::path::PathBuf> = Vec::with_capacity(MAX_FILES_TO_PARSE);
    let mut selected_set: std::collections::HashSet<std::path::PathBuf> =
        std::collections::HashSet::new();
    for (_dir, mut files) in by_dir {
        files.sort_by_key(|p| std::cmp::Reverse(mtime(p)));
        if let Some(f) = files.first()
            && selected_set.insert(f.clone())
        {
            selected.push(f.clone());
        }
    }
    // Step 2: fill remaining slots by mtime (newest first across all dirs).
    all_files.sort_by_key(|p| std::cmp::Reverse(mtime(p)));
    for f in all_files {
        if selected.len() >= MAX_FILES_TO_PARSE {
            break;
        }
        if selected_set.insert(f.clone()) {
            selected.push(f);
        }
    }
    selected
}

fn mtime(p: &std::path::Path) -> std::time::SystemTime {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
}

pub fn parse_project_files(project_dir: &Path) -> Vec<FileData> {
    let primary_ext = detect_project_language(project_dir);
    let paths = collect_files_to_parse(project_dir, primary_ext);
    let mut file_data_list = Vec::with_capacity(paths.len());

    for path in &paths {
        let Some(language) = ts::detect_language(path) else {
            continue;
        };

        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };

        let Ok(parsed) = ts::parse_source(path, &source, language, None) else {
            continue;
        };

        let extraction =
            theo_engine_parser::extractors::extract(path, &source, &parsed.tree, language);

        let rel_path = path
            .strip_prefix(project_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let last_modified = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        file_data_list.push(convert_extraction(
            extraction,
            &rel_path,
            language,
            last_modified,
        ));
    }

    file_data_list
}

/// Build clustering + scorer from a ready graph.
/// Build clustering index. Scorer only built when no RRF pipeline (saves ~200MB).
#[cfg(not(feature = "tantivy-backend"))]
pub fn build_index(graph: &CodeGraph) -> (Vec<Community>, MultiSignalScorer) {
    let cluster_result = tokio_safe_cluster(graph);
    let scorer = MultiSignalScorer::build(&cluster_result.communities, graph);
    (cluster_result.communities, scorer)
}

/// Build clustering only (Tier 1+: scorer not needed, RRF uses FileBm25 directly).
#[cfg(feature = "tantivy-backend")]
pub fn build_index(graph: &CodeGraph) -> Vec<Community> {
    let cluster_result = tokio_safe_cluster(graph);
    cluster_result.communities
}
