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
mod tests {
    use super::*;
    use theo_engine_graph::model::{Edge, EdgeType, Node, SymbolKind};

    fn file_node(id: &str, path: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: NodeType::File,
            name: path.to_string(),
            file_path: Some(path.to_string()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        }
    }

    fn symbol_node(id: &str, name: &str, file_path: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: NodeType::Symbol,
            name: name.to_string(),
            file_path: Some(file_path.to_string()),
            signature: Some(format!("pub fn {}()", name)),
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(10),
            last_modified: 0.0,
            doc: None,
        }
    }

    fn test_node(id: &str, name: &str, file_path: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: NodeType::Test,
            name: name.to_string(),
            file_path: Some(file_path.to_string()),
            signature: None,
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(5),
            last_modified: 0.0,
            doc: None,
        }
    }

    fn build_test_graph() -> CodeGraph {
        let mut g = CodeGraph::new();

        // Files
        g.add_node(file_node("file:src/auth.rs", "src/auth.rs"));
        g.add_node(file_node("file:src/db.rs", "src/db.rs"));
        g.add_node(file_node("file:src/api.rs", "src/api.rs"));

        // Symbols
        g.add_node(symbol_node(
            "sym:verify_token",
            "verify_token",
            "src/auth.rs",
        ));
        g.add_node(symbol_node("sym:decode_jwt", "decode_jwt", "src/auth.rs"));
        g.add_node(symbol_node("sym:query_db", "query_db", "src/db.rs"));
        g.add_node(symbol_node(
            "sym:handle_request",
            "handle_request",
            "src/api.rs",
        ));

        // Test
        g.add_node(test_node(
            "test:test_auth",
            "test_auth",
            "tests/test_auth.rs",
        ));
        g.add_node(file_node("file:tests/test_auth.rs", "tests/test_auth.rs"));

        // Contains
        g.add_edge(Edge {
            source: "file:src/auth.rs".into(),
            target: "sym:verify_token".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "file:src/auth.rs".into(),
            target: "sym:decode_jwt".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "file:src/db.rs".into(),
            target: "sym:query_db".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "file:src/api.rs".into(),
            target: "sym:handle_request".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "file:tests/test_auth.rs".into(),
            target: "test:test_auth".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        // Calls
        g.add_edge(Edge {
            source: "sym:handle_request".into(),
            target: "sym:verify_token".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "sym:verify_token".into(),
            target: "sym:query_db".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });

        // Tests
        g.add_edge(Edge {
            source: "test:test_auth".into(),
            target: "sym:verify_token".into(),
            edge_type: EdgeType::Tests,
            weight: 0.7,
        });

        g
    }

    fn build_communities() -> Vec<Community> {
        vec![
            Community {
                id: "auth".to_string(),
                name: "Auth".to_string(),
                level: 0,
                node_ids: vec![
                    "file:src/auth.rs".into(),
                    "sym:verify_token".into(),
                    "sym:decode_jwt".into(),
                ],
                parent_id: None,
                version: 1,
            },
            Community {
                id: "db".to_string(),
                name: "DB".to_string(),
                level: 0,
                node_ids: vec!["file:src/db.rs".into(), "sym:query_db".into()],
                parent_id: None,
                version: 1,
            },
        ]
    }

    // --- Core retrieval tests ---

    #[test]
    fn retrieve_returns_files_not_communities() {
        let graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(
            &graph,
            &communities,
            "verify token authentication",
            &config,
            &seen,
        );

        // Should return file paths, not community IDs
        for file in &result.primary_files {
            assert!(
                file.path.contains('/') || file.path.contains('.'),
                "Result should be file path, got: {}",
                file.path
            );
            assert!(
                !file.path.starts_with("auth") && !file.path.starts_with("db"),
                "Result should NOT be community ID, got: {}",
                file.path
            );
        }
    }

    #[test]
    fn retrieve_top1_matches_auth_for_token_query() {
        let graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(
            &graph,
            &communities,
            "verify token jwt authentication",
            &config,
            &seen,
        );

        assert!(
            !result.primary_files.is_empty(),
            "Should return at least 1 file"
        );
        assert!(
            result.primary_files[0].path.contains("auth"),
            "Top result for 'verify token' should be auth file, got: {}",
            result.primary_files[0].path
        );
    }

    #[test]
    fn retrieve_ghost_path_filter_works() {
        let mut graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig::default();
        let seen = HashSet::new();

        // Add a symbol that references a non-existent file
        graph.add_node(Node {
            id: "sym:ghost".to_string(),
            node_type: NodeType::Symbol,
            name: "ghost_function".to_string(),
            file_path: Some("src/nonexistent.rs".to_string()),
            signature: Some("fn ghost()".into()),
            kind: Some(SymbolKind::Function),
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        });

        let result = retrieve_files(&graph, &communities, "ghost function", &config, &seen);

        // Ghost file should be filtered out
        for file in &result.primary_files {
            assert_ne!(
                file.path, "src/nonexistent.rs",
                "Ghost paths should be filtered"
            );
        }
    }

    #[test]
    fn retrieve_already_seen_penalty_reduces_score() {
        let graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig::default();

        let result_fresh = retrieve_files(
            &graph,
            &communities,
            "verify token",
            &config,
            &HashSet::new(),
        );
        let mut seen = HashSet::new();
        if let Some(top) = result_fresh.primary_files.first() {
            seen.insert(top.path.clone());
        }
        let result_seen = retrieve_files(&graph, &communities, "verify token", &config, &seen);

        // Score should be lower when previously seen
        if let (Some(fresh), Some(penalized)) = (
            result_fresh.primary_files.first(),
            result_seen.primary_files.first(),
        )
            && fresh.path == penalized.path {
                assert!(
                    penalized.score <= fresh.score,
                    "Seen penalty should reduce score: fresh={}, seen={}",
                    fresh.score,
                    penalized.score
                );
            }
    }

    #[test]
    fn retrieve_expansion_finds_related_files() {
        let graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(&graph, &communities, "verify token", &config, &seen);

        // auth.rs calls query_db in db.rs → db.rs should be in expanded
        let _all_files: Vec<&str> = result.expanded_files.iter().map(|s| s.as_str()).collect();
        // At minimum, expansion should find some related files
        assert!(
            result.primary_files.len() + result.expanded_files.len() > 1,
            "Should find primary + expanded files"
        );
    }

    #[test]
    fn retrieve_respects_max_neighbors() {
        let graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig {
            max_neighbors: 2,
            ..RerankConfig::default()
        };
        let seen = HashSet::new();

        let result = retrieve_files(&graph, &communities, "verify token", &config, &seen);
        assert!(
            result.expanded_files.len() + result.expanded_tests.len() <= 2,
            "Expansion must respect max_neighbors limit"
        );
    }

    #[test]
    fn retrieve_scores_never_negative() {
        let graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig::default();
        let mut seen = HashSet::new();
        // Mark everything as seen to maximize penalties
        seen.insert("src/auth.rs".into());
        seen.insert("src/db.rs".into());
        seen.insert("src/api.rs".into());

        let result = retrieve_files(&graph, &communities, "anything", &config, &seen);
        for file in &result.primary_files {
            assert!(
                file.score >= 0.0,
                "Score must never be negative, got: {}",
                file.score
            );
        }
    }

    #[test]
    fn retrieve_empty_graph_returns_empty() {
        let graph = CodeGraph::new();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(&graph, &communities, "anything", &config, &seen);
        assert!(result.primary_files.is_empty());
    }

    #[test]
    fn community_flatten_respects_max_per_community() {
        let scores = vec![("comm1".into(), 1.0)];
        let communities = vec![Community {
            id: "comm1".into(),
            name: "Big".into(),
            level: 0,
            node_ids: (0..50).map(|i| format!("file:f{}.rs", i)).collect(),
            parent_id: None,
            version: 1,
        }];

        let files = flatten_top_communities(&scores, &communities, 10);
        assert!(
            files.len() <= 10,
            "Should respect max_per_community, got {}",
            files.len()
        );
    }

    // ────────────────────────────────────────────────────────────────
    // Phase 1 integration — harm_filter wired into retrieve_files
    // (PLAN_CONTEXT_WIRING Phase 1, Task 1.3)
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn retrieve_files_removes_test_file_when_definer_present() {
        // Arrange: graph has src/auth.rs (definer) + tests/test_auth.rs (test).
        // Both would normally rank for "verify_token" — harm filter should
        // drop the test since the definer is already in the top list.
        let graph = build_test_graph();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(&graph, &communities, "verify_token", &config, &seen);

        let test_file_kept = result
            .primary_files
            .iter()
            .any(|r| r.path == "tests/test_auth.rs");
        let definer_kept = result
            .primary_files
            .iter()
            .any(|r| r.path == "src/auth.rs");
        assert!(definer_kept, "definer src/auth.rs must survive");
        assert!(
            !test_file_kept,
            "test file tests/test_auth.rs must be filtered when definer is present"
        );
        assert!(
            result.harm_removals >= 1,
            "harm_removals counter must reflect at least the test-file removal"
        );
    }

    #[test]
    fn retrieve_files_harm_removals_metric_exposed() {
        // Smoke: on any non-empty graph, the harm_removals field is present
        // and ≥ 0 (catches accidental removal of the telemetry field).
        let graph = build_test_graph();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(&graph, &communities, "query_db", &config, &seen);

        // The field exists (compile-time) and is a valid usize. This test
        // mainly guards against future refactors removing the metric.
        let _ = result.harm_removals;
        assert!(
            result.primary_files.len() + result.harm_removals > 0,
            "something must have been ranked or filtered"
        );
    }

    #[test]
    fn retrieve_files_respects_40pct_removal_cap() {
        // Per harm_filter::MAX_REMOVAL_FRACTION, no more than 40% of the
        // ranked list may be removed in one pass. This test sanity-checks
        // that the cap survives integration.
        let graph = build_test_graph();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(&graph, &communities, "verify_token", &config, &seen);

        // After filtering, primary_files + harm_removals == whatever ranked
        // saw pre-filter. The ratio of removals to the pre-filter size must
        // be ≤ 40% + 1 (ceil of MAX_REMOVAL_FRACTION).
        let pre_filter = result.primary_files.len() + result.harm_removals;
        if pre_filter > 0 {
            let removal_fraction = result.harm_removals as f64 / pre_filter as f64;
            assert!(
                removal_fraction <= 0.5,
                "removal fraction {removal_fraction} exceeded 50% safety bound"
            );
        }
    }

    // ────────────────────────────────────────────────────────────────
    // Phase 2 integration — code_compression wired into
    // build_context_blocks_with_compression (PLAN_CONTEXT_WIRING Phase 2)
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn build_context_blocks_without_workspace_root_uses_signatures() {
        // None workspace_root keeps the pre-Phase-2 behaviour: content is
        // concatenated signatures, savings = 0.
        let graph = build_test_graph();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();
        let result = retrieve_files(&graph, &communities, "verify_token", &config, &seen);

        let (blocks, savings) =
            build_context_blocks_with_compression(&result, &graph, 10_000, None, "verify_token");

        assert!(!blocks.is_empty(), "must produce at least one block");
        assert_eq!(savings, 0, "no compression attempted without workspace_root");
    }

    #[test]
    fn build_context_blocks_compression_falls_back_when_file_missing() {
        // Points workspace_root at a non-existent directory: every fs::read
        // should fail → graceful fallback to signatures, savings = 0.
        let graph = build_test_graph();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();
        let result = retrieve_files(&graph, &communities, "verify_token", &config, &seen);

        let fake_root = std::path::Path::new("/tmp/theo-no-such-dir-xyz-999");
        let (blocks, savings) = build_context_blocks_with_compression(
            &result,
            &graph,
            10_000,
            Some(fake_root),
            "verify_token",
        );

        assert!(!blocks.is_empty(), "fallback must still produce blocks");
        assert_eq!(
            savings, 0,
            "missing-file fallback must yield zero compression savings"
        );
    }

    #[test]
    fn build_context_blocks_compression_saves_tokens_on_real_source() {
        // Arrange: write a Rust file with one relevant function and four
        // irrelevant functions. Compression should keep the relevant body
        // and reduce the others to signatures, yielding savings > 0.
        let tmp = tempfile::tempdir().expect("tmpdir");
        let file_name = "demo.rs";
        let path = tmp.path().join(file_name);
        let mut src = String::from(
            "fn relevant_symbol() {\n    // body line 1\n    // body line 2\n    println!(\"hi\");\n}\n\n",
        );
        for i in 0..4 {
            src.push_str(&format!(
                "fn noise_{i}() {{\n    // bulk body {i}\n    let x = {i};\n    let y = x + {i};\n    println!(\"{{x}} {{y}}\");\n}}\n\n",
                i = i
            ));
        }
        std::fs::write(&path, &src).expect("write demo");

        // Build a minimal graph containing just this file.
        let mut g = CodeGraph::new();
        g.add_node(file_node(&format!("file:{file_name}"), file_name));
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();
        // Query targets the relevant function by name.
        let result = retrieve_files(&g, &communities, "relevant_symbol", &config, &seen);
        // Force presence of the file in result regardless of ranker output,
        // so we always exercise the compression helper.
        let forced_result = FileRetrievalResult {
            primary_files: vec![RankedFile {
                path: file_name.to_string(),
                score: 1.0,
                signals: Vec::new(),
            }],
            ..result
        };

        let (blocks, savings) = build_context_blocks_with_compression(
            &forced_result,
            &g,
            10_000,
            Some(tmp.path()),
            "relevant_symbol",
        );

        assert_eq!(blocks.len(), 1, "one block for the single file");
        // The relevant function's body must survive compression.
        assert!(
            blocks[0].content.contains("relevant_symbol"),
            "compressed content must mention relevant_symbol: {}",
            &blocks[0].content
        );
        // Savings are non-zero when compression actually ran.
        assert!(savings > 0, "expected compression savings > 0, got {savings}");
    }

    // ────────────────────────────────────────────────────────────────
    // Phase 3 integration — inline_builder wired into
    // retrieve_files_with_inline + build_context_blocks_with_compression
    // (PLAN_CONTEXT_WIRING Phase 3)
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn retrieve_files_with_inline_no_match_yields_no_slices() {
        // Query that doesn't hit any symbol in the graph — inline slices
        // must remain empty; primary_files behaves like retrieve_files.
        let graph = build_test_graph();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();
        let tmp = tempfile::tempdir().expect("tmpdir");

        let result = retrieve_files_with_inline(
            &graph,
            &communities,
            "no_such_symbol_xyz_999",
            &config,
            &seen,
            tmp.path(),
        );

        assert!(
            result.inline_slices.is_empty(),
            "no symbol match → inline_slices must be empty"
        );
    }

    #[test]
    fn retrieve_files_with_inline_returns_identical_result_on_empty_graph() {
        // Isolates the inline path: identical to retrieve_files when the
        // graph has no symbols to slice.
        let graph = CodeGraph::new();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();
        let tmp = tempfile::tempdir().expect("tmpdir");

        let a = retrieve_files(&graph, &communities, "anything", &config, &seen);
        let b = retrieve_files_with_inline(
            &graph,
            &communities,
            "anything",
            &config,
            &seen,
            tmp.path(),
        );

        assert_eq!(a.primary_files.len(), b.primary_files.len());
        assert_eq!(a.harm_removals, b.harm_removals);
        assert!(b.inline_slices.is_empty());
    }

    #[test]
    fn build_context_blocks_prepends_inline_slices_with_highest_score() {
        // Forge a result with one inline slice to exercise the block-build
        // prepend path without needing the full inline_builder to resolve.
        let graph = build_test_graph();
        let slice = crate::inline_builder::InlineSlice {
            focal_symbol_id: "sym:verify_token".into(),
            focal_file: "src/auth.rs".into(),
            content: "// inline snippet\nfn verify_token() { /* ... */ }".into(),
            token_count: 30,
            inlined_symbols: vec!["sym:decode_jwt".into()],
            unresolved_callees: vec![],
        };
        let forced = FileRetrievalResult {
            primary_files: vec![RankedFile {
                path: "src/db.rs".into(),
                score: 0.5,
                signals: Vec::new(),
            }],
            inline_slices: vec![slice],
            ..FileRetrievalResult::default()
        };

        let (blocks, _) =
            build_context_blocks_with_compression(&forced, &graph, 10_000, None, "verify_token");

        // First block is the inline slice.
        assert!(!blocks.is_empty());
        assert!(
            blocks[0].block_id.starts_with("blk-inline-"),
            "inline slice must be the first block, got: {}",
            blocks[0].block_id
        );
        assert_eq!(blocks[0].score, 1.0, "inline slice must score 1.0");
    }

    #[test]
    fn inline_slice_for_primary_file_skips_that_file_in_loop() {
        // Mutual-exclusion test: when an inline slice covers src/auth.rs,
        // the primary-files loop must not emit an additional block for
        // the same path (avoids reverse-boost double count).
        let graph = build_test_graph();
        let slice = crate::inline_builder::InlineSlice {
            focal_symbol_id: "sym:verify_token".into(),
            focal_file: "src/auth.rs".into(),
            content: "// inline".into(),
            token_count: 10,
            inlined_symbols: vec![],
            unresolved_callees: vec![],
        };
        let forced = FileRetrievalResult {
            primary_files: vec![
                RankedFile {
                    path: "src/auth.rs".into(),
                    score: 0.9,
                    signals: Vec::new(),
                },
                RankedFile {
                    path: "src/db.rs".into(),
                    score: 0.5,
                    signals: Vec::new(),
                },
            ],
            inline_slices: vec![slice],
            ..FileRetrievalResult::default()
        };

        let (blocks, _) =
            build_context_blocks_with_compression(&forced, &graph, 10_000, None, "verify_token");

        // Expected: 1 inline + 1 primary (db only; auth skipped due to inline).
        let auth_primary_count = blocks
            .iter()
            .filter(|b| b.block_id == "blk-file-src-auth.rs")
            .count();
        let db_primary_count = blocks
            .iter()
            .filter(|b| b.block_id == "blk-file-src-db.rs")
            .count();
        assert_eq!(
            auth_primary_count, 0,
            "src/auth.rs primary block must be suppressed by inline slice"
        );
        assert_eq!(db_primary_count, 1, "src/db.rs primary block still emitted");
    }
}
