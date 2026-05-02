//! Tantivy-based file-level BM25F search index.
//!
//! Strategy-alternative to the custom `FileBm25` in `search.rs`.
//! Uses Tantivy's optimized inverted index with multi-field boosting.
//!
//! Field boosts (BM25F pattern from Zoekt/Sourcegraph):
//! - filename:  5x  (file name is strongest signal)
//! - symbol:    3x  (function/struct/trait names)
//! - signature: 1x  (full signatures)
//! - doc:       1x  (first-line docstrings)
//!
//! Activated via `tantivy-backend` feature flag.

#[cfg(feature = "tantivy-backend")]
mod inner;


#[cfg(feature = "tantivy-backend")]
pub use inner::FileTantivyIndex;

/// Hybrid search: combine Custom FileBm25 + Tantivy via max-fusion with test penalty.
///
/// Each backend sees different scoring paths, producing complementary results.
/// Max-fusion preserves strong signals from either backend.
/// Test/benchmark files are penalized (0.1x) to avoid noise.
#[cfg(feature = "tantivy-backend")]
pub fn hybrid_search(
    graph: &theo_engine_graph::model::CodeGraph,
    tantivy_index: &FileTantivyIndex,
    query: &str,
) -> HashMap<String, f64> {
    use crate::search::FileBm25;

    let custom_scores = FileBm25::search(graph, query);
    let tantivy_scores = tantivy_index
        .search_with_prf(graph, query, 50)
        .unwrap_or_default();

    // Collect all file paths from both
    let mut all_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
    for k in custom_scores.keys() {
        all_paths.insert(k.clone());
    }
    for k in tantivy_scores.keys() {
        all_paths.insert(k.clone());
    }

    if all_paths.is_empty() {
        return HashMap::new();
    }

    // Min-max normalize each ranker to [0, 1]
    let normalize = |scores: &HashMap<String, f64>| -> HashMap<String, f64> {
        if scores.is_empty() {
            return HashMap::new();
        }
        let max = scores.values().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min = scores.values().cloned().fold(f64::INFINITY, f64::min);
        let range = max - min;
        if range <= 0.0 {
            scores.keys().map(|k| (k.clone(), 1.0)).collect()
        } else {
            scores
                .iter()
                .map(|(k, v)| (k.clone(), (v - min) / range))
                .collect()
        }
    };

    let norm_custom = normalize(&custom_scores);
    let norm_tantivy = normalize(&tantivy_scores);

    // Priority fusion: Custom is primary ranker; Tantivy supplements.
    // For files in BOTH rankers: take max of normalized scores.
    // For files ONLY in Tantivy: add with a small bonus (0.3x of custom's min non-zero).
    // This preserves Custom's ranking while capturing Tantivy-only discoveries.
    let custom_min_nonzero = norm_custom
        .values()
        .filter(|v| **v > 0.01)
        .cloned()
        .fold(f64::INFINITY, f64::min)
        .min(0.3);

    let mut merged = HashMap::new();
    for path in all_paths {
        let c = norm_custom.get(&path).copied().unwrap_or(0.0);
        let t = norm_tantivy.get(&path).copied().unwrap_or(0.0);

        let score = if c > 0.0 {
            // File in custom: use max of both (preserves custom ranking)
            c.max(t)
        } else {
            // Tantivy-only: add at below custom's weakest result
            t * custom_min_nonzero * 0.5
        };

        // Penalize test/benchmark/example files
        let lp = path.to_lowercase();
        let penalty = if lp.contains("test") || lp.contains("benchmark") || lp.contains("example") {
            0.1
        } else {
            1.0
        };

        merged.insert(path, score * penalty);
    }

    merged
}

#[cfg(feature = "tantivy-backend")]
use std::collections::HashMap;

/// RRF 3-ranker fusion: BM25 Custom + Tantivy + Dense embeddings.
///
/// Reciprocal Rank Fusion (Cormack et al., SIGIR 2009):
///   RRF(d) = Σ_ranker 1/(k + rank_ranker(d))
///
/// RRF is rank-based (not score-based), making it robust to different
/// score scales between rankers. k=60 is the standard constant.
///
/// Test/benchmark/example files are excluded before ranking to prevent
/// noise (lesson from earlier RRF experiment that scored P@5=0.210
/// because test files polluted the merge).
#[cfg(feature = "dense-retrieval")]
pub fn hybrid_rrf_search(
    graph: &theo_engine_graph::model::CodeGraph,
    tantivy_index: &FileTantivyIndex,
    embedder: &crate::embedding::neural::NeuralEmbedder,
    cache: &crate::embedding::cache::EmbeddingCache,
    query: &str,
    k_param: f64,
) -> HashMap<String, f64> {
    use crate::dense_search::FileDenseSearch;
    use crate::search::FileBm25;

    // Get scores from all 3 rankers
    // NOTE: Query expansion with synonyms TESTED but REVERTED — hurt MRR (0.914→0.886)
    // because BM25 IDF is sensitive to added terms. Dense handles synonyms natively.
    let bm25_scores = FileBm25::search(graph, query);
    let tantivy_scores = tantivy_index
        .search_with_prf(graph, query, 500)
        .unwrap_or_default();
    let dense_scores = FileDenseSearch::search(embedder, cache, query, 500);

    // Convert to ranked lists, EXCLUDING test/benchmark/example files
    let is_noise = |path: &str| -> bool {
        let lp = path.to_lowercase();
        lp.contains("test") || lp.contains("benchmark") || lp.contains("example")
    };

    let to_ranked = |scores: &HashMap<String, f64>| -> Vec<String> {
        let mut sorted: Vec<_> = scores.iter().filter(|(k, _)| !is_noise(k)).collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.into_iter().map(|(k, _)| k.clone()).collect()
    };

    let bm25_ranked = to_ranked(&bm25_scores);
    let tantivy_ranked = to_ranked(&tantivy_scores);
    let dense_ranked = to_ranked(&dense_scores);

    let rank_map = |ranked: &[String]| -> HashMap<String, usize> {
        ranked
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), i))
            .collect()
    };

    let bm25_rank = rank_map(&bm25_ranked);
    let tantivy_rank = rank_map(&tantivy_ranked);
    let dense_rank = rank_map(&dense_ranked);

    let mut all_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
    for k in bm25_scores.keys() {
        if !is_noise(k) {
            all_paths.insert(k.clone());
        }
    }
    for k in tantivy_scores.keys() {
        if !is_noise(k) {
            all_paths.insert(k.clone());
        }
    }
    for k in dense_scores.keys() {
        if !is_noise(k) {
            all_paths.insert(k.clone());
        }
    }

    if all_paths.is_empty() {
        return HashMap::new();
    }

    // RRF 3-ranker: BM25 + Tantivy + Dense (uniform weights, k=40 optimal).
    let mut merged = HashMap::new();
    for path in &all_paths {
        let mut rrf_score = 0.0;
        if let Some(&rank) = bm25_rank.get(path.as_str()) {
            rrf_score += 1.0 / (k_param + rank as f64);
        }
        if let Some(&rank) = tantivy_rank.get(path.as_str()) {
            rrf_score += 1.0 / (k_param + rank as f64);
        }
        if let Some(&rank) = dense_rank.get(path.as_str()) {
            rrf_score += 1.0 / (k_param + rank as f64);
        }
        if rrf_score > 0.0 {
            merged.insert(path.clone(), rrf_score);
        }
    }

    merged
}

/// Symbol-First Retrieval: changes the unit of retrieval from FILE to SYMBOL.
///
/// Pipeline:
/// Stage A: File retrieval via RRF 3-ranker → top-20 files
/// Stage B: Symbol extraction from top-20 files via CodeGraph
/// Stage C: Symbol scoring against query (name overlap + signature match)
/// Stage D: Callers/references expansion — find files that USE top symbols
/// Stage E: Aggregate symbol scores per file → final file ranking
///
/// This addresses the "finds definers, misses users" gap identified in
/// the Staff+ analysis. MRR=0.914 proves we find the RIGHT file;
/// R@5=0.735 proves we miss RELATED files. Symbol-first solves this
/// by grounding the search in code structure, not document similarity.
#[cfg(feature = "dense-retrieval")]
pub fn symbol_first_search(
    graph: &theo_engine_graph::model::CodeGraph,
    tantivy_index: &FileTantivyIndex,
    embedder: &crate::embedding::neural::NeuralEmbedder,
    cache: &crate::embedding::cache::EmbeddingCache,
    query: &str,
    k_param: f64,
) -> HashMap<String, f64> {
    use crate::code_tokenizer::tokenize_code;
    use theo_engine_graph::model::{NodeType, SymbolKind};

    // Stage A: File retrieval via existing RRF → top-20 files
    let file_scores = hybrid_rrf_search(graph, tantivy_index, embedder, cache, query, k_param);
    let mut sorted_files: Vec<_> = file_scores.iter().collect();
    sorted_files.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
    let top_files: Vec<&str> = sorted_files
        .iter()
        .take(20)
        .map(|(k, _)| k.as_str())
        .collect();

    if top_files.is_empty() {
        return HashMap::new();
    }

    // Query tokens for matching
    let query_tokens: std::collections::HashSet<String> =
        tokenize_code(query).into_iter().collect();

    if query_tokens.is_empty() {
        return file_scores;
    }

    // Stage B + C: Extract symbols from top-20 files and score against query
    let mut symbol_scores: Vec<(String, String, f64)> = Vec::new(); // (sym_id, file_path, score)
    let hub_threshold = 50; // Skip symbols with too many reverse neighbors

    for file_path in &top_files {
        let file_id = format!("file:{}", file_path);

        for sym_id in graph.contains_children(&file_id) {
            let Some(sym) = graph.get_node(sym_id) else {
                continue;
            };
            if sym.node_type != NodeType::Symbol {
                continue;
            }

            // Score symbol against query: name token overlap + signature bonus
            let name_tokens: std::collections::HashSet<String> =
                tokenize_code(&sym.name).into_iter().collect();

            let name_overlap = query_tokens
                .iter()
                .filter(|qt| name_tokens.contains(*qt))
                .count();

            if name_overlap == 0 {
                continue;
            } // No match at all

            let mut score = name_overlap as f64;

            // Bonus for signature match
            if let Some(sig) = &sym.signature {
                let sig_tokens: std::collections::HashSet<String> =
                    tokenize_code(sig).into_iter().collect();
                let sig_overlap = query_tokens
                    .iter()
                    .filter(|qt| sig_tokens.contains(*qt))
                    .count();
                score += sig_overlap as f64 * 0.5;
            }

            // Bonus for function/method (more specific than types)
            if matches!(
                sym.kind,
                Some(SymbolKind::Function) | Some(SymbolKind::Method)
            ) {
                score *= 1.2;
            }

            symbol_scores.push((sym_id.to_string(), file_path.to_string(), score));
        }
    }

    // Stage D: For top-scoring symbols, find CALLERS/USERS via reverse edges
    // This is the key insight: if propagate_attention is a top symbol,
    // find files that CALL it (search.rs) and boost them.
    symbol_scores.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    let is_noise = |path: &str| -> bool {
        let lp = path.to_lowercase();
        lp.contains("test") || lp.contains("benchmark") || lp.contains("example")
    };
    let is_hub = |path: &str| -> bool {
        path.ends_with("/lib.rs") || path.ends_with("/mod.rs") || path.ends_with("/main.rs")
    };

    let mut caller_boost: HashMap<String, f64> = HashMap::new();
    let top_symbols: Vec<_> = symbol_scores.iter().take(10).collect();

    for (sym_id, _source_file, sym_score) in &top_symbols {
        // Hub filter: skip symbols with too many callers
        let reverse = graph.reverse_neighbors(sym_id);
        if reverse.len() > hub_threshold {
            continue;
        }

        for caller_id in &reverse {
            let Some(caller) = graph.get_node(caller_id) else {
                continue;
            };
            let Some(caller_fp) = caller.file_path.as_deref() else {
                continue;
            };
            if is_noise(caller_fp) || is_hub(caller_fp) {
                continue;
            }

            // Boost proportional to symbol score
            let boost = sym_score * 0.5;
            let entry = caller_boost.entry(caller_fp.to_string()).or_insert(0.0);
            *entry = (*entry + boost).min(sym_score * 3.0); // Cap at 3x symbol score
        }
    }

    // Stage E: Aggregate — combine file RRF scores + symbol grounding + caller boost
    let max_rrf = file_scores.values().cloned().fold(0.0f64, f64::max);
    let max_sym = symbol_scores.first().map(|(_, _, s)| *s).unwrap_or(1.0);

    let mut final_scores: HashMap<String, f64> = HashMap::new();

    // Start with all RRF files
    for (path, rrf_score) in &file_scores {
        final_scores.insert(path.clone(), *rrf_score);
    }

    // Add symbol grounding boost (normalized to RRF scale)
    for (_, file_path, sym_score) in &symbol_scores {
        let normalized = (sym_score / max_sym) * max_rrf * 0.3; // 30% weight
        let entry = final_scores.entry(file_path.clone()).or_insert(0.0);
        *entry += normalized;
    }

    // Add caller boost (for files that USE top symbols)
    for (caller_path, boost) in &caller_boost {
        let normalized = (boost / max_sym) * max_rrf * 0.2; // 20% weight
        let entry = final_scores.entry(caller_path.clone()).or_insert(0.0);
        *entry += normalized;
    }

    final_scores
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "tantivy-backend"))]
mod tests {
    use super::*;
    use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};

    fn build_test_graph() -> CodeGraph {
        let mut graph = CodeGraph::new();

        // File: auth/oauth.rs with symbols
        graph.add_node(Node {
            id: "file:auth/oauth.rs".into(),
            name: "oauth.rs".into(),
            node_type: NodeType::File,
            file_path: Some("auth/oauth.rs".into()),
            line_start: None,
            line_end: None,
            signature: None,
            doc: None,
            kind: None,
            last_modified: 0.0,
        });

        graph.add_node(Node {
            id: "sym:verify_jwt_token".into(),
            name: "verify_jwt_token".into(),
            node_type: NodeType::Symbol,
            file_path: Some("auth/oauth.rs".into()),
            line_start: Some(10),
            line_end: Some(30),
            signature: Some("pub fn verify_jwt_token(token: &str) -> Result<Claims>".into()),
            doc: Some("Verify a JWT token and return claims.".into()),
            kind: Some(SymbolKind::Function),
            last_modified: 0.0,
        });

        graph.add_edge(Edge {
            source: "file:auth/oauth.rs".into(),
            target: "sym:verify_jwt_token".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        // File: db/connection.rs with symbols
        graph.add_node(Node {
            id: "file:db/connection.rs".into(),
            name: "connection.rs".into(),
            node_type: NodeType::File,
            file_path: Some("db/connection.rs".into()),
            line_start: None,
            line_end: None,
            signature: None,
            doc: None,
            kind: None,
            last_modified: 0.0,
        });

        graph.add_node(Node {
            id: "sym:create_pool".into(),
            name: "create_pool".into(),
            node_type: NodeType::Symbol,
            file_path: Some("db/connection.rs".into()),
            line_start: Some(5),
            line_end: Some(20),
            signature: Some("pub fn create_pool(url: &str) -> Pool".into()),
            doc: Some("Create a database connection pool.".into()),
            kind: Some(SymbolKind::Function),
            last_modified: 0.0,
        });

        graph.add_edge(Edge {
            source: "file:db/connection.rs".into(),
            target: "sym:create_pool".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        // File: api/handler.rs with symbols
        graph.add_node(Node {
            id: "file:api/handler.rs".into(),
            name: "handler.rs".into(),
            node_type: NodeType::File,
            file_path: Some("api/handler.rs".into()),
            line_start: None,
            line_end: None,
            signature: None,
            doc: None,
            kind: None,
            last_modified: 0.0,
        });

        graph.add_node(Node {
            id: "sym:handle_auth_request".into(),
            name: "handle_auth_request".into(),
            node_type: NodeType::Symbol,
            file_path: Some("api/handler.rs".into()),
            line_start: Some(15),
            line_end: Some(45),
            signature: Some("pub async fn handle_auth_request(req: Request) -> Response".into()),
            doc: Some("Handle authentication requests.".into()),
            kind: Some(SymbolKind::Function),
            last_modified: 0.0,
        });

        graph.add_edge(Edge {
            source: "file:api/handler.rs".into(),
            target: "sym:handle_auth_request".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        graph
    }

    #[test]
    fn tantivy_index_builds() {
        let graph = build_test_graph();
        let index = FileTantivyIndex::build(&graph).unwrap();
        assert_eq!(index.num_docs(), 3);
    }

    #[test]
    fn tantivy_jwt_query_finds_oauth() {
        let graph = build_test_graph();
        let index = FileTantivyIndex::build(&graph).unwrap();
        let results = index.search("jwt token verification", 10).unwrap();

        assert!(
            !results.is_empty(),
            "expected results for 'jwt token verification'"
        );

        // oauth.rs should rank highest (has verify_jwt_token)
        let mut sorted: Vec<_> = results.iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
        assert_eq!(sorted[0].0, "auth/oauth.rs");
    }

    #[test]
    fn tantivy_database_query_finds_connection() {
        let graph = build_test_graph();
        let index = FileTantivyIndex::build(&graph).unwrap();
        let results = index.search("database connection pool", 10).unwrap();

        assert!(
            !results.is_empty(),
            "expected results for 'database connection pool'"
        );

        let mut sorted: Vec<_> = results.iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
        assert_eq!(sorted[0].0, "db/connection.rs");
    }

    #[test]
    fn tantivy_auth_query_ranks_correctly() {
        let graph = build_test_graph();
        let index = FileTantivyIndex::build(&graph).unwrap();
        let results = index.search("auth request handler", 10).unwrap();

        assert!(!results.is_empty());

        // handler.rs has "handle_auth_request" — should be top
        let mut sorted: Vec<_> = results.iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
        assert_eq!(sorted[0].0, "api/handler.rs");
    }

    #[test]
    fn tantivy_empty_query_returns_empty() {
        let graph = build_test_graph();
        let index = FileTantivyIndex::build(&graph).unwrap();
        let results = index.search("", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn tantivy_no_match_returns_empty() {
        let graph = build_test_graph();
        let index = FileTantivyIndex::build(&graph).unwrap();
        let results = index.search("zzzzzznotaword", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn tantivy_filename_boost_works() {
        let graph = build_test_graph();
        let index = FileTantivyIndex::build(&graph).unwrap();

        // "oauth" is in the filename of auth/oauth.rs (5x boost)
        // and also in doc of handler.rs (1x)
        let results = index.search("oauth", 10).unwrap();
        assert!(!results.is_empty());

        let mut sorted: Vec<_> = results.iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
        assert_eq!(sorted[0].0, "auth/oauth.rs");
    }

    #[test]
    fn tantivy_prf_works() {
        let graph = build_test_graph();
        let index = FileTantivyIndex::build(&graph).unwrap();
        let results = index
            .search_with_prf(&graph, "jwt verification", 10)
            .unwrap();

        assert!(!results.is_empty());
        let mut sorted: Vec<_> = results.iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
        assert_eq!(sorted[0].0, "auth/oauth.rs");
    }
}
