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

impl Bm25Index {
    /// Build an index from `communities` using node data from `graph`.
    pub fn build(communities: &[Community], graph: &CodeGraph) -> Self {
        let doc_count = communities.len();
        let mut postings: HashMap<String, Vec<(usize, f64)>> = HashMap::new();
        let mut doc_lengths: Vec<f64> = Vec::with_capacity(doc_count);

        for (idx, community) in communities.iter().enumerate() {
            let doc = community_document(community, graph);
            let tokens = tokenise(&doc);
            let len = tokens.len() as f64;
            doc_lengths.push(len);

            // Count term frequencies in this document.
            let mut tf: HashMap<String, f64> = HashMap::new();
            for token in tokens {
                *tf.entry(token).or_insert(0.0) += 1.0;
            }

            // Append to postings list.
            for (term, freq) in tf {
                postings.entry(term).or_default().push((idx, freq));
            }
        }

        let avg_doc_length = if doc_count == 0 {
            0.0
        } else {
            doc_lengths.iter().sum::<f64>() / doc_count as f64
        };

        Bm25Index {
            postings,
            doc_lengths,
            avg_doc_length,
            doc_count,
            config: Bm25Config::default(),
        }
    }

    /// Score all communities against `query`. Returns results sorted descending.
    pub fn search(&self, query: &str, communities: &[Community]) -> Vec<ScoredCommunity> {
        let query_tokens = tokenise(query);
        let mut scores = vec![0.0f64; communities.len()];

        if !query_tokens.is_empty() {
            for term in &query_tokens {
                let postings = match self.postings.get(term) {
                    Some(p) => p,
                    None => continue,
                };

                // n(t) = number of documents containing this term
                let n_t = postings.len() as f64;
                let n = self.doc_count as f64;
                let idf = ((n - n_t + 0.5) / (n_t + 0.5) + 1.0).ln();

                for &(doc_idx, tf) in postings {
                    if doc_idx >= communities.len() {
                        continue;
                    }
                    let dl = self.doc_lengths[doc_idx];
                    let avgdl = self.avg_doc_length;
                    let k1 = self.config.k1;
                    let b = self.config.b;

                    let norm = if avgdl == 0.0 {
                        tf
                    } else {
                        tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * dl / avgdl))
                    };
                    scores[doc_idx] += idf * norm;
                }
            }
        }

        let mut result: Vec<ScoredCommunity> = communities
            .iter()
            .enumerate()
            .map(|(i, c)| ScoredCommunity {
                community: c.clone(),
                score: scores[i],
            })
            .collect();

        result.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        result
    }
}

// ---------------------------------------------------------------------------
// PageRank helper (community-level)
