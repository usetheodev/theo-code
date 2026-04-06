/// Co-change temporal decay and edge update logic.
///
/// When two files are modified in the same git commit, we record a `CoChanges`
/// edge between them. The edge weight reflects how recently the co-change
/// occurred: recent co-changes are more significant than old ones.
use crate::model::{CodeGraph, Edge, EdgeType};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default decay constant λ.
///
/// Half-life ≈ ln(2) / λ ≈ 69.3 days — co-changes older than ~70 days
/// contribute about half as much as very recent ones.
pub const DEFAULT_LAMBDA: f64 = 0.01;

// ---------------------------------------------------------------------------
// Core function
// ---------------------------------------------------------------------------

/// Exponential temporal decay: w(t) = exp(−λ · days_since).
///
/// * `days_since` — how many days have elapsed since the co-change event.
/// * `lambda`     — decay rate; higher → faster decay.
///
/// Returns a value in (0, 1] where 1.0 means "just happened" and values
/// close to 0 mean "a very long time ago".
#[inline]
pub fn temporal_decay(days_since: f64, lambda: f64) -> f64 {
    (-lambda * days_since).exp()
}

// ---------------------------------------------------------------------------
// Graph update
// ---------------------------------------------------------------------------

/// Record co-change edges between every pair of files in `changed_files`.
///
/// For each unordered pair (a, b) in `changed_files`, a directed
/// `CoChanges` edge is added from `a` to `b` with weight equal to
/// `temporal_decay(days_since_last, DEFAULT_LAMBDA)`.
///
/// If an edge between the same pair already exists and this call represents
/// a more recent commit (smaller `days_since_last`), the weight is updated to
/// the higher (more recent) value. Otherwise a new edge is appended.
///
/// # Arguments
/// * `graph`           — mutable reference to the code graph
/// * `changed_files`   — slice of file ids (must already be nodes in `graph`)
/// * `days_since_last` — days elapsed since this commit (0 = today)
pub fn update_cochanges(graph: &mut CodeGraph, changed_files: &[String], days_since_last: f64) {
    if changed_files.len() < 2 {
        return;
    }

    let new_weight = temporal_decay(days_since_last, DEFAULT_LAMBDA);

    // Collect pairs first to avoid borrow conflicts on `graph`.
    let mut pairs: Vec<(String, String)> = Vec::new();
    for i in 0..changed_files.len() {
        for j in (i + 1)..changed_files.len() {
            pairs.push((changed_files[i].clone(), changed_files[j].clone()));
        }
    }

    // Build index of existing CoChanges edges: (src, tgt) → edge index.
    // This converts the O(E) linear scan per pair to O(1) HashMap lookup.
    let mut cochange_index: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    for (idx, edge) in graph.edges_mut().iter().enumerate() {
        if edge.edge_type == EdgeType::CoChanges {
            cochange_index.insert((edge.source.clone(), edge.target.clone()), idx);
        }
    }

    // New edges to add (collected separately to avoid borrow conflict).
    let mut new_edges: Vec<Edge> = Vec::new();

    for (src, tgt) in pairs {
        if let Some(&idx) = cochange_index.get(&(src.clone(), tgt.clone())) {
            // Existing edge: update weight if newer.
            let edge = &mut graph.edges_mut()[idx];
            if new_weight > edge.weight {
                edge.weight = new_weight;
            }
        } else {
            // New edge: collect for batch addition.
            cochange_index.insert((src.clone(), tgt.clone()), usize::MAX); // mark as seen
            new_edges.push(Edge {
                source: src,
                target: tgt,
                edge_type: EdgeType::CoChanges,
                weight: new_weight,
            });
        }
    }

    // Add all new edges at once.
    for edge in new_edges {
        graph.add_edge(edge);
    }
}
