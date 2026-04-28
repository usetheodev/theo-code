//! Single-purpose slice extracted from `cluster.rs` (T4.2 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::CodeGraph;

use super::*;
use super::helpers::*;
use super::leiden::*;
use super::naming::*;
use super::types::*;

pub fn leiden_communities(
    graph: &CodeGraph,
    resolution: f64,
    max_iterations: usize,
) -> ClusterResult {
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

    // Initialise: each node in its own community.
    let mut assignment: Vec<usize> = (0..node_ids.len()).collect();

    for _iter in 0..max_iterations {
        // Phase 1: Move phase (same as Louvain).
        let improved = louvain_phase1(&mut assignment, &node_ids, &weight_map);

        // Phase 2: Refinement — split disconnected communities, allow boundary moves.
        refine_partition(&node_ids, &mut assignment, &weight_map, resolution);

        if !improved {
            break;
        }
    }

    // Renumber communities 0..k (refinement already does this, but be safe).
    let mut remap: HashMap<usize, usize> = HashMap::new();
    let mut counter = 0usize;
    for a in assignment.iter_mut() {
        if !remap.contains_key(a) {
            remap.insert(*a, counter);
            counter += 1;
        }
        *a = remap[a];
    }

    let modularity = compute_modularity_resolution(&assignment, &node_ids, &weight_map, resolution);

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
// Label Propagation Algorithm (LPA) for community subdivision
// ---------------------------------------------------------------------------

/// Subdivide a large community into smaller modules using label propagation.
///
/// * `graph`        — the full code graph (used for edge weights)
/// * `community`    — the community to subdivide
/// * `max_size`     — target maximum module size (hint; not strictly enforced)
///
/// Returns a list of sub-communities. If the community has 0 or 1 nodes,
/// returns a single level-1 community wrapping those nodes.
/// Seeded Label Propagation Algorithm — O(E) per pass.
///
/// Each node starts with a label derived from `initial_labels` (e.g. directory hash).
/// Nodes without a seed get a unique label. On each pass, every node adopts the
/// heaviest-weighted label among its neighbors. Converges in 3-5 passes for
/// directory-seeded code graphs.
///
/// Returns: node_id → final label (usize).
pub fn lpa_seeded(
    node_ids: &[String],
    weight_map: &HashMap<(String, String), f64>,
    initial_labels: &HashMap<String, usize>,
) -> Result<HashMap<String, usize>, ClusterError> {
    let n = node_ids.len();
    if n == 0 {
        return Ok(HashMap::new());
    }

    // Index: node_id → idx
    let id_to_idx: HashMap<&str, usize> = node_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();

    // Pre-compute adjacency list O(E)
    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for ((a, b), &w) in weight_map.iter() {
        if let (Some(&ai), Some(&bi)) = (id_to_idx.get(a.as_str()), id_to_idx.get(b.as_str())) {
            adj[ai].push((bi, w));
            adj[bi].push((ai, w));
        }
    }

    // Initialize labels from seeds (fallback: unique per node)
    let mut next_unique = initial_labels.values().copied().max().unwrap_or(0) + 1;
    let mut labels: Vec<usize> = node_ids
        .iter()
        .map(|id| {
            if let Some(&lbl) = initial_labels.get(id) {
                lbl
            } else {
                let lbl = next_unique;
                next_unique += 1;
                lbl
            }
        })
        .collect();

    // Iterate until convergence (max 10 passes)
    for _ in 0..10 {
        let mut changed = false;
        for idx in 0..n {
            if adj[idx].is_empty() {
                continue;
            }

            // Weighted label counts among neighbors — O(degree)
            let mut label_weight: HashMap<usize, f64> = HashMap::new();
            for &(nb_idx, w) in &adj[idx] {
                *label_weight.entry(labels[nb_idx]).or_insert(0.0) += w;
            }

            // Adopt heaviest label. partial_cmp tolerates non-NaN
            // weights only; NaN → Equal, biased to first encountered.
            // The .ok_or path can't be triggered here because
            // adj[idx].is_empty() is checked above, but the typed
            // error documents the algorithmic contract.
            let best = label_weight
                .iter()
                .max_by(|(_, wa), (_, wb)| {
                    wa.partial_cmp(wb).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(&lbl, _)| lbl)
                .ok_or(ClusterError::EmptyNeighbors)?;

            if best != labels[idx] {
                labels[idx] = best;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Convert to HashMap output
    Ok(node_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), labels[i]))
        .collect())
}

/// Generate directory-based seed labels from node IDs.
///
/// Extracts the most meaningful directory component from node IDs:
/// - `file:crates/theo-agent-runtime/src/run_engine.rs` → "theo-agent-runtime"
/// - `file:apps/theo-cli/src/main.rs` → "theo-cli"
/// - `sym:crates/theo-domain/src/lib.rs::StateMachine` → "theo-domain"
/// - `src/auth/login.rs::handle` → "auth"
///
/// The goal is to group by **crate/package**, not by generic "src/" directory.
pub fn dir_seed_labels(node_ids: &[String]) -> HashMap<String, usize> {
    let mut dir_to_label: HashMap<String, usize> = HashMap::new();
    let mut next_label = 0usize;
    let mut result = HashMap::new();

    for id in node_ids {
        // Strip prefix (file:, sym:, test:, etc.) and symbol suffix (::name)
        let path_part = id
            .split("::")
            .next()
            .unwrap_or(id)
            .trim_start_matches("file:")
            .trim_start_matches("sym:")
            .trim_start_matches("test:")
            .trim_start_matches("import:")
            .trim_start_matches("type:");

        let parts: Vec<&str> = path_part.split('/').collect();

        // Extract meaningful directory:
        // "crates/<name>/..." or "apps/<name>/..." → use <name>
        // "src/<dir>/..." → use <dir>
        // Otherwise → use parent directory
        let dir = if parts.len() >= 2
            && (parts[0] == "crates" || parts[0] == "apps" || parts[0] == "src")
        {
            parts[1].to_string()
        } else if parts.len() >= 2 {
            // Use the first non-trivial directory
            parts
                .iter()
                .find(|p| !["src", "lib", ".", ""].contains(p) && !p.contains('.'))
                .unwrap_or(&parts[0])
                .to_string()
        } else {
            "root".to_string()
        };

        let label = *dir_to_label.entry(dir).or_insert_with(|| {
            let l = next_label;
            next_label += 1;
            l
        });
        result.insert(id.clone(), label);
    }

    result
}

