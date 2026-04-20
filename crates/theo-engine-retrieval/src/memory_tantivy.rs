//! Tantivy-backed index dedicated to the *memory* namespace.
//!
//! Strictly separate from `FileTantivyIndex` (which indexes the
//! `CodeGraph` over code files). Memory docs are lessons, wiki
//! pages, and journal entries. Mixing the two would require reconciling
//! disparate field schemas and lifecycles — instead we keep two
//! small, orthogonal Tantivy indices, one per mount.
//!
//! Research: `.theo/evolution_research.md` §P2 (cycle
//! evolution/apr20-1553) — closes the concrete backend gap behind
//! `theo_infra_memory::MemoryRetrieval`.
//!
//! Feature-gated on `tantivy-backend` to match the existing index.

#[cfg(feature = "tantivy-backend")]
mod inner {
    use std::collections::HashMap;

    use tantivy::collector::TopDocs;
    use tantivy::query::{BooleanQuery, Occur, Query, TermQuery};
    use tantivy::schema::{
        Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
    };
    use tantivy::tokenizer::{LowerCaser, SimpleTokenizer, TextAnalyzer};
    use tantivy::{Index, IndexWriter, TantivyDocument, doc};

    const MEMORY_TOKENIZER: &str = "memory_simple";

    /// One input doc. `source_type` is an opaque label ("code", "wiki",
    /// "reflection", "other") the caller uses to filter at query time.
    #[derive(Debug, Clone)]
    pub struct MemoryDoc {
        pub slug: String,
        pub source_type: String,
        pub body: String,
    }

    /// One returned hit. `score` is Tantivy's raw BM25; callers combine
    /// with their own threshold configs.
    #[derive(Debug, Clone, PartialEq)]
    pub struct MemoryHit {
        pub slug: String,
        pub source_type: String,
        pub body: String,
        pub score: f64,
    }

    pub struct MemoryTantivyIndex {
        index: Index,
        f_slug: Field,
        f_source_type: Field,
        f_body: Field,
    }

    fn body_text_options() -> TextOptions {
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(MEMORY_TOKENIZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
    }

    impl MemoryTantivyIndex {
        pub fn build(docs: &[MemoryDoc]) -> Result<Self, tantivy::TantivyError> {
            let mut schema_builder = Schema::builder();
            let f_slug = schema_builder.add_text_field("slug", STORED | STRING);
            let f_source_type = schema_builder.add_text_field("source_type", STORED | STRING);
            let f_body = {
                let opts = body_text_options().set_stored();
                schema_builder.add_text_field("body", opts)
            };
            let schema = schema_builder.build();

            let index = Index::create_in_ram(schema);

            let analyzer = TextAnalyzer::builder(SimpleTokenizer::default())
                .filter(LowerCaser)
                .build();
            index.tokenizers().register(MEMORY_TOKENIZER, analyzer);

            let mut writer: IndexWriter = index.writer(15_000_000)?; // 15MB heap — tiny

            for d in docs {
                writer.add_document(doc!(
                    f_slug => d.slug.as_str(),
                    f_source_type => d.source_type.as_str(),
                    f_body => d.body.as_str(),
                ))?;
            }
            writer.commit()?;

            Ok(Self {
                index,
                f_slug,
                f_source_type,
                f_body,
            })
        }

        /// Search body text. When `source_type_filter` is `Some`, only
        /// docs whose `source_type` matches exactly are returned.
        pub fn search(
            &self,
            query: &str,
            top_k: usize,
            source_type_filter: Option<&str>,
        ) -> Result<Vec<MemoryHit>, tantivy::TantivyError> {
            let reader = self.index.reader()?;
            let searcher = reader.searcher();

            let tokens: Vec<String> = query
                .split_whitespace()
                .map(|t| t.to_lowercase())
                .filter(|t| !t.is_empty())
                .collect();
            if tokens.is_empty() {
                return Ok(Vec::new());
            }

            let mut subqueries: Vec<(Occur, Box<dyn Query>)> = tokens
                .iter()
                .map(|t| {
                    let q: Box<dyn Query> = Box::new(TermQuery::new(
                        tantivy::Term::from_field_text(self.f_body, t),
                        IndexRecordOption::WithFreqsAndPositions,
                    ));
                    (Occur::Should, q)
                })
                .collect();

            if let Some(st) = source_type_filter {
                let q: Box<dyn Query> = Box::new(TermQuery::new(
                    tantivy::Term::from_field_text(self.f_source_type, st),
                    IndexRecordOption::Basic,
                ));
                subqueries.push((Occur::Must, q));
            }

            let bool_query = BooleanQuery::new(subqueries);
            let top = searcher.search(&bool_query, &TopDocs::with_limit(top_k))?;

            let mut out = Vec::with_capacity(top.len());
            for (score, doc_addr) in top {
                let doc: TantivyDocument = searcher.doc(doc_addr)?;
                let slug = doc
                    .get_first(self.f_slug)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let source_type = doc
                    .get_first(self.f_source_type)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let body = doc
                    .get_first(self.f_body)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                out.push(MemoryHit {
                    slug,
                    source_type,
                    body,
                    score: score as f64,
                });
            }
            Ok(out)
        }

        /// Doc count, useful for tests + telemetry.
        pub fn num_docs(&self) -> u64 {
            let reader = match self.index.reader() {
                Ok(r) => r,
                Err(_) => return 0,
            };
            reader.searcher().num_docs()
        }
    }

    /// Flat (slug → body) map — retained for parity with
    /// `FileTantivyIndex::search` callers that want a HashMap view.
    pub fn hits_to_map(hits: &[MemoryHit]) -> HashMap<String, f64> {
        let mut m = HashMap::with_capacity(hits.len());
        for h in hits {
            m.insert(h.slug.clone(), h.score);
        }
        m
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn fixtures() -> Vec<MemoryDoc> {
            vec![
                MemoryDoc {
                    slug: "rust-ownership".into(),
                    source_type: "wiki".into(),
                    body: "Rust ownership rules prevent data races at compile time".into(),
                },
                MemoryDoc {
                    slug: "tokio-runtime".into(),
                    source_type: "wiki".into(),
                    body: "Tokio provides async runtime executor".into(),
                },
                MemoryDoc {
                    slug: "fix-auth-bug".into(),
                    source_type: "reflection".into(),
                    body: "Auth token expired check missing in middleware".into(),
                },
                MemoryDoc {
                    slug: "lib-rs".into(),
                    source_type: "code".into(),
                    body: "pub mod memory pub mod decay".into(),
                },
            ]
        }

        #[test]
        fn build_indexes_all_docs() {
            let idx = MemoryTantivyIndex::build(&fixtures()).expect("test fixture ok");
            assert_eq!(idx.num_docs(), 4);
        }

        #[test]
        fn search_returns_scored_hits_for_matching_body() {
            let idx = MemoryTantivyIndex::build(&fixtures()).expect("test fixture ok");
            let hits = idx.search("ownership", 10, None).expect("test fixture ok");
            assert!(!hits.is_empty());
            assert!(hits.iter().any(|h| h.slug == "rust-ownership"));
        }

        #[test]
        fn source_type_filter_narrows_results() {
            let idx = MemoryTantivyIndex::build(&fixtures()).expect("test fixture ok");
            // Query term present in both wiki and code docs.
            let all = idx.search("rust ownership memory", 10, None).expect("test fixture ok");
            let wiki_only = idx
                .search("rust ownership memory", 10, Some("wiki"))
                .expect("test fixture ok");
            assert!(wiki_only.iter().all(|h| h.source_type == "wiki"));
            assert!(
                wiki_only.len() <= all.len(),
                "filter cannot expand the set"
            );
        }

        #[test]
        fn empty_query_returns_no_hits() {
            let idx = MemoryTantivyIndex::build(&fixtures()).expect("test fixture ok");
            assert!(idx.search("", 10, None).expect("test fixture ok").is_empty());
            assert!(idx.search("   ", 10, None).expect("test fixture ok").is_empty());
        }

        #[test]
        fn non_matching_filter_returns_empty() {
            let idx = MemoryTantivyIndex::build(&fixtures()).expect("test fixture ok");
            let hits = idx.search("ownership", 10, Some("nonexistent-ns")).expect("test fixture ok");
            assert!(hits.is_empty());
        }

        #[test]
        fn hits_to_map_preserves_scores() {
            let idx = MemoryTantivyIndex::build(&fixtures()).expect("test fixture ok");
            let hits = idx.search("ownership", 10, None).expect("test fixture ok");
            let map = hits_to_map(&hits);
            assert_eq!(map.len(), hits.len());
            for h in &hits {
                assert_eq!(map.get(&h.slug).copied(), Some(h.score));
            }
        }
    }
}

#[cfg(feature = "tantivy-backend")]
pub use inner::{MemoryDoc, MemoryHit, MemoryTantivyIndex, hits_to_map};
