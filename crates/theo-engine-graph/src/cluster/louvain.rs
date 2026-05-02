//! Single-purpose slice extracted from `cluster.rs` (T4.2 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::CodeGraph;

use super::*;
use super::helpers::*;
use super::leiden::*;
use super::naming::*;
use super::types::*;

pub fn louvain_phase1(
    assignment: &mut [usize],
    node_ids: &[String],
    weight_map: &HashMap<(String, String), f64>,
) -> bool {
    let total_weight: f64 = weight_map.values().sum();
    if total_weight == 0.0 {
        return false;
    }
    let m2 = 2.0 * total_weight;
    let n = node_ids.len();
    let mut improved = false;

    // Pre-compute index: node_id -> idx for O(1) lookup.
    let id_to_idx: HashMap<&str, usize> = node_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();

    // Pre-compute adjacency list: for each node, list of (neighbor_idx, weight).
    // This converts O(N²) neighbor scan to O(degree) per node.
    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for ((a, b), &w) in weight_map.iter() {
        if let (Some(&ai), Some(&bi)) = (id_to_idx.get(a.as_str()), id_to_idx.get(b.as_str())) {
            adj[ai].push((bi, w));
            adj[bi].push((ai, w));
        }
    }

    // Pre-compute degrees: sum of weights per node.
    let degrees: Vec<f64> = adj
        .iter()
        .map(|neighbors| neighbors.iter().map(|(_, w)| w).sum())
        .collect();

    // Community total degree: sum of degrees of all nodes in each community.
    // Maintained incrementally when a node moves.
    let max_comm = n;
    let mut comm_total_degree: Vec<f64> = vec![0.0; max_comm];
    for i in 0..n {
        comm_total_degree[assignment[i]] += degrees[i];
    }

    // Iterate until convergence (max N passes as safety bound).
    for _ in 0..n {
        let mut any_move = false;
        for idx in 0..n {
            let ki = degrees[idx];
            let current_comm = assignment[idx];

            // Collect neighboring communities — O(degree), not O(N).
            let mut neighbor_comms = std::collections::HashSet::new();
            for &(nb_idx, _) in &adj[idx] {
                if nb_idx != idx {
                    neighbor_comms.insert(assignment[nb_idx]);
                }
            }

            if neighbor_comms.is_empty() {
                continue;
            }

            // Temporarily remove node from its community.
            comm_total_degree[current_comm] -= ki;
            assignment[idx] = usize::MAX;

            // Best community to move to.
            let mut best_comm = current_comm;
            let mut best_gain = 0.0;

            // Evaluate gain for each neighboring community — O(degree) per candidate.
            for &candidate_comm in &neighbor_comms {
                // Weight from node to candidate community: sum weights to neighbors in that community.
                let k_i_in: f64 = adj[idx]
                    .iter()
                    .filter(|(nb, _)| assignment[*nb] == candidate_comm)
                    .map(|(_, w)| w)
                    .sum();
                let sigma_tot = comm_total_degree[candidate_comm];
                let delta_q = k_i_in - sigma_tot * ki / m2;

                if delta_q > best_gain {
                    best_gain = delta_q;
                    best_comm = candidate_comm;
                }
            }

            assignment[idx] = best_comm;
            comm_total_degree[best_comm] += ki;
            if best_comm != current_comm {
                any_move = true;
                improved = true;
            }
        }
        if !any_move {
            break;
        }
    }
    improved
}

/// Run the Louvain algorithm on the subset of nodes provided.
///
/// Returns an assignment vector (index → community_id) and the modularity.
pub fn louvain_on_nodes(
    node_ids: &[String],
    weight_map: &HashMap<(String, String), f64>,
) -> (Vec<usize>, f64) {
    if node_ids.is_empty() {
        return (vec![], 0.0);
    }

    // Initialise: each node in its own community.
    let mut assignment: Vec<usize> = (0..node_ids.len()).collect();

    // Phase 1: local moves (repeat until no improvement).
    loop {
        let improved = louvain_phase1(&mut assignment, node_ids, weight_map);
        if !improved {
            break;
        }
    }

    // Renumber communities 0..k
    let mut remap: HashMap<usize, usize> = HashMap::new();
    let mut counter = 0usize;
    for a in assignment.iter_mut() {
        if !remap.contains_key(a) {
            remap.insert(*a, counter);
            counter += 1;
        }
        *a = remap[a];
    }

    let q = compute_modularity(&assignment, node_ids, weight_map);
    (assignment, q)
}

/// Detect communities using the Louvain algorithm.
///
/// Only **symbol** nodes participate in clustering (file/import/type nodes are
/// structural and don't carry meaningful community signal on their own).
pub fn detect_communities(graph: &CodeGraph) -> ClusterResult {
    let node_ids: Vec<String> = graph
        .symbol_nodes()
        .into_iter()
        .map(|n| n.id.clone())
        .collect();

    if node_ids.is_empty() {
        return ClusterResult {
            communities: vec![],
            modularity: 0.0,
        };
    }

    let weight_map = build_weight_map(graph);
    let (assignment, modularity) = louvain_on_nodes(&node_ids, &weight_map);

    // Group by community id.
    let num_comms = assignment.iter().max().map(|&m| m + 1).unwrap_or(0);
    let mut buckets: Vec<Vec<String>> = vec![vec![]; num_comms];
    for (i, &comm) in assignment.iter().enumerate() {
        buckets[comm].push(node_ids[i].clone());
    }

    let communities: Vec<Community> = buckets
        .into_iter()
        .enumerate()
        .filter(|(_, members)| !members.is_empty())
        .map(|(comm_idx, members)| Community {
            id: format!("comm-{comm_idx}"),
            name: format!("Community {comm_idx}"),
            level: 0,
            node_ids: members,
            parent_id: None,
            version: 1,
        })
        .collect();

    ClusterResult {
        communities,
        modularity,
    }
}

// ---------------------------------------------------------------------------
// Leiden algorithm
