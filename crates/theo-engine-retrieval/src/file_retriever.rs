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

use crate::search::{FileBm25, tokenise};

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
