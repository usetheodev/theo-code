//! Single-purpose slice extracted from `search.rs` (T4.3 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::HashMap;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, NodeType};

use crate::graph_attention::propagate_attention;
use crate::neural::NeuralEmbedder;
use crate::tfidf::{TfidfConfig, TfidfModel};
use crate::turboquant::{QuantizedVector, TurboQuantizer};

use super::*;

impl MultiSignalScorer {
    /// Build the scorer from communities and graph data.
    pub fn build(communities: &[Community], graph: &CodeGraph) -> Self {
        let bm25_index = Bm25Index::build(communities, graph);
        let centrality_scores = community_pagerank(communities, graph);
        let recency_scores = community_recency(communities, graph);

        // Pre-compute and pre-tokenize symbol names per community for file-level matching.
        // Uses contains_children index for O(degree) instead of O(total_edges).
        // Pre-tokenizes to avoid re-tokenizing on every query.
        let mut community_symbol_tokens: HashMap<String, Vec<std::collections::HashSet<String>>> =
            HashMap::new();
        for comm in communities {
            let mut token_sets = Vec::new();
            for node_id in &comm.node_ids {
                if let Some(node) = graph.get_node(node_id) {
                    token_sets.push(tokenise(&node.name).into_iter().collect());
                    if matches!(node.node_type, NodeType::File) {
                        for child_id in graph.contains_children(node_id) {
                            if let Some(child) = graph.get_node(child_id) {
                                token_sets.push(tokenise(&child.name).into_iter().collect());
                            }
                        }
                    }
                }
            }
            community_symbol_tokens.insert(comm.id.clone(), token_sets);
        }

        // Build community documents (shared by both neural and TF-IDF paths).
        let community_docs: Vec<String> = communities
            .iter()
            .map(|c| community_document(c, graph))
            .collect();

        // Try neural embeddings first; fall back to TF-IDF if model init fails.
        // In BOTH cases, doc vectors are compressed via TurboQuant for memory efficiency.
        let tfidf_config = TfidfConfig::default();
        let tfidf_model = TfidfModel::build(&community_docs, &tfidf_config);

        // Neural embeddings are opt-in via THEO_NEURAL=1 env var.
        // Default = BM25/TF-IDF only (80% of signals). Neural adds 20% semantic quality
        // but costs ~28s for model load + embedding. Disabled by default for responsiveness.
        let neural_enabled = std::env::var("THEO_NEURAL").is_ok();
        let embedder_result: Result<
            crate::embedding::neural::NeuralEmbedder,
            Box<dyn std::error::Error>,
        > = if neural_enabled {
            NeuralEmbedder::new()
        } else {
            Err("Neural embeddings disabled (set THEO_NEURAL=1 to enable)".into())
        };

        let (embedder, quantizer, quantized_docs, using_neural) = match embedder_result {
            Ok(emb) => {
                // Neural path: embed → TurboQuant compress (384-dim → 96 bytes)
                let doc_refs: Vec<&str> = community_docs.iter().map(|s| s.as_str()).collect();
                let embeddings = emb.embed_batch(&doc_refs);
                let quantizer = TurboQuantizer::new(emb.dim(), 42);
                let mut qmap = HashMap::with_capacity(communities.len());
                for (comm, emb_vec) in communities.iter().zip(embeddings.iter()) {
                    let qv = quantizer.quantize(emb_vec);
                    qmap.insert(comm.id.clone(), qv);
                }
                (Some(emb), quantizer, qmap, true)
            }
            Err(_) => {
                // TF-IDF fallback: TF-IDF → TurboQuant compress (128-dim → 32 bytes)
                let quantizer = TurboQuantizer::new(tfidf_config.target_dim, 42);
                let mut qmap = HashMap::with_capacity(communities.len());
                for (i, comm) in communities.iter().enumerate() {
                    let dense = tfidf_model.transform_normalized(&community_docs[i]);
                    let qv = quantizer.quantize(&dense);
                    qmap.insert(comm.id.clone(), qv);
                }
                (None, quantizer, qmap, false)
            }
        };

        MultiSignalScorer {
            bm25_index,
            centrality_scores,
            recency_scores,
            community_symbol_tokens,
            embedder,
            quantizer,
            quantized_docs,
            tfidf_model,
            using_neural,
            weights: [0.25, 0.20, 0.20, 0.15, 0.10, 0.10],
            scoring_weights: ScoringWeights::default(),
        }
    }

    /// Score communities using normalized weighted sum of 6 signals.
    ///
    /// Weights: BM25 (25%) + Semantic (20%) + File boost (20%) + Graph attention (15%)
    ///          + Centrality (10%) + Recency (10%)
    ///
    /// Graph attention captures transitive relevance through the code graph,
    /// discovering related code that BM25 and embeddings miss.
    pub fn score(
        &self,
        query: &str,
        communities: &[Community],
        graph: &CodeGraph,
    ) -> Vec<ScoredCommunity> {
        if communities.is_empty() {
            return Vec::new();
        }
        let query_tokens: std::collections::HashSet<String> =
            tokenise(query).into_iter().collect();
        let large_repo = communities.len() > 500;
        if large_repo {
            eprintln!(
                "[tiered] fast path: {} communities > 500, skipping neural + graph attention",
                communities.len()
            );
        }

        let bm25_norm = compute_bm25_scores(graph, query, communities);
        // Semantic + graph attention are computed for side-effects (warmup +
        // future weighted combo); current weights only consume BM25 + file
        // boost + centrality + recency. Skipped for large repos.
        if !large_repo {
            self.compute_semantic_scores(query, communities);
            self.compute_graph_attention_scores(graph, communities, &query_tokens);
        }
        let (w_bm25, w_file, w_cent, w_rec) = (
            self.scoring_weights.bm25,
            self.scoring_weights.file_boost,
            self.scoring_weights.centrality,
            self.scoring_weights.recency,
        );
        let mut result: Vec<ScoredCommunity> = communities
            .iter()
            .map(|comm| {
                let norm_bm25 = bm25_norm.get(comm.id.as_str()).copied().unwrap_or(0.0);
                let centrality = *self.centrality_scores.get(&comm.id).unwrap_or(&0.0);
                let recency = *self.recency_scores.get(&comm.id).unwrap_or(&0.0);
                let file_boost = self.compute_file_boost(comm, &query_tokens);
                let score = w_bm25 * norm_bm25
                    + w_file * file_boost
                    + w_cent * centrality
                    + w_rec * recency;
                ScoredCommunity {
                    community: comm.clone(),
                    score,
                }
            })
            .collect();
        result.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        result
    }

    fn compute_semantic_scores(
        &self,
        query: &str,
        communities: &[Community],
    ) -> HashMap<String, f64> {
        let query_vec: Vec<f64> = if let Some(ref embedder) = self.embedder {
            embedder.embed(query)
        } else {
            self.tfidf_model.transform_normalized(query)
        };
        communities
            .iter()
            .map(|comm| {
                let sim = if let Some(qv) = self.quantized_docs.get(&comm.id) {
                    self.quantizer.cosine_similarity(&query_vec, qv)
                } else {
                    0.0
                };
                (comm.id.clone(), sim)
            })
            .collect()
    }

    fn compute_graph_attention_scores(
        &self,
        graph: &CodeGraph,
        communities: &[Community],
        query_tokens: &std::collections::HashSet<String>,
    ) -> HashMap<String, f64> {
        let initial_node_scores = compute_initial_node_scores(graph, query_tokens);
        propagate_attention(&initial_node_scores, graph, communities, 2, 0.5)
    }

    fn compute_file_boost(
        &self,
        comm: &Community,
        query_tokens: &std::collections::HashSet<String>,
    ) -> f64 {
        let Some(token_sets) = self.community_symbol_tokens.get(&comm.id) else {
            return 0.0;
        };
        let mut best = 0.0f64;
        for toks in token_sets {
            let m = query_tokens.iter().filter(|qt| toks.contains(*qt)).count();
            if !query_tokens.is_empty() {
                best = best.max(m as f64 / query_tokens.len() as f64);
            }
        }
        best
    }
}

/// File-level BM25 (Zoekt pattern) max-aggregated to community then
/// min-max-normalized to [0, 1].
fn compute_bm25_scores(
    graph: &CodeGraph,
    query: &str,
    communities: &[Community],
) -> HashMap<String, f64> {
    let file_scores = FileBm25::search(graph, query);
    let community_file_scores = FileBm25::community_scores(&file_scores, communities, graph);
    let raw: HashMap<&str, f64> = community_file_scores
        .iter()
        .map(|(id, score)| (id.as_str(), *score))
        .collect();
    let bm25_max = raw.values().cloned().fold(f64::NEG_INFINITY, f64::max);
    let bm25_min = raw.values().cloned().fold(f64::INFINITY, f64::min);
    let bm25_range = bm25_max - bm25_min;
    raw.into_iter()
        .map(|(id, raw_score)| {
            let norm = if bm25_range > 0.0 {
                (raw_score - bm25_min) / bm25_range
            } else {
                0.0
            };
            (id.to_string(), norm)
        })
        .collect()
}

/// Token-overlap initialisation for graph-attention propagation: each
/// node gets the fraction of query tokens its (name + signature) covers.
fn compute_initial_node_scores(
    graph: &CodeGraph,
    query_tokens: &std::collections::HashSet<String>,
) -> HashMap<String, f64> {
    let mut scores = HashMap::new();
    for nid in graph.node_ids() {
        if let Some(node) = graph.get_node(nid) {
            let text = match &node.signature {
                Some(sig) => format!("{} {}", node.name, sig),
                None => node.name.clone(),
            };
            let toks: std::collections::HashSet<String> = tokenise(&text).into_iter().collect();
            let overlap = if query_tokens.is_empty() {
                0.0
            } else {
                let m = query_tokens.iter().filter(|qt| toks.contains(*qt)).count();
                m as f64 / query_tokens.len() as f64
            };
            if overlap > 0.0 {
                scores.insert(nid.to_string(), overlap);
            }
        }
    }
    scores
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
