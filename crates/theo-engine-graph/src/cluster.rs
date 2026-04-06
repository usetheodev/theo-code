/// Community detection for the code graph.
///
/// Implements:
/// 1. Louvain algorithm for coarse-grained domain clustering.
/// 2. Leiden algorithm with connectivity guarantee.
/// 3. Label Propagation Algorithm (LPA) for fine-grained module subdivision.
/// 4. Two-level hierarchical clustering combining domain + module levels.
use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::CodeGraph;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Community {
    pub id: String,
    pub name: String,
    /// 0 = domain level (Louvain), 1 = module level (LPA)
    pub level: u32,
    pub node_ids: Vec<String>,
    pub parent_id: Option<String>,
    pub version: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClusterResult {
    pub communities: Vec<Community>,
    /// Newman-Girvan modularity Q ∈ (−0.5, 1.0].
    pub modularity: f64,
}

/// Algorithm choice for hierarchical clustering at the domain level.
#[derive(Debug, Clone)]
pub enum ClusterAlgorithm {
    Louvain,
    Leiden { resolution: f64 },
    /// File-level clustering: groups files (not symbols) based on the
    /// aggregated weight of symbol-level edges between them.
    /// Produces 10-30 domains of 3-15 files each — matches how devs think.
    FileLeiden { resolution: f64 },
}

// ---------------------------------------------------------------------------
// Louvain algorithm
// ---------------------------------------------------------------------------

/// Build an adjacency-weight map from the graph (undirected, summing weights).
fn build_weight_map(graph: &CodeGraph) -> HashMap<(String, String), f64> {
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
fn degree(node_id: &str, weight_map: &HashMap<(String, String), f64>) -> f64 {
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
fn total_degree_of_community(
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
fn weight_to_community(
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
fn compute_modularity(
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

/// Louvain Phase 1: local moves — O(E) per pass using adjacency list.
///
/// Returns `true` if any node was moved (improvement found).
fn louvain_phase1(
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
    let degrees: Vec<f64> = adj.iter().map(|neighbors| neighbors.iter().map(|(_, w)| w).sum()).collect();

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
fn louvain_on_nodes(
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
// ---------------------------------------------------------------------------

/// Compute modularity with a resolution parameter (γ).
///
/// Q = (1/2m) Σ_{ij} [A_{ij} − γ * k_i * k_j / 2m] δ(c_i, c_j)
fn compute_modularity_resolution(
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

    let degrees: Vec<f64> = adj.iter().map(|nb| nb.iter().map(|(_, w)| w).sum()).collect();

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
    q = q - resolution * degree_penalty / m2;
    q / m2
}

/// Find connected components within a set of node indices, using the weight map
/// as the adjacency source.
fn connected_components_of(
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
fn refine_partition(
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
            let sigma_cand =
                total_degree_of_community(candidate, assignment, node_ids, weight_map);
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

/// Leiden community detection with connectivity guarantee.
///
/// Returns communities that are always connected subgraphs. The `resolution`
/// parameter controls granularity (1.0 = standard modularity, higher values
/// produce more/smaller communities). Iterates at most `max_iterations` rounds
/// of move + refine.
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

    let modularity =
        compute_modularity_resolution(&assignment, &node_ids, &weight_map, resolution);

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
) -> HashMap<String, usize> {
    let n = node_ids.len();
    if n == 0 {
        return HashMap::new();
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

            // Adopt heaviest label
            let best = label_weight
                .iter()
                .max_by(|(_, wa), (_, wb)| wa.partial_cmp(wb).unwrap())
                .map(|(&lbl, _)| lbl)
                .unwrap();

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
    node_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), labels[i]))
        .collect()
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
        let dir = if parts.len() >= 2 && (parts[0] == "crates" || parts[0] == "apps") {
            parts[1].to_string()
        } else if parts.len() >= 2 && parts[0] == "src" {
            parts[1].to_string()
        } else if parts.len() >= 2 {
            // Use the first non-trivial directory
            parts.iter()
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
    let member_set: std::collections::HashSet<&str> =
        members.iter().map(String::as_str).collect();

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
            if let Some(source_node) = graph.get_node(&edge.source) {
                if matches!(source_node.node_type, NodeType::File) {
                    sym_to_file.insert(edge.target.clone(), edge.source.clone());
                }
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
fn leiden_on_nodes(
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
// ---------------------------------------------------------------------------

/// Run a two-level hierarchical clustering:
///
/// * **Level 0 (domains)** — Louvain or Leiden on all symbol nodes.
/// * **Level 1 (modules)** — LPA on each level-0 community to produce
///   sub-modules. Every level-0 community becomes the `parent_id` of its
///   level-1 children.
///
/// Returns a `ClusterResult` containing communities at both levels.
///
/// Post-processing: singleton communities (1-2 members) are merged into their
/// closest neighbor community based on edge connectivity. This prevents the
/// common issue of Leiden producing thousands of trivial singletons.
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
fn subdivide_with_lpa_seeded(graph: &CodeGraph, parent: &Community) -> Vec<Community> {
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
        lpa_seeded(members, &local_weights, &seeds)
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
            tiny_nodes.extend(nodes.drain(..));
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
fn merge_small_communities(
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
                if let Some(&neigh_comm) = node_to_comm.get(neighbor_id) {
                    if neigh_comm != idx {
                        *neighbor_counts.entry(neigh_comm).or_insert(0) += 1;
                    }
                }
            }
            // Also check reverse edges.
            for neighbor_id in graph.reverse_neighbors(nid) {
                if let Some(&neigh_comm) = node_to_comm.get(neighbor_id) {
                    if neigh_comm != idx {
                        *neighbor_counts.entry(neigh_comm).or_insert(0) += 1;
                    }
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
fn subdivide_file_community(graph: &CodeGraph, community: &Community) -> Vec<Community> {
    use crate::model::EdgeType;

    let file_ids = &community.node_ids;
    let file_set: HashSet<&str> = file_ids.iter().map(|s| s.as_str()).collect();

    // Build sub-graph weight map (only edges between files in this community)
    let mut sym_to_file: HashMap<String, String> = HashMap::new();
    for edge in graph.all_edges() {
        if edge.edge_type == EdgeType::Contains {
            if file_set.contains(edge.source.as_str()) {
                sym_to_file.insert(edge.target.clone(), edge.source.clone());
            }
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
            let (lo, hi) = if s < t { (s.clone(), t.clone()) } else { (t.clone(), s.clone()) };
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

/// Generate a meaningful name for a community based on its members' file paths.
///
/// Finds the common directory prefix and uses it as the name.
/// E.g., if all members are in "crates/graph/src/", the name is "graph".
fn name_community(community: &Community, graph: &CodeGraph) -> String {
    let paths: Vec<&str> = community
        .node_ids
        .iter()
        .filter_map(|id| graph.get_node(id))
        .filter_map(|n| n.file_path.as_deref())
        .collect();

    if paths.is_empty() {
        return format!("community-{}", community.id);
    }

    // Find common path prefix.
    let segments: Vec<Vec<&str>> = paths
        .iter()
        .map(|p| p.split('/').collect::<Vec<_>>())
        .collect();

    let min_len = segments.iter().map(|s| s.len()).min().unwrap_or(0);
    let mut common_depth = 0;
    for i in 0..min_len {
        let first = segments[0][i];
        if segments.iter().all(|s| s[i] == first) {
            common_depth = i + 1;
        } else {
            break;
        }
    }

    // Use the deepest common directory, falling back to the first 2 segments.
    let prefix = if common_depth > 0 {
        segments[0][..common_depth].join("/")
    } else {
        // No common prefix — use the most common directory.
        let mut dir_counts: HashMap<&str, usize> = HashMap::new();
        for segs in &segments {
            if segs.len() >= 2 {
                *dir_counts.entry(segs[segs.len() - 2]).or_insert(0) += 1;
            }
        }
        dir_counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_else(|| paths[0].to_string())
    };

    // Extract meaningful name: try crate/package name, then deepest directory.
    let clean = extract_meaningful_name(&prefix, &paths, community.node_ids.len());
    clean
}

/// Extract a meaningful community name from file paths.
///
/// Priority: crate name > package directory > deepest non-trivial directory > fallback.
fn extract_meaningful_name(prefix: &str, paths: &[&str], member_count: usize) -> String {
    let parts: Vec<&str> = prefix.split('/').filter(|s| !s.is_empty()).collect();

    // Try to find a crate name pattern: "crates/<name>/..." or "apps/<name>/..."
    for (i, part) in parts.iter().enumerate() {
        if (*part == "crates" || *part == "apps") && i + 1 < parts.len() {
            let crate_name = parts[i + 1];
            // Include subdirectory if available (e.g., "theo-agent-runtime/src/config")
            if i + 3 < parts.len() && parts[i + 2] == "src" {
                return format!("{}::{} ({})", crate_name, parts[i + 3], member_count);
            }
            return format!("{} ({})", crate_name, member_count);
        }
    }

    // Try "src/<meaningful_dir>/..." pattern
    for (i, part) in parts.iter().enumerate() {
        if *part == "src" && i + 1 < parts.len() {
            let module = parts[i + 1];
            if module != "lib.rs" && module != "main.rs" {
                return format!("{} ({})", module, member_count);
            }
        }
    }

    // Fallback: use the deepest non-trivial directory
    let trivial = ["src", "lib", "crates", "apps", ".", ""];
    for part in parts.iter().rev() {
        if !trivial.contains(part) {
            return format!("{} ({})", part, member_count);
        }
    }

    // Last resort: count distinct top-level dirs
    let mut top_dirs: Vec<&str> = paths
        .iter()
        .filter_map(|p| p.split('/').next())
        .collect();
    top_dirs.sort();
    top_dirs.dedup();
    if top_dirs.len() <= 3 {
        format!("{} ({})", top_dirs.join("+"), member_count)
    } else {
        format!("mixed ({})", member_count)
    }
}
