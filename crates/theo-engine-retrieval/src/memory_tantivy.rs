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
    use std::path::Path;

    use tantivy::collector::TopDocs;
    use tantivy::directory::MmapDirectory;
    use tantivy::query::{BooleanQuery, Occur, Query, TermQuery};
    use tantivy::schema::{
        Field, IndexRecordOption, NumericOptions, STORED, STRING, Schema, TextFieldIndexing,
        TextOptions, Value,
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
        /// Phase 4 — session identifier (STRING+STORED), allows
        /// grouping transcript docs by session.
        f_session_id: Field,
        /// Phase 4 — turn index within a session (u64 STORED+FAST).
        f_turn_index: Field,
        /// Phase 4 — unix timestamp (seconds) for ordering.
        f_timestamp_unix: Field,
        /// Phase 4 — SHA-256 content hash used for idempotent
        /// re-indexing of the same session.
        f_content_hash: Field,
    }

    /// Transcript document — Phase 4 (PLAN_AUTO_EVOLUTION_SOTA).
    /// Stores one message of a conversation as a searchable record.
    #[derive(Debug, Clone)]
    pub struct TranscriptDoc {
        pub session_id: String,
        pub turn_index: u64,
        pub timestamp_unix: u64,
        pub role: String,
        pub body: String,
        pub content_hash: String,
    }

    /// Hit returned by transcript search.
    #[derive(Debug, Clone, PartialEq)]
    pub struct TranscriptHit {
        pub session_id: String,
        pub turn_index: u64,
        pub timestamp_unix: u64,
        pub role: String,
        pub body: String,
        pub score: f64,
    }

    fn body_text_options() -> TextOptions {
        TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(MEMORY_TOKENIZER)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
    }

    impl MemoryTantivyIndex {
        fn build_schema() -> (
            Schema,
            Field,
            Field,
            Field,
            Field,
            Field,
            Field,
            Field,
        ) {
            let mut schema_builder = Schema::builder();
            let f_slug = schema_builder.add_text_field("slug", STORED | STRING);
            let f_source_type = schema_builder.add_text_field("source_type", STORED | STRING);
            let f_body = {
                let opts = body_text_options().set_stored();
                schema_builder.add_text_field("body", opts)
            };
            let f_session_id = schema_builder.add_text_field("session_id", STORED | STRING);
            let u64_opts = NumericOptions::default().set_stored().set_fast();
            let f_turn_index = schema_builder.add_u64_field("turn_index", u64_opts.clone());
            let f_timestamp_unix = schema_builder.add_u64_field("timestamp_unix", u64_opts);
            let f_content_hash = schema_builder.add_text_field("content_hash", STORED | STRING);
            let schema = schema_builder.build();
            (
                schema,
                f_slug,
                f_source_type,
                f_body,
                f_session_id,
                f_turn_index,
                f_timestamp_unix,
                f_content_hash,
            )
        }

        fn register_tokenizer(index: &Index) {
            let analyzer = TextAnalyzer::builder(SimpleTokenizer::default())
                .filter(LowerCaser)
                .build();
            index.tokenizers().register(MEMORY_TOKENIZER, analyzer);
        }

        fn from_index(index: Index) -> Result<Self, tantivy::TantivyError> {
            let schema = index.schema();
            let f_slug = schema.get_field("slug")?;
            let f_source_type = schema.get_field("source_type")?;
            let f_body = schema.get_field("body")?;
            let f_session_id = schema.get_field("session_id")?;
            let f_turn_index = schema.get_field("turn_index")?;
            let f_timestamp_unix = schema.get_field("timestamp_unix")?;
            let f_content_hash = schema.get_field("content_hash")?;
            Self::register_tokenizer(&index);
            Ok(Self {
                index,
                f_slug,
                f_source_type,
                f_body,
                f_session_id,
                f_turn_index,
                f_timestamp_unix,
                f_content_hash,
            })
        }

        /// Build an in-RAM index from the given documents. Kept for
        /// backward compatibility with tests and callers that want a
        /// throw-away index.
        pub fn build(docs: &[MemoryDoc]) -> Result<Self, tantivy::TantivyError> {
            let (schema, ..) = Self::build_schema();
            let index = Index::create_in_ram(schema);
            let mut this = Self::from_index(index)?;
            this.upsert_memory_docs(docs)?;
            Ok(this)
        }

        /// Phase 4 (PLAN_AUTO_EVOLUTION_SOTA): open an existing
        /// on-disk index or create a fresh one at `index_dir`. The
        /// directory is created if missing.
        pub fn open_or_create(index_dir: &Path) -> Result<Self, tantivy::TantivyError> {
            std::fs::create_dir_all(index_dir)
                .map_err(|e| tantivy::TantivyError::IoError(std::sync::Arc::new(e)))?;
            let dir = MmapDirectory::open(index_dir)
                .map_err(tantivy::TantivyError::OpenDirectoryError)?;
            let (schema, ..) = Self::build_schema();
            let index = Index::open_or_create(dir, schema)?;
            Self::from_index(index)
        }

        fn upsert_memory_docs(&mut self, docs: &[MemoryDoc]) -> Result<(), tantivy::TantivyError> {
            let mut writer: IndexWriter = self.index.writer(15_000_000)?;
            for d in docs {
                writer.add_document(doc!(
                    self.f_slug => d.slug.as_str(),
                    self.f_source_type => d.source_type.as_str(),
                    self.f_body => d.body.as_str(),
                ))?;
            }
            writer.commit()?;
            Ok(())
        }

        /// Phase 4: append a batch of transcript docs and commit.
        /// Returns the number of docs added.
        pub fn add_transcripts(
            &mut self,
            docs: &[TranscriptDoc],
        ) -> Result<usize, tantivy::TantivyError> {
            if docs.is_empty() {
                return Ok(0);
            }
            let mut writer: IndexWriter = self.index.writer(15_000_000)?;
            for d in docs {
                writer.add_document(doc!(
                    self.f_slug => format!("{}:{}", d.session_id, d.turn_index),
                    self.f_source_type => "transcript",
                    self.f_body => d.body.as_str(),
                    self.f_session_id => d.session_id.as_str(),
                    self.f_turn_index => d.turn_index,
                    self.f_timestamp_unix => d.timestamp_unix,
                    self.f_content_hash => d.content_hash.as_str(),
                ))?;
            }
            writer.commit()?;
            Ok(docs.len())
        }

        /// Phase 4: search transcripts by BM25 score over body text.
        pub fn search_transcripts(
            &self,
            query: &str,
            top_k: usize,
        ) -> Result<Vec<TranscriptHit>, tantivy::TantivyError> {
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

            // Body tokens form an inner OR query so at least one must
            // match; the outer query requires the source_type filter
            // AND the inner OR.  Putting the body tokens as SHOULD at
            // the top level would let the source_type MUST match every
            // transcript regardless of query — exactly the bug a naive
            // port of FileTantivyIndex would introduce.
            let body_or: Vec<(Occur, Box<dyn Query>)> = tokens
                .iter()
                .map(|t| {
                    let q: Box<dyn Query> = Box::new(TermQuery::new(
                        tantivy::Term::from_field_text(self.f_body, t),
                        IndexRecordOption::WithFreqsAndPositions,
                    ));
                    (Occur::Should, q)
                })
                .collect();
            let body_query: Box<dyn Query> = Box::new(BooleanQuery::new(body_or));

            let subqueries: Vec<(Occur, Box<dyn Query>)> = vec![
                (Occur::Must, body_query),
                (
                    Occur::Must,
                    Box::new(TermQuery::new(
                        tantivy::Term::from_field_text(self.f_source_type, "transcript"),
                        IndexRecordOption::Basic,
                    )),
                ),
            ];

            let bool_query = BooleanQuery::new(subqueries);
            let top = searcher.search(&bool_query, &TopDocs::with_limit(top_k))?;

            let mut out = Vec::with_capacity(top.len());
            for (score, addr) in top {
                let doc: TantivyDocument = searcher.doc(addr)?;
                let session_id = doc
                    .get_first(self.f_session_id)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let turn_index = doc
                    .get_first(self.f_turn_index)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let timestamp_unix = doc
                    .get_first(self.f_timestamp_unix)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let body = doc
                    .get_first(self.f_body)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                // `role` is not a dedicated field; keep default for now
                // — persisted in the body when callers serialize it.
                let role = String::new();
                out.push(TranscriptHit {
                    session_id,
                    turn_index,
                    timestamp_unix,
                    role,
                    body,
                    score: score as f64,
                });
            }
            Ok(out)
        }

        /// Phase 4: check whether `session_id` has already been indexed
        /// with the given `content_hash`. Used as the idempotency key
        /// for transcript re-indexing on session reopen.
        pub fn contains_session_with_hash(
            &self,
            session_id: &str,
            content_hash: &str,
        ) -> Result<bool, tantivy::TantivyError> {
            let reader = self.index.reader()?;
            let searcher = reader.searcher();
            let subqueries: Vec<(Occur, Box<dyn Query>)> = vec![
                (
                    Occur::Must,
                    Box::new(TermQuery::new(
                        tantivy::Term::from_field_text(self.f_session_id, session_id),
                        IndexRecordOption::Basic,
                    )),
                ),
                (
                    Occur::Must,
                    Box::new(TermQuery::new(
                        tantivy::Term::from_field_text(self.f_content_hash, content_hash),
                        IndexRecordOption::Basic,
                    )),
                ),
            ];
            let q = BooleanQuery::new(subqueries);
            let hits = searcher.search(&q, &TopDocs::with_limit(1))?;
            Ok(!hits.is_empty())
        }

        /// Phase 4: fetch all messages from one session, sorted by
        /// turn_index. Used by the Tier-3 `memory_search` mode.
        pub fn session_transcript(
            &self,
            session_id: &str,
        ) -> Result<Vec<TranscriptHit>, tantivy::TantivyError> {
            let reader = self.index.reader()?;
            let searcher = reader.searcher();
            let subqueries: Vec<(Occur, Box<dyn Query>)> = vec![
                (
                    Occur::Must,
                    Box::new(TermQuery::new(
                        tantivy::Term::from_field_text(self.f_session_id, session_id),
                        IndexRecordOption::Basic,
                    )),
                ),
                (
                    Occur::Must,
                    Box::new(TermQuery::new(
                        tantivy::Term::from_field_text(self.f_source_type, "transcript"),
                        IndexRecordOption::Basic,
                    )),
                ),
            ];
            let q = BooleanQuery::new(subqueries);
            let top = searcher.search(&q, &TopDocs::with_limit(10_000))?;
            let mut out = Vec::with_capacity(top.len());
            for (score, addr) in top {
                let doc: TantivyDocument = searcher.doc(addr)?;
                out.push(TranscriptHit {
                    session_id: session_id.to_string(),
                    turn_index: doc
                        .get_first(self.f_turn_index)
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    timestamp_unix: doc
                        .get_first(self.f_timestamp_unix)
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    role: String::new(),
                    body: doc
                        .get_first(self.f_body)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    score: score as f64,
                });
            }
            out.sort_by_key(|h| h.turn_index);
            Ok(out)
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

        // ── Phase 4 (PLAN_AUTO_EVOLUTION_SOTA) ────────────────────

        fn transcript(session: &str, turn: u64, body: &str) -> TranscriptDoc {
            TranscriptDoc {
                session_id: session.into(),
                turn_index: turn,
                timestamp_unix: 1_700_000_000 + turn,
                role: "user".into(),
                body: body.into(),
                content_hash: format!("h{session}-{turn}"),
            }
        }

        #[test]
        fn open_or_create_persists_to_disk() {
            let tmp = tempfile::tempdir().expect("tmp");
            {
                let mut idx = MemoryTantivyIndex::open_or_create(tmp.path()).expect("open");
                idx.add_transcripts(&[
                    transcript("s1", 0, "user asked about bm25 scoring"),
                    transcript("s1", 1, "assistant explained term frequency"),
                ])
                .expect("add");
            }
            // Reopen the same dir — docs survive restart.
            let idx = MemoryTantivyIndex::open_or_create(tmp.path()).expect("reopen");
            let hits = idx
                .search_transcripts("bm25", 5)
                .expect("search transcripts");
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].session_id, "s1");
            assert_eq!(hits[0].turn_index, 0);
        }

        #[test]
        fn search_transcripts_ranks_by_bm25() {
            let tmp = tempfile::tempdir().expect("tmp");
            let mut idx = MemoryTantivyIndex::open_or_create(tmp.path()).expect("open");
            idx.add_transcripts(&[
                transcript("s1", 0, "unique term marshmallow"),
                transcript("s1", 1, "different body entirely"),
                transcript("s1", 2, "marshmallow marshmallow marshmallow"),
            ])
            .expect("add");

            let hits = idx
                .search_transcripts("marshmallow", 5)
                .expect("search");
            assert!(hits.len() >= 2);
            // The doc with 3x the term should outscore the one with 1x.
            let top = hits.iter().max_by(|a, b| a.score.partial_cmp(&b.score).unwrap());
            assert_eq!(top.expect("have hits").turn_index, 2);
        }

        #[test]
        fn search_transcripts_filters_out_plain_memory_docs() {
            let tmp = tempfile::tempdir().expect("tmp");
            let mut idx = MemoryTantivyIndex::open_or_create(tmp.path()).expect("open");
            idx.upsert_memory_docs(&[MemoryDoc {
                slug: "wiki-1".into(),
                source_type: "wiki".into(),
                body: "wiki body contains marshmallow note".into(),
            }])
            .expect("upsert");
            idx.add_transcripts(&[transcript("s1", 0, "transcript about marshmallow")])
                .expect("add");

            let hits = idx
                .search_transcripts("marshmallow", 5)
                .expect("search transcripts");
            // Must NOT include the wiki document.
            assert!(hits.iter().all(|h| h.session_id == "s1"));
        }

        #[test]
        fn contains_session_with_hash_is_idempotency_key() {
            let tmp = tempfile::tempdir().expect("tmp");
            let mut idx = MemoryTantivyIndex::open_or_create(tmp.path()).expect("open");
            assert!(!idx
                .contains_session_with_hash("s1", "h1")
                .expect("check"));
            idx.add_transcripts(&[TranscriptDoc {
                session_id: "s1".into(),
                turn_index: 0,
                timestamp_unix: 0,
                role: "user".into(),
                body: "hello".into(),
                content_hash: "h1".into(),
            }])
            .expect("add");
            assert!(idx.contains_session_with_hash("s1", "h1").expect("check"));
            assert!(!idx
                .contains_session_with_hash("s1", "different-hash")
                .expect("check"));
        }

        #[test]
        fn session_transcript_returns_messages_sorted_by_turn() {
            let tmp = tempfile::tempdir().expect("tmp");
            let mut idx = MemoryTantivyIndex::open_or_create(tmp.path()).expect("open");
            idx.add_transcripts(&[
                transcript("s1", 2, "third"),
                transcript("s1", 0, "first"),
                transcript("s1", 1, "second"),
                transcript("s2", 0, "other session"),
            ])
            .expect("add");
            let msgs = idx.session_transcript("s1").expect("session");
            assert_eq!(msgs.len(), 3);
            assert_eq!(msgs[0].turn_index, 0);
            assert_eq!(msgs[2].turn_index, 2);
            assert!(!msgs.iter().any(|h| h.session_id == "s2"));
        }

        #[test]
        fn add_transcripts_empty_is_noop() {
            let tmp = tempfile::tempdir().expect("tmp");
            let mut idx = MemoryTantivyIndex::open_or_create(tmp.path()).expect("open");
            let n = idx.add_transcripts(&[]).expect("empty add");
            assert_eq!(n, 0);
        }

        #[test]
        fn build_without_persistence_still_works() {
            // Ensure the legacy RAM-only path still works for tests that
            // don't care about disk.
            let idx = MemoryTantivyIndex::build(&fixtures()).expect("build ram");
            assert_eq!(idx.num_docs(), 4);
        }
    }
}

#[cfg(feature = "tantivy-backend")]
pub use inner::{
    MemoryDoc, MemoryHit, MemoryTantivyIndex, TranscriptDoc, TranscriptHit, hits_to_map,
};
