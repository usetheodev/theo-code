//! Dense vector index over wiki pages — Phase 0 / T0.1 of the
//! `wiki-graph-memory-blend-retrieval-plan`.
//!
//! Persists one f32 vector per wiki page, keyed by page slug. Schema
//! versioning + embedder-id tagging means a model swap (e.g. AllMiniLM →
//! Jina Code) triggers a graceful cold rebuild rather than mismatched
//! vectors silently degrading retrieval.
//!
//! The retrieval crate consumes this index via `WikiRetriever` (Phase 1);
//! the wiki module owns the index lifecycle (build / save / load /
//! invalidate) per ADR D1.
//!
//! # Persistence format
//!
//! `bincode`-encoded `WikiDenseIndex` (schema, embedder_id, dim, vectors).
//! Schema bumps trigger cold rebuild; mismatched embedder_id or dim returns
//! `None` from `load` so callers fall back to rebuild instead of using
//! stale data.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::wiki::model::WikiDoc;

/// Bumped on breaking changes to the on-disk format.
const SCHEMA_VERSION: u32 = 1;

/// Maximum number of body characters folded into the embedding text per
/// page. Captures the human-authored bridge (summary + tags) plus enough
/// context without exceeding typical embedder windows.
pub const EMBEDDING_BODY_LIMIT: usize = 2000;

/// Trait the dense index uses to obtain embeddings.
///
/// Implemented by `crate::embedding::neural::NeuralEmbedder` (production)
/// and by simple fakes in unit tests. The `id()` is persisted alongside
/// vectors so a swap to a different embedder triggers a cold rebuild
/// rather than producing scoring nonsense.
pub trait Embedder: Send + Sync {
    /// Stable identifier for this embedder family + version (e.g.
    /// "AllMiniLM-L6-v2-Q-384"). Mismatch on load → cold rebuild.
    fn id(&self) -> &str;
    /// Output vector dimension. Mismatch on load → cold rebuild.
    fn dim(&self) -> usize;
    /// Embed a batch of texts. Returned vector MUST have the same outer
    /// length as `texts`. Per-page dim mismatches are handled by the
    /// caller (page is skipped) so implementations may return a wrong
    /// dim per item without panicking the build.
    fn embed_batch(&self, texts: &[&str]) -> Vec<Vec<f32>>;
}

/// Persisted dense vector index over wiki pages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiDenseIndex {
    schema: u32,
    pub embedder_id: String,
    pub dim: u32,
    pub vectors: HashMap<String, Vec<f32>>,
}

impl WikiDenseIndex {
    /// Construct an empty index tagged with the given embedder identity.
    pub fn empty(embedder_id: impl Into<String>, dim: usize) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            embedder_id: embedder_id.into(),
            dim: dim as u32,
            vectors: HashMap::new(),
        }
    }

    /// Number of vectors stored.
    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    /// Whether the index has zero vectors.
    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    /// Build the embedding text for a single page.
    ///
    /// Layout: summary, tags, crate description, module doc, slug —
    /// truncated at `EMBEDDING_BODY_LIMIT` characters. Empty fields are
    /// skipped so a page with only a slug still produces a non-empty
    /// text (and therefore a non-degenerate vector).
    pub fn embedding_text_for_page(doc: &WikiDoc) -> String {
        let mut out = String::with_capacity(EMBEDDING_BODY_LIMIT);
        if !doc.summary.is_empty() {
            out.push_str(doc.summary.trim());
            out.push('\n');
        }
        if !doc.tags.is_empty() {
            out.push_str(&doc.tags.join(" "));
            out.push('\n');
        }
        if let Some(desc) = &doc.crate_description
            && !desc.is_empty()
        {
            out.push_str(desc.trim());
            out.push('\n');
        }
        if let Some(doc_block) = &doc.module_doc
            && !doc_block.is_empty()
        {
            out.push_str(doc_block.trim());
            out.push('\n');
        }
        out.push_str(&doc.slug);
        if out.len() > EMBEDDING_BODY_LIMIT {
            let mut cut = EMBEDDING_BODY_LIMIT;
            while !out.is_char_boundary(cut) && cut > 0 {
                cut -= 1;
            }
            out.truncate(cut);
        }
        out
    }

    /// Build a fresh index from the given pages using the embedder.
    ///
    /// On per-page embedder failure (vector length != `embedder.dim()`),
    /// the page is skipped — no panic. Empty input → empty index.
    pub fn build_from_pages<E: Embedder + ?Sized>(docs: &[WikiDoc], embedder: &E) -> Self {
        let mut index = Self::empty(embedder.id(), embedder.dim());
        if docs.is_empty() {
            return index;
        }
        let texts: Vec<String> = docs.iter().map(Self::embedding_text_for_page).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let vectors = embedder.embed_batch(&refs);
        let expected_dim = embedder.dim();
        for (doc, vec) in docs.iter().zip(vectors) {
            if vec.len() == expected_dim {
                index.vectors.insert(doc.slug.clone(), vec);
            }
        }
        index
    }

    /// Get a vector by page slug.
    pub fn get(&self, page_id: &str) -> Option<&[f32]> {
        self.vectors.get(page_id).map(Vec::as_slice)
    }

    /// Drop the vector for a single page.
    pub fn invalidate(&mut self, page_id: &str) {
        self.vectors.remove(page_id);
    }

    /// Persist to disk using `bincode`.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let bytes = bincode::serialize(self).map_err(std::io::Error::other)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)
    }

    /// Load from disk. Returns `None` (not Err) on:
    ///
    /// * missing file (cold start);
    /// * deserialization failure (corrupt or pre-schema-bump);
    /// * schema mismatch;
    /// * embedder-id mismatch (different model);
    /// * dim mismatch (different model variant).
    ///
    /// Caller treats `None` as a signal to rebuild from pages.
    pub fn load(path: &Path, expected_embedder_id: &str, expected_dim: usize) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let index: WikiDenseIndex = bincode::deserialize(&bytes).ok()?;
        if index.schema != SCHEMA_VERSION
            || index.embedder_id != expected_embedder_id
            || index.dim as usize != expected_dim
        {
            return None;
        }
        Some(index)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki::model::TestCoverage;

    /// Deterministic in-memory embedder for unit tests.
    struct FakeEmbedder {
        id: String,
        dim: usize,
    }

    impl FakeEmbedder {
        fn new(id: &str, dim: usize) -> Self {
            Self {
                id: id.to_string(),
                dim,
            }
        }
    }

    impl Embedder for FakeEmbedder {
        fn id(&self) -> &str {
            &self.id
        }
        fn dim(&self) -> usize {
            self.dim
        }
        fn embed_batch(&self, texts: &[&str]) -> Vec<Vec<f32>> {
            texts
                .iter()
                .map(|t| {
                    let mut v = vec![0.0_f32; self.dim];
                    for (i, b) in t.bytes().enumerate() {
                        v[i % self.dim] += (b as f32) / 255.0;
                    }
                    v
                })
                .collect()
        }
    }

    /// Embedder that returns the wrong dim — tests robustness.
    struct BrokenEmbedder {
        id: String,
        claimed_dim: usize,
        actual_dim: usize,
    }

    impl Embedder for BrokenEmbedder {
        fn id(&self) -> &str {
            &self.id
        }
        fn dim(&self) -> usize {
            self.claimed_dim
        }
        fn embed_batch(&self, texts: &[&str]) -> Vec<Vec<f32>> {
            texts.iter().map(|_| vec![0.0; self.actual_dim]).collect()
        }
    }

    fn make_doc(slug: &str, summary: &str, tags: &[&str]) -> WikiDoc {
        WikiDoc {
            slug: slug.to_string(),
            title: slug.to_string(),
            community_id: format!("c-{}", slug),
            file_count: 0,
            symbol_count: 0,
            primary_language: "rust".to_string(),
            files: vec![],
            entry_points: vec![],
            public_api: vec![],
            dependencies: vec![],
            call_flow: vec![],
            test_coverage: TestCoverage {
                tested: 0,
                total: 0,
                percentage: 0.0,
                untested: vec![],
            },
            source_refs: vec![],
            summary: summary.to_string(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            crate_description: None,
            module_doc: None,
            generated_at: "2026-04-30T00:00:00Z".to_string(),
            enriched: false,
        }
    }

    #[test]
    fn test_dense_index_build_from_empty_returns_empty() {
        let embedder = FakeEmbedder::new("fake-384", 384);
        let index = WikiDenseIndex::build_from_pages(&[], &embedder);
        assert!(index.is_empty());
        assert_eq!(index.embedder_id, "fake-384");
        assert_eq!(index.dim, 384);
    }

    #[test]
    fn test_dense_index_build_from_3_pages_has_3_vectors() {
        let embedder = FakeEmbedder::new("fake-16", 16);
        let docs = vec![
            make_doc("page-a", "summary a", &["tag1"]),
            make_doc("page-b", "summary b", &[]),
            make_doc("page-c", "", &[]),
        ];
        let index = WikiDenseIndex::build_from_pages(&docs, &embedder);
        assert_eq!(index.len(), 3);
        assert!(index.get("page-a").is_some());
        assert!(index.get("page-b").is_some());
        assert!(index.get("page-c").is_some());
        for slug in ["page-a", "page-b", "page-c"] {
            assert_eq!(index.get(slug).unwrap().len(), 16);
        }
    }

    #[test]
    fn test_dense_index_save_load_round_trip_preserves_vectors() {
        let embedder = FakeEmbedder::new("fake-32", 32);
        let docs = vec![make_doc("page-a", "alpha", &["x", "y"])];
        let original = WikiDenseIndex::build_from_pages(&docs, &embedder);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dense.bin");
        original.save(&path).unwrap();

        let loaded = WikiDenseIndex::load(&path, "fake-32", 32).expect("load");
        assert_eq!(loaded.len(), original.len());
        for (slug, vec) in &original.vectors {
            let other = loaded.get(slug).unwrap();
            assert_eq!(vec.as_slice(), other);
        }
    }

    #[test]
    fn test_dense_index_load_returns_none_on_dim_mismatch() {
        let embedder = FakeEmbedder::new("fake-32", 32);
        let docs = vec![make_doc("p", "s", &[])];
        let original = WikiDenseIndex::build_from_pages(&docs, &embedder);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dense.bin");
        original.save(&path).unwrap();

        let loaded = WikiDenseIndex::load(&path, "fake-32", 64);
        assert!(loaded.is_none());
    }

    #[test]
    fn test_dense_index_load_returns_none_on_embedder_id_mismatch() {
        let embedder = FakeEmbedder::new("fake-32", 32);
        let docs = vec![make_doc("p", "s", &[])];
        let original = WikiDenseIndex::build_from_pages(&docs, &embedder);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dense.bin");
        original.save(&path).unwrap();

        let loaded = WikiDenseIndex::load(&path, "different-32", 32);
        assert!(loaded.is_none());
    }

    #[test]
    fn test_dense_index_invalidate_removes_only_target_entry() {
        let embedder = FakeEmbedder::new("fake-8", 8);
        let docs = vec![make_doc("a", "x", &[]), make_doc("b", "y", &[])];
        let mut index = WikiDenseIndex::build_from_pages(&docs, &embedder);
        assert_eq!(index.len(), 2);
        index.invalidate("a");
        assert!(index.get("a").is_none());
        assert!(index.get("b").is_some());
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn test_dense_index_get_returns_none_for_unknown_id() {
        let embedder = FakeEmbedder::new("fake-8", 8);
        let docs = vec![make_doc("a", "x", &[])];
        let index = WikiDenseIndex::build_from_pages(&docs, &embedder);
        assert!(index.get("nonexistent").is_none());
    }

    #[test]
    fn test_dense_index_load_returns_none_on_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.bin");
        let loaded = WikiDenseIndex::load(&path, "fake-32", 32);
        assert!(loaded.is_none());
    }

    #[test]
    fn test_dense_index_skips_pages_with_wrong_dim_vectors() {
        let embedder = BrokenEmbedder {
            id: "broken".to_string(),
            claimed_dim: 16,
            actual_dim: 8,
        };
        let docs = vec![make_doc("a", "x", &[])];
        let index = WikiDenseIndex::build_from_pages(&docs, &embedder);
        assert!(index.get("a").is_none());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn test_embedding_text_for_page_includes_summary_tags_and_slug() {
        let doc = make_doc("module-x", "summary text", &["alpha", "beta"]);
        let text = WikiDenseIndex::embedding_text_for_page(&doc);
        assert!(text.contains("summary text"));
        assert!(text.contains("alpha"));
        assert!(text.contains("beta"));
        assert!(text.contains("module-x"));
    }

    #[test]
    fn test_embedding_text_for_page_handles_empty_fields() {
        let doc = make_doc("solo-slug", "", &[]);
        let text = WikiDenseIndex::embedding_text_for_page(&doc);
        assert!(text.contains("solo-slug"));
        assert!(!text.is_empty());
    }
}
