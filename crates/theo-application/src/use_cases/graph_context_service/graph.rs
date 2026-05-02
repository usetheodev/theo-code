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

#[cfg(feature = "dense-retrieval")]
pub fn build_dense_components(
    graph: &CodeGraph,
    project_dir: &Path,
) -> (Option<NeuralEmbedder>, Option<EmbeddingCache>) {
    // Try loading embedder (AllMiniLM default, ~200MB; Jina Code opt-in ~1.2GB)
    let embedder = match NeuralEmbedder::new() {
        Ok(e) => e,
        Err(err) => {
            eprintln!("[graphctx] Dense retrieval disabled: embedder init failed: {err}");
            return (None, None);
        }
    };

    // Try loading cached embeddings
    let cache_path = project_dir.join(".theo").join("embeddings.bin");
    let graph_hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        for node_id in graph.node_ids() {
            if let Some(node) = graph.get_node(node_id) {
                if node.node_type == theo_engine_graph::model::NodeType::File {
                    let path = node.file_path.as_deref().unwrap_or(&node.name);
                    path.hash(&mut hasher);
                    node.last_modified.to_bits().hash(&mut hasher);
                }
            }
        }
        hasher.finish()
    };

    if let Some(cache) = EmbeddingCache::load(&cache_path, graph_hash) {
        eprintln!("[graphctx] Loaded embedding cache ({} files)", cache.len());
        return (Some(embedder), Some(cache));
    }

    // Build embeddings (slow: ~5-30s depending on repo size and model)
    eprintln!("[graphctx] Building embedding cache...");
    let cache = EmbeddingCache::build(graph, &embedder);
    eprintln!("[graphctx] Embedding cache built ({} files)", cache.len());

    // Save to disk for next startup
    if let Err(e) = cache.save(&cache_path) {
        eprintln!("[graphctx] Warning: failed to save embedding cache: {e}");
    }

    (Some(embedder), Some(cache))
}

/// Run clustering with a fallback: try FileLeiden first, if it panics or
/// produces zero communities, fall back to a single "all" community.
pub fn tokio_safe_cluster(graph: &CodeGraph) -> ClusterResult {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        theo_engine_graph::cluster::hierarchical_cluster(
            graph,
            ClusterAlgorithm::FileLeiden {
                resolution: LEIDEN_RESOLUTION,
            },
        )
    }));

    match result {
        Ok(cr) if !cr.communities.is_empty() => cr,
        _ => {
            // Fallback: single community with all nodes.
            let all_ids: Vec<String> = graph.file_nodes().iter().map(|n| n.id.clone()).collect();
            ClusterResult {
                communities: vec![Community {
                    id: "all".to_string(),
                    name: "all".to_string(),
                    node_ids: all_ids,
                    level: 0,
                    parent_id: None,
                    version: 0,
                }],
                modularity: 0.0,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FileExtraction → FileData conversor
// ---------------------------------------------------------------------------

/// Convert parser's `FileExtraction` to graph's `FileData` DTO.
///
/// Some fields are lost in translation (confidence, field info, aliases)
/// — this is intentional as the graph bridge doesn't use them.
pub fn convert_extraction(
    ext: FileExtraction,
    rel_path: &str,
    language: SupportedLanguage,
    last_modified: f64,
) -> FileData {
    let symbols: Vec<SymbolData> = ext
        .symbols
        .iter()
        .map(|s| {
            let qualified = match &s.parent {
                Some(p) => format!("{p}::{}", s.name),
                None => s.name.clone(),
            };
            SymbolData {
                qualified_name: qualified,
                name: s.name.clone(),
                kind: convert_symbol_kind(&s.kind),
                line_start: s.anchor.line,
                line_end: s.anchor.end_line,
                signature: s.signature.clone(),
                is_test: s.is_test,
                parent: s.parent.clone(),
                doc: s.doc.clone(),
            }
        })
        .collect();

    let imports: Vec<ImportData> = ext
        .imports
        .iter()
        .map(|i| ImportData {
            source: i.source.clone(),
            specifiers: i.specifiers.clone(),
            line: i.line,
        })
        .collect();

    let references: Vec<ReferenceData> = ext
        .references
        .iter()
        .map(|r| ReferenceData {
            source_symbol: r.source_symbol.clone(),
            source_file: r.source_file.to_string_lossy().to_string(),
            target_symbol: r.target_symbol.clone(),
            target_file: r
                .target_file
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            kind: convert_reference_kind(&r.reference_kind),
        })
        .collect();

    let data_models: Vec<DataModelData> = ext
        .data_models
        .iter()
        .map(|dm| DataModelData {
            name: dm.name.clone(),
            file_path: dm.anchor.file.to_string_lossy().to_string(),
            line_start: dm.anchor.line,
            line_end: dm.anchor.end_line,
            parent_type: dm.parent_type.clone(),
            implemented_interfaces: dm.implemented_interfaces.clone(),
        })
        .collect();

    FileData {
        path: rel_path.to_string(),
        language: format!("{:?}", language),
        line_count: ext.estimated_tokens as usize / 4, // rough estimate
        last_modified,
        symbols,
        imports,
        references,
        data_models,
    }
}

use crate::use_cases::conversion::{convert_reference_kind, convert_symbol_kind};

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// Graph cache manifest — stored alongside graph.bin.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct GraphManifest {
    /// Hash of project file state (sorted path:mtime pairs).
    pub content_hash: String,
    /// When the graph was built (Unix seconds).
    pub built_at_secs: u64,
    /// Number of files in the snapshot.
    pub file_count: usize,
}
