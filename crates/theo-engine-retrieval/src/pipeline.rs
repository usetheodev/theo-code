//! Full retrieval pipeline: RRF → Rerank → scored file map.
//!
//! Stage 1: BM25 + Tantivy + Dense → RRF fusion → top-50
//! Stage 2: Cross-encoder reranker → top-20
//!
//! Returns file_path → score map for assembly.

#[cfg(feature = "reranker")]
mod inner {
    use std::collections::HashMap;

    use theo_engine_graph::model::CodeGraph;

    use crate::embedding::cache::EmbeddingCache;
    use crate::embedding::neural::NeuralEmbedder;
    use crate::reranker::CrossEncoderReranker;
    use crate::tantivy_search::{FileTantivyIndex, hybrid_rrf_search};

    /// Run the full retrieval pipeline.
    ///
    /// Stage 1: RRF 3-ranker fusion (BM25 + Tantivy + Dense) → top candidates
    /// Stage 2: Cross-encoder reranking → refined top-K
    ///
    /// Returns file_path → score mapping.
    pub fn retrieve_and_rerank(
        graph: &CodeGraph,
        tantivy_index: &FileTantivyIndex,
        embedder: &NeuralEmbedder,
        cache: &EmbeddingCache,
        reranker: &CrossEncoderReranker,
        query: &str,
        rrf_k: f64,
        rerank_top_k: usize,
    ) -> HashMap<String, f64> {
        // Stage 1: RRF fusion (already filters test/benchmark files)
        let rrf_scores = hybrid_rrf_search(graph, tantivy_index, embedder, cache, query, rrf_k);

        if rrf_scores.is_empty() {
            return HashMap::new();
        }

        // Sort by RRF score for reranker input
        let mut sorted: Vec<_> = rrf_scores.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Cap at 50 for reranker (governance condition)
        sorted.truncate(50);

        // Stage 2: Cross-encoder reranking
        let candidates: Vec<(String, f64)> = sorted;
        reranker.rerank(query, &candidates, graph, rerank_top_k)
    }
}

#[cfg(feature = "reranker")]
pub use inner::retrieve_and_rerank;
