//! Single-purpose slice extracted from `cluster.rs` (T4.2 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::CodeGraph;

use super::*;
use super::helpers::*;
use super::leiden::*;
use super::naming::*;
use super::types::*;

pub fn hierarchical_cluster(graph: &CodeGraph, algorithm: ClusterAlgorithm) -> ClusterResult {
    // Level-0: chosen algorithm.
    let level0_result = match algorithm {
        ClusterAlgorithm::Louvain => detect_communities(graph),
        ClusterAlgorithm::Leiden { resolution } => leiden_communities(graph, resolution, 10),
        ClusterAlgorithm::FileLeiden { resolution } => detect_file_communities(graph, resolution),
    };

    // Post-process: merge small communities into neighbors.
    let merged = merge_small_communities(graph, level0_result.communities, 3);

    // Name communities based on their file paths.
    let named: Vec<Community> = merged
        .into_iter()
        .map(|mut c| {
            c.name = name_community(&c, graph);
            c
        })
        .collect();

    // Level-1: Subdivide mega-communities using seeded LPA.
    // Communities with >30 members are REPLACED by subcommunities based on directory labels.
    const MEGA_THRESHOLD: usize = 30;

    let mut all_communities: Vec<Community> = Vec::new();
    for l0_comm in named {
        if l0_comm.node_ids.len() > MEGA_THRESHOLD {
            let sub = subdivide_with_lpa_seeded(graph, &l0_comm);
            if sub.is_empty() {
                // LPA couldn't subdivide — keep original
                all_communities.push(l0_comm);
            } else {
                // Replace mega-community with subcommunities
                all_communities.extend(sub);
            }
        } else {
            all_communities.push(l0_comm);
        }
    }

    ClusterResult {
        communities: all_communities,
        modularity: level0_result.modularity,
    }
}

/// Subdivide a mega-community using seeded LPA with directory labels.
///
/// Produces subcommunities aligned with directory structure (e.g., "auth", "api").
/// If LPA converges to a single group, the original community is kept as-is.
pub fn subdivide_with_lpa_seeded(graph: &CodeGraph, parent: &Community) -> Vec<Community> {
    let members = &parent.node_ids;
    if members.len() <= 1 {
        return vec![];
    }

    // Build local weight map restricted to this community's members.
    let member_set: HashSet<&str> = members.iter().map(String::as_str).collect();
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

    // Seed labels from directory structure.
    let seeds = dir_seed_labels(members);

    // If local edges exist, use LPA to refine seeds with structural info.
    // If no edges (isolated nodes merged together), use directory labels directly.
    let labels = if local_weights.is_empty() {
        seeds.clone()
    } else {
        // ADR-019: lpa_seeded returns Result; on the rare case the
        // partition state is internally inconsistent (programming bug),
        // log + degrade gracefully by falling back to directory seeds —
        // the user-facing API still produces *some* partition rather
        // than panicking through the public boundary.
        match lpa_seeded(members, &local_weights, &seeds) {
            Ok(labels) => labels,
            Err(err) => {
                eprintln!(
                    "lpa_seeded failed (ADR-019); falling back to directory seeds: {err}"
                );
                seeds.clone()
            }
        }
    };

    // Group by label.
    let mut buckets: HashMap<usize, Vec<String>> = HashMap::new();
    for (node_id, label) in &labels {
        buckets.entry(*label).or_default().push(node_id.clone());
    }

    // If LPA produced only 1 group, fall back to pure directory split.
    // This handles the case where edges are too strong/uniform for LPA to separate.
    if buckets.len() <= 1 {
        buckets.clear();
        for (node_id, label) in &seeds {
            buckets.entry(*label).or_default().push(node_id.clone());
        }
    }

    // Still 1 bucket after dir split? Give up.
    if buckets.len() <= 1 {
        return vec![];
    }

    // Merge tiny subcommunities (<3 members) into the largest bucket.
    let largest_label = buckets
        .iter()
        .max_by_key(|(_, v)| v.len())
        .map(|(k, _)| *k)
        .unwrap_or(0);

    let mut tiny_nodes: Vec<String> = Vec::new();
    buckets.retain(|label, nodes| {
        if nodes.len() < 3 && *label != largest_label {
            tiny_nodes.append(nodes);
            false
        } else {
            true
        }
    });
    if !tiny_nodes.is_empty() {
        buckets.entry(largest_label).or_default().extend(tiny_nodes);
    }

    // If merging reduced to 1 bucket, skip.
    if buckets.len() <= 1 {
        return vec![];
    }

    // Emit subcommunities.
    buckets
        .into_values()
        .enumerate()
        .map(|(i, node_ids)| {
            let mut sub = Community {
                id: format!("{}-sub-{}", parent.id, i),
                name: String::new(), // will be named below
                level: 1,
                node_ids,
                parent_id: Some(parent.id.clone()),
                version: parent.version,
            };
            sub.name = name_community(&sub, graph);
            sub
        })
        .collect()
}

/// Merge communities with fewer than `min_size` members into their best neighbor.
///
/// "Best neighbor" = the community that shares the most edges with the small one.
pub fn merge_small_communities(
    graph: &CodeGraph,
    mut communities: Vec<Community>,
    min_size: usize,
) -> Vec<Community> {
    if communities.is_empty() {
        return communities;
    }

    // Build node_id → community_index map.
    let mut node_to_comm: HashMap<String, usize> = HashMap::new();
    for (idx, comm) in communities.iter().enumerate() {
        for nid in &comm.node_ids {
            node_to_comm.insert(nid.clone(), idx);
        }
    }

    // Find small communities and their best merge target.
    let mut merge_into: HashMap<usize, usize> = HashMap::new(); // small_idx → target_idx

    for (idx, comm) in communities.iter().enumerate() {
        if comm.node_ids.len() >= min_size {
            continue;
        }

        // Count edges to each neighbor community.
        let mut neighbor_counts: HashMap<usize, usize> = HashMap::new();
        for nid in &comm.node_ids {
            for neighbor_id in graph.neighbors(nid) {
                if let Some(&neigh_comm) = node_to_comm.get(neighbor_id)
                    && neigh_comm != idx {
                        *neighbor_counts.entry(neigh_comm).or_insert(0) += 1;
                    }
            }
            // Also check reverse edges.
            for neighbor_id in graph.reverse_neighbors(nid) {
                if let Some(&neigh_comm) = node_to_comm.get(neighbor_id)
                    && neigh_comm != idx {
                        *neighbor_counts.entry(neigh_comm).or_insert(0) += 1;
                    }
            }
        }

        // Merge into the community with the most connections.
        if let Some((best_target, _)) = neighbor_counts.iter().max_by_key(|(_, count)| **count) {
            merge_into.insert(idx, *best_target);
        }
    }

    // Apply merges (only one level — don't chain).
    // Resolve chains: if A→B and B→C, resolve A→C.
    let resolved: HashMap<usize, usize> = merge_into
        .iter()
        .map(|(&from, &to)| {
            let mut target = to;
            // Follow one level of indirection.
            if let Some(&next) = merge_into.get(&target) {
                target = next;
            }
            (from, target)
        })
        .collect();

    // Move nodes from small communities to their targets.
    let mut to_merge: Vec<(usize, Vec<String>)> = Vec::new();
    for (&from, &to) in &resolved {
        let nodes = communities[from].node_ids.clone();
        to_merge.push((to, nodes));
        communities[from].node_ids.clear();
    }
    for (target, nodes) in to_merge {
        communities[target].node_ids.extend(nodes);
    }

    // Remove empty communities and renumber.
    communities
        .into_iter()
        .filter(|c| !c.node_ids.is_empty())
        .enumerate()
        .map(|(new_idx, mut c)| {
            c.id = format!("comm-{new_idx}");
            c
        })
        .collect()
}

/// Subdivide a large file-level community by re-running Leiden with higher resolution.
#[allow(dead_code)] // Kept as alternative to LPA-based subdivision
pub fn subdivide_file_community(graph: &CodeGraph, community: &Community) -> Vec<Community> {
    use crate::model::EdgeType;

    let file_ids = &community.node_ids;
    let file_set: HashSet<&str> = file_ids.iter().map(|s| s.as_str()).collect();

    // Build sub-graph weight map (only edges between files in this community)
    let mut sym_to_file: HashMap<String, String> = HashMap::new();
    for edge in graph.all_edges() {
        if edge.edge_type == EdgeType::Contains
            && file_set.contains(edge.source.as_str()) {
                sym_to_file.insert(edge.target.clone(), edge.source.clone());
            }
    }

    let mut sub_weights: HashMap<(String, String), f64> = HashMap::new();
    for edge in graph.all_edges() {
        if edge.edge_type == EdgeType::Contains {
            continue;
        }
        let sf = sym_to_file.get(&edge.source).or_else(|| {
            if file_set.contains(edge.source.as_str()) {
                Some(&edge.source)
            } else {
                None
            }
        });
        let tf = sym_to_file.get(&edge.target).or_else(|| {
            if file_set.contains(edge.target.as_str()) {
                Some(&edge.target)
            } else {
                None
            }
        });
        if let (Some(s), Some(t)) = (sf, tf) {
            if s == t || !file_set.contains(s.as_str()) || !file_set.contains(t.as_str()) {
                continue;
            }
            let (lo, hi) = if s < t {
                (s.clone(), t.clone())
            } else {
                (t.clone(), s.clone())
            };
            *sub_weights.entry((lo, hi)).or_insert(0.0) += edge.weight;
        }
    }

    // Re-run Leiden with much higher resolution to aggressively split
    let node_refs: Vec<&str> = file_ids.iter().map(|s| s.as_str()).collect();
    let (assignment, _) = leiden_on_nodes(&node_refs, &sub_weights, 5.0, 15);

    let num_comms = assignment.iter().max().map(|&m| m + 1).unwrap_or(0);
    if num_comms <= 1 {
        return vec![]; // Can't split further
    }

    let mut buckets: Vec<Vec<String>> = vec![vec![]; num_comms];
    for (i, &comm) in assignment.iter().enumerate() {
        buckets[comm].push(file_ids[i].clone());
    }

    buckets
        .into_iter()
        .enumerate()
        .filter(|(_, members)| !members.is_empty())
        .map(|(idx, members)| {
            let mut sub = Community {
                id: format!("{}-sub-{idx}", community.id),
                name: String::new(),
                level: 1,
                node_ids: members,
                parent_id: Some(community.id.clone()),
                version: 1,
            };
            sub.name = name_community(&sub, graph);
            sub
        })
        .collect()
}
