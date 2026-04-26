//! Full retrieval pipeline: RRF → Rerank → scored file map.
//!
//! Stage 1: BM25 + Tantivy + Dense → RRF fusion → top-50
//! Stage 2: Cross-encoder reranker → top-20
//!
//! Returns file_path → score map for assembly.
//!
//! T8.1 — Whole module gated by `dense-retrieval` because RRF needs
//! the tantivy index. The reranker itself is always compiled (see
//! `reranker` module). Whether to invoke this pipeline vs. plain RRF
//! is a runtime config choice, not a build feature.

mod inner {
    use std::collections::HashMap;

    use theo_engine_graph::model::CodeGraph;

    use crate::embedding::cache::EmbeddingCache;
    use crate::embedding::neural::NeuralEmbedder;
    use crate::reranker::{CrossEncoderConfig, CrossEncoderReranker};
    use crate::tantivy_search::{FileTantivyIndex, hybrid_rrf_search};

    /// Run the full retrieval pipeline.
    ///
    /// Stage 1: RRF 3-ranker fusion (BM25 + Tantivy + Dense) → top candidates
    /// Stage 2: Cross-encoder reranking → refined top-K
    ///
    /// Returns file_path → score mapping.
    ///
    /// LEGACY: this entry point requires the reranker to be enabled.
    /// Prefer `retrieve_with_config` for the runtime-gated path.
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
        let config = CrossEncoderConfig {
            use_reranker: true,
            top_k: rerank_top_k,
            max_candidates: 50,
        };
        retrieve_with_config(
            graph,
            tantivy_index,
            embedder,
            cache,
            Some(reranker),
            query,
            rrf_k,
            &config,
        )
    }

    /// T8.1 — Runtime-gated retrieval pipeline.
    ///
    /// Honours `CrossEncoderConfig::use_reranker`. When the flag is
    /// `false` OR `reranker` is `None`, returns the RRF top-K
    /// unchanged. When the flag is `true` AND a reranker is supplied,
    /// runs Stage 2 (cross-encoder rerank).
    ///
    /// This decouples build (always compiled) from runtime (config
    /// flag). Callers choose per-request without any rebuild.
    pub fn retrieve_with_config(
        graph: &CodeGraph,
        tantivy_index: &FileTantivyIndex,
        embedder: &NeuralEmbedder,
        cache: &EmbeddingCache,
        reranker: Option<&CrossEncoderReranker>,
        query: &str,
        rrf_k: f64,
        config: &CrossEncoderConfig,
    ) -> HashMap<String, f64> {
        // Stage 1: RRF fusion (always runs — gives the candidate set).
        let rrf_scores = hybrid_rrf_search(graph, tantivy_index, embedder, cache, query, rrf_k);
        if rrf_scores.is_empty() {
            return HashMap::new();
        }

        // Sort RRF candidates by score (high → low).
        let mut sorted: Vec<(String, f64)> = rrf_scores.into_iter().collect();
        sorted.sort_by(|a, b| {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Runtime gate: skip the reranker stage when off OR unavailable.
        let Some(rr) = reranker else {
            return rrf_top_k_only(sorted, config.top_k);
        };
        if !config.use_reranker {
            return rrf_top_k_only(sorted, config.top_k);
        }

        // Stage 2: Cross-encoder reranking.
        sorted.truncate(config.max_candidates);
        rr.rerank(query, &sorted, graph, config.top_k)
    }

    /// Helper used when the reranker is bypassed — return the top-K
    /// of the RRF result as a HashMap. Bounded by `top_k` so callers
    /// see consistent shape regardless of which path ran.
    fn rrf_top_k_only(
        mut sorted: Vec<(String, f64)>,
        top_k: usize,
    ) -> HashMap<String, f64> {
        sorted.truncate(top_k);
        sorted.into_iter().collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn t81pl_rrf_top_k_only_preserves_score_and_bounds_k() {
            let sorted = vec![
                ("a.rs".to_string(), 0.9),
                ("b.rs".to_string(), 0.5),
                ("c.rs".to_string(), 0.2),
            ];
            let out = rrf_top_k_only(sorted, 2);
            assert_eq!(out.len(), 2);
            assert_eq!(out.get("a.rs"), Some(&0.9));
            assert_eq!(out.get("b.rs"), Some(&0.5));
            assert!(out.get("c.rs").is_none());
        }

        #[test]
        fn t81pl_rrf_top_k_only_handles_empty() {
            let out = rrf_top_k_only(Vec::new(), 5);
            assert!(out.is_empty());
        }

        #[test]
        fn t81pl_rrf_top_k_only_handles_top_k_larger_than_input() {
            let sorted = vec![("only.rs".to_string(), 1.0)];
            let out = rrf_top_k_only(sorted, 99);
            assert_eq!(out.len(), 1);
            assert_eq!(out.get("only.rs"), Some(&1.0));
        }
    }
}

pub use inner::{retrieve_and_rerank, retrieve_with_config};
