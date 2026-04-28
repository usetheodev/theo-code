//! Single-purpose slice extracted from `cluster.rs` (T4.2 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::CodeGraph;

use super::*;
use super::helpers::*;
use super::leiden::*;
use super::naming::*;
use super::types::*;

pub fn subdivide_community(
    graph: &CodeGraph,
    community: &Community,
    _max_size: usize,
) -> Vec<Community> {
    let members = &community.node_ids;

    if members.len() <= 1 {
        return vec![Community {
            id: format!("{}-mod-0", community.id),
            name: format!("{} / mod-0", community.name),
            level: 1,
            node_ids: members.clone(),
            parent_id: Some(community.id.clone()),
            version: community.version,
        }];
    }

    // Build a local weight map restricted to community members.
    let member_set: std::collections::HashSet<&str> = members.iter().map(String::as_str).collect();

    let mut local_weights: HashMap<(String, String), f64> = HashMap::new();
    for edge in graph.all_edges() {
        if member_set.contains(edge.source.as_str()) && member_set.contains(edge.target.as_str()) {
            let (lo, hi) = if edge.source < edge.target {
                (edge.source.clone(), edge.target.clone())
            } else {
                (edge.target.clone(), edge.source.clone())
            };
            *local_weights.entry((lo, hi)).or_insert(0.0) += edge.weight;
        }
    }

    // Label propagation: initialise each node with its own unique label.
    let mut labels: HashMap<&str, usize> = members
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();

    // Iterate up to 100 times or until stable.
    for _iter in 0..100 {
        let mut changed = false;

        for node_id in members.iter() {
            // Collect weighted label counts among neighbours.
            let mut label_weight: HashMap<usize, f64> = HashMap::new();

            for other_id in members.iter() {
                if other_id == node_id {
                    continue;
                }
                let (lo, hi) = if node_id < other_id {
                    (node_id.clone(), other_id.clone())
                } else {
                    (other_id.clone(), node_id.clone())
                };
                if let Some(&w) = local_weights.get(&(lo, hi)) {
                    let lbl = *labels.get(other_id.as_str()).unwrap();
                    *label_weight.entry(lbl).or_insert(0.0) += w;
                }
            }

            if label_weight.is_empty() {
                continue;
            }

            // Adopt the most-frequent (heaviest) label.
            let best_label = label_weight
                .iter()
                .max_by(|(_, wa), (_, wb)| wa.partial_cmp(wb).unwrap())
                .map(|(&lbl, _)| lbl)
                .unwrap();

            let current = *labels.get(node_id.as_str()).unwrap();
            if best_label != current {
                labels.insert(node_id.as_str(), best_label);
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    // Group nodes by their final label.
    let mut buckets: HashMap<usize, Vec<String>> = HashMap::new();
    for node_id in members.iter() {
        let lbl = *labels.get(node_id.as_str()).unwrap();
        buckets.entry(lbl).or_default().push(node_id.clone());
    }

    // Emit sub-communities.
    buckets
        .into_values()
        .enumerate()
        .map(|(i, node_ids)| Community {
            id: format!("{}-mod-{i}", community.id),
            name: format!("{} / mod-{i}", community.name),
            level: 1,
            node_ids,
            parent_id: Some(community.id.clone()),
            version: community.version,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// File-level clustering
// ---------------------------------------------------------------------------

/// Cluster at the FILE level instead of symbol level.
///
/// Builds an aggregated file graph where edge weight between two files =
/// sum of all symbol-level edge weights between their symbols. Then runs
/// Leiden on this reduced graph. Result: 10-30 domains of 3-15 files.
///
/// Each community's `node_ids` contains File node IDs (not symbol IDs).
/// This matches how developers think about modules.
pub fn detect_file_communities(graph: &CodeGraph, resolution: f64) -> ClusterResult {
    use crate::model::{EdgeType, NodeType};

    let file_nodes: Vec<String> = graph
        .file_nodes()
        .into_iter()
        .map(|n| n.id.clone())
        .collect();

    if file_nodes.is_empty() {
        return ClusterResult {
            communities: vec![],
            modularity: 0.0,
        };
    }

    // Map: symbol_id → file_id (via Contains edges)
    let mut sym_to_file: HashMap<String, String> = HashMap::new();
    for edge in graph.all_edges() {
        if edge.edge_type == EdgeType::Contains {
            // source is file, target is symbol/import/type/test
            if let Some(source_node) = graph.get_node(&edge.source)
                && matches!(source_node.node_type, NodeType::File) {
                    sym_to_file.insert(edge.target.clone(), edge.source.clone());
                }
        }
    }

    // Build file-to-file weight map by aggregating symbol edges.
    let mut file_weights: HashMap<(String, String), f64> = HashMap::new();

    for edge in graph.all_edges() {
        // Skip Contains edges (they connect file→symbol, not file→file)
        if edge.edge_type == EdgeType::Contains {
            continue;
        }

        let src_file = sym_to_file.get(&edge.source).or_else(|| {
            // The node might BE a file (e.g., CoChanges between files)
            if graph
                .get_node(&edge.source)
                .is_some_and(|n| matches!(n.node_type, NodeType::File))
            {
                Some(&edge.source)
            } else {
                None
            }
        });

        let tgt_file = sym_to_file.get(&edge.target).or_else(|| {
            if graph
                .get_node(&edge.target)
                .is_some_and(|n| matches!(n.node_type, NodeType::File))
            {
                Some(&edge.target)
            } else {
                None
            }
        });

        if let (Some(sf), Some(tf)) = (src_file, tgt_file) {
            if sf == tf {
                continue; // skip intra-file edges
            }
            let (lo, hi) = if sf < tf {
                (sf.clone(), tf.clone())
            } else {
                (tf.clone(), sf.clone())
            };
            *file_weights.entry((lo, hi)).or_insert(0.0) += edge.weight;
        }
    }

    // Run Leiden on the file graph.
    let file_node_refs: Vec<&str> = file_nodes.iter().map(|s| s.as_str()).collect();
    let (assignment, modularity) = leiden_on_nodes(&file_node_refs, &file_weights, resolution, 10);

    // Group into communities.
    let num_comms = assignment.iter().max().map(|&m| m + 1).unwrap_or(0);
    let mut buckets: Vec<Vec<String>> = vec![vec![]; num_comms];
    for (i, &comm) in assignment.iter().enumerate() {
        buckets[comm].push(file_nodes[i].clone());
    }

    let communities: Vec<Community> = buckets
        .into_iter()
        .enumerate()
        .filter(|(_, members)| !members.is_empty())
        .map(|(idx, members)| Community {
            id: format!("fcomm-{idx}"),
            name: String::new(), // named later
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

/// Run Leiden on arbitrary node set with pre-computed weight map.
/// Reuses the existing Leiden logic but with a generic weight map.
pub fn leiden_on_nodes(
    node_ids: &[&str],
    weight_map: &HashMap<(String, String), f64>,
    resolution: f64,
    max_iterations: usize,
) -> (Vec<usize>, f64) {
    // Convert to owned strings for existing functions
    let owned_ids: Vec<String> = node_ids.iter().map(|s| s.to_string()).collect();
    let n = owned_ids.len();
    if n == 0 {
        return (vec![], 0.0);
    }

    // Pre-compute adjacency list: O(E) instead of O(N²) per iteration.
    let id_to_idx: HashMap<&str, usize> = owned_ids
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

    // Initialize: each node in its own community
    let mut assignment: Vec<usize> = (0..n).collect();
    let mut next_comm;

    for _ in 0..max_iterations {
        let mut improved = false;

        // Phase 1: Local moves — O(E) per iteration using adjacency list.
        for i in 0..n {
            let current_comm = assignment[i];

            // Neighbor communities via adjacency list — O(degree), not O(N).
            let mut comm_weights: HashMap<usize, f64> = HashMap::new();
            for &(nb_idx, w) in &adj[i] {
                *comm_weights.entry(assignment[nb_idx]).or_insert(0.0) += w;
            }

            // Find best community
            let mut best_comm = current_comm;
            let mut best_gain = 0.0;
            for (&comm, &w) in &comm_weights {
                let gain = w - resolution;
                if gain > best_gain {
                    best_gain = gain;
                    best_comm = comm;
                }
            }

            if best_comm != current_comm {
                assignment[i] = best_comm;
                improved = true;
            }
        }

        if !improved {
            break;
        }

        // Renumber communities to be contiguous
        let mut id_map: HashMap<usize, usize> = HashMap::new();
        next_comm = 0;
        for a in &mut assignment {
            let new_id = *id_map.entry(*a).or_insert_with(|| {
                let id = next_comm;
                next_comm += 1;
                id
            });
            *a = new_id;
        }
    }

    // Compute modularity
    let total_weight: f64 = weight_map.values().sum();
    let modularity = if total_weight > 0.0 {
        compute_modularity_resolution(&assignment, &owned_ids, weight_map, resolution)
    } else {
        0.0
    };

    (assignment, modularity)
}

// ---------------------------------------------------------------------------
// Two-level hierarchical clustering
