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

pub fn compute_project_hash(project_dir: &Path) -> String {
    // Load cached hashes (path → (mtime_secs, size_bytes, content_hash)).
    // T2.7: bounded deserialization — a corrupted or oversized cache file
    // falls back to an empty map instead of allocating unbounded memory.
    let cache_path = project_dir.join(".theo").join("hash_cache.json");
    let mut cached: BTreeMap<String, (u64, u64, String)> = std::fs::read_to_string(&cache_path)
        .ok()
        .and_then(|s| {
            theo_domain::safe_json::from_str_bounded(
                &s,
                theo_domain::safe_json::DEFAULT_JSON_LIMIT,
            )
            .ok()
        })
        .unwrap_or_default();

    let mut entries: BTreeMap<String, String> = BTreeMap::new();
    let mut cache_dirty = false;

    let mut hash_wb = ignore::WalkBuilder::new(project_dir);
    hash_wb.hidden(true).git_ignore(true).max_depth(Some(10));
    let _ = hash_wb.add_ignore(project_dir.join(".gitignore"));
    hash_wb.add_custom_ignore_filename(".theoignore");
    hash_wb.filter_entry(|entry| {
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            let name = entry.file_name().to_str().unwrap_or("");
            return !theo_domain::graph_context::EXCLUDED_DIRS.contains(&name);
        }
        true
    });
    let walker = hash_wb.build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(
            ext,
            "rs" | "py"
                | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "go"
                | "java"
                | "rb"
                | "php"
                | "c"
                | "cpp"
                | "cs"
                | "sh"
                | "yaml"
                | "toml"
        ) {
            continue;
        }

        let rel = match path.strip_prefix(project_dir) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        // Incremental: use mtime as pre-filter
        let current_mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let current_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        // If BOTH mtime AND size match cache, reuse cached hash (skip file read)
        if let Some((cached_mtime, cached_size, cached_hash)) = cached.get(&rel)
            && *cached_mtime == current_mtime && *cached_size == current_size {
                entries.insert(rel, cached_hash.clone());
                continue;
            }

        // Mtime or size changed (or not in cache) → read and hash
        if let Ok(content) = std::fs::read(path) {
            let file_hash = blake3::hash(&content).to_hex().to_string();
            cached.insert(
                rel.clone(),
                (current_mtime, current_size, file_hash.clone()),
            );
            entries.insert(rel, file_hash);
            cache_dirty = true;
        }
    }

    // Remove stale entries (files that no longer exist)
    let current_keys: std::collections::HashSet<&String> = entries.keys().collect();
    let stale: Vec<String> = cached
        .keys()
        .filter(|k| !current_keys.contains(k))
        .cloned()
        .collect();
    for key in stale {
        cached.remove(&key);
        cache_dirty = true;
    }

    // Persist cache if changed
    if cache_dirty {
        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(
            &cache_path,
            serde_json::to_string(&cached).unwrap_or_default(),
        );
    }

    let mut project_hasher = blake3::Hasher::new();
    for (path, content_hash) in &entries {
        project_hasher.update(path.as_bytes());
        project_hasher.update(content_hash.as_bytes());
    }
    project_hasher.finalize().to_hex()[..16].to_string()
}

/// Try loading a cached graph if the project state matches.
///
/// Uses content-hash comparison instead of TTL — eliminates both
/// false cache-hits (code changed within TTL) and false cache-misses
/// (1h passed with no changes).
pub fn try_load_cache(cache_path: &Path, project_dir: &Path) -> Option<CodeGraph> {
    if !cache_path.exists() {
        return None;
    }

    let manifest_path = cache_path.with_extension("manifest.json");
    let manifest_content = std::fs::read_to_string(&manifest_path).ok()?;
    // T2.7: bounded deserialization of the graph manifest cache.
    let manifest: GraphManifest = theo_domain::safe_json::from_str_bounded(
        &manifest_content,
        theo_domain::safe_json::DEFAULT_JSON_LIMIT,
    )
    .ok()?;

    let current_hash = compute_project_hash(project_dir);
    if manifest.content_hash != current_hash {
        return None; // Project changed since last build
    }

    // Safety: also reject very old caches (>24h) as a fallback
    let now_secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now_secs - manifest.built_at_secs > 86400 {
        return None;
    }

    theo_engine_graph::persist::load(cache_path).ok()
}

/// Atomic cache write with manifest.
pub fn save_cache_atomic(cache_path: &Path, graph: &CodeGraph, project_dir: &Path) {
    let tmp_path = cache_path.with_extension("bin.tmp");
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if theo_engine_graph::persist::save(graph, &tmp_path).is_ok() {
        let _ = std::fs::rename(&tmp_path, cache_path);

        // Write manifest
        let manifest = GraphManifest {
            content_hash: compute_project_hash(project_dir),
            built_at_secs: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            file_count: graph.node_count(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&manifest) {
            let manifest_tmp = cache_path.with_extension("manifest.json.tmp");
            if std::fs::write(&manifest_tmp, &json).is_ok() {
                let _ = std::fs::rename(&manifest_tmp, cache_path.with_extension("manifest.json"));
            }
        }
    }
}
