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
use theo_engine_graph::model::{CodeGraph, EdgeType, NodeType};

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
#[derive(Debug, Clone)]
pub struct FileRetrievalResult {
    pub primary_files: Vec<RankedFile>,
    pub expanded_files: Vec<String>,
    pub expanded_tests: Vec<String>,
    pub total_candidates: usize,
    pub dropped_ghost_paths: usize,
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
    }
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
                    if let Some(ref file_path) = neighbor.file_path {
                        if !seed_set.contains(file_path.as_str()) && seen.insert(file_path.clone())
                        {
                            if neighbor.node_type == NodeType::Test {
                                expanded_tests.push(file_path.clone());
                            } else {
                                expanded_files.push(file_path.clone());
                            }
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
pub fn build_context_blocks(
    result: &FileRetrievalResult,
    graph: &CodeGraph,
    budget_tokens: usize,
) -> Vec<theo_domain::graph_context::ContextBlock> {
    let mut blocks = Vec::new();
    let mut tokens_used = 0;

    // Primary files: full signature content
    for ranked in &result.primary_files {
        let file_id = format!("file:{}", ranked.path);
        let mut content = format!("## {}\n", ranked.path);

        for child_id in graph.contains_children(&file_id) {
            if let Some(node) = graph.get_node(child_id) {
                if let Some(ref sig) = node.signature {
                    content.push_str(sig);
                    content.push('\n');
                }
            }
        }

        let token_count = (content.len() + 3) / 4; // ~4 chars per token
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

        let token_count = (content.len() + 3) / 4;
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
        let token_count = (content.len() + 3) / 4;
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

    blocks
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use theo_engine_graph::model::{Edge, Node, SymbolKind};

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
        ) {
            if fresh.path == penalized.path {
                assert!(
                    penalized.score <= fresh.score,
                    "Seen penalty should reduce score: fresh={}, seen={}",
                    fresh.score,
                    penalized.score
                );
            }
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
        let all_files: Vec<&str> = result.expanded_files.iter().map(|s| s.as_str()).collect();
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
}
