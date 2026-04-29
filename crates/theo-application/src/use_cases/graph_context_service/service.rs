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

pub struct GraphContextService {
    pub state: Arc<tokio::sync::RwLock<GraphBuildState>>,
    /// Ensures only one build runs at a time.
    pub build_in_progress: Arc<AtomicBool>,
    /// PLAN_CONTEXT_WIRING Phase 4 — sink for `RetrievalExecuted` events.
    /// Defaults to `NoopEventSink`; the runtime replaces it with an adapter
    /// around its broadcast `EventBus` via `with_event_sink`.
    pub event_sink: Arc<dyn theo_domain::graph_context::EventSink>,
}

impl GraphContextService {
    pub fn new() -> Self {
        Self {
            state: Arc::new(tokio::sync::RwLock::new(GraphBuildState::Uninitialized)),
            build_in_progress: Arc::new(AtomicBool::new(false)),
            event_sink: Arc::new(theo_domain::graph_context::NoopEventSink),
        }
    }

    /// Attach an event sink for retrieval telemetry. The sink is called
    /// synchronously on the read path; implementations must be cheap and
    /// non-blocking.
    pub fn with_event_sink(
        mut self,
        sink: Arc<dyn theo_domain::graph_context::EventSink>,
    ) -> Self {
        self.event_sink = sink;
        self
    }

    /// Whether the service is already Ready or Building (idempotency guard).
    async fn is_already_initialized(&self) -> bool {
        let current = self.state.read().await;
        matches!(
            *current,
            GraphBuildState::Ready(_) | GraphBuildState::Building { .. }
        )
    }

    /// Install a graph loaded from disk cache as the Ready state.
    async fn install_cached_graph(&self, graph: CodeGraph, dir: std::path::PathBuf) {
        #[cfg(not(feature = "tantivy-backend"))]
        let (communities, scorer) = build_index(&graph);
        #[cfg(feature = "tantivy-backend")]
        let communities = build_index(&graph);
        #[cfg(feature = "tantivy-backend")]
        let tantivy_index = FileTantivyIndex::build(&graph).ok();
        #[cfg(feature = "dense-retrieval")]
        let (embedder, embedding_cache) = build_dense_components(&graph, &dir);

        // Generate Code Wiki (deterministic, ~50ms, cached by graph_hash)
        generate_wiki_if_stale(&graph, &communities, &dir);

        let mut state = self.state.write().await;
        *state = GraphBuildState::Ready(GraphState {
            graph,
            communities,
            project_dir: dir,
            #[cfg(not(feature = "tantivy-backend"))]
            scorer,
            #[cfg(feature = "tantivy-backend")]
            tantivy_index,
            #[cfg(feature = "dense-retrieval")]
            embedder,
            #[cfg(feature = "dense-retrieval")]
            embedding_cache,
            // T8.1 — reranker is heavy (~200 MB Jina v2). Start as None
            // unless the operator opted into preload via THEO_RERANKER_PRELOAD=1.
            #[cfg(feature = "dense-retrieval")]
            reranker: try_construct_reranker_if_enabled(),
            #[cfg(feature = "dense-retrieval")]
            cross_encoder_config: CrossEncoderConfig::default(),
        });
    }

    /// Atomic CAS on `build_in_progress`. Returns false if another build
    /// is already running (and the caller should bail).
    fn acquire_build_lock(&self) -> bool {
        self.build_in_progress
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Move the state machine into `Building`, preserving any prior `Ready`
    /// graph as `stale` so concurrent queries get continuity.
    async fn transition_to_building(&self) {
        let mut state = self.state.write().await;
        let stale = match std::mem::replace(&mut *state, GraphBuildState::Uninitialized) {
            GraphBuildState::Ready(gs) => Some(gs),
            GraphBuildState::Building { stale } => stale,
            _ => None,
        };
        *state = GraphBuildState::Building { stale };
    }

    /// Fire-and-forget the background build task. Result handling and
    /// state-machine update happen entirely in the spawned task.
    fn spawn_background_build(&self, dir: std::path::PathBuf, cache_path: std::path::PathBuf) {
        let state_ref = self.state.clone();
        let build_flag = self.build_in_progress.clone();
        let dir_clone = dir.clone();
        let dir_for_cache = dir;
        tokio::spawn(async move {
            let result = tokio::time::timeout(
                BUILD_TIMEOUT,
                tokio::task::spawn_blocking(move || {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        build_graph_from_project(&dir_clone)
                    }))
                }),
            )
            .await;
            let mut state = state_ref.write().await;
            apply_build_result(&mut state, result, &cache_path, &dir_for_cache);
            build_flag.store(false, Ordering::SeqCst);
        });
    }
}

/// The four-layer Result returned by the timed `spawn_blocking(catch_unwind(...))`
/// background-build pipeline. Aliased to keep `apply_build_result`'s signature legible.
type BuildOutcome = Result<
    Result<std::thread::Result<(CodeGraph, Vec<Community>)>, tokio::task::JoinError>,
    tokio::time::error::Elapsed,
>;

/// Translate the build pipeline's nested-Result into a `GraphBuildState`
/// mutation. Centralized here so `spawn_background_build` stays small.
fn apply_build_result(
    state: &mut GraphBuildState,
    result: BuildOutcome,
    cache_path: &Path,
    dir_for_cache: &Path,
) {
    match result {
        Ok(Ok(Ok((graph, communities)))) => {
            save_cache_atomic(cache_path, &graph, dir_for_cache);
            #[cfg(not(feature = "tantivy-backend"))]
            let scorer = MultiSignalScorer::build(&communities, &graph);
            #[cfg(feature = "tantivy-backend")]
            let tantivy_index = FileTantivyIndex::build(&graph).ok();
            #[cfg(feature = "dense-retrieval")]
            let (embedder, embedding_cache) = build_dense_components(&graph, dir_for_cache);
            generate_wiki_if_stale(&graph, &communities, dir_for_cache);
            *state = GraphBuildState::Ready(GraphState {
                graph,
                communities,
                project_dir: dir_for_cache.to_path_buf(),
                #[cfg(not(feature = "tantivy-backend"))]
                scorer,
                #[cfg(feature = "tantivy-backend")]
                tantivy_index,
                #[cfg(feature = "dense-retrieval")]
                embedder,
                #[cfg(feature = "dense-retrieval")]
                embedding_cache,
                #[cfg(feature = "dense-retrieval")]
                reranker: try_construct_reranker_if_enabled(),
                #[cfg(feature = "dense-retrieval")]
                cross_encoder_config: CrossEncoderConfig::default(),
            });
        }
        Ok(Ok(Err(_panic))) => {
            *state = GraphBuildState::Failed("panic during graph build".into());
        }
        Ok(Err(join_err)) => {
            *state = GraphBuildState::Failed(format!("spawn_blocking failed: {join_err}"));
        }
        Err(_timeout) => {
            *state = GraphBuildState::Failed(format!(
                "build timed out after {}s",
                BUILD_TIMEOUT.as_secs()
            ));
        }
    }
}

impl Default for GraphContextService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl GraphContextProvider for GraphContextService {
    /// Starts graph build in background and returns immediately.
    ///
    /// If a build is already in progress, this is a no-op.
    /// If cache exists and is fresh, loads synchronously (fast path).
    async fn initialize(&self, project_dir: &Path) -> Result<(), GraphContextError> {
        if self.is_already_initialized().await {
            return Ok(());
        }
        let dir = project_dir.to_path_buf();
        let cache_path = dir.join(".theo").join("graph.bin");

        if let Some(graph) = try_load_cache(&cache_path, &dir) {
            self.install_cached_graph(graph, dir).await;
            return Ok(());
        }
        if !self.acquire_build_lock() {
            return Ok(());
        }
        self.transition_to_building().await;
        self.spawn_background_build(dir, cache_path);
        Ok(())
    }

    async fn query_context(
        &self,
        query: &str,
        budget_tokens: usize,
    ) -> Result<GraphContextResult, GraphContextError> {
        let state = self.state.read().await;

        let empty = Ok(GraphContextResult {
            blocks: vec![],
            total_tokens: 0,
            budget_tokens,
            exploration_hints: String::new(),
            budget_report: None,
        });

        match &*state {
            GraphBuildState::Uninitialized => return Err(GraphContextError::NotInitialized),
            GraphBuildState::Building { stale: None } => return empty,
            GraphBuildState::Building { stale: Some(_) } => {} // Serve stale — fall through
            GraphBuildState::Failed(e) => return Err(GraphContextError::BuildFailed(e.clone())),
            GraphBuildState::Ready(_) => {} // Fall through to query.
        }

        if budget_tokens == 0 || query.is_empty() {
            return empty;
        }

        // LAYER 0: Wiki cache lookup (<5ms) with Absolute Confidence Calibration.
        // Uses evaluate_direct_return() with 3 gates:
        // Gate 1: BM25 absolute floor (below = never return)
        // Gate 2: Decision confidence from raw signals (not normalized)
        // Gate 3: Per-category threshold
        {
            use theo_engine_retrieval::wiki::lookup::{DEFAULT_BM25_FLOOR, evaluate_direct_return};

            let wiki_dir = std::path::PathBuf::from(".theo/wiki");
            let wiki_results = theo_engine_retrieval::wiki::lookup::lookup(&wiki_dir, query, 3);

            // Ranking decision log
            if !wiki_results.is_empty() {
                let (allow, conf, reason) =
                    evaluate_direct_return(&wiki_results, query, DEFAULT_BM25_FLOOR);
                let query_class = theo_engine_retrieval::wiki::model::classify_query(query);
                eprintln!(
                    "[wiki-decision] query=\"{}\" class={} allow={} conf={:.2} reason={} top=[{}]",
                    query,
                    query_class.as_str(),
                    allow,
                    conf,
                    reason,
                    wiki_results
                        .iter()
                        .take(3)
                        .map(|r| format!(
                            "{}:T:{}:bm25={:.1}:conf={:.0}%",
                            r.slug,
                            r.authority_tier.as_str(),
                            r.bm25_raw,
                            r.confidence * 100.0
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }

            let (allow, _conf, _reason) =
                evaluate_direct_return(&wiki_results, query, DEFAULT_BM25_FLOOR);

            if allow
                && let Some(top) = wiki_results.first()
                    && top.token_count <= budget_tokens {
                        let blocks: Vec<ContextBlock> = wiki_results
                            .iter()
                            .take(3)
                            .filter(|r| {
                                r.bm25_raw >= DEFAULT_BM25_FLOOR && r.token_count <= budget_tokens
                            })
                            .map(|r| ContextBlock {
                                block_id: format!("blk-wiki-{}", r.slug),
                                source_id: format!(
                                    "wiki:{}[T:{}]",
                                    r.slug,
                                    r.authority_tier.as_str()
                                ),
                                content: r.content.clone(),
                                token_count: r.token_count,
                                score: r.confidence,
                            })
                            .collect();

                        if !blocks.is_empty() {
                            let total_tokens: usize = blocks.iter().map(|b| b.token_count).sum();
                            let query_class =
                                theo_engine_retrieval::wiki::model::classify_query(query);
                            return Ok(GraphContextResult {
                                total_tokens,
                                budget_tokens,
                                exploration_hints: format!(
                                    "Wiki direct return: {} (T:{}, bm25={:.1}, class={}, {})",
                                    top.title,
                                    top.authority_tier.as_str(),
                                    top.bm25_raw,
                                    query_class.as_str(),
                                    top.page_kind
                                ),
                                blocks,
                                budget_report: None,
                            });
                        }
                    }
        }

        // Safe: we checked Ready or Building(stale) above.
        let graph_state = match &*state {
            GraphBuildState::Ready(gs) => gs,
            GraphBuildState::Building { stale: Some(gs) } => gs,
            _ => unreachable!(),
        };

        // Tiered scoring: use best available pipeline.
        // Tier 2 (dense-retrieval): BM25 + Tantivy + Dense → RRF 3-ranker (MRR=0.914)
        // Tier 1 (tantivy-backend): BM25 + Tantivy → hybrid_search (2-ranker)
        // Tier 0 (always): BM25 only → FileBm25::search
        //
        // Fallback cascade: Tier 2 → 1 → 0 (infalível).
        let file_scores: std::collections::HashMap<String, f64> = {
            // Try Tier 2 first: full RRF 3-ranker (BM25 + Tantivy + Dense).
            // T2.5 cleanup: let-chains destructure the Options in place so we
            // never unwrap after a separate `is_some()` check.
            #[cfg(feature = "dense-retrieval")]
            {
                if let Some(idx) = graph_state.tantivy_index.as_ref()
                    && let Some(embedder) = graph_state.embedder.as_ref()
                    && let Some(cache) = graph_state.embedding_cache.as_ref()
                {
                    // T8.1 — Run the runtime-gated pipeline. When
                    // `cross_encoder_config.use_reranker` is true AND
                    // a reranker model is loaded, this includes Stage
                    // 2 cross-encoder reranking; otherwise it returns
                    // the RRF top-K (identical shape to the legacy
                    // `hybrid_rrf_search` call). The reranker field
                    // starts as None — first query that needs it can
                    // construct the model lazily in a future change.
                    retrieve_with_config(
                        &graph_state.graph,
                        idx,
                        embedder,
                        cache,
                        graph_state.reranker.as_deref(),
                        query,
                        20.0, // RRF k parameter (empirically optimal)
                        &graph_state.cross_encoder_config,
                    )
                } else if let Some(idx) = graph_state.tantivy_index.as_ref() {
                    theo_engine_retrieval::tantivy_search::hybrid_search(
                        &graph_state.graph,
                        idx,
                        query,
                    )
                } else {
                    FileBm25::search(&graph_state.graph, query)
                }
            }
            // Without dense-retrieval: try Tier 1, then Tier 0
            #[cfg(all(feature = "tantivy-backend", not(feature = "dense-retrieval")))]
            {
                if let Some(idx) = graph_state.tantivy_index.as_ref() {
                    theo_engine_retrieval::tantivy_search::hybrid_search(
                        &graph_state.graph,
                        idx,
                        query,
                    )
                } else {
                    FileBm25::search(&graph_state.graph, query)
                }
            }
            // Without any features: Tier 0 only
            #[cfg(not(any(feature = "tantivy-backend", feature = "dense-retrieval")))]
            {
                FileBm25::search(&graph_state.graph, query)
            }
        };

        // File Retriever: file-first pipeline with reranking + graph expansion.
        // Falls back to community-level assembly if file retriever returns empty.
        let blocks: Vec<ContextBlock> = {
            let config = theo_engine_retrieval::file_retriever::RerankConfig::default();
            let seen = std::collections::HashSet::new();
            // PLAN_CONTEXT_WIRING Phase 3: use the _with_inline variant so
            // queries that match a symbol name get inline slices (focal +
            // callees/callers) as high-priority context blocks.
            let mut retrieval_result =
                theo_engine_retrieval::file_retriever::retrieve_files_with_inline(
                    &graph_state.graph,
                    &graph_state.communities,
                    query,
                    &config,
                    &seen,
                    &graph_state.project_dir,
                );

            if !retrieval_result.primary_files.is_empty() {
                // File-first path with Phase 2 compression. The mutating
                // sibling populates `compression_savings_tokens` on the
                // result struct (PLAN_CONTEXT_WIRING Task 2.4) so the
                // telemetry payload reads from a single source of truth.
                let ctx_blocks = theo_engine_retrieval::file_retriever::
                    build_context_blocks_with_compression_mut(
                        &mut retrieval_result,
                        &graph_state.graph,
                        budget_tokens,
                        Some(&graph_state.project_dir),
                        query,
                    );
                // PLAN_CONTEXT_WIRING Phase 4: publish retrieval telemetry
                // through the attached EventSink (real EventBus in prod,
                // NoopEventSink otherwise).
                self.event_sink.emit(theo_domain::event::DomainEvent::new(
                    theo_domain::event::EventType::RetrievalExecuted,
                    "graph-context",
                    serde_json::json!({
                        "primary_files": retrieval_result.primary_files.len(),
                        "harm_removals": retrieval_result.harm_removals,
                        "compression_savings_tokens": retrieval_result.compression_savings_tokens,
                        "inline_slices_count": retrieval_result.inline_slices.len(),
                        "query_len": query.len(),
                    }),
                ));
                ctx_blocks
            } else {
                // Fallback: community-level assembly (legacy path)
                let payload = assembly::assemble_files_direct(
                    &file_scores,
                    &graph_state.graph,
                    &graph_state.communities,
                    budget_tokens,
                );
                payload
                    .items
                    .iter()
                    .map(|item| ContextBlock {
                        block_id: format!("blk-{}", item.community_id),
                        source_id: item.community_id.clone(),
                        content: item.content.clone(),
                        token_count: item.token_count,
                        score: item.score,
                    })
                    .collect()
            }
        };

        // Compute totals from blocks
        let total_tokens: usize = blocks.iter().map(|b| b.token_count).sum();

        // WRITE-BACK: Save RRF result to wiki cache for future queries.
        if !blocks.is_empty() && total_tokens > 100 {
            let wiki_dir = std::path::PathBuf::from(".theo/wiki/cache");
            if let Err(e) = write_back_to_wiki(&wiki_dir, query, &blocks) {
                eprintln!("[wiki-cache] Write-back failed: {e}");
            }
        }

        Ok(GraphContextResult {
            total_tokens,
            budget_tokens,
            exploration_hints: String::new(),
            blocks,
            budget_report: None,
        })
    }

    fn is_ready(&self) -> bool {
        // Non-blocking check via try_read.
        self.state
            .try_read()
            .map(|s| matches!(*s, GraphBuildState::Ready(_)))
            .unwrap_or(false)
    }
}
