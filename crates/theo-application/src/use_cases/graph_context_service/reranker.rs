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

pub fn env_reranker_preload_enabled() -> bool {
    match std::env::var("THEO_RERANKER_PRELOAD") {
        Ok(v) => {
            let lower = v.to_ascii_lowercase();
            matches!(lower.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

/// T8.1 part 4 — Best-effort cross-encoder construction.
/// Returns `Some` when the model loaded; `None` on ANY failure
/// (network, missing dep, init panic). The graph build path uses
/// this to populate `GraphState.reranker` without ever propagating
/// reranker errors to the agent — a missing reranker just means
/// retrieval falls back to RRF-only, which is still a working
/// pipeline.
#[cfg(feature = "dense-retrieval")]
pub fn try_construct_reranker_if_enabled() -> Option<Arc<CrossEncoderReranker>> {
    if !env_reranker_preload_enabled() {
        return None;
    }
    // CrossEncoderReranker::new() returns Result<Self, Box<dyn Error>>;
    // `catch_unwind` guards against any panic inside fastembed/onnx
    // initialization (the worst-case offline path) so a misconfigured
    // host can never crash a graph build.
    let result = std::panic::catch_unwind(|| CrossEncoderReranker::new());
    match result {
        Ok(Ok(rr)) => Some(Arc::new(rr)),
        Ok(Err(e)) => {
            eprintln!(
                "[theo:reranker] preload failed (falling back to RRF-only): {e}"
            );
            None
        }
        Err(_) => {
            eprintln!(
                "[theo:reranker] preload panicked (falling back to RRF-only)"
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Max time for graph build (clustering can be slow for large repos).
/// 60s accommodates debug builds; release builds are ~5-10x faster.
pub const BUILD_TIMEOUT: Duration = Duration::from_secs(60);

/// Cache validity period.
/// Leiden resolution parameter (1.0 = standard modularity).
pub const LEIDEN_RESOLUTION: f64 = 1.0;

// ---------------------------------------------------------------------------
// Internal state machine
// ---------------------------------------------------------------------------

pub struct GraphState {
    pub graph: CodeGraph,
    pub communities: Vec<Community>,
    /// Root of the indexed workspace. Required by the context-wiring
    /// phases (compression, inline-builder) that read source off disk.
    pub project_dir: std::path::PathBuf,
    /// MultiSignalScorer: only built when no RRF pipeline available (Tier 0 only).
    /// When tantivy-backend is active, query_context uses FileBm25 directly,
    /// saving ~200MB RAM from scorer's BM25 index + TF-IDF model.
    /// Held even when no `&self.scorer` reads exist on the current build —
    /// the indexer still needs to be constructed at startup so the
    /// fallback path is wired. Mark `#[allow(dead_code)]` to silence
    /// clippy on builds where the field genuinely has no readers.
    #[cfg(not(feature = "tantivy-backend"))]
    #[allow(dead_code)]
    pub scorer: MultiSignalScorer,
    /// Tantivy BM25F index (Tier 1).
    #[cfg(feature = "tantivy-backend")]
    pub tantivy_index: Option<FileTantivyIndex>,
    /// Neural embedder for dense search (Tier 2). AllMiniLM default, Jina Code opt-in.
    #[cfg(feature = "dense-retrieval")]
    pub embedder: Option<NeuralEmbedder>,
    /// Pre-computed file embeddings (Tier 2). Cached to .theo/embeddings.bin.
    #[cfg(feature = "dense-retrieval")]
    pub embedding_cache: Option<EmbeddingCache>,
    /// T8.1 — Cross-encoder reranker (Stage 2). When `Some`, the
    /// retrieval pipeline runs RRF → rerank; when `None`, falls back
    /// to RRF top-K (current behaviour). Initialised lazily — model
    /// download (~200 MB Jina v2) happens on first construction so
    /// quick `theo init` runs that don't query never pay the cost.
    #[cfg(feature = "dense-retrieval")]
    pub reranker: Option<Arc<CrossEncoderReranker>>,
    /// T8.1 — Runtime config for the reranker stage (`use_reranker`,
    /// `top_k`, `max_candidates`). Always present; defaults are SOTA
    /// (use_reranker=true, top_k=20, max_candidates=50).
    #[cfg(feature = "dense-retrieval")]
    pub cross_encoder_config: CrossEncoderConfig,
}

/// Explicit state machine for background graph build lifecycle.
pub enum GraphBuildState {
    /// No initialization started yet.
    Uninitialized,
    /// Build running in background. Stale cache served if available.
    Building { stale: Option<GraphState> },
    /// Graph built and ready for queries.
    Ready(GraphState),
    /// Build failed. Agent operates without context.
    Failed(String),
}

// ---------------------------------------------------------------------------
// Service
