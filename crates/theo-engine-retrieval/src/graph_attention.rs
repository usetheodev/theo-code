/// Graph Attention Propagation for code search.
///
/// Computes relevance scores by propagating attention through the code graph,
/// capturing transitive relevance that BM25 and embeddings miss.
///
/// Example: query "Leiden" -> leiden_communities -> refine_partition -> cluster.rs
/// BM25 only finds nodes mentioning "Leiden", but propagation follows edges
/// to discover related code that never mentions the term directly.
///
/// # Algorithm
///
/// 1. Start with initial per-node attention scores (from BM25 or token overlap).
/// 2. Propagate through edges for `hops` iterations using double-buffering:
///    `a_{k+1}(node) = damping * a_k(node) + (1 - damping) * max(a_k(neighbor) * edge_weight)`
/// 3. Aggregate per community: `attention(community) = max(a_K(node) for node in community)`.
/// 4. Normalize scores to [0, 1].
use std::collections::HashMap;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::CodeGraph;

/// Compute attention scores for communities via graph propagation.
///
/// Unlike flat BM25/embedding scoring, this captures transitive relevance:
/// "Leiden" -> leiden_communities -> refine_partition -> cluster.rs
///
/// * `query_scores` -- initial per-node attention (from BM25 or neural similarity)
/// * `graph` -- the code graph with edges
/// * `communities` -- communities to score
/// * `hops` -- number of propagation steps (default: 2)
/// * `damping` -- weight of original score vs propagated (default: 0.5)
pub fn propagate_attention(
    query_scores: &HashMap<String, f64>,
    graph: &CodeGraph,
    communities: &[Community],
    hops: usize,
    damping: f64,
) -> HashMap<String, f64> {
    if communities.is_empty() {
        return HashMap::new();
    }

    // Collect all node IDs that belong to any community.
    let community_node_ids: Vec<String> = communities
        .iter()
        .flat_map(|c| c.node_ids.iter().cloned())
        .collect();

    if community_node_ids.is_empty() {
        return communities.iter().map(|c| (c.id.clone(), 0.0)).collect();
    }

    // Initialize attention from query_scores (nodes not in the map get 0.0).
    let mut current: HashMap<String, f64> = community_node_ids
        .iter()
        .map(|nid| {
            let score = query_scores.get(nid).copied().unwrap_or(0.0);
            (nid.clone(), score)
        })
        .collect();

    // Pre-compute max edge weight from each neighbor to each node.
    // Build outgoing edge index once for O(1) per-pair weight lookups.
    let outgoing_index = graph.outgoing_edge_index();

    // Also build reverse index: target -> Vec<(source, max_weight)>
    let mut reverse_index: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for (src, targets) in &outgoing_index {
        for (tgt, w) in targets {
            let entry = reverse_index.entry(tgt.clone()).or_default();
            // Merge: keep max weight per source
            if let Some(existing) = entry.iter_mut().find(|(s, _)| s == src) {
                existing.1 = existing.1.max(*w);
            } else {
                entry.push((src.clone(), *w));
            }
        }
    }

    let mut neighbor_weights: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for nid in &community_node_ids {
        let mut nw: HashMap<String, f64> = HashMap::new();

        // Forward neighbors (outgoing edges: nid -> neighbor).
        if let Some(targets) = outgoing_index.get(nid) {
            for (neighbor_id, w) in targets {
                let entry = nw.entry(neighbor_id.clone()).or_insert(0.0);
                *entry = entry.max(*w);
            }
        }

        // Reverse neighbors (incoming edges: neighbor -> nid).
        if let Some(sources) = reverse_index.get(nid) {
            for (neighbor_id, w) in sources {
                let entry = nw.entry(neighbor_id.clone()).or_insert(0.0);
                *entry = entry.max(*w);
            }
        }

        neighbor_weights.insert(nid.clone(), nw.into_iter().collect());
    }

    // Propagation with double-buffering.
    for _ in 0..hops {
        let mut next: HashMap<String, f64> = HashMap::with_capacity(current.len());

        for nid in &community_node_ids {
            let self_score = current.get(nid).copied().unwrap_or(0.0);

            // Find max(neighbor_score * edge_weight) across all neighbors.
            let max_neighbor_contribution = neighbor_weights
                .get(nid)
                .map(|neighbors| {
                    neighbors
                        .iter()
                        .map(|(neighbor_id, edge_w)| {
                            let neighbor_score = current.get(neighbor_id).copied().unwrap_or(0.0);
                            neighbor_score * edge_w
                        })
                        .fold(0.0_f64, f64::max)
                })
                .unwrap_or(0.0);

            let new_score = damping * self_score + (1.0 - damping) * max_neighbor_contribution;
            next.insert(nid.clone(), new_score);
        }

        current = next;
    }

    // Aggregate per community: max score among member nodes.
    let raw: Vec<(String, f64)> = communities
        .iter()
        .map(|comm| {
            let max_score = comm
                .node_ids
                .iter()
                .map(|nid| current.get(nid).copied().unwrap_or(0.0))
                .fold(0.0_f64, f64::max);
            (comm.id.clone(), max_score)
        })
        .collect();

    // Normalize to [0, 1].
    let max_val = raw.iter().map(|(_, s)| *s).fold(0.0_f64, f64::max);
    let min_val = raw.iter().map(|(_, s)| *s).fold(f64::INFINITY, f64::min);
    let range = max_val - min_val;

    raw.into_iter()
        .map(|(id, score)| {
            let normalized = if range > 0.0 {
                (score - min_val) / range
            } else if max_val > 0.0 {
                // All scores are equal and positive -> 1.0
                1.0
            } else {
                0.0
            };
            (id, normalized)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use theo_engine_graph::model::{Edge, EdgeType, Node, NodeType, SymbolKind};

    fn make_node(id: &str, name: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: NodeType::Symbol,
            name: name.to_string(),
            file_path: Some(format!("src/{}.rs", id)),
            signature: Some(format!("fn {}()", name)),
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(10),
            last_modified: 1000.0,
            doc: None,
        }
    }

    /// A -> B -> C chain with weight 1.0 edges.
    fn chain_graph() -> (CodeGraph, Vec<Community>) {
        let mut graph = CodeGraph::new();
        graph.add_node(make_node("a", "func_a"));
        graph.add_node(make_node("b", "func_b"));
        graph.add_node(make_node("c", "func_c"));

        graph.add_edge(Edge {
            source: "a".to_string(),
            target: "b".to_string(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "b".to_string(),
            target: "c".to_string(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });

        let communities = vec![
            Community {
                id: "comm-ab".to_string(),
                name: "AB community".to_string(),
                level: 0,
                node_ids: vec!["a".to_string(), "b".to_string()],
                parent_id: None,
                version: 1,
            },
            Community {
                id: "comm-c".to_string(),
                name: "C community".to_string(),
                level: 0,
                node_ids: vec!["c".to_string()],
                parent_id: None,
                version: 1,
            },
        ];

        (graph, communities)
    }

    #[test]
    fn test_propagation_increases_neighbor_scores() {
        let (graph, communities) = chain_graph();

        // Node A has high attention, B is its neighbor.
        let mut query_scores = HashMap::new();
        query_scores.insert("a".to_string(), 1.0);
        // B and C start at 0.

        let result = propagate_attention(&query_scores, &graph, &communities, 1, 0.5);

        // After 1 hop: B should get contribution from A (its reverse neighbor).
        // B_new = 0.5 * 0.0 + 0.5 * (1.0 * 1.0) = 0.5
        // A_new = 0.5 * 1.0 + 0.5 * 0.0 = 0.5 (no neighbor with score)
        // comm-ab max = max(A=0.5, B=0.5) = 0.5
        // comm-c max = C=0.0
        // After normalization: comm-ab = 1.0, comm-c = 0.0
        let ab_score = result.get("comm-ab").copied().unwrap_or(0.0);
        let c_score = result.get("comm-c").copied().unwrap_or(0.0);

        assert!(
            ab_score > c_score,
            "community with high-attention node and its neighbor should score higher"
        );
    }

    #[test]
    fn test_propagation_respects_edge_weight() {
        let mut graph = CodeGraph::new();
        graph.add_node(make_node("src", "source_func"));
        graph.add_node(make_node("hi", "high_weight_neighbor"));
        graph.add_node(make_node("lo", "low_weight_neighbor"));

        // src -> hi with high weight
        graph.add_edge(Edge {
            source: "src".to_string(),
            target: "hi".to_string(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        // src -> lo with low weight
        graph.add_edge(Edge {
            source: "src".to_string(),
            target: "lo".to_string(),
            edge_type: EdgeType::Calls,
            weight: 0.1,
        });

        let communities = vec![
            Community {
                id: "comm-src".to_string(),
                name: "source".to_string(),
                level: 0,
                node_ids: vec!["src".to_string()],
                parent_id: None,
                version: 1,
            },
            Community {
                id: "comm-hi".to_string(),
                name: "high weight".to_string(),
                level: 0,
                node_ids: vec!["hi".to_string()],
                parent_id: None,
                version: 1,
            },
            Community {
                id: "comm-lo".to_string(),
                name: "low weight".to_string(),
                level: 0,
                node_ids: vec!["lo".to_string()],
                parent_id: None,
                version: 1,
            },
        ];

        let mut query_scores = HashMap::new();
        query_scores.insert("src".to_string(), 1.0);

        let result = propagate_attention(&query_scores, &graph, &communities, 1, 0.5);

        let hi_score = result.get("comm-hi").copied().unwrap_or(0.0);
        let lo_score = result.get("comm-lo").copied().unwrap_or(0.0);

        assert!(
            hi_score > lo_score,
            "higher edge weight should propagate more attention: hi={}, lo={}",
            hi_score,
            lo_score
        );
    }

    #[test]
    fn test_two_hops_reaches_further() {
        // A -> B -> C -> D, each in its own community.
        // With A having initial attention, 1 hop reaches B but not C or D.
        // 2 hops reaches C. We compare C's score under 1 vs 2 hops.
        let mut graph = CodeGraph::new();
        graph.add_node(make_node("a", "func_a"));
        graph.add_node(make_node("b", "func_b"));
        graph.add_node(make_node("c", "func_c"));
        graph.add_node(make_node("d", "func_d"));

        graph.add_edge(Edge {
            source: "a".to_string(),
            target: "b".to_string(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "b".to_string(),
            target: "c".to_string(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "c".to_string(),
            target: "d".to_string(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });

        let communities = vec![
            Community {
                id: "comm-a".to_string(),
                name: "A".to_string(),
                level: 0,
                node_ids: vec!["a".to_string()],
                parent_id: None,
                version: 1,
            },
            Community {
                id: "comm-b".to_string(),
                name: "B".to_string(),
                level: 0,
                node_ids: vec!["b".to_string()],
                parent_id: None,
                version: 1,
            },
            Community {
                id: "comm-c".to_string(),
                name: "C".to_string(),
                level: 0,
                node_ids: vec!["c".to_string()],
                parent_id: None,
                version: 1,
            },
            Community {
                id: "comm-d".to_string(),
                name: "D".to_string(),
                level: 0,
                node_ids: vec!["d".to_string()],
                parent_id: None,
                version: 1,
            },
        ];

        let mut query_scores = HashMap::new();
        query_scores.insert("a".to_string(), 1.0);

        // With 1 hop: B gets attention from A, C gets nothing (B was 0 initially).
        // With 2 hops: C gets attention from B (which got it from A in hop 1).
        let result_1hop = propagate_attention(&query_scores, &graph, &communities, 1, 0.5);
        let c_1hop = result_1hop.get("comm-c").copied().unwrap_or(0.0);

        let result_2hop = propagate_attention(&query_scores, &graph, &communities, 2, 0.5);
        let c_2hop = result_2hop.get("comm-c").copied().unwrap_or(0.0);

        assert!(
            c_2hop >= c_1hop,
            "2 hops should propagate at least as far as 1 hop: c_2hop={}, c_1hop={}",
            c_2hop,
            c_1hop
        );
    }

    #[test]
    fn test_damping_preserves_original() {
        let (graph, communities) = chain_graph();

        let mut query_scores = HashMap::new();
        query_scores.insert("a".to_string(), 1.0);
        query_scores.insert("b".to_string(), 0.5);
        query_scores.insert("c".to_string(), 0.0);

        // With damping=1.0, no propagation occurs (100% original score retained).
        let result = propagate_attention(&query_scores, &graph, &communities, 2, 1.0);

        // comm-ab max = max(1.0, 0.5) = 1.0, comm-c = 0.0
        // normalized: comm-ab = 1.0, comm-c = 0.0
        let ab_score = result.get("comm-ab").copied().unwrap_or(0.0);
        let c_score = result.get("comm-c").copied().unwrap_or(0.0);

        assert!(
            (ab_score - 1.0).abs() < 1e-9,
            "damping=1.0 should preserve original: ab={}",
            ab_score
        );
        assert!(
            c_score.abs() < 1e-9,
            "damping=1.0 should preserve original: c={}",
            c_score
        );
    }

    #[test]
    fn test_empty_graph_returns_empty() {
        let graph = CodeGraph::new();
        let communities: Vec<Community> = vec![];
        let query_scores = HashMap::new();

        let result = propagate_attention(&query_scores, &graph, &communities, 2, 0.5);

        assert!(result.is_empty(), "empty input should return empty result");
    }
}
