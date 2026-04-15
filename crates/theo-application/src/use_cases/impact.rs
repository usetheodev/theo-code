/// BFS-based impact analysis for GRAPHCTX.
///
/// Starting from an edited file, the algorithm walks the code graph to
/// determine which communities, tests, and co-change partners are affected.
///
/// Moved from theo-governance to theo-application to respect boundary rules:
/// governance → domain only, application → all engines.
use std::collections::{HashSet, VecDeque};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, EdgeType, NodeType};

use theo_domain::graph_context::ImpactReport;

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
    let seed_symbols = collect_seed_symbols(edited_file, graph);
    let reached = bfs_reachable(&seed_symbols, graph, max_depth);

    let affected_communities = communities
        .iter()
        .filter(|c| c.node_ids.iter().any(|id| reached.contains(id.as_str())))
        .map(|c| c.id.clone())
        .collect::<Vec<_>>();

    let tests_covering_edit = find_covering_tests(&reached, graph);
    let co_change_candidates = find_co_changes(edited_file, graph);

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

fn collect_seed_symbols(edited_file: &str, graph: &CodeGraph) -> Vec<String> {
    let mut seeds: Vec<String> = Vec::new();

    for edge in graph.edges_of_type(&EdgeType::Contains) {
        if edge.source == edited_file {
            if let Some(node) = graph.get_node(&edge.target) {
                if matches!(node.node_type, NodeType::Symbol) {
                    seeds.push(node.id.clone());
                }
            }
        }
    }

    if seeds.is_empty() {
        for node in graph.symbol_nodes() {
            if node.file_path.as_deref() == Some(edited_file) {
                seeds.push(node.id.clone());
            }
        }
    }

    seeds
}

fn is_propagation_edge(et: &EdgeType) -> bool {
    matches!(
        et,
        EdgeType::Calls | EdgeType::Imports | EdgeType::Inherits | EdgeType::TypeDepends
    )
}

fn bfs_reachable<'a>(seeds: &[String], graph: &'a CodeGraph, max_depth: usize) -> HashSet<&'a str> {
    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<(&str, usize)> = VecDeque::new();

    for seed_id in seeds {
        if let Some(node) = graph.get_node(seed_id) {
            if visited.insert(node.id.as_str()) {
                queue.push_back((node.id.as_str(), 0));
            }
        }
    }

    if max_depth == 0 {
        return visited;
    }

    while let Some((current_id, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

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

fn find_covering_tests<'a>(reached: &HashSet<&str>, graph: &'a CodeGraph) -> Vec<String> {
    let mut tests: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();

    for edge in graph.edges_of_type(&EdgeType::Tests) {
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

fn build_risk_alerts(
    edited_file: &str,
    seed_symbols: &[String],
    affected_communities: &[String],
    tests_covering_edit: &[String],
    co_change_candidates: &[String],
    communities: &[Community],
) -> Vec<String> {
    let mut alerts: Vec<String> = Vec::new();

    if tests_covering_edit.is_empty() && !seed_symbols.is_empty() {
        for sym in seed_symbols {
            alerts.push(format!("Untested modification: {sym} has no test coverage"));
        }
    }

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

    for candidate in co_change_candidates {
        alerts.push(format!(
            "Co-change alert: {candidate} historically co-changes with {edited_file}"
        ));
    }

    alerts
}
