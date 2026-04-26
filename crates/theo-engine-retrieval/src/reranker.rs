//! Cross-encoder reranker for retrieval refinement.
//!
//! Stage 2 of the retrieval pipeline: takes top-50 candidates from RRF
//! and reranks them using a cross-encoder model for higher precision.
//!
//! Model: Jina Reranker v2 Base Multilingual (supports EN, PT, ZH, etc.)
//! via fastembed TextRerank (ONNX, CPU-only, ~10ms/doc).
//!
//! T8.1 — Always compiled (fastembed is a non-optional dep). Whether to
//! USE the reranker at runtime is gated by `RetrievalConfig.use_reranker`,
//! NOT by a build feature.

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
    pub(super) fn build_rerank_document(graph: &CodeGraph, file_path: &str) -> String {
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

pub use inner::CrossEncoderReranker;

// Re-export the private helper for tests so we can verify behaviour
// without spinning up the cross-encoder model.
#[cfg(test)]
use inner::build_rerank_document;

/// Runtime gate + tuning for the cross-encoder reranker.
///
/// T8.1 — The reranker module is always compiled, but whether to
/// INVOKE it on a given retrieval is a runtime config decision.
/// `use_reranker = true` is the SOTA-default (≈+15pt nDCG@10 vs
/// plain RRF in the plan's A/B target); set `false` to fall back to
/// pure RRF for cost-sensitive paths.
#[derive(Debug, Clone, PartialEq)]
pub struct CrossEncoderConfig {
    /// Master switch. `false` = skip the reranker stage entirely
    /// (pipeline returns the RRF top-K unchanged).
    pub use_reranker: bool,
    /// Final number of files returned after reranking.
    pub top_k: usize,
    /// Cap on candidates fed to the cross-encoder. Higher = better
    /// recall but slower (~10ms/doc). 50 is the SOTA default.
    pub max_candidates: usize,
}

impl Default for CrossEncoderConfig {
    fn default() -> Self {
        Self {
            // SOTA-default: reranker on. Set explicitly so a future
            // refactor that drops `Default` makes the choice visible.
            use_reranker: true,
            top_k: 20,
            max_candidates: 50,
        }
    }
}

impl CrossEncoderConfig {
    /// Cost-sensitive preset: skip the reranker entirely.
    pub fn rrf_only() -> Self {
        Self {
            use_reranker: false,
            top_k: 20,
            max_candidates: 50,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// T8.1 — Tests run on default build (no feature flag required).
// Heavy tests that download the Jina model from HuggingFace are
// gated by --ignored to keep `cargo test` fast.
#[cfg(test)]
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
            line_start: None,
            line_end: None,
            signature: None,
            doc: None,
            kind: None,
            last_modified: 0.0,
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
            line_start: None,
            line_end: None,
            signature: None,
            doc: None,
            kind: None,
            last_modified: 0.0,
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

    // T8.1 — Heavy tests that pull the Jina model from HuggingFace
    // are gated behind --ignored so default `cargo test` runs fast
    // and works offline.
    #[test]
    #[ignore]
    fn reranker_init_succeeds() {
        let reranker = CrossEncoderReranker::new();
        assert!(reranker.is_ok(), "reranker should initialize successfully");
    }

    #[test]
    #[ignore]
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
    #[ignore]
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
    #[ignore]
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

    // Offline tests — verify the module surface without downloading
    // any model. Document the always-compiled invariant so a future
    // PR adding a build-time gate would be caught here.

    #[test]
    fn t81_reranker_module_compiles_without_feature_flag() {
        // Pure type-system smoke test: if T8.1 regressed by re-adding
        // a `#[cfg(feature = "reranker")]` gate to `inner`, this
        // would not compile in the default build.
        let _build_doc = build_rerank_document(&build_test_graph(), "auth/oauth.rs");
        // build_rerank_document is a helper exposed inside `inner`; if
        // T8.1's "always compiled" property holds, importing `super::*`
        // brings it into scope without any feature flag.
    }

    #[test]
    fn t81_build_rerank_document_includes_path_and_top_symbols() {
        // Pure function test — no model download required. Verifies
        // the document the reranker would feed to the cross-encoder
        // includes the file path + symbol names + signatures + first
        // doc line.
        let graph = build_test_graph();
        let doc = build_rerank_document(&graph, "auth/oauth.rs");
        assert!(doc.contains("auth/oauth.rs"));
        assert!(doc.contains("verify_jwt_token"));
        assert!(doc.contains("Result<Claims>"));
        assert!(doc.contains("Verify JWT authentication token"));
    }

    #[test]
    fn t81_build_rerank_document_for_unknown_file_returns_just_path() {
        let graph = build_test_graph();
        let doc = build_rerank_document(&graph, "nonexistent/file.rs");
        // Falls back to just the path (no node in graph).
        assert!(doc.contains("nonexistent/file.rs"));
    }

    #[test]
    fn t81cfg_default_enables_reranker() {
        // SOTA-default invariant: a fresh CrossEncoderConfig must
        // ship with use_reranker = true. Regression here would
        // silently revert the SOTA gain, so the test is explicit.
        let cfg = CrossEncoderConfig::default();
        assert!(
            cfg.use_reranker,
            "Default::default() must have use_reranker = true (SOTA gate)"
        );
        assert!(cfg.top_k > 0);
        assert!(cfg.max_candidates >= cfg.top_k);
    }

    #[test]
    fn t81cfg_rrf_only_preset_disables_reranker() {
        let cfg = CrossEncoderConfig::rrf_only();
        assert!(!cfg.use_reranker);
        // Other tunables remain reasonable so a caller switching to
        // RRF-only still gets the same shape of output.
        assert!(cfg.top_k > 0);
        assert!(cfg.max_candidates >= cfg.top_k);
    }

    #[test]
    fn t81cfg_clone_and_partialeq_round_trip() {
        let a = CrossEncoderConfig::default();
        let b = a.clone();
        assert_eq!(a, b);
        let c = CrossEncoderConfig::rrf_only();
        assert_ne!(a, c, "default vs rrf_only must differ");
    }

    #[test]
    fn t81_build_rerank_document_caps_symbols_at_five() {
        // Build a graph with 10 symbols in one file — only 5 should
        // make it into the document (token-budget guard inside
        // build_rerank_document).
        use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};
        let mut graph = CodeGraph::new();
        graph.add_node(Node {
            id: "file:big.rs".into(),
            name: "big.rs".into(),
            node_type: NodeType::File,
            file_path: Some("big.rs".into()),
            line_start: None,
            line_end: None,
            signature: None,
            doc: None,
            kind: None,
            last_modified: 0.0,
        });
        for i in 0..10 {
            let sym_id = format!("sym:f{i}");
            let unique_name = format!("FUNC_NAME_{i}_VERY_DISTINCT");
            graph.add_node(Node {
                id: sym_id.clone(),
                name: unique_name,
                node_type: NodeType::Symbol,
                file_path: Some("big.rs".into()),
                line_start: Some(i),
                line_end: Some(i + 1),
                signature: Some(format!("pub fn f{i}()")),
                doc: None,
                kind: Some(SymbolKind::Function),
                last_modified: 0.0,
            });
            graph.add_edge(Edge {
                source: "file:big.rs".into(),
                target: sym_id,
                edge_type: EdgeType::Contains,
                weight: 1.0,
            });
        }
        let doc = build_rerank_document(&graph, "big.rs");
        let included = (0..10)
            .filter(|i| doc.contains(&format!("FUNC_NAME_{i}_VERY_DISTINCT")))
            .count();
        assert_eq!(
            included, 5,
            "build_rerank_document must cap symbols at 5; included {included}"
        );
    }

    #[test]
    #[ignore]
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
