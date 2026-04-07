//! Cross-encoder reranker for retrieval refinement.
//!
//! Stage 2 of the retrieval pipeline: takes top-50 candidates from RRF
//! and reranks them using a cross-encoder model for higher precision.
//!
//! Model: Jina Reranker v2 Base Multilingual (supports EN, PT, ZH, etc.)
//! via fastembed TextRerank (ONNX, CPU-only, ~10ms/doc).

#[cfg(feature = "reranker")]
mod inner {
    use std::collections::HashMap;

    use fastembed::{RerankInitOptions, RerankerModel, TextRerank};

    use theo_engine_graph::model::{CodeGraph, NodeType};

    /// Cross-encoder reranker wrapper over fastembed TextRerank.
    ///
    /// Reranks candidate files by computing cross-attention between
    /// query and document text. More expensive than BM25/dense but
    /// significantly more accurate for top-K refinement.
    pub struct CrossEncoderReranker {
        model: TextRerank,
    }

    impl CrossEncoderReranker {
        /// Initialize the reranker model.
        ///
        /// Uses Jina Reranker v2 Base Multilingual — supports EN, PT, ZH, ES, etc.
        /// Model is downloaded to ~/.cache/fastembed/ on first run.
        pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
            let options = RerankInitOptions::new(RerankerModel::JINARerankerV2BaseMultiligual);
            let model = TextRerank::try_new(options)?;
            Ok(CrossEncoderReranker { model })
        }

        /// Rerank candidate files against a query.
        ///
        /// Input: query + candidate file_paths with initial scores.
        /// For each candidate, constructs a document from the CodeGraph
        /// (path + top symbols + signatures + first-line docs).
        ///
        /// Returns reranked file_path → cross-encoder score mapping.
        /// Max 50 candidates to bound latency.
        pub fn rerank(
            &self,
            query: &str,
            candidates: &[(String, f64)],
            graph: &CodeGraph,
            top_k: usize,
        ) -> HashMap<String, f64> {
            if query.is_empty() || candidates.is_empty() {
                return HashMap::new();
            }

            // Cap at 50 candidates (governance condition)
            let capped: Vec<_> = candidates.iter().take(50).collect();

            // Build document text for each candidate
            let documents: Vec<String> = capped
                .iter()
                .map(|(path, _)| build_rerank_document(graph, path))
                .collect();

            let doc_refs: Vec<&str> = documents.iter().map(|s| s.as_str()).collect();

            // Rerank via cross-encoder
            match self.model.rerank(query, doc_refs, false, None) {
                Ok(results) => {
                    let mut scored = HashMap::new();
                    for result in results.into_iter().take(top_k) {
                        let idx = result.index;
                        if idx < capped.len() {
                            let path = &capped[idx].0;
                            scored.insert(path.clone(), result.score as f64);
                        }
                    }
                    scored
                }
                Err(e) => {
                    eprintln!("[reranker] cross-encoder failed, falling back to input order: {e}");
                    // Fallback: return input candidates with original scores
                    candidates
                        .iter()
                        .take(top_k)
                        .map(|(p, s)| (p.clone(), *s))
                        .collect()
                }
            }
        }
    }

    /// Build a compact document for cross-encoder reranking.
    ///
    /// Includes: file path + top symbol names + signatures + first-line docs.
    /// Kept short (<200 tokens) for fast cross-encoder inference.
    fn build_rerank_document(graph: &CodeGraph, file_path: &str) -> String {
        let file_id = format!("file:{}", file_path);
        let mut parts = vec![file_path.to_string()];

        if let Some(node) = graph.get_node(&file_id) {
            parts.push(node.name.clone());
        }

        // Top symbols from the file (limit 5 for token budget)
        let mut symbol_count = 0;
        for child_id in graph.contains_children(&file_id) {
            if symbol_count >= 5 {
                break;
            }
            if let Some(child) = graph.get_node(child_id) {
                if child.node_type == NodeType::Symbol {
                    parts.push(child.name.clone());
                    if let Some(sig) = &child.signature {
                        parts.push(sig.clone());
                    }
                    if let Some(doc) = &child.doc {
                        if let Some(first_line) = doc.lines().next() {
                            parts.push(first_line.to_string());
                        }
                    }
                    symbol_count += 1;
                }
            }
        }

        parts.join(" ")
    }
}

#[cfg(feature = "reranker")]
pub use inner::CrossEncoderReranker;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "reranker"))]
mod tests {
    use super::*;
    use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};

    fn build_test_graph() -> CodeGraph {
        let mut graph = CodeGraph::new();

        graph.add_node(Node {
            id: "file:auth/oauth.rs".into(),
            name: "oauth.rs".into(),
            node_type: NodeType::File,
            file_path: Some("auth/oauth.rs".into()),
            line_start: None, line_end: None,
            signature: None, doc: None, kind: None,
            last_modified: 0.0,
        });
        graph.add_node(Node {
            id: "sym:verify_jwt".into(),
            name: "verify_jwt_token".into(),
            node_type: NodeType::Symbol,
            file_path: Some("auth/oauth.rs".into()),
            line_start: Some(10), line_end: Some(30),
            signature: Some("pub fn verify_jwt_token(token: &str) -> Result<Claims>".into()),
            doc: Some("Verify JWT authentication token".into()),
            kind: Some(SymbolKind::Function),
            last_modified: 0.0,
        });
        graph.add_edge(Edge {
            source: "file:auth/oauth.rs".into(),
            target: "sym:verify_jwt".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        graph.add_node(Node {
            id: "file:db/pool.rs".into(),
            name: "pool.rs".into(),
            node_type: NodeType::File,
            file_path: Some("db/pool.rs".into()),
            line_start: None, line_end: None,
            signature: None, doc: None, kind: None,
            last_modified: 0.0,
        });
        graph.add_node(Node {
            id: "sym:create_pool".into(),
            name: "create_connection_pool".into(),
            node_type: NodeType::Symbol,
            file_path: Some("db/pool.rs".into()),
            line_start: Some(5), line_end: Some(20),
            signature: Some("pub fn create_connection_pool(url: &str) -> Pool".into()),
            doc: Some("Create database connection pool".into()),
            kind: Some(SymbolKind::Function),
            last_modified: 0.0,
        });
        graph.add_edge(Edge {
            source: "file:db/pool.rs".into(),
            target: "sym:create_pool".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        graph
    }

    #[test]
    fn reranker_init_succeeds() {
        let reranker = CrossEncoderReranker::new();
        assert!(reranker.is_ok(), "reranker should initialize successfully");
    }

    #[test]
    fn reranker_empty_input() {
        let reranker = match CrossEncoderReranker::new() {
            Ok(r) => r,
            Err(_) => return,
        };
        let graph = build_test_graph();
        let result = reranker.rerank("jwt token", &[], &graph, 10);
        assert!(result.is_empty());
    }

    #[test]
    fn reranker_empty_query() {
        let reranker = match CrossEncoderReranker::new() {
            Ok(r) => r,
            Err(_) => return,
        };
        let graph = build_test_graph();
        let candidates = vec![("auth/oauth.rs".to_string(), 1.0)];
        let result = reranker.rerank("", &candidates, &graph, 10);
        assert!(result.is_empty());
    }

    #[test]
    fn reranker_ranks_relevant_higher() {
        let reranker = match CrossEncoderReranker::new() {
            Ok(r) => r,
            Err(_) => return,
        };
        let graph = build_test_graph();
        let candidates = vec![
            ("db/pool.rs".to_string(), 0.5),
            ("auth/oauth.rs".to_string(), 0.5),
        ];
        let result = reranker.rerank("JWT authentication verification", &candidates, &graph, 10);

        assert!(!result.is_empty());
        let oauth_score = result.get("auth/oauth.rs").copied().unwrap_or(-1.0);
        let pool_score = result.get("db/pool.rs").copied().unwrap_or(-1.0);
        assert!(
            oauth_score > pool_score,
            "oauth.rs ({oauth_score:.4}) should rank above pool.rs ({pool_score:.4}) for auth query"
        );
    }

    #[test]
    fn reranker_respects_top_k() {
        let reranker = match CrossEncoderReranker::new() {
            Ok(r) => r,
            Err(_) => return,
        };
        let graph = build_test_graph();
        let candidates = vec![
            ("auth/oauth.rs".to_string(), 1.0),
            ("db/pool.rs".to_string(), 0.5),
        ];
        let result = reranker.rerank("authentication", &candidates, &graph, 1);
        assert!(result.len() <= 1);
    }
}
