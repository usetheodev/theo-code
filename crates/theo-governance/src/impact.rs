/// BFS-based impact analysis for GRAPHCTX governance.
///
/// Starting from an edited file, the algorithm walks the code graph to
/// determine which communities, tests, and co-change partners are affected.

use std::collections::{HashSet, VecDeque};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, EdgeType, NodeType};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Result of impact analysis for a single file edit.
#[derive(Debug, Clone)]
pub struct ImpactReport {
    /// The file that was edited (as passed to `analyze_impact`).
    pub edited_file: String,
    /// Community IDs that contain at least one node reached by BFS.
    pub affected_communities: Vec<String>,
    /// IDs of test nodes that have a TESTS edge pointing to any reached symbol.
    pub tests_covering_edit: Vec<String>,
    /// File paths that historically co-change with `edited_file` (weight >= 0.1).
    pub co_change_candidates: Vec<String>,
    /// Human-readable warning strings.
    pub risk_alerts: Vec<String>,
    /// The `max_depth` value used during analysis.
    pub bfs_depth: usize,
}

// ---------------------------------------------------------------------------
// Core algorithm
// ---------------------------------------------------------------------------

/// Perform BFS-based impact analysis starting from `edited_file`.
///
/// # Parameters
/// - `edited_file`  — file path as stored in `Node::file_path` / `Node::id`
/// - `graph`        — the full code graph
/// - `communities`  — pre-computed community slices (e.g. from Louvain)
/// - `max_depth`    — maximum BFS hops from the seed symbols (0 = no expansion)
pub fn analyze_impact(
    edited_file: &str,
    graph: &CodeGraph,
    communities: &[Community],
    max_depth: usize,
) -> ImpactReport {
    // ------------------------------------------------------------------
    // 1. Collect seed symbols: all Symbol nodes whose file_path matches
    //    edited_file, found via CONTAINS edges from the file node.
    // ------------------------------------------------------------------
    let seed_symbols = collect_seed_symbols(edited_file, graph);

    // ------------------------------------------------------------------
    // 2. BFS from seed symbols over "propagation" edge types.
    // ------------------------------------------------------------------
    let reached = bfs_reachable(&seed_symbols, graph, max_depth);

    // ------------------------------------------------------------------
    // 3. Affected communities: any community with a node in `reached`.
    // ------------------------------------------------------------------
    let affected_communities = communities
        .iter()
        .filter(|c| c.node_ids.iter().any(|id| reached.contains(id.as_str())))
        .map(|c| c.id.clone())
        .collect::<Vec<_>>();

    // ------------------------------------------------------------------
    // 4. Tests covering the edit: test nodes with a TESTS edge pointing
    //    to any reached symbol.
    // ------------------------------------------------------------------
    let tests_covering_edit = find_covering_tests(&reached, graph);

    // ------------------------------------------------------------------
    // 5. Co-change candidates: CO_CHANGES edges from the edited file
    //    with weight >= 0.1.
    // ------------------------------------------------------------------
    let co_change_candidates = find_co_changes(edited_file, graph);

    // ------------------------------------------------------------------
    // 6. Generate risk alerts.
    // ------------------------------------------------------------------
    let risk_alerts = build_risk_alerts(
        edited_file,
        &seed_symbols,
        &affected_communities,
        &tests_covering_edit,
        &co_change_candidates,
        communities,
    );

    ImpactReport {
        edited_file: edited_file.to_string(),
        affected_communities,
        tests_covering_edit,
        co_change_candidates,
        risk_alerts,
        bfs_depth: max_depth,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Collect all symbol node IDs that belong to `edited_file`.
///
/// Strategy:
/// 1. Follow CONTAINS edges from the file node.
/// 2. Fall back to scanning all Symbol nodes whose `file_path` matches.
fn collect_seed_symbols(edited_file: &str, graph: &CodeGraph) -> Vec<String> {
    let mut seeds: Vec<String> = Vec::new();

    // Primary: CONTAINS edges from the file node.
    for edge in graph.edges_of_type(&EdgeType::Contains) {
        if edge.source == edited_file {
            if let Some(node) = graph.get_node(&edge.target) {
                if matches!(node.node_type, NodeType::Symbol) {
                    seeds.push(node.id.clone());
                }
            }
        }
    }

    // Fallback: scan all Symbol nodes by file_path.
    if seeds.is_empty() {
        for node in graph.symbol_nodes() {
            if node.file_path.as_deref() == Some(edited_file) {
                seeds.push(node.id.clone());
            }
        }
    }

    seeds
}

/// The edge types that BFS should follow to find downstream impact.
fn is_propagation_edge(et: &EdgeType) -> bool {
    matches!(
        et,
        EdgeType::Calls
            | EdgeType::Imports
            | EdgeType::Inherits
            | EdgeType::TypeDepends
    )
}

/// BFS over propagation edges from `seeds`, up to `max_depth` hops.
///
/// Returns the set of all node IDs reachable (including the seeds themselves).
fn bfs_reachable<'a>(seeds: &[String], graph: &'a CodeGraph, max_depth: usize) -> HashSet<&'a str> {
    let mut visited: HashSet<&str> = HashSet::new();
    // Queue entries: (node_id, current_depth)
    let mut queue: VecDeque<(&str, usize)> = VecDeque::new();

    // Seed the BFS with all symbols in the edited file. We need a reference
    // into the graph's own storage so lifetimes work out.
    for seed_id in seeds {
        if let Some(node) = graph.get_node(seed_id) {
            if visited.insert(node.id.as_str()) {
                queue.push_back((node.id.as_str(), 0));
            }
        }
    }

    // If max_depth is 0, return only the seeds (no expansion).
    if max_depth == 0 {
        return visited;
    }

    while let Some((current_id, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        // Follow all propagation edges from this node.
        for edge in graph.all_edges() {
            if edge.source != current_id {
                continue;
            }
            if !is_propagation_edge(&edge.edge_type) {
                continue;
            }
            if let Some(target_node) = graph.get_node(&edge.target) {
                if visited.insert(target_node.id.as_str()) {
                    queue.push_back((target_node.id.as_str(), depth + 1));
                }
            }
        }
    }

    visited
}

/// Find test node IDs that have a TESTS edge pointing to any node in `reached`.
fn find_covering_tests<'a>(reached: &HashSet<&str>, graph: &'a CodeGraph) -> Vec<String> {
    let mut tests: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();

    for edge in graph.edges_of_type(&EdgeType::Tests) {
        // TESTS edge: source is test node, target is the symbol under test.
        if reached.contains(edge.target.as_str()) {
            if let Some(test_node) = graph.get_node(&edge.source) {
                if seen.insert(test_node.id.as_str()) {
                    tests.push(test_node.id.clone());
                }
            }
        }
    }
    tests
}

/// Find co-change candidates via CO_CHANGES edges from `edited_file` with weight >= 0.1.
fn find_co_changes(edited_file: &str, graph: &CodeGraph) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();

    for edge in graph.edges_of_type(&EdgeType::CoChanges) {
        if edge.weight < 0.1 {
            continue;
        }
        if edge.source == edited_file && edge.target != edited_file {
            candidates.push(edge.target.clone());
        } else if edge.target == edited_file && edge.source != edited_file {
            candidates.push(edge.source.clone());
        }
    }
    candidates
}

/// Build human-readable risk alert strings.
fn build_risk_alerts(
    edited_file: &str,
    seed_symbols: &[String],
    affected_communities: &[String],
    tests_covering_edit: &[String],
    co_change_candidates: &[String],
    communities: &[Community],
) -> Vec<String> {
    let mut alerts: Vec<String> = Vec::new();

    // Alert: untested symbols.
    if tests_covering_edit.is_empty() && !seed_symbols.is_empty() {
        for sym in seed_symbols {
            alerts.push(format!(
                "Untested modification: {sym} has no test coverage"
            ));
        }
    }

    // Alert: cross-cluster impact (multiple communities affected).
    if affected_communities.len() > 1 {
        let names: Vec<String> = communities
            .iter()
            .filter(|c| affected_communities.contains(&c.id))
            .map(|c| c.name.clone())
            .collect();
        alerts.push(format!(
            "Cross-cluster impact: edit affects {} communities: {}",
            affected_communities.len(),
            names.join(", ")
        ));
    }

    // Alert: co-change candidates.
    for candidate in co_change_candidates {
        alerts.push(format!(
            "Co-change alert: {candidate} historically co-changes with {edited_file}"
        ));
    }

    alerts
}
