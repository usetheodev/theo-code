//! Single-purpose slice extracted from `cluster.rs` (T4.2 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::CodeGraph;

use super::*;
use super::leiden::*;
use super::naming::*;
use super::types::*;

pub fn build_weight_map(graph: &CodeGraph) -> HashMap<(String, String), f64> {
    let mut map: HashMap<(String, String), f64> = HashMap::new();
    for edge in graph.all_edges() {
        let a = edge.source.clone();
        let b = edge.target.clone();
        if a == b {
            continue;
        }
        // Treat as undirected: always store with lex-sorted key.
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        *map.entry((lo, hi)).or_insert(0.0) += edge.weight;
    }
    map
}

/// Sum of weights of all edges incident to `node_id`.
pub fn degree(node_id: &str, weight_map: &HashMap<(String, String), f64>) -> f64 {
    weight_map
        .iter()
        .filter_map(|((a, b), &w)| {
            if a == node_id || b == node_id {
                Some(w)
            } else {
                None
            }
        })
        .sum()
}

/// Sum of weights of all edges whose at least one endpoint is in community `comm_id`.
pub fn total_degree_of_community(
    comm_id: usize,
    assignment: &[usize],
    node_ids: &[String],
    weight_map: &HashMap<(String, String), f64>,
) -> f64 {
    node_ids
        .iter()
        .enumerate()
        .filter(|(i, _)| assignment[*i] == comm_id)
        .map(|(_, id)| degree(id, weight_map))
        .sum()
}

/// Compute the weight of edges between `node_id` and community `comm_id`.
pub fn weight_to_community(
    node_id: &str,
    comm_id: usize,
    assignment: &[usize],
    node_ids: &[String],
    weight_map: &HashMap<(String, String), f64>,
) -> f64 {
    node_ids
        .iter()
        .enumerate()
        .filter(|(i, id)| assignment[*i] == comm_id && id.as_str() != node_id)
        .map(|(_, nb_id)| {
            let (lo, hi) = if node_id < nb_id.as_str() {
                (node_id.to_string(), nb_id.clone())
            } else {
                (nb_id.clone(), node_id.to_string())
            };
            *weight_map.get(&(lo, hi)).unwrap_or(&0.0)
        })
        .sum()
}

/// Newman-Girvan modularity Q.
pub fn compute_modularity(
    assignment: &[usize],
    node_ids: &[String],
    weight_map: &HashMap<(String, String), f64>,
) -> f64 {
    let total_weight: f64 = weight_map.values().sum();
    if total_weight == 0.0 {
        return 0.0;
    }
    let m2 = 2.0 * total_weight;

    let mut q = 0.0;
    for i in 0..node_ids.len() {
        for j in 0..node_ids.len() {
            if assignment[i] != assignment[j] {
                continue;
            }
            let a = &node_ids[i];
            let b = &node_ids[j];
            let (lo, hi) = if a < b {
                (a.clone(), b.clone())
            } else {
                (b.clone(), a.clone())
            };
            let a_ij = *weight_map.get(&(lo, hi)).unwrap_or(&0.0);
            let ki = degree(a, weight_map);
            let kj = degree(b, weight_map);
            q += a_ij - ki * kj / m2;
        }
    }
    q / m2
}
