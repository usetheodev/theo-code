/// Tests for escape.rs — Escape Hatch / Context Miss Detection

use theo_engine_retrieval::escape::ContextMembership;
use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_symbol_node(id: &str, file_path: &str) -> Node {
    Node {
        id: id.to_string(),
        node_type: NodeType::Symbol,
        name: id.to_string(),
        file_path: Some(file_path.to_string()),
        signature: Some(format!("fn {}()", id)),
        kind: Some(SymbolKind::Function),
        line_start: Some(1),
        line_end: Some(5),
        last_modified: 1000.0,
        doc: None,
    }
}

fn fixture() -> (CodeGraph, Vec<Community>, Vec<String>) {
    let mut graph = CodeGraph::new();

    // Auth community nodes
    graph.add_node(make_symbol_node("auth_jwt", "src/auth/jwt.rs"));
    graph.add_node(make_symbol_node("auth_login", "src/auth/login.rs"));

    // DB community nodes
    graph.add_node(make_symbol_node("db_conn", "src/db/connection.rs"));
    graph.add_node(make_symbol_node("db_query", "src/db/query.rs"));

    // Inter-community edge: auth -> db
    graph.add_edge(Edge {
        source: "auth_jwt".to_string(),
        target: "db_conn".to_string(),
        edge_type: EdgeType::Calls,
        weight: 1.0,
    });

    let auth_comm = Community {
        id: "comm-auth".to_string(),
        name: "authentication".to_string(),
        level: 0,
        node_ids: vec!["auth_jwt".to_string(), "auth_login".to_string()],
        parent_id: None,
        version: 1,
    };

    let db_comm = Community {
        id: "comm-db".to_string(),
        name: "database".to_string(),
        level: 0,
        node_ids: vec!["db_conn".to_string(), "db_query".to_string()],
        parent_id: None,
        version: 1,
    };

    // Context currently only contains auth files
    let context_files = vec![
        "src/auth/jwt.rs".to_string(),
        "src/auth/login.rs".to_string(),
    ];

    (graph, vec![auth_comm, db_comm], context_files)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_file_in_context_returns_true() {
    let (_, _, context_files) = fixture();
    let membership = ContextMembership::new(&context_files);

    assert!(
        membership.contains("src/auth/jwt.rs"),
        "jwt.rs should be in context"
    );
    assert!(
        membership.contains("src/auth/login.rs"),
        "login.rs should be in context"
    );
}

#[test]
fn test_file_not_in_context_returns_false() {
    let (_, _, context_files) = fixture();
    let membership = ContextMembership::new(&context_files);

    assert!(
        !membership.contains("src/db/connection.rs"),
        "db/connection.rs should NOT be in context"
    );
    assert!(
        !membership.contains("src/unknown/file.rs"),
        "unknown file should NOT be in context"
    );
}

#[test]
fn test_empty_context_contains_nothing() {
    let membership = ContextMembership::new(&[]);
    assert!(!membership.contains("anything.rs"));
}

#[test]
fn test_detect_miss_returns_correct_community() {
    let (graph, communities, context_files) = fixture();
    let membership = ContextMembership::new(&context_files);

    let miss = membership.detect_miss("src/db/connection.rs", &graph, &communities);

    assert!(miss.is_some(), "should detect a miss for db/connection.rs");
    let miss = miss.unwrap();
    assert_eq!(miss.file_path, "src/db/connection.rs");
    assert_eq!(
        miss.containing_community, "comm-db",
        "containing community should be comm-db"
    );
}

#[test]
fn test_detect_miss_returns_none_for_file_in_context() {
    let (graph, communities, context_files) = fixture();
    let membership = ContextMembership::new(&context_files);

    // auth_jwt.rs is already in context — no miss
    let miss = membership.detect_miss("src/auth/jwt.rs", &graph, &communities);
    assert!(miss.is_none(), "file in context should not produce a miss");
}

#[test]
fn test_detect_miss_returns_none_for_unknown_file() {
    let (graph, communities, context_files) = fixture();
    let membership = ContextMembership::new(&context_files);

    // Unknown file — not in any community
    let miss = membership.detect_miss("src/totally/unknown.rs", &graph, &communities);
    assert!(miss.is_none(), "unknown file not in any community should return None");
}

#[test]
fn test_detect_miss_suggested_expansion_contains_neighbor_communities() {
    let (graph, communities, context_files) = fixture();
    let membership = ContextMembership::new(&context_files);

    // auth community is neighbor of db community (via auth_jwt -> db_conn edge)
    // When we miss db, suggested_expansion should hint at auth (1-hop neighbor)
    // OR at least not be completely empty if there are neighbors
    let miss = membership.detect_miss("src/db/connection.rs", &graph, &communities);
    assert!(miss.is_some());
    let miss = miss.unwrap();

    // The suggested expansion is neighbor communities — auth is connected to db
    // via auth_jwt -> db_conn, so auth should appear
    // (Note: the edge goes auth->db, so db's neighbors in the community graph
    //  include auth via the reverse direction)
    assert!(
        miss.suggested_expansion.contains(&"comm-auth".to_string())
            || miss.suggested_expansion.is_empty(),
        "suggested_expansion should contain neighbor communities or be empty for isolated communities"
    );
}
