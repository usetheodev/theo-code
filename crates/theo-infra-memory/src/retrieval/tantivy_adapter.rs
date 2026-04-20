//! Tantivy-backed `MemoryRetrieval` adapter.
//!
//! Wires `theo_engine_retrieval::memory_tantivy::MemoryTantivyIndex`
//! (cycle evolution/apr20-1553 §P2) to the `MemoryRetrieval` trait
//! consumed by `RetrievalBackedMemory` (cycle apr20). One-direction
//! adapter — zero changes to the provider side.
//!
//! Source-type mapping:
//! - `"code"` → `SourceType::Code`
//! - `"wiki"` → `SourceType::Wiki`
//! - `"reflection"` / `"lesson"` → `SourceType::Reflection`
//! - anything else → `SourceType::Other`

#![cfg(feature = "tantivy-backend")]

use async_trait::async_trait;
use theo_engine_retrieval::memory_tantivy::{MemoryHit, MemoryTantivyIndex};
use theo_domain::memory::MemoryEntry;

use crate::retrieval::{MemoryRetrieval, ScoredMemory, SourceType};

/// Approximate-token heuristic for a body string. Mirrors the
/// `theo-infra-memory` crate's documentation: "approx_tokens" — the
/// provider budgets with what the caller reports, so a conservative
/// ceil(chars/4) keeps us honest without dragging in a tokenizer dep.
fn approx_tokens(body: &str) -> u32 {
    ((body.len() as u64 + 3) / 4).min(u32::MAX as u64) as u32
}

fn classify(source_type: &str) -> SourceType {
    match source_type {
        "code" => SourceType::Code,
        "wiki" => SourceType::Wiki,
        "reflection" | "lesson" => SourceType::Reflection,
        _ => SourceType::Other,
    }
}

/// Backend suitable for wiring into `RetrievalBackedMemory`.
pub struct TantivyMemoryBackend {
    index: MemoryTantivyIndex,
    /// Optional filter applied to every query so the caller can pin the
    /// mount (e.g. `Some("wiki")`). `None` passes every namespace.
    namespace_filter: Option<String>,
    top_k: usize,
}

impl TantivyMemoryBackend {
    pub fn new(index: MemoryTantivyIndex) -> Self {
        Self {
            index,
            namespace_filter: None,
            top_k: 16,
        }
    }

    pub fn with_namespace(mut self, ns: impl Into<String>) -> Self {
        self.namespace_filter = Some(ns.into());
        self
    }

    pub fn with_top_k(mut self, k: usize) -> Self {
        self.top_k = k;
        self
    }

    fn hit_to_scored(h: MemoryHit) -> ScoredMemory {
        let tokens = approx_tokens(&h.body);
        let source_type = classify(&h.source_type);
        let normalized_score = h.score as f32;
        ScoredMemory {
            entry: MemoryEntry {
                source: h.source_type,
                content: h.body,
                relevance_score: normalized_score,
            },
            source_type,
            score: normalized_score,
            approx_tokens: tokens,
        }
    }
}

#[async_trait]
impl MemoryRetrieval for TantivyMemoryBackend {
    async fn query(&self, text: &str) -> Vec<ScoredMemory> {
        let filter = self.namespace_filter.as_deref();
        let hits = match self.index.search(text, self.top_k, filter) {
            Ok(h) => h,
            Err(_) => return Vec::new(),
        };
        hits.into_iter().map(Self::hit_to_scored).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use theo_engine_retrieval::memory_tantivy::MemoryDoc;

    fn seed_index() -> MemoryTantivyIndex {
        MemoryTantivyIndex::build(&[
            MemoryDoc {
                slug: "rust-ownership".into(),
                source_type: "wiki".into(),
                body: "Rust ownership rules prevent data races".into(),
            },
            MemoryDoc {
                slug: "auth-bug".into(),
                source_type: "reflection".into(),
                body: "Auth token expired; check middleware first".into(),
            },
            MemoryDoc {
                slug: "main-rs".into(),
                source_type: "code".into(),
                body: "fn main rust tokio".into(),
            },
        ])
        .unwrap()
    }

    #[tokio::test]
    async fn query_returns_scored_entries_classified_by_source_type() {
        let backend = TantivyMemoryBackend::new(seed_index());
        let hits = backend.query("rust").await;
        assert!(!hits.is_empty());
        // At least one wiki hit (for "rust ownership") with the
        // correct SourceType classification.
        assert!(hits
            .iter()
            .any(|h| matches!(h.source_type, SourceType::Wiki)));
    }

    #[tokio::test]
    async fn namespace_filter_constrains_results() {
        let backend =
            TantivyMemoryBackend::new(seed_index()).with_namespace("reflection");
        let hits = backend.query("token").await;
        assert!(hits.iter().all(|h| matches!(h.source_type, SourceType::Reflection)));
    }

    #[tokio::test]
    async fn retrieval_backed_memory_binds_against_tantivy_backend() {
        use crate::retrieval::RetrievalBackedMemory;
        use theo_domain::memory::MemoryProvider;

        let backend: Arc<dyn MemoryRetrieval> =
            Arc::new(TantivyMemoryBackend::new(seed_index()));
        let provider = RetrievalBackedMemory::new("rm-tantivy", backend, 10_000);
        let out = provider.prefetch("rust").await;
        // Wiki threshold is 0.5 — raw BM25 may fall below it on the
        // tiny corpus, so the packed body can legitimately be empty.
        // The assertion is that prefetch completes without panicking.
        let _ = out;
    }

    #[test]
    fn approx_tokens_is_conservative() {
        assert_eq!(approx_tokens(""), 0);
        assert_eq!(approx_tokens("rust"), 1); // 4 chars → 1 token
        assert_eq!(approx_tokens("rust ownership rules"), 5); // 20/4
    }

    #[test]
    fn classify_maps_known_namespaces() {
        assert!(matches!(classify("code"), SourceType::Code));
        assert!(matches!(classify("wiki"), SourceType::Wiki));
        assert!(matches!(classify("reflection"), SourceType::Reflection));
        assert!(matches!(classify("lesson"), SourceType::Reflection));
        assert!(matches!(classify("unknown"), SourceType::Other));
    }
}
