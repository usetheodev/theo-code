//! File-level dense semantic search via embedding similarity.
//!
//! Complements BM25 by covering the "semantic gap":
//! - "error handling" matches "retry.rs" (no term overlap)
//! - "authentication" matches "pkce.rs" (conceptual link)
//!
//! Uses pre-computed embeddings from `EmbeddingCache` for O(N) cosine scan.

#[cfg(feature = "dense-retrieval")]
mod inner {
    use std::collections::HashMap;

    use crate::embedding::cache::EmbeddingCache;
    use crate::embedding::neural::NeuralEmbedder;

    /// File-level dense search via embedding cosine similarity.
    pub struct FileDenseSearch;

    /// Cosine scan helper: embed query, scan cache, return sorted scores.
    fn cosine_scan(query_vec: &[f64], cache: &EmbeddingCache) -> Vec<(String, f64)> {
        let query_norm: f64 = query_vec.iter().map(|x| x * x).sum::<f64>().sqrt();
        if query_norm < 1e-10 {
            return Vec::new();
        }

        let mut scores: Vec<(String, f64)> = Vec::new();
        for (file_path, embedding) in cache.iter() {
            let emb_norm: f64 = embedding.iter().map(|x| x * x).sum::<f64>().sqrt();
            if emb_norm < 1e-10 {
                continue;
            }

            let mut sim = NeuralEmbedder::cosine_similarity(query_vec, embedding);

            // Test/benchmark/example penalty
            let lp = file_path.to_lowercase();
            if lp.contains("test") || lp.contains("benchmark") || lp.contains("example") {
                sim *= 0.1;
            }

            if sim > 0.0 {
                scores.push((file_path.to_string(), sim));
            }
        }

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores
    }

    impl FileDenseSearch {
        /// Search with PRF: embed query → find top-1 → expand query with top-1's
        /// document text → re-embed → merge scores.
        ///
        /// This bridges the "definer vs user" gap in dense search:
        /// Stage 1: "TurboQuantizer quantize" → finds turboquant.rs (#1)
        /// Stage 2: expanded with turboquant.rs symbols → search.rs rises
        ///          because it mentions the same symbols
        pub fn search(
            embedder: &NeuralEmbedder,
            cache: &EmbeddingCache,
            query: &str,
            top_k: usize,
        ) -> HashMap<String, f64> {
            if query.is_empty() || cache.is_empty() {
                return HashMap::new();
            }

            // Stage 1: initial dense search
            let query_vec = embedder.embed(query);
            let initial = cosine_scan(&query_vec, cache);

            if initial.is_empty() {
                return HashMap::new();
            }

            // PRF: if top-1 is confident (1.3x over #2), expand query toward top-1.
            // Finds similar files to the best match.
            if initial.len() >= 2 && initial[0].1 > initial[1].1 * 1.3 {
                let top_path = &initial[0].0;

                if let Some(top_emb) = cache.get(top_path) {
                    let expanded: Vec<f64> = query_vec
                        .iter()
                        .zip(top_emb.iter())
                        .map(|(q, d)| 0.7 * q + 0.3 * d)
                        .collect();

                    let expanded_scores = cosine_scan(&expanded, cache);

                    // Merge: max of initial and expanded scores
                    let mut merged: HashMap<String, f64> = HashMap::new();
                    for (path, score) in initial.iter().chain(expanded_scores.iter()) {
                        let entry = merged.entry(path.clone()).or_insert(0.0);
                        *entry = entry.max(*score);
                    }

                    let mut result: Vec<_> = merged.into_iter().collect();
                    result
                        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    result.truncate(top_k);
                    return result.into_iter().collect();
                }
            }

            // No PRF: return initial results
            initial
                .into_iter()
                .take(top_k)
                .collect::<Vec<_>>()
                .into_iter()
                .collect()
        }
    }
}

#[cfg(feature = "dense-retrieval")]
pub use inner::FileDenseSearch;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "dense-retrieval"))]
mod tests {
    use super::*;
    use crate::embedding::cache::EmbeddingCache;
    use crate::embedding::neural::NeuralEmbedder;
    use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};

    fn build_test_graph() -> CodeGraph {
        let mut graph = CodeGraph::new();

        // File: auth/oauth.rs
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
            last_modified: 100.0,
        });
        graph.add_node(Node {
            id: "sym:verify_jwt".into(),
            name: "verify_jwt_token".into(),
            node_type: NodeType::Symbol,
            file_path: Some("auth/oauth.rs".into()),
            line_start: Some(10),
            line_end: Some(30),
            signature: Some("pub fn verify_jwt_token(token: &str) -> Result<Claims>".into()),
            doc: Some("Verify JWT authentication token".into()),
            kind: Some(SymbolKind::Function),
            last_modified: 100.0,
        });
        graph.add_edge(Edge {
            source: "file:auth/oauth.rs".into(),
            target: "sym:verify_jwt".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        // File: db/pool.rs
        graph.add_node(Node {
            id: "file:db/pool.rs".into(),
            name: "pool.rs".into(),
            node_type: NodeType::File,
            file_path: Some("db/pool.rs".into()),
            line_start: None,
            line_end: None,
            signature: None,
            doc: None,
            kind: None,
            last_modified: 200.0,
        });
        graph.add_node(Node {
            id: "sym:create_pool".into(),
            name: "create_connection_pool".into(),
            node_type: NodeType::Symbol,
            file_path: Some("db/pool.rs".into()),
            line_start: Some(5),
            line_end: Some(20),
            signature: Some("pub fn create_connection_pool(url: &str) -> Pool".into()),
            doc: Some("Create database connection pool".into()),
            kind: Some(SymbolKind::Function),
            last_modified: 200.0,
        });
        graph.add_edge(Edge {
            source: "file:db/pool.rs".into(),
            target: "sym:create_pool".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        // File: tests/test_auth.rs (test file — should be penalized)
        graph.add_node(Node {
            id: "file:tests/test_auth.rs".into(),
            name: "test_auth.rs".into(),
            node_type: NodeType::File,
            file_path: Some("tests/test_auth.rs".into()),
            line_start: None,
            line_end: None,
            signature: None,
            doc: None,
            kind: None,
            last_modified: 300.0,
        });
        graph.add_node(Node {
            id: "sym:test_verify".into(),
            name: "test_verify_jwt".into(),
            node_type: NodeType::Symbol,
            file_path: Some("tests/test_auth.rs".into()),
            line_start: Some(1),
            line_end: Some(10),
            signature: Some("fn test_verify_jwt()".into()),
            doc: Some("Test JWT verification".into()),
            kind: Some(SymbolKind::Function),
            last_modified: 300.0,
        });
        graph.add_edge(Edge {
            source: "file:tests/test_auth.rs".into(),
            target: "sym:test_verify".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        graph
    }

    #[test]
    fn dense_empty_query_returns_empty() {
        let graph = build_test_graph();
        let embedder = match NeuralEmbedder::new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let cache = EmbeddingCache::build(&graph, &embedder);
        let results = FileDenseSearch::search(&embedder, &cache, "", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn dense_empty_cache_returns_empty() {
        let graph = CodeGraph::new();
        let embedder = match NeuralEmbedder::new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let cache = EmbeddingCache::build(&graph, &embedder);
        let results = FileDenseSearch::search(&embedder, &cache, "authentication", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn dense_auth_query_finds_oauth() {
        let graph = build_test_graph();
        let embedder = match NeuralEmbedder::new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let cache = EmbeddingCache::build(&graph, &embedder);
        let results = FileDenseSearch::search(
            &embedder,
            &cache,
            "JWT authentication token verification",
            10,
        );

        assert!(!results.is_empty(), "expected results for auth query");

        // oauth.rs should score higher than pool.rs
        let oauth_score = results.get("auth/oauth.rs").copied().unwrap_or(0.0);
        let pool_score = results.get("db/pool.rs").copied().unwrap_or(0.0);
        assert!(
            oauth_score > pool_score,
            "oauth.rs ({oauth_score:.4}) should score higher than pool.rs ({pool_score:.4})"
        );
    }

    #[test]
    fn dense_database_query_finds_pool() {
        let graph = build_test_graph();
        let embedder = match NeuralEmbedder::new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let cache = EmbeddingCache::build(&graph, &embedder);
        let results = FileDenseSearch::search(&embedder, &cache, "database connection pool", 10);

        assert!(!results.is_empty());
        let pool_score = results.get("db/pool.rs").copied().unwrap_or(0.0);
        let oauth_score = results.get("auth/oauth.rs").copied().unwrap_or(0.0);
        assert!(
            pool_score > oauth_score,
            "pool.rs ({pool_score:.4}) should score higher than oauth.rs ({oauth_score:.4})"
        );
    }

    #[test]
    fn dense_test_file_penalized() {
        let graph = build_test_graph();
        let embedder = match NeuralEmbedder::new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let cache = EmbeddingCache::build(&graph, &embedder);
        let results = FileDenseSearch::search(&embedder, &cache, "JWT verification test", 10);

        // test_auth.rs should score lower than oauth.rs due to 0.1x penalty
        let test_score = results.get("tests/test_auth.rs").copied().unwrap_or(0.0);
        let oauth_score = results.get("auth/oauth.rs").copied().unwrap_or(0.0);
        assert!(
            oauth_score > test_score,
            "oauth.rs ({oauth_score:.4}) should beat test_auth.rs ({test_score:.4}) due to test penalty"
        );
    }

    #[test]
    fn dense_top_k_respected() {
        let graph = build_test_graph();
        let embedder = match NeuralEmbedder::new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let cache = EmbeddingCache::build(&graph, &embedder);
        let results = FileDenseSearch::search(&embedder, &cache, "code", 1);
        assert!(results.len() <= 1);
    }
}
