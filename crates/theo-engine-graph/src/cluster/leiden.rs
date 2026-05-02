//! Single-purpose slice extracted from `cluster.rs` (T4.2 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::CodeGraph;

use super::*;
use super::helpers::*;
use super::naming::*;
use super::types::*;

pub fn compute_modularity_resolution(
    assignment: &[usize],
    node_ids: &[String],
    weight_map: &HashMap<(String, String), f64>,
    resolution: f64,
) -> f64 {
    let total_weight: f64 = weight_map.values().sum();
    if total_weight == 0.0 {
        return 0.0;
    }
    let m2 = 2.0 * total_weight;
    let n = node_ids.len();

    // Pre-compute: node index, adjacency, degrees — O(E) total.
    let id_to_idx: HashMap<&str, usize> = node_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();

    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for ((a, b), &w) in weight_map.iter() {
        if let (Some(&ai), Some(&bi)) = (id_to_idx.get(a.as_str()), id_to_idx.get(b.as_str())) {
            adj[ai].push((bi, w));
            adj[bi].push((ai, w));
        }
    }

    let degrees: Vec<f64> = adj
        .iter()
        .map(|nb| nb.iter().map(|(_, w)| w).sum())
        .collect();

    // Compute modularity Q — O(E) by iterating edges, not node pairs.
    let mut q = 0.0;

    // Edge contribution: sum a_ij for same-community pairs (via adjacency)
    for i in 0..n {
        for &(j, w) in &adj[i] {
            if assignment[i] == assignment[j] {
                q += w; // Each edge counted twice (i→j and j→i), but we sum all
            }
        }
    }

    // Degree penalty: sum ki*kj/m2 for all same-community pairs
    // Group nodes by community, then sum products of degrees within each community.
    let max_comm = assignment.iter().copied().max().unwrap_or(0) + 1;
    let mut comm_degree_sum: Vec<f64> = vec![0.0; max_comm];
    let mut comm_degree_sq_sum: Vec<f64> = vec![0.0; max_comm];
    for i in 0..n {
        let c = assignment[i];
        comm_degree_sum[c] += degrees[i];
    }
    // Sum of ki*kj for all pairs in community c = (sum_ki)^2 (includes i==j terms, but those cancel)
    for c in 0..max_comm {
        comm_degree_sq_sum[c] = comm_degree_sum[c] * comm_degree_sum[c];
    }

    let degree_penalty: f64 = comm_degree_sq_sum.iter().sum();
    q -= resolution * degree_penalty / m2;
    q / m2
}

/// Find connected components within a set of node indices, using the weight map
/// as the adjacency source.
pub fn connected_components_of(
    indices: &[usize],
    node_ids: &[String],
    weight_map: &HashMap<(String, String), f64>,
) -> Vec<Vec<usize>> {
    if indices.is_empty() {
        return vec![];
    }

    let index_set: HashSet<usize> = indices.iter().copied().collect();
    let mut visited: HashSet<usize> = HashSet::new();
    let mut components: Vec<Vec<usize>> = Vec::new();

    for &start in indices {
        if visited.contains(&start) {
            continue;
        }
        let mut component = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);
        visited.insert(start);

        while let Some(current) = queue.pop_front() {
            component.push(current);
            let current_id = &node_ids[current];

            for &other in indices {
                if visited.contains(&other) || !index_set.contains(&other) {
                    continue;
                }
                let other_id = &node_ids[other];
                let (lo, hi) = if current_id < other_id {
                    (current_id.clone(), other_id.clone())
                } else {
                    (other_id.clone(), current_id.clone())
                };
                if weight_map.contains_key(&(lo, hi)) {
                    visited.insert(other);
                    queue.push_back(other);
                }
            }
        }

        components.push(component);
    }

    components
}

/// Leiden refinement phase: ensure communities are connected subgraphs, and
/// allow nodes at community boundaries to leave if modularity improves.
pub fn refine_partition(
    node_ids: &[String],
    assignment: &mut [usize],
    weight_map: &HashMap<(String, String), f64>,
    resolution: f64,
) {
    let total_weight: f64 = weight_map.values().sum();
    if total_weight == 0.0 {
        return;
    }
    let m2 = 2.0 * total_weight;

    // Step 1: Split disconnected communities into connected components.
    let num_comms = assignment.iter().max().map(|&m| m + 1).unwrap_or(0);
    let mut next_comm_id = num_comms;

    for comm_id in 0..num_comms {
        let members: Vec<usize> = (0..node_ids.len())
            .filter(|&i| assignment[i] == comm_id)
            .collect();

        if members.len() <= 1 {
            continue;
        }

        let components = connected_components_of(&members, node_ids, weight_map);

        // If already connected (single component), nothing to split.
        if components.len() <= 1 {
            continue;
        }

        // Keep the largest component with the original comm_id, assign new ids to others.
        let largest_idx = components
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| c.len())
            .map(|(i, _)| i)
            .unwrap_or(0);

        for (comp_idx, component) in components.iter().enumerate() {
            if comp_idx == largest_idx {
                continue;
            }
            for &node_idx in component {
                assignment[node_idx] = next_comm_id;
            }
            next_comm_id += 1;
        }
    }

    // Step 2: Allow boundary nodes to move if modularity gain is positive.
    // A boundary node is one that has at least one neighbor in a different community.
    for idx in 0..node_ids.len() {
        let node_id = &node_ids[idx];
        let ki = degree(node_id, weight_map);
        let current_comm = assignment[idx];

        // Collect neighboring communities (only direct neighbors).
        let mut neighbor_comms: HashSet<usize> = HashSet::new();
        for (j, other_id) in node_ids.iter().enumerate() {
            if j == idx {
                continue;
            }
            let (lo, hi) = if node_id < other_id {
                (node_id.clone(), other_id.clone())
            } else {
                (other_id.clone(), node_id.clone())
            };
            if weight_map.contains_key(&(lo, hi)) && assignment[j] != current_comm {
                neighbor_comms.insert(assignment[j]);
            }
        }

        if neighbor_comms.is_empty() {
            continue;
        }

        // Compute gain of removing node from current community.
        let k_i_current =
            weight_to_community(node_id, current_comm, assignment, node_ids, weight_map);
        let sigma_current =
            total_degree_of_community(current_comm, assignment, node_ids, weight_map) - ki;
        let loss = k_i_current - resolution * sigma_current * ki / m2;

        // Find best neighboring community to move into.
        let mut best_comm = current_comm;
        let mut best_net_gain = 0.0;

        for &candidate in &neighbor_comms {
            let k_i_cand =
                weight_to_community(node_id, candidate, assignment, node_ids, weight_map);
            let sigma_cand = total_degree_of_community(candidate, assignment, node_ids, weight_map);
            let gain = k_i_cand - resolution * sigma_cand * ki / m2;
            let net = gain - loss;

            if net > best_net_gain {
                best_net_gain = net;
                best_comm = candidate;
            }
        }

        // Only move if the source community stays connected after removal.
        if best_comm != current_comm {
            let remaining: Vec<usize> = (0..node_ids.len())
                .filter(|&i| assignment[i] == current_comm && i != idx)
                .collect();

            if remaining.is_empty()
                || connected_components_of(&remaining, node_ids, weight_map).len() == 1
            {
                assignment[idx] = best_comm;
            }
        }
    }

    // Renumber communities to be contiguous 0..k.
    let mut remap: HashMap<usize, usize> = HashMap::new();
    let mut counter = 0usize;
    for a in assignment.iter_mut() {
        if !remap.contains_key(a) {
            remap.insert(*a, counter);
            counter += 1;
        }
        *a = remap[a];
    }
}
