//! Tantivy-backed FileTantivyIndex (T2.4 split, D5).

#![allow(unused_imports, dead_code)]

    use std::collections::HashMap;

    use tantivy::collector::TopDocs;
    use tantivy::query::{BooleanQuery, BoostQuery, Occur, Query, TermQuery};
    use tantivy::schema::{
        Field, IndexRecordOption, STORED, Schema, TextFieldIndexing, TextOptions, Value,
    };
    use tantivy::tokenizer::{LowerCaser, SimpleTokenizer, TextAnalyzer};
    use tantivy::{Index, IndexWriter, TantivyDocument, doc};

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
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
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
                let Some(node) = graph.get_node(node_id) else {
                    continue;
                };
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
                    .flat_map(tokenize_code)
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
                        if let Some(d) = &child.doc
                            && let Some(first_line) = d.lines().next() {
                                doc_parts.extend(tokenize_code(first_line));
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
        pub fn search(
            &self,
            query: &str,
            top_k: usize,
        ) -> Result<HashMap<String, f64>, tantivy::TantivyError> {
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
                    (
                        Occur::Should,
                        Box::new(BoostQuery::new(
                            Box::new(TermQuery::new(
                                tantivy::Term::from_field_text(self.f_filename, token),
                                IndexRecordOption::WithFreqs,
                            )),
                            5.0,
                        )),
                    ),
                    // path segments: 3x boost (disambiguates mod.rs files by directory)
                    (
                        Occur::Should,
                        Box::new(BoostQuery::new(
                            Box::new(TermQuery::new(
                                tantivy::Term::from_field_text(self.f_path_segments, token),
                                IndexRecordOption::WithFreqs,
                            )),
                            3.0,
                        )),
                    ),
                    // symbol: 3x boost
                    (
                        Occur::Should,
                        Box::new(BoostQuery::new(
                            Box::new(TermQuery::new(
                                tantivy::Term::from_field_text(self.f_symbol, token),
                                IndexRecordOption::WithFreqs,
                            )),
                            3.0,
                        )),
                    ),
                    // signature: 1x
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(
                            tantivy::Term::from_field_text(self.f_signature, token),
                            IndexRecordOption::WithFreqs,
                        )),
                    ),
                    // doc: 1x
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(
                            tantivy::Term::from_field_text(self.f_doc, token),
                            IndexRecordOption::WithFreqs,
                        )),
                    ),
                    // imports: 0.5x (captures "who uses this")
                    (
                        Occur::Should,
                        Box::new(BoostQuery::new(
                            Box::new(TermQuery::new(
                                tantivy::Term::from_field_text(self.f_imports, token),
                                IndexRecordOption::WithFreqs,
                            )),
                            0.5,
                        )),
                    ),
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
                if let Some(path_value) = doc.get_first(self.f_path)
                    && let Some(path_str) = Value::as_str(&path_value) {
                        results.insert(path_str.to_string(), score as f64);
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
                    if let Some(child) = graph.get_node(child_id)
                        && child.name.len() >= 5 {
                            expansion.push(child.name.clone());
                        }
                }
                expansion.truncate(5);

                if !expansion.is_empty() {
                    let expansion_query = expansion.join(" ");
                    let expanded = self.search(&expansion_query, top_k)?;

                    // Merge: original + 0.3x expanded
                    let mut merged = initial;
                    for (path, exp_score) in expanded {
                        merged
                            .entry(path)
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
            self.index
                .reader()
                .map(|r| r.searcher().num_docs())
                .unwrap_or(0)
        }
    }
