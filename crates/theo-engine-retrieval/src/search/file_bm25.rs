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

pub struct FileBm25;

impl FileBm25 {
    /// Search with Pseudo-Relevance Feedback (PRF).
    ///
    /// Stage 1: Initial BM25 search.
    /// Stage 2: If top result is confident, extract its symbol names as expansion
    ///          terms and merge with original scores.
    ///
    /// PRF is a classic IR technique (Rocchio, 1971) that uses the top result's
    /// vocabulary to find related documents. Unlike graph expansion, PRF is purely
    /// lexical and doesn't suffer from dense import noise.
    pub fn search(graph: &CodeGraph, query: &str) -> HashMap<String, f64> {
        let initial = Self::search_inner(graph, query);

        // Only expand if we have a confident top result
        let mut sorted: Vec<_> = initial.iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

        if sorted.len() >= 2 && *sorted[0].1 > sorted[1].1 * 2.0 {
            let top_file = sorted[0].0.as_str();
            let file_id = format!("file:{}", top_file);

            // Extract top-5 most unique symbol names from the top file
            let mut expansion: Vec<String> = Vec::new();
            for child_id in graph.contains_children(&file_id) {
                if let Some(child) = graph.get_node(child_id) {
                    // Only expand with meaningful symbol names (not common ones)
                    if child.name.len() >= 5 && !is_stop_word(&child.name.to_lowercase()) {
                        expansion.push(child.name.clone());
                    }
                }
            }
            expansion.truncate(5);

            if !expansion.is_empty() {
                // Run second BM25 with expansion terms only (not original query)
                let expansion_query = expansion.join(" ");
                let expanded = Self::search_inner(graph, &expansion_query);

                // Merge: original scores + 0.3x expanded scores
                let mut merged = initial;
                for (path, exp_score) in expanded {
                    merged
                        .entry(path)
                        .and_modify(|s| *s += exp_score * 0.3)
                        .or_insert(exp_score * 0.3);
                }
                return merged;
            }
        }

        initial
    }

    /// Inner BM25 search (single pass).
    fn search_inner(graph: &CodeGraph, query: &str) -> HashMap<String, f64> {
        let query_tokens: Vec<String> = tokenise(query)
            .into_iter()
            .filter(|t| !is_stop_word(t))
            .collect();
        if query_tokens.is_empty() {
            return HashMap::new();
        }
        let file_nodes: Vec<(&str, &str)> = collect_file_nodes(graph);
        let doc_count = file_nodes.len();
        if doc_count == 0 {
            return HashMap::new();
        }
        let (postings, doc_lengths) = build_inverted_index(graph, &file_nodes);
        let scores = score_documents(&query_tokens, &postings, &doc_lengths);
        let mut result = HashMap::new();
        for (idx, (_, file_path)) in file_nodes.iter().enumerate() {
            if scores[idx] > 0.0 {
                result.insert(file_path.to_string(), scores[idx]);
            }
        }
        result
    }

    /// Aggregate file scores to community level via max.
    pub fn community_scores(
        file_scores: &HashMap<String, f64>,
        communities: &[Community],
        graph: &CodeGraph,
    ) -> HashMap<String, f64> {
        communities
            .iter()
            .map(|comm| {
                let max_score = comm
                    .node_ids
                    .iter()
                    .filter_map(|nid| {
                        graph
                            .get_node(nid)
                            .and_then(|n| n.file_path.as_deref())
                            .and_then(|fp| file_scores.get(fp))
                    })
                    .copied()
                    .fold(0.0f64, f64::max);
                (comm.id.clone(), max_score)
            })
            .collect()
    }
}

fn collect_file_nodes(graph: &CodeGraph) -> Vec<(&str, &str)> {
    graph
        .node_ids()
        .filter_map(|id| {
            let n = graph.get_node(id)?;
            if n.node_type == NodeType::File {
                Some((id, n.file_path.as_deref().unwrap_or(&n.name)))
            } else {
                None
            }
        })
        .collect()
}

/// Build a per-file inverted index with BM25F-style boosts:
/// filename 5x, path segments 3x, child-symbol names 3x, signatures
/// + doc-first-line 1x, neighbor-symbol enrichment 0.15x.
fn build_inverted_index(
    graph: &CodeGraph,
    file_nodes: &[(&str, &str)],
) -> (HashMap<String, Vec<(usize, f64)>>, Vec<f64>) {
    let doc_count = file_nodes.len();
    let mut postings: HashMap<String, Vec<(usize, f64)>> = HashMap::new();
    let mut doc_lengths: Vec<f64> = Vec::with_capacity(doc_count);
    for (idx, (file_id, _)) in file_nodes.iter().enumerate() {
        let Some(file_node) = graph.get_node(file_id) else {
            doc_lengths.push(0.0);
            continue;
        };
        let mut weighted_tf: HashMap<String, f64> = HashMap::new();
        boost_filename(file_node, &mut weighted_tf);
        boost_path_segments(file_node, &mut weighted_tf);
        boost_children(graph, file_id, &mut weighted_tf);
        let len: f64 = weighted_tf.values().sum();
        doc_lengths.push(len);
        for (term, freq) in weighted_tf {
            postings.entry(term).or_default().push((idx, freq));
        }
    }
    (postings, doc_lengths)
}

fn boost_filename(file_node: &theo_engine_graph::model::Node, weighted_tf: &mut HashMap<String, f64>) {
    for token in tokenise(&file_node.name) {
        if !is_stop_word(&token) {
            *weighted_tf.entry(token).or_default() += 5.0;
        }
    }
}

fn boost_path_segments(file_node: &theo_engine_graph::model::Node, weighted_tf: &mut HashMap<String, f64>) {
    let Some(fp) = &file_node.file_path else {
        return;
    };
    for segment in fp.split('/') {
        for token in tokenise(segment) {
            if !is_stop_word(&token) {
                *weighted_tf.entry(token).or_default() += 3.0;
            }
        }
    }
}

fn boost_children(graph: &CodeGraph, file_id: &str, weighted_tf: &mut HashMap<String, f64>) {
    for child_id in graph.contains_children(file_id) {
        let Some(child) = graph.get_node(child_id) else {
            continue;
        };
        for token in tokenise(&child.name) {
            if !is_stop_word(&token) {
                *weighted_tf.entry(token).or_default() += 3.0;
            }
        }
        if let Some(sig) = &child.signature {
            for token in tokenise(sig) {
                if !is_stop_word(&token) {
                    *weighted_tf.entry(token).or_default() += 1.0;
                }
            }
        }
        if let Some(doc) = &child.doc
            && let Some(fl) = doc.lines().next()
        {
            for token in tokenise(fl) {
                if !is_stop_word(&token) {
                    *weighted_tf.entry(token).or_default() += 1.0;
                }
            }
        }
        boost_neighbor_symbols(graph, child_id, weighted_tf);
    }
}

/// 2-hop import enrichment: symbols this child CALLS/IMPORTS at low
/// boost (0.15x) — higher values regress BM25 baseline.
fn boost_neighbor_symbols(
    graph: &CodeGraph,
    child_id: &str,
    weighted_tf: &mut HashMap<String, f64>,
) {
    for target_id in graph.neighbors(child_id) {
        if let Some(target) = graph.get_node(target_id)
            && target.node_type == NodeType::Symbol
        {
            for token in tokenise(&target.name) {
                if !is_stop_word(&token) {
                    *weighted_tf.entry(token).or_default() += 0.15;
                }
            }
        }
    }
}

fn score_documents(
    query_tokens: &[String],
    postings: &HashMap<String, Vec<(usize, f64)>>,
    doc_lengths: &[f64],
) -> Vec<f64> {
    let doc_count = doc_lengths.len();
    let avg_dl = if doc_count > 0 {
        doc_lengths.iter().sum::<f64>() / doc_count as f64
    } else {
        1.0
    };
    let (k1, b) = (1.2f64, 0.75f64);
    let n = doc_count as f64;
    let mut scores = vec![0.0f64; doc_count];
    for term in query_tokens {
        let Some(posts) = postings.get(term.as_str()) else {
            continue;
        };
        let n_t = posts.len() as f64;
        let idf = ((n - n_t + 0.5) / (n_t + 0.5) + 1.0).ln();
        for &(doc_idx, tf) in posts {
            let dl = doc_lengths[doc_idx];
            let norm = tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * dl / avg_dl));
            scores[doc_idx] += idf * norm;
        }
    }
    scores
}

// ---------------------------------------------------------------------------
// MultiSignalScorer implementation
