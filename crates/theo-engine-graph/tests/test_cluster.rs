/// Tests for Louvain community detection and label propagation.
use theo_engine_graph::cluster::{
    detect_communities, hierarchical_cluster, leiden_communities, subdivide_community,
    ClusterAlgorithm,
};
use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};

fn sym(id: &str) -> Node {
    Node {
        id: id.to_string(),
        node_type: NodeType::Symbol,
        name: id.to_string(),
        file_path: None,
        signature: None,
        kind: Some(SymbolKind::Function),
        line_start: None,
        line_end: None,
        last_modified: 0.0,
        doc: None,
    }
}

fn calls_edge(src: &str, tgt: &str) -> Edge {
    Edge {
        source: src.to_string(),
        target: tgt.to_string(),
        edge_type: EdgeType::Calls,
        weight: 1.0,
    }
}

fn weak_edge(src: &str, tgt: &str) -> Edge {
    Edge {
        source: src.to_string(),
        target: tgt.to_string(),
        edge_type: EdgeType::References,
        weight: 0.01,
    }
}

/// Build a graph with two clearly separated 5-node cliques connected by one weak bridge.
fn two_clique_graph() -> CodeGraph {
    let mut g = CodeGraph::new();

    // Cluster A: a0..a4
    for i in 0..5 {
        g.add_node(sym(&format!("a{i}")));
    }
    for i in 0..5 {
        for j in (i + 1)..5 {
            g.add_edge(calls_edge(&format!("a{i}"), &format!("a{j}")));
            g.add_edge(calls_edge(&format!("a{j}"), &format!("a{i}")));
        }
    }

    // Cluster B: b0..b4
    for i in 0..5 {
        g.add_node(sym(&format!("b{i}")));
    }
    for i in 0..5 {
        for j in (i + 1)..5 {
            g.add_edge(calls_edge(&format!("b{i}"), &format!("b{j}")));
            g.add_edge(calls_edge(&format!("b{j}"), &format!("b{i}")));
        }
    }

    // Weak bridge between the two clusters
    g.add_edge(weak_edge("a0", "b0"));
    g
}

#[test]
fn louvain_finds_two_communities_in_biclique_graph() {
    let g = two_clique_graph();
    let result = detect_communities(&g);

    // Should find exactly 2 communities
    assert_eq!(result.communities.len(), 2, "Expected 2 communities, got {}", result.communities.len());

    // Each community should have 5 nodes
    let sizes: Vec<usize> = {
        let mut s: Vec<usize> = result.communities.iter().map(|c| c.node_ids.len()).collect();
        s.sort();
        s
    };
    assert_eq!(sizes, vec![5, 5], "Expected two 5-node communities, got {:?}", sizes);

    // Modularity should be positive (good partition)
    assert!(result.modularity > 0.0, "Modularity should be positive, got {}", result.modularity);
}

#[test]
fn single_node_produces_one_community() {
    let mut g = CodeGraph::new();
    g.add_node(sym("only"));

    let result = detect_communities(&g);
    assert_eq!(result.communities.len(), 1);
    assert_eq!(result.communities[0].node_ids.len(), 1);
}

#[test]
fn empty_graph_produces_zero_communities() {
    let g = CodeGraph::new();
    let result = detect_communities(&g);
    assert!(result.communities.is_empty());
}

#[test]
fn fully_connected_graph_produces_one_community() {
    let mut g = CodeGraph::new();
    for i in 0..6 {
        g.add_node(sym(&format!("n{i}")));
    }
    // All nodes connected with same weight
    for i in 0..6 {
        for j in (i + 1)..6 {
            g.add_edge(calls_edge(&format!("n{i}"), &format!("n{j}")));
            g.add_edge(calls_edge(&format!("n{j}"), &format!("n{i}")));
        }
    }

    let result = detect_communities(&g);
    assert_eq!(result.communities.len(), 1, "Expected 1 community for fully connected graph");
}

#[test]
fn label_propagation_subdivides_large_community() {
    let mut g = CodeGraph::new();
    // 40 nodes in two loosely connected subgroups
    for i in 0..20 {
        g.add_node(sym(&format!("p{i}")));
    }
    for i in 0..20 {
        g.add_node(sym(&format!("q{i}")));
    }
    // Dense within each group
    for i in 0..20 {
        for j in (i + 1)..20 {
            g.add_edge(calls_edge(&format!("p{i}"), &format!("p{j}")));
            g.add_edge(calls_edge(&format!("q{i}"), &format!("q{j}")));
        }
    }
    // Sparse bridge
    g.add_edge(weak_edge("p0", "q0"));

    // Build a "community" containing all 40 nodes
    let all_ids: Vec<String> = (0..20)
        .map(|i| format!("p{i}"))
        .chain((0..20).map(|i| format!("q{i}")))
        .collect();

    let big_community = theo_engine_graph::cluster::Community {
        id: "big".to_string(),
        name: "big".to_string(),
        level: 1,
        node_ids: all_ids,
        parent_id: None,
        version: 1,
    };

    let sub = subdivide_community(&g, &big_community, 25);

    // Should split into at least 2 sub-communities
    assert!(sub.len() >= 2, "Expected at least 2 sub-communities, got {}", sub.len());
    // Total nodes across sub-communities should equal 40
    let total: usize = sub.iter().map(|c| c.node_ids.len()).sum();
    assert_eq!(total, 40, "All nodes should be assigned to sub-communities");
}

#[test]
fn hierarchical_cluster_produces_level0_communities() {
    let g = two_clique_graph();
    let result = hierarchical_cluster(&g, ClusterAlgorithm::Louvain);

    let has_level0 = result.communities.iter().any(|c| c.level == 0);
    assert!(has_level0, "Should have level-0 (domain) communities");

    // Level-1 subdivision only triggers for communities >30 members.
    // Small test graphs won't produce level-1 communities — that's expected.
    assert!(!result.communities.is_empty(), "Should have at least 1 community");
}

#[test]
fn community_ids_are_unique() {
    let g = two_clique_graph();
    let result = detect_communities(&g);
    let mut ids: Vec<&str> = result.communities.iter().map(|c| c.id.as_str()).collect();
    let original_len = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), original_len, "Community IDs must be unique");
}

// ---------------------------------------------------------------------------
// Leiden algorithm tests
// ---------------------------------------------------------------------------

/// Build a barbell graph: two 5-node cliques connected by a single path of length 3.
/// This topology is known to cause Louvain to produce disconnected communities because
/// the bridge nodes may be absorbed into different cliques during the move phase.
fn barbell_graph() -> CodeGraph {
    let mut g = CodeGraph::new();

    // Left clique: L0..L4
    for i in 0..5 {
        g.add_node(sym(&format!("L{i}")));
    }
    for i in 0..5 {
        for j in (i + 1)..5 {
            g.add_edge(calls_edge(&format!("L{i}"), &format!("L{j}")));
            g.add_edge(calls_edge(&format!("L{j}"), &format!("L{i}")));
        }
    }

    // Right clique: R0..R4
    for i in 0..5 {
        g.add_node(sym(&format!("R{i}")));
    }
    for i in 0..5 {
        for j in (i + 1)..5 {
            g.add_edge(calls_edge(&format!("R{i}"), &format!("R{j}")));
            g.add_edge(calls_edge(&format!("R{j}"), &format!("R{i}")));
        }
    }

    // Bridge path: L0 -- bridge0 -- bridge1 -- R0
    g.add_node(sym("bridge0"));
    g.add_node(sym("bridge1"));
    g.add_edge(calls_edge("L0", "bridge0"));
    g.add_edge(calls_edge("bridge0", "L0"));
    g.add_edge(calls_edge("bridge0", "bridge1"));
    g.add_edge(calls_edge("bridge1", "bridge0"));
    g.add_edge(calls_edge("bridge1", "R0"));
    g.add_edge(calls_edge("R0", "bridge1"));

    g
}

/// Helper: check that every community in the result is a connected subgraph.
fn assert_communities_connected(result: &theo_engine_graph::cluster::ClusterResult, graph: &CodeGraph) {
    use std::collections::{HashSet, VecDeque};

    for comm in &result.communities {
        if comm.node_ids.len() <= 1 {
            continue;
        }
        let member_set: HashSet<&str> = comm.node_ids.iter().map(String::as_str).collect();

        // BFS from first node, restricted to community members.
        let mut visited: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<&str> = VecDeque::new();
        let start = comm.node_ids[0].as_str();
        queue.push_back(start);
        visited.insert(start);

        while let Some(current) = queue.pop_front() {
            // Check both forward and reverse neighbors.
            for nb in graph.neighbors(current).into_iter().chain(graph.reverse_neighbors(current)) {
                if member_set.contains(nb) && !visited.contains(nb) {
                    visited.insert(nb);
                    queue.push_back(nb);
                }
            }
        }

        assert_eq!(
            visited.len(),
            comm.node_ids.len(),
            "Community '{}' with {} nodes is not connected (only {} reachable from '{}')",
            comm.id,
            comm.node_ids.len(),
            visited.len(),
            start,
        );
    }
}

#[test]
fn leiden_produces_connected_communities_on_simple_graph() {
    let g = two_clique_graph();
    let result = leiden_communities(&g, 1.0, 10);

    assert!(
        !result.communities.is_empty(),
        "Leiden should produce at least one community"
    );

    // Every community must be a connected subgraph.
    assert_communities_connected(&result, &g);

    // Should find 2 communities for the biclique graph.
    assert_eq!(
        result.communities.len(),
        2,
        "Expected 2 communities, got {}",
        result.communities.len()
    );
}

#[test]
fn leiden_produces_connected_communities_on_barbell_graph() {
    let g = barbell_graph();
    let result = leiden_communities(&g, 1.0, 10);

    assert!(
        !result.communities.is_empty(),
        "Leiden should produce at least one community"
    );

    // The key guarantee: every community must be connected.
    assert_communities_connected(&result, &g);

    // All 12 nodes should be assigned.
    let total: usize = result.communities.iter().map(|c| c.node_ids.len()).sum();
    assert_eq!(total, 12, "All 12 nodes should be in some community");
}

#[test]
fn hierarchical_cluster_works_with_leiden() {
    let g = two_clique_graph();
    let result = hierarchical_cluster(&g, ClusterAlgorithm::Leiden { resolution: 1.0 });

    let has_level0 = result.communities.iter().any(|c| c.level == 0);
    assert!(has_level0, "Should have level-0 (domain) communities");
    assert!(!result.communities.is_empty(), "Should have at least 1 community");

    // Level-0 communities from Leiden must be connected.
    let level0_result = theo_engine_graph::cluster::ClusterResult {
        communities: result
            .communities
            .iter()
            .filter(|c| c.level == 0)
            .cloned()
            .collect(),
        modularity: result.modularity,
    };
    assert_communities_connected(&level0_result, &g);
}

#[test]
fn leiden_has_reasonable_modularity_on_clustered_graph() {
    // Build a graph with 3 well-separated cliques of 5 nodes each.
    let mut g = CodeGraph::new();

    for cluster in 0..3 {
        let prefix = format!("c{cluster}_");
        for i in 0..5 {
            g.add_node(sym(&format!("{prefix}{i}")));
        }
        for i in 0..5 {
            for j in (i + 1)..5 {
                g.add_edge(calls_edge(&format!("{prefix}{i}"), &format!("{prefix}{j}")));
                g.add_edge(calls_edge(&format!("{prefix}{j}"), &format!("{prefix}{i}")));
            }
        }
    }

    // Very weak bridges between clusters.
    g.add_edge(weak_edge("c0_0", "c1_0"));
    g.add_edge(weak_edge("c1_0", "c2_0"));

    let result = leiden_communities(&g, 1.0, 10);

    assert!(
        result.modularity > 0.3,
        "Leiden modularity should be > 0.3 on a well-clustered graph, got {}",
        result.modularity,
    );

    assert_communities_connected(&result, &g);
}
