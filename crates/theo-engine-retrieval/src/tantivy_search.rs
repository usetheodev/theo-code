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
mod inner {
    use std::collections::HashMap;

    use tantivy::collector::TopDocs;
    use tantivy::query::{BooleanQuery, BoostQuery, Occur, Query, TermQuery};
    use tantivy::schema::{Field, Schema, STORED, IndexRecordOption, TextFieldIndexing, TextOptions, Value};
    use tantivy::tokenizer::{SimpleTokenizer, TextAnalyzer, LowerCaser};
    use tantivy::{doc, Index, IndexWriter, TantivyDocument};

    use theo_engine_graph::model::{CodeGraph, NodeType};

    use crate::code_tokenizer::tokenize_code;

    /// Name for our custom tokenizer: whitespace split + lowercase, NO stemming.
    /// We pre-tokenize with code_tokenizer (which handles camelCase, snake_case, stemming).
    /// Tantivy's default TEXT uses en_stem Snowball stemmer which would double-stem
    /// our tokens (e.g., "verify" → "verifi"), causing query/index mismatch.
    const CODE_TOKENIZER: &str = "code_simple";

    /// File-level search index backed by Tantivy.
    ///
    /// Each file in the CodeGraph becomes a Tantivy document with 4 fields.
    /// Query-time field boosting implements BM25F scoring.
    pub struct FileTantivyIndex {
        index: Index,
        #[allow(dead_code)]
        schema: Schema,
        f_path: Field,
        f_filename: Field,
        f_path_segments: Field,
        f_symbol: Field,
        f_signature: Field,
        f_doc: Field,
        /// Symbols and files that this file IMPORTS/CALLS.
        /// Bridges the "definer vs user" gap: if search.rs imports
        /// propagate_attention, a query for "propagate_attention"
        /// will match search.rs via this field.
        f_imports: Field,
    }

    /// Build text options with our custom tokenizer (no stemming).
    fn code_text_options() -> TextOptions {
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(CODE_TOKENIZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions)
        )
    }

    impl FileTantivyIndex {
        /// Build the index from a CodeGraph.
        ///
        /// Iterates all File nodes, extracts child symbols, and indexes
        /// each file as a multi-field document in a RAM-backed Tantivy index.
        ///
        /// Uses a custom tokenizer (whitespace + lowercase only) because the input
        /// text is already pre-tokenized by our code_tokenizer.
        pub fn build(graph: &CodeGraph) -> Result<Self, tantivy::TantivyError> {
            let mut schema_builder = Schema::builder();

            let code_opts = code_text_options();

            // Stored path for result retrieval
            let f_path = schema_builder.add_text_field("path", STORED);
            // Searchable fields with our custom tokenizer (no double-stemming)
            let f_filename = schema_builder.add_text_field("filename", code_opts.clone());
            // Path segments: directory components tokenized (2x boost).
            // "crates/theo-infra-llm/src/provider/registry.rs" → "infra llm provider registry"
            // This helps queries like "LLM provider" match files in the right directory.
            let f_path_segments = schema_builder.add_text_field("path_segments", code_opts.clone());
            let f_symbol = schema_builder.add_text_field("symbol", code_opts.clone());
            let f_signature = schema_builder.add_text_field("signature", code_opts.clone());
            let f_doc = schema_builder.add_text_field("doc", code_opts.clone());
            // Import targets: names of symbols/files this file uses.
            // Bridges definer-vs-user gap (0.5x boost, lower than own symbols).
            let f_imports = schema_builder.add_text_field("imports", code_opts);

            let schema = schema_builder.build();
            let index = Index::create_in_ram(schema.clone());

            // Register custom tokenizer: SimpleTokenizer (split on non-alpha) + LowerCaser
            // No stemmer — our code_tokenizer already handles stemming.
            let code_analyzer = TextAnalyzer::builder(SimpleTokenizer::default())
                .filter(LowerCaser)
                .build();
            index.tokenizers().register(CODE_TOKENIZER, code_analyzer);

            let mut writer: IndexWriter = index.writer(50_000_000)?; // 50MB heap

            for node_id in graph.node_ids() {
                let Some(node) = graph.get_node(node_id) else { continue };
                if node.node_type != NodeType::File {
                    continue;
                }

                let file_path = node.file_path.as_deref().unwrap_or(&node.name);

                // Tokenize filename into searchable terms
                let filename_text = tokenize_code(&node.name).join(" ");

                // Path segments: tokenize each directory component
                // "crates/theo-infra-llm/src/provider/registry.rs" → "infra llm provider registry"
                let path_tokens: Vec<String> = file_path
                    .split('/')
                    .flat_map(|seg| tokenize_code(seg))
                    .collect();
                let path_text = path_tokens.join(" ");

                // Collect child symbols
                let mut symbol_parts: Vec<String> = Vec::new();
                let mut sig_parts: Vec<String> = Vec::new();
                let mut doc_parts: Vec<String> = Vec::new();

                for child_id in graph.contains_children(node_id) {
                    if let Some(child) = graph.get_node(child_id) {
                        symbol_parts.extend(tokenize_code(&child.name));

                        if let Some(sig) = &child.signature {
                            sig_parts.extend(tokenize_code(sig));
                        }
                        if let Some(d) = &child.doc {
                            if let Some(first_line) = d.lines().next() {
                                doc_parts.extend(tokenize_code(first_line));
                            }
                        }
                    }
                }

                // Collect IMPORT/CALL targets via 2-hop traversal:
                // file → child symbols → their call/import targets.
                // This bridges the "definer vs user" gap — if search.rs's
                // MultiSignalScorer calls propagate_attention, that name
                // goes into search.rs's index. A query for "propagate_attention"
                // then matches BOTH graph_attention.rs (definer) AND search.rs (user).
                let mut import_parts: Vec<String> = Vec::new();
                for child_id in graph.contains_children(node_id) {
                    // 2nd hop: symbols that this child calls/imports
                    for target_id in graph.neighbors(child_id) {
                        if let Some(target) = graph.get_node(target_id) {
                            // Only add symbol names (not files) to avoid noise
                            if target.node_type == NodeType::Symbol {
                                import_parts.extend(tokenize_code(&target.name));
                            }
                        }
                    }
                }

                writer.add_document(doc!(
                    f_path => file_path,
                    f_filename => filename_text,
                    f_path_segments => path_text,
                    f_symbol => symbol_parts.join(" "),
                    f_signature => sig_parts.join(" "),
                    f_doc => doc_parts.join(" "),
                    f_imports => import_parts.join(" "),
                ))?;
            }

            writer.commit()?;

            Ok(FileTantivyIndex {
                index,
                schema,
                f_path,
                f_filename,
                f_path_segments,
                f_symbol,
                f_signature,
                f_doc,
                f_imports,
            })
        }

        /// Search the index with BM25F field boosting.
        ///
        /// Returns file_path → score mapping, same interface as `FileBm25::search`.
        /// Field boosts: filename 5x, symbol 3x, signature 1x, doc 1x.
        pub fn search(&self, query: &str, top_k: usize) -> Result<HashMap<String, f64>, tantivy::TantivyError> {
            let reader = self.index.reader()?;
            let searcher = reader.searcher();

            let query_tokens = tokenize_code(query);
            if query_tokens.is_empty() {
                return Ok(HashMap::new());
            }

            // Build a BooleanQuery: for each token, create boosted TermQueries across all fields
            let mut subqueries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

            for token in &query_tokens {
                let term_queries: Vec<(Occur, Box<dyn Query>)> = vec![
                    // filename: 5x boost
                    (Occur::Should, Box::new(BoostQuery::new(
                        Box::new(TermQuery::new(
                            tantivy::Term::from_field_text(self.f_filename, token),
                            IndexRecordOption::WithFreqs,
                        )),
                        5.0,
                    ))),
                    // path segments: 3x boost (disambiguates mod.rs files by directory)
                    (Occur::Should, Box::new(BoostQuery::new(
                        Box::new(TermQuery::new(
                            tantivy::Term::from_field_text(self.f_path_segments, token),
                            IndexRecordOption::WithFreqs,
                        )),
                        3.0,
                    ))),
                    // symbol: 3x boost
                    (Occur::Should, Box::new(BoostQuery::new(
                        Box::new(TermQuery::new(
                            tantivy::Term::from_field_text(self.f_symbol, token),
                            IndexRecordOption::WithFreqs,
                        )),
                        3.0,
                    ))),
                    // signature: 1x
                    (Occur::Should, Box::new(TermQuery::new(
                        tantivy::Term::from_field_text(self.f_signature, token),
                        IndexRecordOption::WithFreqs,
                    ))),
                    // doc: 1x
                    (Occur::Should, Box::new(TermQuery::new(
                        tantivy::Term::from_field_text(self.f_doc, token),
                        IndexRecordOption::WithFreqs,
                    ))),
                    // imports: 0.5x (captures "who uses this")
                    (Occur::Should, Box::new(BoostQuery::new(
                        Box::new(TermQuery::new(
                            tantivy::Term::from_field_text(self.f_imports, token),
                            IndexRecordOption::WithFreqs,
                        )),
                        0.5,
                    ))),
                ];

                // Each token's field matches are OR'd together
                let token_query = BooleanQuery::new(term_queries);
                subqueries.push((Occur::Should, Box::new(token_query)));
            }

            let combined = BooleanQuery::new(subqueries);
            let top_docs = searcher.search(&combined, &TopDocs::with_limit(top_k))?;

            let mut results = HashMap::new();
            for (score, doc_address) in top_docs {
                let doc: TantivyDocument = searcher.doc(doc_address)?;
                if let Some(path_value) = doc.get_first(self.f_path) {
                    if let Some(path_str) = Value::as_str(&path_value) {
                        results.insert(path_str.to_string(), score as f64);
                    }
                }
            }

            Ok(results)
        }

        /// Search with Pseudo-Relevance Feedback (PRF).
        ///
        /// Same 2-stage approach as `FileBm25::search`:
        /// Stage 1: Initial BM25 search
        /// Stage 2: If top result is confident, expand query with its symbols
        pub fn search_with_prf(
            &self,
            graph: &CodeGraph,
            query: &str,
            top_k: usize,
        ) -> Result<HashMap<String, f64>, tantivy::TantivyError> {
            let initial = self.search(query, top_k)?;

            // Find top result with confidence check
            let mut sorted: Vec<_> = initial.iter().collect();
            sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

            if sorted.len() >= 2 && *sorted[0].1 > sorted[1].1 * 2.0 {
                let top_file = sorted[0].0.as_str();
                let file_id = format!("file:{}", top_file);

                // Extract symbol names from top file for expansion
                let mut expansion: Vec<String> = Vec::new();
                for child_id in graph.contains_children(&file_id) {
                    if let Some(child) = graph.get_node(child_id) {
                        if child.name.len() >= 5 {
                            expansion.push(child.name.clone());
                        }
                    }
                }
                expansion.truncate(5);

                if !expansion.is_empty() {
                    let expansion_query = expansion.join(" ");
                    let expanded = self.search(&expansion_query, top_k)?;

                    // Merge: original + 0.3x expanded
                    let mut merged = initial;
                    for (path, exp_score) in expanded {
                        merged.entry(path)
                            .and_modify(|s| *s += exp_score * 0.3)
                            .or_insert(exp_score * 0.3);
                    }
                    return Ok(merged);
                }
            }

            Ok(initial)
        }

        /// Number of documents in the index.
        pub fn num_docs(&self) -> u64 {
            self.index.reader()
                .map(|r| r.searcher().num_docs())
                .unwrap_or(0)
        }
    }
}

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
    for k in custom_scores.keys() { all_paths.insert(k.clone()); }
    for k in tantivy_scores.keys() { all_paths.insert(k.clone()); }

    if all_paths.is_empty() {
        return HashMap::new();
    }

    // Min-max normalize each ranker to [0, 1]
    let normalize = |scores: &HashMap<String, f64>| -> HashMap<String, f64> {
        if scores.is_empty() { return HashMap::new(); }
        let max = scores.values().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min = scores.values().cloned().fold(f64::INFINITY, f64::min);
        let range = max - min;
        if range <= 0.0 {
            scores.iter().map(|(k, _)| (k.clone(), 1.0)).collect()
        } else {
            scores.iter().map(|(k, v)| (k.clone(), (v - min) / range)).collect()
        }
    };

    let norm_custom = normalize(&custom_scores);
    let norm_tantivy = normalize(&tantivy_scores);

    // Priority fusion: Custom is primary ranker; Tantivy supplements.
    // For files in BOTH rankers: take max of normalized scores.
    // For files ONLY in Tantivy: add with a small bonus (0.3x of custom's min non-zero).
    // This preserves Custom's ranking while capturing Tantivy-only discoveries.
    let custom_min_nonzero = norm_custom.values()
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
    use crate::search::FileBm25;
    use crate::dense_search::FileDenseSearch;

    // Get scores from all 3 rankers
    let bm25_scores = FileBm25::search(graph, query);
    // Use large top_k to avoid missing files in large repos (e.g., FastAPI 1125 files).
    // Cost is negligible: Tantivy is index-based, Dense scans all cached embeddings anyway.
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
        let mut sorted: Vec<_> = scores.iter()
            .filter(|(k, _)| !is_noise(k))
            .collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.into_iter().map(|(k, _)| k.clone()).collect()
    };

    let bm25_ranked = to_ranked(&bm25_scores);
    let tantivy_ranked = to_ranked(&tantivy_scores);
    let dense_ranked = to_ranked(&dense_scores);

    let rank_map = |ranked: &[String]| -> HashMap<String, usize> {
        ranked.iter().enumerate().map(|(i, p)| (p.clone(), i)).collect()
    };

    let bm25_rank = rank_map(&bm25_ranked);
    let tantivy_rank = rank_map(&tantivy_ranked);
    let dense_rank = rank_map(&dense_ranked);

    let mut all_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
    for k in bm25_scores.keys() { if !is_noise(k) { all_paths.insert(k.clone()); } }
    for k in tantivy_scores.keys() { if !is_noise(k) { all_paths.insert(k.clone()); } }
    for k in dense_scores.keys() { if !is_noise(k) { all_paths.insert(k.clone()); } }

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "tantivy-backend"))]
mod tests {
    use super::*;
    use theo_engine_graph::model::{CodeGraph, Node, Edge, EdgeType, NodeType, SymbolKind};

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

        assert!(!results.is_empty(), "expected results for 'jwt token verification'");

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

        assert!(!results.is_empty(), "expected results for 'database connection pool'");

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
        let results = index.search_with_prf(&graph, "jwt verification", 10).unwrap();

        assert!(!results.is_empty());
        let mut sorted: Vec<_> = results.iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
        assert_eq!(sorted[0].0, "auth/oauth.rs");
    }
}
