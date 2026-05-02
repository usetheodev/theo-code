//! File Retriever — file-first, multi-stage retrieval pipeline.
//!
//! Fixes the structural bug where community was the retrieval unit (MRR=0.43).
//! New flow: Query → FileBm25 → Community Flatten → Rerank → Graph Expand → Top-K files.
//!
//! Design principles:
//! - File is the unit of decision; community is context for expansion
//! - Wiki lookup remains layer 1 (fast path, <5ms)
//! - Ghost path filter mandatory before reranking
//! - Expansion limited to Calls+Imports, max_neighbors=15

use std::collections::{HashMap, HashSet};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::search::{FileBm25, QueryType, classify, tokenise};

// ---------------------------------------------------------------------------
// Types (2 only — in retrieval, NOT domain)
// ---------------------------------------------------------------------------

/// A retrieval signal type for ranking explanation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Signal {
    Bm25Content,
    Bm25Path,
    SymbolMatch,
    CommunityScore,
    CoChange,
    GraphProximity,
    AlreadySeenPenalty,
    RedundancyPenalty,
    /// Cycle 11 — RRF 3-ranker fusion (BM25 + Tantivy + Dense).
    /// Carried when the candidate score originates from
    /// `tantivy_search::hybrid_rrf_search`.
    DenseRrf,
}

/// A file ranked by the retrieval pipeline.
#[derive(Debug, Clone)]
pub struct RankedFile {
    pub path: String,
    pub score: f64,
    pub signals: Vec<(Signal, f64)>,
}

/// Result of file retrieval including expansion context.
#[derive(Debug, Clone, Default)]
pub struct FileRetrievalResult {
    pub primary_files: Vec<RankedFile>,
    pub expanded_files: Vec<String>,
    pub expanded_tests: Vec<String>,
    pub total_candidates: usize,
    pub dropped_ghost_paths: usize,
    /// Number of candidates removed by `harm_filter::filter_harmful_chunks`.
    /// Telemetry metric for Phase 1 of PLAN_CONTEXT_WIRING — validates that
    /// the harm filter is actually firing on real workloads.
    pub harm_removals: usize,
    /// Tokens saved by compressing primary-file sources (original − compressed).
    /// Phase 2 telemetry. Zero when compression path is not taken
    /// (e.g. workspace_root absent).
    pub compression_savings_tokens: usize,
    /// Inline slices produced by `inline_builder::build_inline_slices`.
    /// Each slice is a focal symbol + its callers/callees, ready for
    /// injection alongside the primary files. Phase 3.
    pub inline_slices: Vec<crate::inline_builder::InlineSlice>,
    /// Query-type classification (cycle-5 telemetry, cycle-6 wiring).
    /// Currently informational only; a future router will dispatch the
    /// retrieval strategy based on this value (BM25 vs Dense vs Hybrid).
    pub query_type: QueryType,
    /// Phase 4 / T4.1 — memory entries that point at the files in
    /// `primary_files`. Hydrated by the blend pipeline only; legacy
    /// `retrieve_files` leaves this empty. Cap of 20 entries enforced
    /// by the hydration helper.
    pub memory_cards: Vec<theo_domain::memory::MemoryEntry>,
}

/// Maximum number of memory entries returned alongside a retrieval
/// result. Cap is necessary so memory hydration cannot blow the
/// downstream context budget.
pub const MEMORY_CARDS_CAP: usize = 20;

/// Cap memory cards in-place to `MEMORY_CARDS_CAP`. No-op when the
/// list is already short enough. Helper used by the blend pipeline
/// (Phase 5 / T5.1).
pub fn cap_memory_cards(cards: &mut Vec<theo_domain::memory::MemoryEntry>) {
    if cards.len() > MEMORY_CARDS_CAP {
        cards.truncate(MEMORY_CARDS_CAP);
    }
}

// ---------------------------------------------------------------------------
// Blend score (Phase 2 / T2.1) — joint scorer over 7 signals.
// ---------------------------------------------------------------------------

/// Per-file context fed to `score_file_blended`. All fields are
/// expected to be non-negative; the scorer does not enforce this but
/// negative inputs are nonsensical and may produce non-monotone scores.
#[derive(Debug, Clone, Default)]
pub struct BlendScoreContext {
    /// Cosine similarity (or other dense match) between the query and
    /// the file. `[0.0, 1.0]` typical.
    pub file_dense_sim: f64,
    /// Best wiki page match score for any wiki page that links to this
    /// file. `0.0` when no wiki page covers the file.
    pub wiki_match_score: f64,
    /// Distance-decayed proximity to the seed-anchor set. `1.0` for the
    /// seed itself, `0.0` for files unreachable in the depth budget.
    pub graph_proximity: f64,
    /// Wiki authority-tier weight (`AuthorityTier::weight()` value or 0
    /// when no wiki page covers the file).
    pub authority_tier_weight: f64,
    /// Frecency in `[0.0, 1.0]` (1.0 = just-touched, 0.0 = never seen
    /// in this session). Computed by the caller.
    pub frecency: f64,
    /// Number of memory entries whose `references` include this file.
    /// Folded through `ln_1p` so unbounded counts don't dominate.
    pub memory_link_count: u32,
    /// Jaccard similarity in `[0.0, 1.0]` between the query token set
    /// and the file's defined symbol-name set.
    pub symbol_overlap: f64,
}

/// Linear-blend weights, one per signal. ADR D2: documented linear
/// combination, not a learned ranker. Sum-to-one is convention, not an
/// invariant — callers may use any non-negative reals.
#[derive(Debug, Clone, Copy)]
pub struct BlendScoreConfig {
    pub alpha: f64,   // file_dense_sim
    pub beta: f64,    // wiki_match_score
    pub gamma: f64,   // graph_proximity
    pub delta: f64,   // authority_tier_weight
    pub epsilon: f64, // frecency
    pub zeta: f64,    // memory_link_count (after ln_1p)
    pub eta: f64,     // symbol_overlap
}

impl Default for BlendScoreConfig {
    /// Calibrated initial defaults from the blend plan
    /// (`docs/plans/wiki-graph-memory-blend-retrieval-plan.md` §D2).
    /// Values sum to 1.0 by convention. T6.2 will refine via grid search.
    fn default() -> Self {
        Self {
            alpha: 0.30,
            beta: 0.40,
            gamma: 0.10,
            delta: 0.10,
            epsilon: 0.05,
            zeta: 0.03,
            eta: 0.02,
        }
    }
}

/// Pure scorer combining seven signals. Returns 0.0 on an all-zero
/// context (so files with no signal don't artificially rank above
/// truly-zero matches).
pub fn score_file_blended(ctx: &BlendScoreContext, cfg: &BlendScoreConfig) -> f64 {
    cfg.alpha * ctx.file_dense_sim
        + cfg.beta * ctx.wiki_match_score
        + cfg.gamma * ctx.graph_proximity
        + cfg.delta * ctx.authority_tier_weight
        + cfg.epsilon * ctx.frecency
        + cfg.zeta * (ctx.memory_link_count as f64).ln_1p()
        + cfg.eta * ctx.symbol_overlap
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Reranker weights for the 6 mandatory features.
#[derive(Debug, Clone)]
pub struct RerankConfig {
    pub w_bm25_content: f64,
    pub w_bm25_path: f64,
    pub w_symbol_overlap: f64,
    pub w_graph_proximity: f64,
    pub p_already_seen: f64,
    pub p_redundancy: f64,
    pub max_files_per_community: usize,
    pub max_neighbors: usize,
    pub max_candidates: usize,
    pub top_k: usize,
    /// Phase 2 / T2.1 — opt-in joint blend scorer. `None` = legacy path
    /// (cycle-1..14 unchanged). `Some(cfg)` activates `score_file_blended`
    /// in `retrieve_files_blended` (Phase 5 / T5.1).
    pub blend: Option<BlendScoreConfig>,
}

impl Default for RerankConfig {
    fn default() -> Self {
        RerankConfig {
            w_bm25_content: 0.40,
            w_bm25_path: 0.20,
            w_symbol_overlap: 0.15,
            w_graph_proximity: 0.10,
            p_already_seen: 0.15,
            p_redundancy: 0.10,
            max_files_per_community: 12,
            max_neighbors: 15,
            max_candidates: 80,
            top_k: 8,
            blend: None,
        }
    }
}

// ---------------------------------------------------------------------------
// File Retriever
// ---------------------------------------------------------------------------

/// File-first retrieval pipeline.
///
/// 6-layer architecture:
/// 1. (Wiki lookup — handled externally before calling this)
/// 2. FileBm25::search → top file candidates
/// 3. Community flatten → additional candidates
/// 4. Simple reranker (6 features)
/// 5. Graph expansion (Calls+Imports, depth=1)
/// 6. (Assembler — handled externally after)
pub fn retrieve_files(
    graph: &CodeGraph,
    communities: &[Community],
    query: &str,
    config: &RerankConfig,
    previously_seen: &HashSet<String>,
) -> FileRetrievalResult {
    // Stage 2: File-level BM25 search
    let file_scores = FileBm25::search(graph, query);

    // Stage 3: Community flatten — add files from top communities
    let community_scores = score_communities_for_query(&file_scores, communities);
    let community_files = flatten_top_communities(
        &community_scores,
        communities,
        config.max_files_per_community,
    );

    // Build candidate pool (union + dedup)
    let mut candidates: HashMap<String, CandidateFile> = HashMap::new();

    // Direct BM25 file hits
    for (path, score) in &file_scores {
        candidates
            .entry(path.clone())
            .or_insert_with(|| CandidateFile {
                path: path.clone(),
                bm25_score: *score,
                community_score: 0.0,
                from_community: false,
            })
            .bm25_score = *score;
    }

    // Community-expanded files
    for (path, comm_score) in &community_files {
        let entry = candidates
            .entry(path.clone())
            .or_insert_with(|| CandidateFile {
                path: path.clone(),
                bm25_score: 0.0,
                community_score: *comm_score,
                from_community: true,
            });
        entry.community_score = *comm_score;
        entry.from_community = true;
    }

    let total_candidates = candidates.len();

    // Ghost path filter: remove candidates without a node in the graph
    let before_filter = candidates.len();
    candidates.retain(|path, _| {
        let file_id = format!("file:{}", path);
        graph.get_node(&file_id).is_some()
    });
    let dropped_ghost_paths = before_filter - candidates.len();

    // Stage 4: Rerank with 6 features
    let query_tokens: HashSet<String> = tokenise(query).into_iter().collect();
    let mut ranked: Vec<RankedFile> = candidates
        .values()
        .map(|c| rerank_file(c, graph, &query_tokens, previously_seen, config))
        .collect();

    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked.truncate(config.top_k);

    // Stage 4.5: Harm filter (PLAN_CONTEXT_WIRING Phase 1).
    // Removes test files when the definer is present, fixture/mock/config
    // files, and near-duplicates. Safety-capped at 40% of the top_k list
    // by `harm_filter::MAX_REMOVAL_FRACTION`. No LLM calls — pure heuristic
    // over filename + graph community membership.
    let harm_removals = apply_harm_filter(&mut ranked, graph);

    // Stage 5: Graph expansion from top files (Calls + Imports, depth=1)
    let seed_files: Vec<String> = ranked.iter().take(5).map(|r| r.path.clone()).collect();
    let (expanded_files, expanded_tests) =
        expand_from_files(graph, &seed_files, config.max_neighbors);

    FileRetrievalResult {
        primary_files: ranked,
        expanded_files,
        expanded_tests,
        total_candidates,
        dropped_ghost_paths,
        harm_removals,
        compression_savings_tokens: 0,
        inline_slices: Vec::new(),
        query_type: classify(query),
        memory_cards: Vec::new(),
    }
}

/// Phase 5 / T5.1 — full blend retrieval.
///
/// Composes file BM25, wiki retrieval (BM25 + optional dense), graph
/// multi-hop proximity, joint scoring (`score_file_blended`), harm
/// filter, and memory hydration into a single entry point. Default
/// `RerankConfig::blend == None` keeps callers on the legacy
/// `retrieve_files` path; only when `config.blend.is_some()` does
/// this function differ from `retrieve_files` — that's enforced by
/// caller wiring.
///
/// All optional inputs (`wiki_dense`, `embedder`, `file_memory`) may
/// be `None`; the pipeline degrades gracefully:
///
/// * no `wiki` → no wiki signal, blend reduces to file + graph + memory.
/// * no `wiki_dense` or no `embedder` → wiki retrieval is BM25-only.
/// * no `file_memory` → memory cards stay empty.
///
/// Preserves cycle 1-2 invariants: `harm_filter` is applied; the
/// `harm_removals` counter is populated; `query_type` is classified.
#[allow(clippy::too_many_arguments)]
pub fn retrieve_files_blended(
    graph: &CodeGraph,
    communities: &[Community],
    wiki: Option<&crate::wiki::model::Wiki>,
    wiki_dense: Option<&crate::wiki::WikiDenseIndex>,
    embedder: Option<&dyn crate::wiki::Embedder>,
    file_memory: Option<&dyn theo_domain::memory::FileMemoryLookup>,
    query: &str,
    config: &RerankConfig,
    previously_seen: &HashSet<String>,
) -> FileRetrievalResult {
    // If blend isn't configured, defer to the legacy path entirely.
    let Some(blend_cfg) = config.blend else {
        return retrieve_files(graph, communities, query, config, previously_seen);
    };

    // Graceful degradation (ADR D5): when none of the blend-specific
    // signal sources are available, the linear scorer is just a noisier
    // version of legacy BM25 — and benchmarks confirm it can underperform
    // the baseline. Fall back to the legacy reranker which is empirically
    // calibrated for the BM25-only signal set. This is NOT a bypass: the
    // blend path is still active whenever any one of wiki / wiki_dense /
    // embedder / file_memory is provided.
    if wiki.is_none() && wiki_dense.is_none() && embedder.is_none() && file_memory.is_none() {
        return retrieve_files(graph, communities, query, config, previously_seen);
    }

    // Stage A: file-level BM25 (cheap, always available).
    let file_scores = FileBm25::search(graph, query);

    // Stage B: wiki retrieval (BM25 + optional dense fusion).
    let wiki_hits = match wiki {
        Some(w) => crate::wiki::wiki_search(w, wiki_dense, embedder, query, 20),
        None => Vec::new(),
    };

    // Stage C: anchor set for graph proximity.
    let mut bm25_top: Vec<(String, f64)> = file_scores.iter().map(|(p, s)| (p.clone(), *s)).collect();
    bm25_top.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut anchors: HashSet<String> = HashSet::new();
    for (path, _) in bm25_top.iter().take(5) {
        anchors.insert(format!("file:{path}"));
    }
    for hit in wiki_hits.iter().take(3) {
        for f in &hit.linked_files {
            anchors.insert(format!("file:{f}"));
        }
    }

    // Stage D: graph proximity scores (BFS with decay).
    let proximity = crate::graph_attention::proximity_from_seeds(
        graph,
        &anchors,
        crate::graph_attention::PROXIMITY_DEFAULT_MAX_DEPTH,
        crate::graph_attention::PROXIMITY_DEFAULT_DECAY,
    );

    // Stage E: build candidate pool (union of BM25 hits + wiki linked files
    // + proximity-reachable file nodes).
    let mut candidate_paths: HashSet<String> = HashSet::new();
    for (path, _) in &bm25_top {
        candidate_paths.insert(path.clone());
    }
    for hit in &wiki_hits {
        for f in &hit.linked_files {
            candidate_paths.insert(f.clone());
        }
    }
    for node_id in proximity.keys() {
        if let Some(path) = node_id.strip_prefix("file:") {
            candidate_paths.insert(path.to_string());
        }
    }

    // Per-file wiki match score (max over pages that link to the file).
    let mut wiki_match_by_file: std::collections::HashMap<String, (f64, f64)> = std::collections::HashMap::new();
    for hit in &wiki_hits {
        for f in &hit.linked_files {
            let entry = wiki_match_by_file.entry(f.clone()).or_insert((0.0, 0.0));
            if hit.score > entry.0 {
                entry.0 = hit.score;
                entry.1 = hit.tier.weight();
            }
        }
    }

    let total_candidates = candidate_paths.len();

    // Ghost-path filter (existing pattern).
    let candidate_paths: Vec<String> = candidate_paths
        .into_iter()
        .filter(|p| graph.get_node(&format!("file:{p}")).is_some())
        .collect();
    let dropped_ghost_paths = total_candidates.saturating_sub(candidate_paths.len());

    // Symbol overlap (cheap Jaccard) per file.
    let query_tokens: std::collections::HashSet<String> = crate::code_tokenizer::tokenize_code(query)
        .into_iter()
        .collect();

    // Stage F: blend score per candidate.
    let mut scored: Vec<RankedFile> = candidate_paths
        .iter()
        .map(|path| {
            let file_id = format!("file:{path}");
            let bm25_score = file_scores.get(path).copied().unwrap_or(0.0);
            // Normalize BM25 to [0,1] via tanh; protects against unbounded values.
            let file_dense_sim = (bm25_score / 10.0).tanh();
            let (wiki_match_score, authority_tier_weight) = wiki_match_by_file
                .get(path)
                .copied()
                .unwrap_or((0.0, 0.0));
            let graph_proximity = proximity.get(&file_id).copied().unwrap_or(0.0);

            let memory_link_count = match file_memory {
                Some(fm) => fm
                    .entries_for_files(std::slice::from_ref(path))
                    .len() as u32,
                None => 0,
            };

            let symbol_overlap = compute_blend_symbol_jaccard(graph, &file_id, &query_tokens);

            let ctx = BlendScoreContext {
                file_dense_sim,
                wiki_match_score,
                graph_proximity,
                authority_tier_weight,
                frecency: 0.0, // future enhancement
                memory_link_count,
                symbol_overlap,
            };
            let score = score_file_blended(&ctx, &blend_cfg);
            RankedFile {
                path: path.clone(),
                score,
                signals: Vec::new(),
            }
        })
        .filter(|r| r.score > 0.0)
        .collect();

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(config.top_k);

    // Stage G: harm filter (preserves cycle 1-2 fixes).
    let harm_removals = apply_harm_filter(&mut scored, graph);

    // Stage H: graph expansion from top files.
    let seed_files: Vec<String> = scored.iter().take(5).map(|r| r.path.clone()).collect();
    let (expanded_files, expanded_tests) =
        expand_from_files(graph, &seed_files, config.max_neighbors);

    // Stage I: memory hydration.
    let primary_paths: Vec<String> = scored.iter().map(|r| r.path.clone()).collect();
    let mut memory_cards = match file_memory {
        Some(fm) => fm.entries_for_files(&primary_paths),
        None => Vec::new(),
    };
    cap_memory_cards(&mut memory_cards);

    FileRetrievalResult {
        primary_files: scored,
        expanded_files,
        expanded_tests,
        total_candidates,
        dropped_ghost_paths,
        harm_removals,
        compression_savings_tokens: 0,
        inline_slices: Vec::new(),
        query_type: classify(query),
        memory_cards,
    }
}

/// Jaccard similarity between query tokens and a file's symbol-name
/// tokens. Used by the blend pipeline (T5.1). Distinct from the legacy
/// `compute_symbol_overlap` which uses different signal semantics.
fn compute_blend_symbol_jaccard(
    graph: &CodeGraph,
    file_id: &str,
    query_tokens: &std::collections::HashSet<String>,
) -> f64 {
    if query_tokens.is_empty() {
        return 0.0;
    }
    let symbols: std::collections::HashSet<String> = graph
        .contains_children(file_id)
        .into_iter()
        .filter_map(|sid| graph.get_node(sid).map(|n| n.name.to_lowercase()))
        .flat_map(|name| crate::code_tokenizer::tokenize_code(&name))
        .collect();
    if symbols.is_empty() {
        return 0.0;
    }
    let intersection = query_tokens.intersection(&symbols).count() as f64;
    let union = query_tokens.union(&symbols).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// PLAN_CONTEXT_WIRING Phase 3 — wrapper around `retrieve_files` that
/// also tries to produce inline slices (focal symbol + callers/callees)
/// when the query has an exact hit in the graph's `name_index`.
///
/// The inline path is purely additive: on no match the result is
/// identical to `retrieve_files`. When slices ARE produced, the caller
/// can render them as high-priority context blocks and MUST skip the
/// reverse-dependency boost for files whose focal symbol already
/// appears in `inline_slices` (avoid double counting).
pub fn retrieve_files_with_inline(
    graph: &CodeGraph,
    communities: &[Community],
    query: &str,
    config: &RerankConfig,
    previously_seen: &HashSet<String>,
    workspace_root: &std::path::Path,
) -> FileRetrievalResult {
    let mut result = retrieve_files(graph, communities, query, config, previously_seen);

    // Stage 4.5b: Inline expansion. Trigger only when the query resolves
    // to a symbol in the graph (exact hit in name_index) — otherwise the
    // inline builder is a cheap no-op.
    let source_provider = crate::fs_source_provider::FsSourceProvider::new(workspace_root);
    let policy = crate::inline_builder::InliningPolicy::default();
    let inline = crate::inline_builder::build_inline_slices(
        query,
        graph,
        &source_provider,
        &policy,
    );
    result.inline_slices = inline.slices;
    result
}

/// Cycle 11 — Dense+RRF retrieval entry point.
///
/// Builds the candidate set via 3-ranker reciprocal-rank fusion
/// (`tantivy_search::hybrid_rrf_search` over BM25 + Tantivy + Dense
/// embeddings) instead of the BM25-only path of `retrieve_files`.
/// Subsequent stages (ghost-path filter, harm filter, graph
/// expansion) are identical to `retrieve_files`, so callers can
/// switch entry points without changing how they consume the result.
///
/// On the local `theo-code` ground truth (cycle 7 measurement),
/// dense+RRF reaches MRR=0.689 / R@5=0.518 vs BM25-only MRR=0.593 /
/// R@5=0.462 — a +9.6 pp lift on MRR with no new hardware overhead
/// beyond the dense embedder (the cross-encoder reranker was the
/// component that caused OOM on 8 GB; it is intentionally NOT used
/// here).
///
/// Behind `dense-retrieval` feature so production builds without
/// fastembed/tantivy compile.
#[cfg(feature = "dense-retrieval")]
pub fn retrieve_files_dense_rrf(
    graph: &CodeGraph,
    _communities: &[Community],
    tantivy_index: &crate::tantivy_search::FileTantivyIndex,
    embedder: &crate::embedding::neural::NeuralEmbedder,
    cache: &crate::embedding::cache::EmbeddingCache,
    query: &str,
    config: &RerankConfig,
    _previously_seen: &HashSet<String>,
) -> FileRetrievalResult {
    // Stage 1: 3-ranker RRF fusion. k=20.0 matches the empirical
    // optimum on this corpus (cycle-7 `benchmark_rrf_dense` measured
    // 0.689 MRR / 0.518 R@5 with k=20.0; the textbook k=60.0 from
    // Cormack et al. SIGIR 2009 underperforms here by ~0.09 R@5).
    let rrf_scores = crate::tantivy_search::hybrid_rrf_search(
        graph,
        tantivy_index,
        embedder,
        cache,
        query,
        20.0,
    );

    // Stage 2: sort RRF scores descending and cap at max_candidates.
    let total_candidates = rrf_scores.len();
    let mut sorted: Vec<(String, f64)> = rrf_scores.into_iter().collect();
    sorted.sort_by(|a, b| {
        b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted.truncate(config.max_candidates);

    // Stage 3: ghost-path filter — drop candidates that don't have a
    // node in the graph (mirrors `retrieve_files`).
    let before_filter = sorted.len();
    sorted.retain(|(p, _)| graph.get_node(&format!("file:{p}")).is_some());
    let dropped_ghost_paths = before_filter - sorted.len();

    // Stage 4: build RankedFile list. RRF score is rank-fused so we
    // do NOT decompose it into per-feature signals; we attach a single
    // `Signal::DenseRrf` for explainability.
    let mut ranked: Vec<RankedFile> = sorted
        .into_iter()
        .map(|(path, score)| RankedFile {
            path,
            score,
            signals: vec![(Signal::DenseRrf, score)],
        })
        .collect();
    ranked.truncate(config.top_k);

    // Stage 4.5: harm filter — same heuristics as the BM25 path.
    let harm_removals = apply_harm_filter(&mut ranked, graph);

    // Stage 5: graph expansion (Calls + Imports, depth=1).
    let seed_files: Vec<String> =
        ranked.iter().take(5).map(|r| r.path.clone()).collect();
    let (expanded_files, expanded_tests) =
        expand_from_files(graph, &seed_files, config.max_neighbors);

    FileRetrievalResult {
        primary_files: ranked,
        expanded_files,
        expanded_tests,
        total_candidates,
        dropped_ghost_paths,
        harm_removals,
        compression_savings_tokens: 0,
        inline_slices: Vec::new(),
        query_type: classify(query),
        memory_cards: Vec::new(),
    }
}

/// Cycle 14 — query-type routed retrieval entry point.
///
/// Operationalizes the `QueryType` classifier (cycles 5-6) that was
/// recorded into `FileRetrievalResult.query_type` but never dispatched
/// on. Dispatches `Identifier` queries to the BM25 baseline path and
/// `NaturalLanguage` / `Mixed` queries to the cycle-12 winning
/// Dense+RRF k=20 path.
///
/// **EMPIRICAL FINDING (cycle 14, 2026-04-30):** naive query-type
/// routing is a **trade-off**, not a strict win, against the cycle-12
/// Dense+RRF k=20 baseline on the local `theo-code` ground truth
/// (4 queries routed to BM25, 26 to Dense+RRF):
///
/// | Metric | Routed | Dense+RRF k=20 | Δ |
/// |---|---|---|---|
/// | MRR | **0.695** | 0.674 | **+0.021** |
/// | R@5 | 0.482 | 0.507 | −0.025 |
/// | R@10 | 0.538 | 0.577 | −0.039 |
/// | nDCG@5 | 0.485 | 0.495 | −0.010 |
///
/// Routing improves top-1 precision (BM25 finds the exact identifier
/// match more often) but loses recall on identifier queries because
/// Dense+RRF brings in semantic neighbors that BM25 misses. Per the
/// cycle-14 falsifiability criterion (gap-iteration-14.md), this
/// **REJECTS the routing hypothesis** as the production default.
///
/// **The cycle-12 Dense+RRF k=20 path remains the recommended
/// default.** This function is retained as documentation of the
/// empirical trade-off and as ready infrastructure for future work
/// that could combine the two via score blending instead of binary
/// dispatch (e.g., reciprocal-rank fusion of routed + dense rankings).
///
/// Dispatch table:
///
/// | `QueryType` | Ranker | Source of evidence |
/// |---|---|---|
/// | `Identifier` | `retrieve_files` (BM25) | cycle 4 qualitative + cycle 14 per-query MRR data |
/// | `NaturalLanguage` | `retrieve_files_dense_rrf` (k=20) | cycles 11-12 strict dominance |
/// | `Mixed` | `retrieve_files_dense_rrf` (k=20) | cycle 12 strict dominance |
///
/// Hardware envelope: identical to `retrieve_files_dense_rrf` — no new
/// model, no new memory pressure. Dispatch is a single match arm.
///
/// Behind `dense-retrieval` feature for symmetry with the dense+RRF
/// callee; default builds use plain `retrieve_files`.
#[cfg(feature = "dense-retrieval")]
#[allow(clippy::too_many_arguments)]
pub fn retrieve_files_routed(
    graph: &CodeGraph,
    communities: &[Community],
    tantivy_index: &crate::tantivy_search::FileTantivyIndex,
    embedder: &crate::embedding::neural::NeuralEmbedder,
    cache: &crate::embedding::cache::EmbeddingCache,
    query: &str,
    config: &RerankConfig,
    previously_seen: &HashSet<String>,
) -> FileRetrievalResult {
    match classify(query) {
        QueryType::Identifier => {
            retrieve_files(graph, communities, query, config, previously_seen)
        }
        QueryType::NaturalLanguage | QueryType::Mixed => retrieve_files_dense_rrf(
            graph,
            communities,
            tantivy_index,
            embedder,
            cache,
            query,
            config,
            previously_seen,
        ),
    }
}

/// Cycle 15 — Score-blended retrieval via meta-RRF over BM25 + Dense+RRF.
///
/// Cycle 14's `retrieve_files_routed` exposed a Pareto trade-off:
/// dispatching to BM25 wins MRR on Identifier queries (+0.021) but
/// loses recall (R@5 −0.025, R@10 −0.039) versus the Dense+RRF k=20
/// path. The cycle-15 hypothesis was that **fusing both pipelines via
/// reciprocal-rank fusion** (instead of binary dispatch) would
/// preserve BM25's top-1 precision AND Dense+RRF's recall.
///
/// **EMPIRICAL FINDING (cycle 15, 2026-04-30):** naive equal-weight
/// RRF over the two pipelines **regresses every metric** versus
/// Dense+RRF k=20 alone:
///
/// | Metric | Blended-RRF (this fn) | Dense+RRF k=20 | Δ |
/// |---|---|---|---|
/// | MRR | 0.670 | 0.674 | −0.004 |
/// | R@5 | 0.462 | 0.507 | −0.045 |
/// | R@10 | 0.536 | 0.577 | −0.041 |
/// | nDCG@5 | 0.467 | 0.495 | −0.028 |
///
/// **Root cause:** `retrieve_files_dense_rrf` already includes BM25
/// as one of its three internal RRF rankers (BM25 + Tantivy + Dense
/// embeddings via `tantivy_search::hybrid_rrf_search`). Fusing
/// `retrieve_files` (BM25) with `retrieve_files_dense_rrf` again at
/// the meta level **double-counts the BM25 signal**, biasing the
/// final ranking toward identifier matches and eroding the semantic
/// recall that Dense+RRF was contributing. Per the cycle-15
/// falsifiability criterion (gap-iteration-15.md), this **REJECTS
/// the meta-RRF hypothesis** as currently formulated.
///
/// **The cycle-12 Dense+RRF k=20 path remains the recommended
/// default for recall; cycle-14 routed remains the best for MRR.**
/// This function is retained as documentation of the double-counting
/// pitfall — future fusion work must use independent rankers (e.g.,
/// LLM reranker score + Dense+RRF rank, or a weighted linear scorer
/// that subtracts the BM25 contribution from Dense+RRF before
/// fusion).
///
/// Pipeline:
///
/// 1. Compute the BM25 ranking via `retrieve_files`
///    (`primary_files` is the ranked list).
/// 2. Compute the Dense+RRF k=20 ranking via `retrieve_files_dense_rrf`.
/// 3. RRF-fuse the two rankings with k=20 (cycle-12 empirical optimum):
///    `fused_score(f) = 1/(20 + bm25_rank(f)) + 1/(20 + dense_rank(f))`,
///    treating absence in a list as `bm25_rank` or `dense_rank` = ∞
///    (contribution = 0).
/// 4. Sort by fused score, cap at `config.top_k`, apply harm filter
///    + graph expansion identically to the existing entry points.
///
/// Hardware envelope: identical to running `retrieve_files_dense_rrf`
/// because BM25's incremental cost is negligible once the graph is
/// loaded. No new model. 8 GB envelope is preserved.
///
/// Behind `dense-retrieval` feature for symmetry with the dense+RRF
/// callee; default builds use plain `retrieve_files`.
#[cfg(feature = "dense-retrieval")]
#[allow(clippy::too_many_arguments)]
pub fn retrieve_files_blended_rrf(
    graph: &CodeGraph,
    communities: &[Community],
    tantivy_index: &crate::tantivy_search::FileTantivyIndex,
    embedder: &crate::embedding::neural::NeuralEmbedder,
    cache: &crate::embedding::cache::EmbeddingCache,
    query: &str,
    config: &RerankConfig,
    previously_seen: &HashSet<String>,
) -> FileRetrievalResult {
    // RRF k-parameter: cycle-12 empirical optimum on this corpus.
    const RRF_K: f64 = 20.0;

    // Stage 1: compute both source rankings.
    let bm25_result = retrieve_files(graph, communities, query, config, previously_seen);
    let dense_result = retrieve_files_dense_rrf(
        graph,
        communities,
        tantivy_index,
        embedder,
        cache,
        query,
        config,
        previously_seen,
    );

    // Stage 2: build per-source rank maps. Rank is 1-based; absent
    // files contribute zero to the fused score.
    let bm25_ranks: HashMap<&str, usize> = bm25_result
        .primary_files
        .iter()
        .enumerate()
        .map(|(i, r)| (r.path.as_str(), i + 1))
        .collect();
    let dense_ranks: HashMap<&str, usize> = dense_result
        .primary_files
        .iter()
        .enumerate()
        .map(|(i, r)| (r.path.as_str(), i + 1))
        .collect();

    // Stage 3: union of paths from both sources, RRF-fused score.
    let mut fused_scores: HashMap<String, f64> = HashMap::new();
    for (path, rank) in &bm25_ranks {
        let entry = fused_scores.entry((*path).to_string()).or_insert(0.0);
        *entry += 1.0 / (RRF_K + (*rank as f64));
    }
    for (path, rank) in &dense_ranks {
        let entry = fused_scores.entry((*path).to_string()).or_insert(0.0);
        *entry += 1.0 / (RRF_K + (*rank as f64));
    }

    let total_candidates = fused_scores.len();

    // Stage 4: sort fused scores descending and build RankedFile list.
    let mut sorted: Vec<(String, f64)> = fused_scores.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted.truncate(config.max_candidates);

    // Ghost-path filter (mirror of `retrieve_files_dense_rrf`).
    let before_filter = sorted.len();
    sorted.retain(|(p, _)| graph.get_node(&format!("file:{p}")).is_some());
    let dropped_ghost_paths = before_filter - sorted.len();

    let mut ranked: Vec<RankedFile> = sorted
        .into_iter()
        .map(|(path, score)| RankedFile {
            path,
            score,
            // RRF over two pipelines — neither single signal label
            // captures it cleanly. Re-use DenseRrf for explainability
            // since one of the inputs is dense+RRF.
            signals: vec![(Signal::DenseRrf, score)],
        })
        .collect();
    ranked.truncate(config.top_k);

    // Stage 4.5: harm filter (same heuristics).
    let harm_removals = apply_harm_filter(&mut ranked, graph);

    // Stage 5: graph expansion from top files (Calls + Imports, depth=1).
    let seed_files: Vec<String> =
        ranked.iter().take(5).map(|r| r.path.clone()).collect();
    let (expanded_files, expanded_tests) =
        expand_from_files(graph, &seed_files, config.max_neighbors);

    FileRetrievalResult {
        primary_files: ranked,
        expanded_files,
        expanded_tests,
        total_candidates,
        dropped_ghost_paths,
        harm_removals,
        compression_savings_tokens: 0,
        inline_slices: Vec::new(),
        query_type: classify(query),
        memory_cards: Vec::new(),
    }
}

/// Cycle 16 — Score-blended retrieval via meta-RRF over Dense+RRF + graph proximity.
///
/// Cycle 15 falsified naive meta-RRF over BM25 + Dense+RRF because
/// BM25 lives inside both inputs (Dense+RRF is itself a 3-ranker
/// RRF over BM25 + Tantivy + Dense), so the meta-fusion
/// double-counted BM25 and regressed all four metrics. Cycle 16
/// hypothesized that fusing Dense+RRF with a **purely structural**
/// signal — graph proximity from Dense+RRF's top-K seeds — would
/// avoid the double-counting because the proximity signal uses
/// zero shared tokens or embeddings; it is pure Calls/Imports
/// topology.
///
/// **EMPIRICAL FINDING (cycle 16, 2026-04-30):** the proximity
/// blend **regresses every metric substantially** versus Dense+RRF
/// k=20 alone — and worse than cycle-15's blended-RRF:
///
/// | Metric | Proximity-blended (this fn) | Dense+RRF k=20 | Δ |
/// |---|---|---|---|
/// | MRR | 0.626 | 0.674 | −0.048 |
/// | R@5 | 0.469 | 0.507 | −0.038 |
/// | R@10 | 0.519 | 0.577 | −0.058 |
/// | nDCG@5 | 0.456 | 0.495 | −0.039 |
///
/// **Root cause:** structural proximity is **not a query-relevance
/// signal**, it is a "this file is connected" signal. The BFS over
/// 2 hops from top-5 seeds returns all neighbours regardless of
/// whether they are answer-relevant for the query, so the proximity
/// ranking is largely noise. RRF then promotes these
/// structurally-near-but-semantically-irrelevant files into the
/// top-K, displacing the real Dense+RRF answers. The independence
/// hypothesis was right (proximity ⊥ tokens), but RRF requires
/// **independent QUERY-AWARE rankings**, not just an independent
/// signal source.
///
/// **The cycle-12 Dense+RRF k=20 path remains the recommended
/// default for recall; cycle-14 routed for MRR.** This function is
/// retained as documentation that "independent signal" is necessary
/// but not sufficient for useful meta-RRF — the second ranker must
/// also be query-aware on its own (e.g., a co-change ranker seeded
/// by changed-file similarity, or an LLM reranker).
///
/// Pipeline:
///
/// 1. Compute the Dense+RRF ranking via `retrieve_files_dense_rrf`
///    (cycle-12 winner, k=20).
/// 2. Take the top-`PROXIMITY_SEED_COUNT` files as seeds.
/// 3. Run `proximity_from_seeds(graph, seeds, max_depth=2, decay=0.5)`
///    to score files within 2 graph hops of the seeds.
/// 4. Convert proximity scores to a 1-based rank.
/// 5. RRF-fuse the Dense+RRF rank and proximity rank with k=20:
///    `fused(f) = 1/(20 + dense_rank(f)) + 1/(20 + proximity_rank(f))`.
/// 6. Sort by fused score, cap at `config.top_k`, apply harm filter
///    + graph expansion (Stage 5) identically to other entry points.
///
/// Hardware envelope: identical to running `retrieve_files_dense_rrf`
/// because proximity BFS is graph-only (microseconds per query).
/// 8 GB envelope is preserved.
///
/// Behind `dense-retrieval` feature for symmetry with the dense+RRF
/// callee; default builds use plain `retrieve_files`.
#[cfg(feature = "dense-retrieval")]
#[allow(clippy::too_many_arguments)]
pub fn retrieve_files_proximity_blended(
    graph: &CodeGraph,
    communities: &[Community],
    tantivy_index: &crate::tantivy_search::FileTantivyIndex,
    embedder: &crate::embedding::neural::NeuralEmbedder,
    cache: &crate::embedding::cache::EmbeddingCache,
    query: &str,
    config: &RerankConfig,
    previously_seen: &HashSet<String>,
) -> FileRetrievalResult {
    // RRF k-parameter: cycle-12 empirical optimum on this corpus.
    const RRF_K: f64 = 20.0;
    // Top-N seeds from Dense+RRF used as starting points for the proximity walk.
    const PROXIMITY_SEED_COUNT: usize = 5;
    // BFS depth for proximity walk; depth=2 captures callers and callers-of-callers.
    const PROXIMITY_MAX_DEPTH: usize = 2;
    // Decay per hop: depth-1 = 0.5, depth-2 = 0.25, depth-3 = 0.125.
    const PROXIMITY_DECAY: f64 = 0.5;

    // Stage 1: Dense+RRF ranking (cycle-12 winner).
    let dense_result = retrieve_files_dense_rrf(
        graph,
        communities,
        tantivy_index,
        embedder,
        cache,
        query,
        config,
        previously_seen,
    );

    // Stage 2: take top-K paths as proximity seeds, transform to graph node IDs.
    let seeds: HashSet<String> = dense_result
        .primary_files
        .iter()
        .take(PROXIMITY_SEED_COUNT)
        .map(|r| format!("file:{}", r.path))
        .collect();

    // Stage 3: compute proximity scores over the call graph.
    let proximity_scores = crate::graph_attention::proximity_from_seeds(
        graph,
        &seeds,
        PROXIMITY_MAX_DEPTH,
        PROXIMITY_DECAY,
    );

    // Stage 4: build per-source rank maps. Dense+RRF rank is 1-based by primary_files
    // order; proximity rank is 1-based by descending proximity score.
    let dense_ranks: HashMap<&str, usize> = dense_result
        .primary_files
        .iter()
        .enumerate()
        .map(|(i, r)| (r.path.as_str(), i + 1))
        .collect();

    // Convert proximity node IDs (`file:<path>`) back to file paths.
    let mut proximity_sorted: Vec<(String, f64)> = proximity_scores
        .into_iter()
        .filter_map(|(node_id, score)| {
            node_id.strip_prefix("file:").map(|p| (p.to_string(), score))
        })
        .collect();
    proximity_sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let proximity_ranks: HashMap<String, usize> = proximity_sorted
        .iter()
        .enumerate()
        .map(|(i, (p, _))| (p.clone(), i + 1))
        .collect();

    // Stage 5: union of paths from both sources, RRF-fused score.
    let mut fused_scores: HashMap<String, f64> = HashMap::new();
    for (path, rank) in &dense_ranks {
        let entry = fused_scores.entry((*path).to_string()).or_insert(0.0);
        *entry += 1.0 / (RRF_K + (*rank as f64));
    }
    for (path, rank) in &proximity_ranks {
        let entry = fused_scores.entry(path.clone()).or_insert(0.0);
        *entry += 1.0 / (RRF_K + (*rank as f64));
    }

    let total_candidates = fused_scores.len();

    // Stage 6: sort fused scores descending and cap.
    let mut sorted: Vec<(String, f64)> = fused_scores.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted.truncate(config.max_candidates);

    // Ghost-path filter (mirror of `retrieve_files_dense_rrf`).
    let before_filter = sorted.len();
    sorted.retain(|(p, _)| graph.get_node(&format!("file:{p}")).is_some());
    let dropped_ghost_paths = before_filter - sorted.len();

    let mut ranked: Vec<RankedFile> = sorted
        .into_iter()
        .map(|(path, score)| RankedFile {
            path,
            score,
            // Same labelling rationale as `retrieve_files_blended_rrf`.
            signals: vec![(Signal::DenseRrf, score)],
        })
        .collect();
    ranked.truncate(config.top_k);

    // Stage 6.5: harm filter (same heuristics).
    let harm_removals = apply_harm_filter(&mut ranked, graph);

    // Stage 7: graph expansion from top files (Calls + Imports, depth=1).
    let seed_files: Vec<String> =
        ranked.iter().take(5).map(|r| r.path.clone()).collect();
    let (expanded_files, expanded_tests) =
        expand_from_files(graph, &seed_files, config.max_neighbors);

    FileRetrievalResult {
        primary_files: ranked,
        expanded_files,
        expanded_tests,
        total_candidates,
        dropped_ghost_paths,
        harm_removals,
        compression_savings_tokens: 0,
        inline_slices: Vec::new(),
        query_type: classify(query),
        memory_cards: Vec::new(),
    }
}

/// Cycle 13 — Dense+RRF + cross-encoder rerank entry point.
///
/// Extends `retrieve_files_dense_rrf` with a Stage-5 cross-encoder
/// reranker (`reranker::CrossEncoderReranker`) over the top-50 RRF
/// candidates. Uses the BGE-Base lite variant (~278 MB) by default,
/// keeping the workload under 8 GB.
///
/// Pipeline:
///   1. RRF 3-ranker (BM25 + Tantivy + Dense, k=20) → top-50 candidates
///   2. Cross-encoder rerank → top-K
///   3. Ghost-path filter, harm filter, graph expansion (same as
///      `retrieve_files_dense_rrf`)
///
/// Behind `dense-retrieval` feature.
#[cfg(feature = "dense-retrieval")]
pub fn retrieve_files_dense_rrf_with_rerank(
    graph: &CodeGraph,
    _communities: &[Community],
    tantivy_index: &crate::tantivy_search::FileTantivyIndex,
    embedder: &crate::embedding::neural::NeuralEmbedder,
    cache: &crate::embedding::cache::EmbeddingCache,
    reranker: &crate::reranker::CrossEncoderReranker,
    query: &str,
    config: &RerankConfig,
    _previously_seen: &HashSet<String>,
) -> FileRetrievalResult {
    // Stage 1: RRF candidate set (k=20 matches cycle-7 empirical optimum).
    let rrf_scores = crate::tantivy_search::hybrid_rrf_search(
        graph,
        tantivy_index,
        embedder,
        cache,
        query,
        20.0,
    );

    let total_candidates = rrf_scores.len();
    let mut sorted: Vec<(String, f64)> = rrf_scores.into_iter().collect();
    sorted.sort_by(|a, b| {
        b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted.truncate(50); // Cap candidate pool for reranker latency.

    // Stage 2: Cross-encoder rerank.
    let reranked_map = reranker.rerank(query, &sorted, graph, config.max_candidates);
    let mut reranked_sorted: Vec<(String, f64)> =
        reranked_map.into_iter().collect();
    reranked_sorted.sort_by(|a, b| {
        b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Stage 3: ghost-path filter.
    let before_filter = reranked_sorted.len();
    reranked_sorted.retain(|(p, _)| graph.get_node(&format!("file:{p}")).is_some());
    let dropped_ghost_paths = before_filter - reranked_sorted.len();

    // Stage 4: build RankedFile list.
    let mut ranked: Vec<RankedFile> = reranked_sorted
        .into_iter()
        .map(|(path, score)| RankedFile {
            path,
            score,
            signals: vec![(Signal::DenseRrf, score)],
        })
        .collect();
    ranked.truncate(config.top_k);

    // Stage 4.5: harm filter.
    let harm_removals = apply_harm_filter(&mut ranked, graph);

    // Stage 5: graph expansion.
    let seed_files: Vec<String> =
        ranked.iter().take(5).map(|r| r.path.clone()).collect();
    let (expanded_files, expanded_tests) =
        expand_from_files(graph, &seed_files, config.max_neighbors);

    FileRetrievalResult {
        primary_files: ranked,
        expanded_files,
        expanded_tests,
        total_candidates,
        dropped_ghost_paths,
        harm_removals,
        compression_savings_tokens: 0,
        inline_slices: Vec::new(),
        query_type: classify(query),
        memory_cards: Vec::new(),
    }
}

/// Apply the harm filter to an already-ranked list, mutating it in place.
/// Returns the number of candidates removed — caller stores in
/// `FileRetrievalResult.harm_removals` for telemetry.
fn apply_harm_filter(ranked: &mut Vec<RankedFile>, graph: &CodeGraph) -> usize {
    if ranked.is_empty() {
        return 0;
    }
    let pairs: Vec<(String, f64)> = ranked.iter().map(|r| (r.path.clone(), r.score)).collect();
    let result = crate::harm_filter::filter_harmful_chunks(&pairs, graph);
    let kept: HashSet<String> = result.kept.iter().map(|(p, _)| p.clone()).collect();
    let before = ranked.len();
    ranked.retain(|r| kept.contains(&r.path));
    before - ranked.len()
}

// ---------------------------------------------------------------------------
// Internal: Candidate
// ---------------------------------------------------------------------------

struct CandidateFile {
    path: String,
    bm25_score: f64,
    community_score: f64,
    from_community: bool,
}

// ---------------------------------------------------------------------------
// Internal: Community scoring
// ---------------------------------------------------------------------------

/// Score communities by aggregating file BM25 scores of their members.
fn score_communities_for_query(
    file_scores: &HashMap<String, f64>,
    communities: &[Community],
) -> Vec<(String, f64)> {
    let mut scored: Vec<(String, f64)> = communities
        .iter()
        .map(|c| {
            let comm_score: f64 = c
                .node_ids
                .iter()
                .filter_map(|nid| {
                    // node_ids are "file:path" or "sym:..." — extract file paths
                    nid.strip_prefix("file:").and_then(|p| file_scores.get(p))
                })
                .sum();
            (c.id.clone(), comm_score)
        })
        .filter(|(_, score)| *score > 0.0)
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

/// Flatten top communities into individual file paths with community score.
fn flatten_top_communities(
    community_scores: &[(String, f64)],
    communities: &[Community],
    max_per_community: usize,
) -> Vec<(String, f64)> {
    let comm_map: HashMap<&str, &Community> =
        communities.iter().map(|c| (c.id.as_str(), c)).collect();

    let mut files = Vec::new();
    for (comm_id, score) in community_scores.iter().take(5) {
        if let Some(comm) = comm_map.get(comm_id.as_str()) {
            let file_paths: Vec<&str> = comm
                .node_ids
                .iter()
                .filter_map(|nid| nid.strip_prefix("file:"))
                .take(max_per_community)
                .collect();
            for path in file_paths {
                files.push((path.to_string(), *score));
            }
        }
    }
    files
}

// ---------------------------------------------------------------------------
// Internal: Reranker (6 features)
// ---------------------------------------------------------------------------

fn rerank_file(
    candidate: &CandidateFile,
    graph: &CodeGraph,
    query_tokens: &HashSet<String>,
    previously_seen: &HashSet<String>,
    config: &RerankConfig,
) -> RankedFile {
    let mut signals = Vec::new();

    // Feature 1: BM25 content score (normalized)
    let bm25_content = candidate.bm25_score;
    signals.push((Signal::Bm25Content, bm25_content));

    // Feature 2: BM25 path match
    let path_tokens: HashSet<String> = tokenise(&candidate.path).into_iter().collect();
    let path_overlap =
        query_tokens.intersection(&path_tokens).count() as f64 / query_tokens.len().max(1) as f64;
    signals.push((Signal::Bm25Path, path_overlap));

    // Feature 3: Symbol overlap
    let file_id = format!("file:{}", candidate.path);
    let symbol_overlap = compute_symbol_overlap(graph, &file_id, query_tokens);
    signals.push((Signal::SymbolMatch, symbol_overlap));

    // Feature 4: Graph proximity (avg degree of file / max degree)
    let graph_proximity = compute_graph_proximity(graph, &file_id);
    signals.push((Signal::GraphProximity, graph_proximity));

    // Feature 5: Already-seen penalty
    let seen_penalty = if previously_seen.contains(&candidate.path) {
        1.0
    } else {
        0.0
    };
    signals.push((Signal::AlreadySeenPenalty, seen_penalty));

    // Feature 6: Redundancy penalty (community overlap with already-selected)
    let redundancy = if candidate.from_community && candidate.bm25_score == 0.0 {
        0.5
    } else {
        0.0
    };
    signals.push((Signal::RedundancyPenalty, redundancy));

    // Weighted score
    let score = config.w_bm25_content * bm25_content
        + config.w_bm25_path * path_overlap
        + config.w_symbol_overlap * symbol_overlap
        + config.w_graph_proximity * graph_proximity
        - config.p_already_seen * seen_penalty
        - config.p_redundancy * redundancy;

    RankedFile {
        path: candidate.path.clone(),
        score: score.max(0.0), // Floor: never negative
        signals,
    }
}

fn compute_symbol_overlap(graph: &CodeGraph, file_id: &str, query_tokens: &HashSet<String>) -> f64 {
    let children = graph.contains_children(file_id);
    let child_count = children.len();
    if child_count == 0 {
        return 0.0;
    }
    let mut match_count = 0;
    for child_id in children {
        if let Some(node) = graph.get_node(child_id) {
            let name_tokens: HashSet<String> = tokenise(&node.name).into_iter().collect();
            if !query_tokens.is_disjoint(&name_tokens) {
                match_count += 1;
            }
        }
    }
    match_count as f64 / child_count.max(1) as f64
}

fn compute_graph_proximity(graph: &CodeGraph, file_id: &str) -> f64 {
    // Proxy: number of outgoing edges from this file's symbols / max reasonable
    let mut total_edges = 0;
    for child_id in graph.contains_children(file_id) {
        total_edges += graph.neighbors(child_id).len();
    }
    // Normalize: 20 edges = score 1.0
    (total_edges as f64 / 20.0).min(1.0)
}

// ---------------------------------------------------------------------------
// Internal: Graph expansion
// ---------------------------------------------------------------------------

fn expand_from_files(
    graph: &CodeGraph,
    seed_files: &[String],
    max_neighbors: usize,
) -> (Vec<String>, Vec<String>) {
    let seed_set: HashSet<&str> = seed_files.iter().map(|s| s.as_str()).collect();
    let mut expanded_files: Vec<String> = Vec::new();
    let mut expanded_tests: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for seed in seed_files {
        let file_id = format!("file:{}", seed);

        // Get symbols in this file
        for child_id in graph.contains_children(&file_id) {
            // Follow Calls + Imports edges (1-hop)
            for neighbor_id in graph.neighbors(child_id) {
                if let Some(neighbor) = graph.get_node(neighbor_id) {
                    // Find the file that contains this neighbor
                    if let Some(ref file_path) = neighbor.file_path
                        && !seed_set.contains(file_path.as_str()) && seen.insert(file_path.clone())
                        {
                            if neighbor.node_type == NodeType::Test {
                                expanded_tests.push(file_path.clone());
                            } else {
                                expanded_files.push(file_path.clone());
                            }
                        }
                }
            }

            if expanded_files.len() + expanded_tests.len() >= max_neighbors {
                break;
            }
        }

        if expanded_files.len() + expanded_tests.len() >= max_neighbors {
            break;
        }
    }

    expanded_files.truncate(max_neighbors);
    expanded_tests.truncate(5);
    (expanded_files, expanded_tests)
}

// ---------------------------------------------------------------------------
// Assembly helper: convert FileRetrievalResult to ContextBlocks
// ---------------------------------------------------------------------------

/// Build context blocks from file retrieval results.
///
/// For each ranked file: extract symbol signatures as content.
/// For expanded files: lighter content (just path + top symbols).
///
/// This thin wrapper keeps the pre-PLAN_CONTEXT_WIRING interface intact.
/// Callers that can supply a workspace root should use
/// [`build_context_blocks_with_compression`] for the Phase 2 path that
/// compresses primary-file sources via `code_compression::compress_for_context`.
pub fn build_context_blocks(
    result: &FileRetrievalResult,
    graph: &CodeGraph,
    budget_tokens: usize,
) -> Vec<theo_domain::graph_context::ContextBlock> {
    let (blocks, _savings) =
        build_context_blocks_with_compression(result, graph, budget_tokens, None, "");
    blocks
}

/// Mutating sibling of `build_context_blocks_with_compression` —
/// populates `result.compression_savings_tokens` with the per-call
/// savings (PLAN_CONTEXT_WIRING Task 2.4 — counter exposed on the
/// struct, not just as a tuple return). Preferred entry point for
/// callers that want telemetry on the result without juggling the
/// extra return value.
pub fn build_context_blocks_with_compression_mut(
    result: &mut FileRetrievalResult,
    graph: &CodeGraph,
    budget_tokens: usize,
    workspace_root: Option<&std::path::Path>,
    query: &str,
) -> Vec<theo_domain::graph_context::ContextBlock> {
    let (blocks, savings) = build_context_blocks_with_compression(
        result,
        graph,
        budget_tokens,
        workspace_root,
        query,
    );
    result.compression_savings_tokens = savings;
    blocks
}

/// Build context blocks with optional source-compression for primary files
/// (PLAN_CONTEXT_WIRING Phase 2).
///
/// When `workspace_root` is `Some`, each primary file's source is read from
/// disk, parsed via Tree-Sitter, and passed through
/// `code_compression::compress_for_context` — query-relevant symbols keep
/// their full bodies, others collapse to signatures. Savings = original −
/// compressed tokens, reported back as the second return value so callers
/// can attach it to `FileRetrievalResult.compression_savings_tokens`.
///
/// When `workspace_root` is `None` (or any path-read fails), falls back to
/// the pre-Phase-2 behaviour of concatenating each child's signature —
/// identical output to `build_context_blocks`.
pub fn build_context_blocks_with_compression(
    result: &FileRetrievalResult,
    graph: &CodeGraph,
    budget_tokens: usize,
    workspace_root: Option<&std::path::Path>,
    query: &str,
) -> (Vec<theo_domain::graph_context::ContextBlock>, usize) {
    let mut blocks = Vec::new();
    let mut tokens_used = 0;
    let mut compression_savings = 0usize;
    let query_tokens: HashSet<String> = tokenise(query).into_iter().collect();

    // PLAN_CONTEXT_WIRING Phase 3 — inline slices first.
    // A slice bundles a focal symbol with its callees/callers already
    // resolved, so it deserves the highest score (1.0). When a slice
    // exists for a primary file, the primary loop below skips that file
    // to avoid double counting (mutual exclusion with reverse boost).
    let inline_focal_files: HashSet<String> = result
        .inline_slices
        .iter()
        .map(|s| s.focal_file.clone())
        .collect();

    for slice in &result.inline_slices {
        if tokens_used + slice.token_count > budget_tokens {
            break;
        }
        blocks.push(theo_domain::graph_context::ContextBlock {
            block_id: format!("blk-inline-{}", slice.focal_symbol_id),
            source_id: slice.focal_file.clone(),
            content: slice.content.clone(),
            token_count: slice.token_count,
            score: 1.0,
        });
        tokens_used += slice.token_count;
    }

    // Primary files: full signature content (or compressed source when
    // workspace_root is available and parsing succeeds).
    for ranked in &result.primary_files {
        // Skip primary files that already appear in an inline slice —
        // avoids the reverse-dependency-boost double counting flagged
        // by the original plan (PLAN_CONTEXT_WIRING line 249-257).
        if inline_focal_files.contains(&ranked.path) {
            continue;
        }
        let file_id = format!("file:{}", ranked.path);
        let (content, token_count, saved) = match workspace_root {
            Some(root) => compress_primary_or_fallback(
                root,
                ranked,
                &file_id,
                graph,
                &query_tokens,
            ),
            None => fallback_signatures_only(&ranked.path, &file_id, graph),
        };
        compression_savings += saved;

        if tokens_used + token_count > budget_tokens {
            break;
        }

        blocks.push(theo_domain::graph_context::ContextBlock {
            block_id: format!("blk-file-{}", ranked.path.replace('/', "-")),
            source_id: ranked.path.clone(),
            content,
            token_count,
            score: ranked.score,
        });
        tokens_used += token_count;
    }

    // Expanded files: lighter content (path + symbol names only)
    for path in &result.expanded_files {
        let file_id = format!("file:{}", path);
        let mut content = format!("## {} (related)\n", path);

        let children = graph.contains_children(&file_id);
        for child_id in children.iter().take(5) {
            if let Some(node) = graph.get_node(child_id) {
                content.push_str(&format!("  {}\n", node.name));
            }
        }

        let token_count = content.len().div_ceil(4);
        if tokens_used + token_count > budget_tokens {
            break;
        }

        blocks.push(theo_domain::graph_context::ContextBlock {
            block_id: format!("blk-exp-{}", path.replace('/', "-")),
            source_id: path.clone(),
            content,
            token_count,
            score: 0.3, // Lower score for expanded files
        });
        tokens_used += token_count;
    }

    // Expanded tests
    for path in &result.expanded_tests {
        let content = format!("## {} (test)\n", path);
        let token_count = content.len().div_ceil(4);
        if tokens_used + token_count > budget_tokens {
            break;
        }

        blocks.push(theo_domain::graph_context::ContextBlock {
            block_id: format!("blk-test-{}", path.replace('/', "-")),
            source_id: path.clone(),
            content,
            token_count,
            score: 0.2,
        });
        tokens_used += token_count;
    }

    (blocks, compression_savings)
}

/// Fallback: concatenate child-symbol signatures (pre-Phase-2 behaviour).
fn fallback_signatures_only(
    path: &str,
    file_id: &str,
    graph: &CodeGraph,
) -> (String, usize, usize) {
    let mut content = format!("## {}\n", path);
    for child_id in graph.contains_children(file_id) {
        if let Some(node) = graph.get_node(child_id)
            && let Some(ref sig) = node.signature {
                content.push_str(sig);
                content.push('\n');
            }
    }
    let token_count = content.len().div_ceil(4);
    (content, token_count, 0)
}

/// Try to read + compress `ranked.path` from disk. On any failure
/// (fs read / language detection / parser / no symbols) falls back
/// to `fallback_signatures_only` — graceful degradation.
fn compress_primary_or_fallback(
    workspace_root: &std::path::Path,
    ranked: &RankedFile,
    file_id: &str,
    graph: &CodeGraph,
    query_tokens: &HashSet<String>,
) -> (String, usize, usize) {
    use theo_engine_parser::extractors::symbols::extract_symbols;
    use theo_engine_parser::tree_sitter::{detect_language, parse_source};

    let full_path = workspace_root.join(&ranked.path);
    let Ok(source) = std::fs::read_to_string(&full_path) else {
        return fallback_signatures_only(&ranked.path, file_id, graph);
    };
    let Some(language) = detect_language(&full_path) else {
        return fallback_signatures_only(&ranked.path, file_id, graph);
    };
    let Ok(parsed) = parse_source(&full_path, &source, language, None) else {
        return fallback_signatures_only(&ranked.path, file_id, graph);
    };
    let symbols = extract_symbols(&source, &parsed.tree, language, &full_path);
    if symbols.is_empty() {
        return fallback_signatures_only(&ranked.path, file_id, graph);
    }

    // Relevant symbols = symbols whose name (case-insensitive) matches any
    // query token. Keeps them in full body; others get signature-only.
    let mut relevant: HashSet<String> = HashSet::new();
    for sym in &symbols {
        if query_tokens.contains(&sym.name.to_lowercase()) {
            relevant.insert(sym.name.clone());
        }
    }

    let compressed = theo_engine_parser::code_compression::compress_for_context(
        &source,
        &symbols,
        &relevant,
        &ranked.path,
    );
    let savings = compressed
        .original_tokens
        .saturating_sub(compressed.compressed_tokens);
    (compressed.text, compressed.compressed_tokens, savings)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "file_retriever_tests.rs"]
mod tests;
