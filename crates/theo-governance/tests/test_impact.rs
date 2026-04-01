/// Integration tests for impact analysis (BFS).
///
/// Graph topology used across most tests:
///
///   auth.py --[CONTAINS]--> login()
///   auth.py --[CONTAINS]--> validate_token()
///   db.py   --[CONTAINS]--> query()
///   db.py   --[CONTAINS]--> connect()
///   api.py  --[CONTAINS]--> handle_request()
///   api.py  --[CONTAINS]--> handle_response()
///
///   handle_request() --[CALLS]--> login()
///   login()          --[CALLS]--> query()
///
///   test_login  --[TESTS]--> login()
///   test_query  --[TESTS]--> query()
///
///   auth.py <--[CO_CHANGES]--> api.py  (weight 0.5)
///
/// Communities:
///   auth_community : [login, validate_token]
///   db_community   : [query, connect]
///   api_community  : [handle_request, handle_response]

use theo_governance::impact::analyze_impact;
use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};
use theo_engine_graph::cluster::Community;

// ---------------------------------------------------------------------------
// Helper: build the standard test graph
// ---------------------------------------------------------------------------

fn make_node(id: &str, node_type: NodeType, name: &str, file_path: Option<&str>) -> Node {
    Node {
        id: id.to_string(),
        node_type,
        name: name.to_string(),
        file_path: file_path.map(str::to_string),
        signature: None,
        kind: Some(SymbolKind::Function),
        line_start: None,
        line_end: None,
        last_modified: 0.0,
        doc: None,
    }
}

fn make_edge(source: &str, target: &str, edge_type: EdgeType, weight: f64) -> Edge {
    Edge {
        source: source.to_string(),
        target: target.to_string(),
        edge_type,
        weight,
    }
}

fn build_test_graph() -> CodeGraph {
    let mut g = CodeGraph::new();

    // File nodes
    g.add_node(make_node("auth.py", NodeType::File, "auth.py", Some("auth.py")));
    g.add_node(make_node("db.py", NodeType::File, "db.py", Some("db.py")));
    g.add_node(make_node("api.py", NodeType::File, "api.py", Some("api.py")));

    // Symbol nodes
    g.add_node(make_node("login", NodeType::Symbol, "login", Some("auth.py")));
    g.add_node(make_node("validate_token", NodeType::Symbol, "validate_token", Some("auth.py")));
    g.add_node(make_node("query", NodeType::Symbol, "query", Some("db.py")));
    g.add_node(make_node("connect", NodeType::Symbol, "connect", Some("db.py")));
    g.add_node(make_node("handle_request", NodeType::Symbol, "handle_request", Some("api.py")));
    g.add_node(make_node("handle_response", NodeType::Symbol, "handle_response", Some("api.py")));

    // Test nodes
    g.add_node(make_node("test_login", NodeType::Test, "test_login", None));
    g.add_node(make_node("test_query", NodeType::Test, "test_query", None));

    // CONTAINS edges: file -> symbol
    g.add_edge(make_edge("auth.py", "login", EdgeType::Contains, 1.0));
    g.add_edge(make_edge("auth.py", "validate_token", EdgeType::Contains, 1.0));
    g.add_edge(make_edge("db.py", "query", EdgeType::Contains, 1.0));
    g.add_edge(make_edge("db.py", "connect", EdgeType::Contains, 1.0));
    g.add_edge(make_edge("api.py", "handle_request", EdgeType::Contains, 1.0));
    g.add_edge(make_edge("api.py", "handle_response", EdgeType::Contains, 1.0));

    // CALLS edges
    g.add_edge(make_edge("handle_request", "login", EdgeType::Calls, 1.0));
    g.add_edge(make_edge("login", "query", EdgeType::Calls, 1.0));

    // TESTS edges
    g.add_edge(make_edge("test_login", "login", EdgeType::Tests, 0.7));
    g.add_edge(make_edge("test_query", "query", EdgeType::Tests, 0.7));

    // CO_CHANGES edge: auth.py <-> api.py
    g.add_edge(make_edge("auth.py", "api.py", EdgeType::CoChanges, 0.5));

    g
}

fn build_communities() -> Vec<Community> {
    vec![
        Community {
            id: "auth_community".to_string(),
            name: "Auth Community".to_string(),
            level: 0,
            node_ids: vec!["login".to_string(), "validate_token".to_string()],
            parent_id: None,
            version: 1,
        },
        Community {
            id: "db_community".to_string(),
            name: "DB Community".to_string(),
            level: 0,
            node_ids: vec!["query".to_string(), "connect".to_string()],
            parent_id: None,
            version: 1,
        },
        Community {
            id: "api_community".to_string(),
            name: "API Community".to_string(),
            level: 0,
            node_ids: vec!["handle_request".to_string(), "handle_response".to_string()],
            parent_id: None,
            version: 1,
        },
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_edit_auth_affects_auth_community() {
    let graph = build_test_graph();
    let communities = build_communities();
    let report = analyze_impact("auth.py", &graph, &communities, 3);
    assert!(
        report.affected_communities.contains(&"auth_community".to_string()),
        "auth_community must be affected when auth.py is edited, got: {:?}",
        report.affected_communities
    );
}

#[test]
fn test_edit_auth_affects_db_community_via_bfs() {
    // login -> query (CALLS), so db_community is reached at depth 2
    let graph = build_test_graph();
    let communities = build_communities();
    let report = analyze_impact("auth.py", &graph, &communities, 3);
    assert!(
        report.affected_communities.contains(&"db_community".to_string()),
        "db_community must be affected via CALLS login->query, got: {:?}",
        report.affected_communities
    );
}

#[test]
fn test_edit_auth_co_change_api_in_candidates() {
    let graph = build_test_graph();
    let communities = build_communities();
    let report = analyze_impact("auth.py", &graph, &communities, 3);
    assert!(
        report.co_change_candidates.contains(&"api.py".to_string()),
        "api.py must appear as co-change candidate, got: {:?}",
        report.co_change_candidates
    );
}

#[test]
fn test_tests_covering_edit_includes_test_login() {
    let graph = build_test_graph();
    let communities = build_communities();
    let report = analyze_impact("auth.py", &graph, &communities, 3);
    assert!(
        report.tests_covering_edit.contains(&"test_login".to_string()),
        "test_login must cover auth.py edit, got: {:?}",
        report.tests_covering_edit
    );
}

#[test]
fn test_risk_alerts_contain_co_change_for_api() {
    let graph = build_test_graph();
    let communities = build_communities();
    let report = analyze_impact("auth.py", &graph, &communities, 3);
    let has_cochange_alert = report
        .risk_alerts
        .iter()
        .any(|a| a.contains("Co-change") || a.contains("co-change") || a.contains("api.py"));
    assert!(
        has_cochange_alert,
        "Expected a co-change alert mentioning api.py, got: {:?}",
        report.risk_alerts
    );
}

#[test]
fn test_cross_cluster_alert_when_multiple_communities_affected() {
    let graph = build_test_graph();
    let communities = build_communities();
    let report = analyze_impact("auth.py", &graph, &communities, 3);
    // At least auth_community and db_community are affected
    let has_cross_cluster = report
        .risk_alerts
        .iter()
        .any(|a| a.contains("Cross-cluster") || a.contains("cross-cluster") || a.contains("communities"));
    assert!(
        has_cross_cluster,
        "Expected cross-cluster alert since multiple communities affected, got: {:?}",
        report.risk_alerts
    );
}

#[test]
fn test_untested_symbol_generates_alert() {
    // Build a graph with a symbol that has no test coverage
    let mut graph = CodeGraph::new();
    graph.add_node(make_node("auth.py", NodeType::File, "auth.py", Some("auth.py")));
    graph.add_node(make_node("login", NodeType::Symbol, "login", Some("auth.py")));
    graph.add_edge(make_edge("auth.py", "login", EdgeType::Contains, 1.0));
    // No TESTS edge for login

    let communities = vec![Community {
        id: "auth_community".to_string(),
        name: "Auth Community".to_string(),
        level: 0,
        node_ids: vec!["login".to_string()],
        parent_id: None,
        version: 1,
    }];

    let report = analyze_impact("auth.py", &graph, &communities, 3);
    let has_untested = report
        .risk_alerts
        .iter()
        .any(|a| a.contains("Untested") || a.contains("untested") || a.contains("no test"));
    assert!(
        has_untested,
        "Expected untested modification alert, got: {:?}",
        report.risk_alerts
    );
}

#[test]
fn test_empty_graph_returns_empty_report_no_panic() {
    let graph = CodeGraph::new();
    let communities: Vec<Community> = vec![];
    let report = analyze_impact("nonexistent.py", &graph, &communities, 3);
    assert!(report.affected_communities.is_empty());
    assert!(report.tests_covering_edit.is_empty());
    assert!(report.co_change_candidates.is_empty());
}

#[test]
fn test_max_depth_zero_does_not_bfs_expand() {
    let graph = build_test_graph();
    let communities = build_communities();
    // With max_depth=0, BFS should not follow any edges from the symbols in auth.py
    // So db_community (only reachable via login->query) should NOT be affected
    let report = analyze_impact("auth.py", &graph, &communities, 0);
    assert!(
        !report.affected_communities.contains(&"db_community".to_string()),
        "db_community must NOT be affected at depth 0, got: {:?}",
        report.affected_communities
    );
}

#[test]
fn test_edited_file_is_recorded() {
    let graph = build_test_graph();
    let communities = build_communities();
    let report = analyze_impact("auth.py", &graph, &communities, 3);
    assert_eq!(report.edited_file, "auth.py");
}

#[test]
fn test_bfs_depth_is_recorded() {
    let graph = build_test_graph();
    let communities = build_communities();
    let report = analyze_impact("auth.py", &graph, &communities, 3);
    assert_eq!(report.bfs_depth, 3);
}
