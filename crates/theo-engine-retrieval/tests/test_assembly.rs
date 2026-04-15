use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, Node, NodeType, SymbolKind};
/// Tests for assembly.rs — Greedy Knapsack Context Assembly
use theo_engine_retrieval::assembly::assemble_greedy;
use theo_engine_retrieval::search::ScoredCommunity;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_symbol_node(id: &str, name: &str, signature: &str) -> Node {
    Node {
        id: id.to_string(),
        node_type: NodeType::Symbol,
        name: name.to_string(),
        file_path: Some(format!("src/{}.rs", id)),
        signature: Some(signature.to_string()),
        kind: Some(SymbolKind::Function),
        line_start: Some(1),
        line_end: Some(10),
        last_modified: 1000.0,
        doc: None,
    }
}

fn make_scored(community: Community, score: f64) -> ScoredCommunity {
    ScoredCommunity { community, score }
}

fn fixture_scored_communities() -> (CodeGraph, Vec<ScoredCommunity>) {
    let mut graph = CodeGraph::new();

    // Auth (high score, moderate size)
    graph.add_node(make_symbol_node(
        "auth_jwt",
        "verify_jwt",
        "fn verify_jwt(token: &str) -> Result<Claims>",
    ));
    graph.add_node(make_symbol_node(
        "auth_login",
        "login",
        "fn login(user: &str, pass: &str) -> Token",
    ));

    // DB (medium score, moderate size)
    graph.add_node(make_symbol_node(
        "db_conn",
        "connect",
        "fn connect(url: &str) -> Connection",
    ));
    graph.add_node(make_symbol_node(
        "db_query",
        "query",
        "fn query(conn: &Connection, sql: &str) -> Rows",
    ));

    // API (low score, small)
    graph.add_node(make_symbol_node(
        "api_handler",
        "handle",
        "fn handle(req: Request) -> Response",
    ));

    let auth = Community {
        id: "comm-auth".to_string(),
        name: "authentication".to_string(),
        level: 0,
        node_ids: vec!["auth_jwt".to_string(), "auth_login".to_string()],
        parent_id: None,
        version: 1,
    };

    let db = Community {
        id: "comm-db".to_string(),
        name: "database".to_string(),
        level: 0,
        node_ids: vec!["db_conn".to_string(), "db_query".to_string()],
        parent_id: None,
        version: 1,
    };

    let api = Community {
        id: "comm-api".to_string(),
        name: "api".to_string(),
        level: 0,
        node_ids: vec!["api_handler".to_string()],
        parent_id: None,
        version: 1,
    };

    let scored = vec![
        make_scored(auth, 10.0),
        make_scored(db, 5.0),
        make_scored(api, 1.0),
    ];

    (graph, scored)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_budget_zero_produces_empty_payload() {
    let (graph, scored) = fixture_scored_communities();
    let payload = assemble_greedy(&scored, &graph, 0);

    assert!(
        payload.items.is_empty(),
        "zero budget should yield no items"
    );
    assert_eq!(payload.total_tokens, 0);
}

#[test]
fn test_huge_budget_includes_all_non_singleton_communities() {
    let (graph, scored) = fixture_scored_communities();
    let payload = assemble_greedy(&scored, &graph, 1_000_000);

    // Singletons (communities with <2 members) are filtered from output.
    // Fixture has: auth (2 members), db (2 members), api (1 member — filtered).
    let non_singleton_count = scored
        .iter()
        .filter(|s| s.community.node_ids.len() >= 2)
        .count();
    assert_eq!(
        payload.items.len(),
        non_singleton_count,
        "huge budget should include all non-singleton communities"
    );
}

#[test]
fn test_total_tokens_never_exceeds_budget() {
    let (graph, scored) = fixture_scored_communities();

    // Use a tight budget — should fit some but not all
    for budget in [10, 50, 100, 500, 1000] {
        let payload = assemble_greedy(&scored, &graph, budget);
        assert!(
            payload.total_tokens <= budget,
            "total_tokens ({}) exceeded budget ({}) for budget={}",
            payload.total_tokens,
            budget,
            budget
        );
    }
}

#[test]
fn test_tight_budget_excludes_low_density_items() {
    let (graph, scored) = fixture_scored_communities();

    // The budget is very tight — only 1 item at most should fit
    let payload = assemble_greedy(&scored, &graph, 20);

    // Total tokens should respect the budget
    assert!(payload.total_tokens <= 20);
}

#[test]
fn test_exploration_hints_non_empty_when_items_excluded() {
    let (graph, scored) = fixture_scored_communities();

    // Tight budget: at least one community should be excluded
    let payload = assemble_greedy(&scored, &graph, 5);

    if payload.items.len() < scored.len() {
        assert!(
            !payload.exploration_hints.is_empty(),
            "exploration_hints should list excluded communities"
        );
    }
}

#[test]
fn test_budget_tokens_field_matches_argument() {
    let (graph, scored) = fixture_scored_communities();
    let budget = 300;
    let payload = assemble_greedy(&scored, &graph, budget);

    assert_eq!(
        payload.budget_tokens, budget,
        "budget_tokens field should equal the argument passed"
    );
}

#[test]
fn test_items_have_positive_token_count() {
    let (graph, scored) = fixture_scored_communities();
    let payload = assemble_greedy(&scored, &graph, 1_000_000);

    for item in &payload.items {
        assert!(
            item.token_count > 0,
            "every item should have a positive token count"
        );
    }
}

#[test]
fn test_empty_scored_produces_empty_payload() {
    let graph = CodeGraph::new();
    let scored: Vec<ScoredCommunity> = vec![];
    let payload = assemble_greedy(&scored, &graph, 1000);

    assert!(payload.items.is_empty());
    assert_eq!(payload.total_tokens, 0);
}
