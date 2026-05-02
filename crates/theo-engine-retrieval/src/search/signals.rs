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

pub fn community_pagerank(communities: &[Community], graph: &CodeGraph) -> HashMap<String, f64> {
    let n = communities.len();
    if n == 0 {
        return HashMap::new();
    }

    // Map node_id -> community index.
    let mut node_to_idx: HashMap<&str, usize> = HashMap::new();
    for (i, comm) in communities.iter().enumerate() {
        for nid in &comm.node_ids {
            node_to_idx.insert(nid.as_str(), i);
        }
    }

    // Build SPARSE weighted adjacency between communities.
    // sparse_adj[i] = Vec<(j, weight)> for non-zero weights only.
    let mut adj_raw: HashMap<(usize, usize), f64> = HashMap::new();
    for edge in graph.all_edges() {
        let src_idx = node_to_idx.get(edge.source.as_str()).copied();
        let tgt_idx = node_to_idx.get(edge.target.as_str()).copied();
        if let (Some(s), Some(t)) = (src_idx, tgt_idx)
            && s != t {
                *adj_raw.entry((s, t)).or_insert(0.0) += edge.weight;
            }
    }

    // Row-normalize to get sparse transition: transition[i] = Vec<(j, prob)>.
    let mut row_sums = vec![0.0f64; n];
    for (&(s, _), &w) in &adj_raw {
        row_sums[s] += w;
    }

    // Group by source community.
    let mut sparse_transition: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for (&(s, t), &w) in &adj_raw {
        if row_sums[s] > 0.0 {
            sparse_transition[s].push((t, w / row_sums[s]));
        }
    }

    // Also build reverse index: for each target j, list of (source_i, prob).
    // This is what PageRank needs: pr[j] += damping * sum_i(transition[i][j] * pr[i]).
    let mut reverse_transition: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for (i, neighbors) in sparse_transition.iter().enumerate() {
        for &(j, prob) in neighbors {
            reverse_transition[j].push((i, prob));
        }
    }

    // Dangling node handling: if a community has no outgoing edges,
    // its PageRank mass is distributed uniformly (teleport).
    let dangling: Vec<bool> = (0..n).map(|i| row_sums[i] == 0.0).collect();

    // Iterative PageRank (20 iterations, damping = 0.85).
    let damping = 0.85f64;
    let teleport = (1.0 - damping) / n as f64;
    let uniform = 1.0 / n as f64;
    let mut pr = vec![uniform; n];

    for _ in 0..20 {
        // Compute dangling mass: sum of PR of dangling nodes.
        let dangling_mass: f64 = pr
            .iter()
            .enumerate()
            .filter(|(i, _)| dangling[*i])
            .map(|(_, &p)| p)
            .sum();
        let dangling_contribution = damping * dangling_mass * uniform;

        let mut new_pr = vec![teleport + dangling_contribution; n];
        for j in 0..n {
            let mut sum = 0.0;
            for &(i, prob) in &reverse_transition[j] {
                sum += prob * pr[i];
            }
            new_pr[j] += damping * sum;
        }
        pr = new_pr;
    }

    // Normalize to [0, 1].
    let max_pr = pr.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min_pr = pr.iter().cloned().fold(f64::INFINITY, f64::min);
    let range = max_pr - min_pr;

    communities
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let normalized = if range > 0.0 {
                (pr[i] - min_pr) / range
            } else {
                0.0
            };
            (c.id.clone(), normalized)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Recency helper
// ---------------------------------------------------------------------------

/// For each community: max(node.last_modified for node in community.node_ids).
/// Normalizes to [0, 1] across all communities.
pub fn community_recency(communities: &[Community], graph: &CodeGraph) -> HashMap<String, f64> {
    let raw: Vec<(String, f64)> = communities
        .iter()
        .map(|comm| {
            let max_ts = comm
                .node_ids
                .iter()
                .filter_map(|nid| graph.get_node(nid))
                .map(|n| n.last_modified)
                .fold(f64::NEG_INFINITY, f64::max);
            let ts = if max_ts.is_finite() { max_ts } else { 0.0 };
            (comm.id.clone(), ts)
        })
        .collect();

    let max_ts = raw
        .iter()
        .map(|(_, ts)| *ts)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_ts = raw.iter().map(|(_, ts)| *ts).fold(f64::INFINITY, f64::min);
    let range = max_ts - min_ts;

    raw.into_iter()
        .map(|(id, ts)| {
            let normalized = if range > 0.0 {
                (ts - min_ts) / range
            } else {
                0.0
            };
            (id, normalized)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Rust stop words — filter high-frequency code tokens from BM25
// ---------------------------------------------------------------------------

const RUST_STOP_WORDS: &[&str] = &[
    "fn", "pub", "let", "mut", "use", "mod", "impl", "struct", "enum", "trait", "type", "const",
    "static", "self", "super", "crate", "where", "for", "in", "if", "else", "match", "return",
    "async", "await", "move", "ref", "as", "str", "string", "bool", "i32", "i64", "u8", "u32",
    "u64", "usize", "f64", "option", "result", "ok", "err", "some", "none", "vec", "box", "arc",
    "true", "false", "new", "default",
];

pub fn is_stop_word(token: &str) -> bool {
    RUST_STOP_WORDS.contains(&token)
}

// ---------------------------------------------------------------------------
// File-level BM25 with BM25F boosts (CodeCompass/Zoekt pattern)
