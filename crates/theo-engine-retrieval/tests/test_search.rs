use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};
/// Tests for search.rs — BM25 + MultiSignalScorer
use theo_engine_retrieval::search::{Bm25Index, MultiSignalScorer};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_symbol_node(id: &str, name: &str, signature: &str, last_modified: f64) -> Node {
    Node {
        id: id.to_string(),
        node_type: NodeType::Symbol,
        name: name.to_string(),
        file_path: Some(format!("src/{}.rs", id)),
        signature: Some(signature.to_string()),
        kind: Some(SymbolKind::Function),
        line_start: Some(1),
        line_end: Some(10),
        last_modified,
        doc: None,
    }
}

/// Build a graph + 3 communities for auth/db/api tests.
fn three_community_fixture() -> (CodeGraph, Vec<Community>) {
    let mut graph = CodeGraph::new();

    // Auth nodes
    graph.add_node(make_symbol_node(
        "auth_jwt",
        "verify_jwt_token",
        "fn verify_jwt_token(token: &str) -> Result<Claims>",
        1000.0,
    ));
    graph.add_node(make_symbol_node(
        "auth_login",
        "login_user",
        "fn login_user(username: &str, password: &str) -> Result<Token>",
        900.0,
    ));

    // DB nodes
    graph.add_node(make_symbol_node(
        "db_conn",
        "create_connection",
        "fn create_connection(url: &str) -> Result<Connection>",
        800.0,
    ));
    graph.add_node(make_symbol_node(
        "db_query",
        "execute_query",
        "fn execute_query(conn: &Connection, sql: &str) -> Result<Rows>",
        700.0,
    ));

    // API nodes
    graph.add_node(make_symbol_node(
        "api_handler",
        "handle_request",
        "fn handle_request(req: Request) -> Response",
        600.0,
    ));
    graph.add_node(make_symbol_node(
        "api_router",
        "build_router",
        "fn build_router() -> Router",
        500.0,
    ));

    // Edges within communities (intra-community)
    graph.add_edge(Edge {
        source: "auth_jwt".to_string(),
        target: "auth_login".to_string(),
        edge_type: EdgeType::Calls,
        weight: 1.0,
    });
    graph.add_edge(Edge {
        source: "db_conn".to_string(),
        target: "db_query".to_string(),
        edge_type: EdgeType::Calls,
        weight: 1.0,
    });
    graph.add_edge(Edge {
        source: "api_handler".to_string(),
        target: "api_router".to_string(),
        edge_type: EdgeType::Calls,
        weight: 1.0,
    });

    // Inter-community edge (auth -> api, so api community has a neighbor)
    graph.add_edge(Edge {
        source: "auth_login".to_string(),
        target: "api_handler".to_string(),
        edge_type: EdgeType::Calls,
        weight: 0.5,
    });

    let communities = vec![
        Community {
            id: "comm-auth".to_string(),
            name: "authentication JWT token login".to_string(),
            level: 0,
            node_ids: vec!["auth_jwt".to_string(), "auth_login".to_string()],
            parent_id: None,
            version: 1,
        },
        Community {
            id: "comm-db".to_string(),
            name: "database connection query SQL".to_string(),
            level: 0,
            node_ids: vec!["db_conn".to_string(), "db_query".to_string()],
            parent_id: None,
            version: 1,
        },
        Community {
            id: "comm-api".to_string(),
            name: "api router request handler http".to_string(),
            level: 0,
            node_ids: vec!["api_handler".to_string(), "api_router".to_string()],
            parent_id: None,
            version: 1,
        },
    ];

    (graph, communities)
}

// ---------------------------------------------------------------------------
// BM25 Tests
// ---------------------------------------------------------------------------

#[test]
fn test_bm25_jwt_token_query_ranks_auth_first() {
    let (graph, communities) = three_community_fixture();
    let index = Bm25Index::build(&communities, &graph);
    let results = index.search("JWT token", &communities);

    assert!(!results.is_empty(), "results should not be empty");
    assert_eq!(
        results[0].community.id, "comm-auth",
        "auth community should rank first for 'JWT token'"
    );
}

#[test]
fn test_bm25_database_connection_ranks_db_first() {
    let (graph, communities) = three_community_fixture();
    let index = Bm25Index::build(&communities, &graph);
    let results = index.search("database connection", &communities);

    assert!(!results.is_empty(), "results should not be empty");
    assert_eq!(
        results[0].community.id, "comm-db",
        "db community should rank first for 'database connection'"
    );
}

#[test]
fn test_bm25_empty_query_all_scores_zero() {
    let (graph, communities) = three_community_fixture();
    let index = Bm25Index::build(&communities, &graph);
    let results = index.search("", &communities);

    assert_eq!(results.len(), communities.len());
    for r in &results {
        assert_eq!(r.score, 0.0, "empty query should produce zero scores");
    }
}

#[test]
fn test_bm25_single_community_always_returned() {
    let mut graph = CodeGraph::new();
    graph.add_node(make_symbol_node(
        "only_node",
        "only_func",
        "fn only_func()",
        100.0,
    ));

    let communities = vec![Community {
        id: "comm-only".to_string(),
        name: "the only community".to_string(),
        level: 0,
        node_ids: vec!["only_node".to_string()],
        parent_id: None,
        version: 1,
    }];

    let index = Bm25Index::build(&communities, &graph);
    let results = index.search("only community", &communities);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].community.id, "comm-only");
    assert!(results[0].score >= 0.0);
}

#[test]
fn test_bm25_returns_all_communities() {
    let (graph, communities) = three_community_fixture();
    let index = Bm25Index::build(&communities, &graph);
    let results = index.search("authentication", &communities);

    assert_eq!(
        results.len(),
        communities.len(),
        "all communities should be returned"
    );
}

#[test]
fn test_bm25_results_sorted_descending() {
    let (graph, communities) = three_community_fixture();
    let index = Bm25Index::build(&communities, &graph);
    let results = index.search("JWT token", &communities);

    for window in results.windows(2) {
        assert!(
            window[0].score >= window[1].score,
            "results must be sorted descending by score"
        );
    }
}

// ---------------------------------------------------------------------------
// MultiSignalScorer Tests
// ---------------------------------------------------------------------------

#[test]
fn test_multi_signal_returns_all_communities() {
    let (graph, communities) = three_community_fixture();
    let scorer = MultiSignalScorer::build(&communities, &graph);
    let results = scorer.score("JWT token", &communities, &graph);

    assert_eq!(results.len(), communities.len());
}

#[test]
fn test_multi_signal_sorted_descending() {
    let (graph, communities) = three_community_fixture();
    let scorer = MultiSignalScorer::build(&communities, &graph);
    let results = scorer.score("database connection", &communities, &graph);

    for window in results.windows(2) {
        assert!(
            window[0].score >= window[1].score,
            "multi-signal results must be sorted descending"
        );
    }
}

#[test]
fn test_multi_signal_scores_non_negative() {
    let (graph, communities) = three_community_fixture();
    let scorer = MultiSignalScorer::build(&communities, &graph);
    let results = scorer.score("api router", &communities, &graph);

    for r in &results {
        assert!(r.score >= 0.0, "scores must be non-negative");
    }
}

#[test]
fn test_multi_signal_empty_communities() {
    let graph = CodeGraph::new();
    let communities: Vec<Community> = vec![];
    let scorer = MultiSignalScorer::build(&communities, &graph);
    let results = scorer.score("anything", &communities, &graph);
    assert!(results.is_empty());
}
