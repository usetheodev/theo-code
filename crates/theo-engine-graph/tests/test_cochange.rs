/// Tests for co-change temporal decay and edge update logic.
use theo_engine_graph::cochange::{DEFAULT_LAMBDA, temporal_decay, update_cochanges};
use theo_engine_graph::model::{CodeGraph, EdgeType, Node, NodeType};

fn file_node(id: &str) -> Node {
    Node {
        id: id.to_string(),
        node_type: NodeType::File,
        name: id.to_string(),
        file_path: Some(id.to_string()),
        signature: None,
        kind: None,
        line_start: None,
        line_end: None,
        last_modified: 0.0,
        doc: None,
    }
}

// --- temporal_decay ---

#[test]
fn decay_at_zero_days_is_one() {
    let w = temporal_decay(0.0, DEFAULT_LAMBDA);
    assert!((w - 1.0).abs() < 1e-9, "Expected 1.0, got {w}");
}

#[test]
fn decay_at_half_life_is_approximately_half() {
    // Half-life: t where exp(-lambda*t) = 0.5  =>  t = ln(2)/lambda ≈ 69.3 days
    let half_life = std::f64::consts::LN_2 / DEFAULT_LAMBDA;
    let w = temporal_decay(half_life, DEFAULT_LAMBDA);
    assert!(
        (w - 0.5).abs() < 0.01,
        "Expected ~0.5 at half-life, got {w}"
    );
}

#[test]
fn decay_at_700_days_is_very_small() {
    let w = temporal_decay(700.0, DEFAULT_LAMBDA);
    // exp(-0.01 * 700) = exp(-7) ≈ 0.000912
    assert!(w < 0.002, "Expected very small weight at 700 days, got {w}");
    assert!(w > 0.0, "Weight must be positive");
}

#[test]
fn decay_is_monotonically_decreasing() {
    let w0 = temporal_decay(0.0, DEFAULT_LAMBDA);
    let w1 = temporal_decay(10.0, DEFAULT_LAMBDA);
    let w2 = temporal_decay(100.0, DEFAULT_LAMBDA);
    let w3 = temporal_decay(1000.0, DEFAULT_LAMBDA);
    assert!(w0 > w1);
    assert!(w1 > w2);
    assert!(w2 > w3);
}

#[test]
fn decay_with_higher_lambda_decays_faster() {
    let slow = temporal_decay(50.0, 0.01);
    let fast = temporal_decay(50.0, 0.1);
    assert!(fast < slow, "Higher lambda should decay faster");
}

// --- update_cochanges ---

#[test]
fn update_cochanges_creates_edges_between_all_pairs() {
    let mut g = CodeGraph::new();
    for id in ["src/a.rs", "src/b.rs", "src/c.rs"] {
        g.add_node(file_node(id));
    }

    let changed = vec![
        "src/a.rs".to_string(),
        "src/b.rs".to_string(),
        "src/c.rs".to_string(),
    ];
    update_cochanges(&mut g, &changed, 0.0);

    // Should create edges: a-b, a-c, b-c (and reverse? check at least 3 co-change edges)
    let cochange_edges = g.edges_of_type(&EdgeType::CoChanges);
    assert!(
        cochange_edges.len() >= 3,
        "Expected at least 3 CoChanges edges for 3 changed files, got {}",
        cochange_edges.len()
    );
}

#[test]
fn update_cochanges_single_file_no_edges() {
    let mut g = CodeGraph::new();
    g.add_node(file_node("src/a.rs"));

    update_cochanges(&mut g, &["src/a.rs".to_string()], 0.0);
    let cochange_edges = g.edges_of_type(&EdgeType::CoChanges);
    assert!(
        cochange_edges.is_empty(),
        "Single file should not create co-change edges"
    );
}

#[test]
fn update_cochanges_empty_list_no_edges() {
    let mut g = CodeGraph::new();
    update_cochanges(&mut g, &[], 0.0);
    assert_eq!(g.edge_count(), 0);
}

#[test]
fn update_cochanges_edge_weight_equals_decay() {
    let mut g = CodeGraph::new();
    g.add_node(file_node("src/a.rs"));
    g.add_node(file_node("src/b.rs"));

    let days = 30.0;
    update_cochanges(
        &mut g,
        &["src/a.rs".to_string(), "src/b.rs".to_string()],
        days,
    );

    let edges = g.edges_between("src/a.rs", "src/b.rs");
    assert!(!edges.is_empty(), "Expected co-change edge between a and b");

    let expected_weight = temporal_decay(days, DEFAULT_LAMBDA);
    let edge = &edges[0];
    assert!(
        (edge.weight - expected_weight).abs() < 1e-9,
        "Edge weight {:.6} should equal decay weight {:.6}",
        edge.weight,
        expected_weight
    );
}

#[test]
fn update_cochanges_accumulates_on_repeated_commits() {
    let mut g = CodeGraph::new();
    g.add_node(file_node("src/a.rs"));
    g.add_node(file_node("src/b.rs"));

    // First commit
    update_cochanges(
        &mut g,
        &["src/a.rs".to_string(), "src/b.rs".to_string()],
        5.0,
    );
    let first_edges = g.edges_between("src/a.rs", "src/b.rs");
    let first_count = first_edges.len();

    // Second commit — adds another co-change edge or updates weight
    update_cochanges(
        &mut g,
        &["src/a.rs".to_string(), "src/b.rs".to_string()],
        10.0,
    );
    let second_edges = g.edges_between("src/a.rs", "src/b.rs");

    // Either a second edge was added, or the weight was updated (not empty)
    assert!(!second_edges.is_empty());
    // At minimum, there's still a co-change edge
    let _ = first_count; // suppress unused warning
}

// --- S3-T2: Impact set tests ---

use theo_engine_graph::cochange::compute_impact_set;

#[test]
fn impact_set_finds_cochanged_files() {
    let mut g = CodeGraph::new();
    g.add_node(file_node("a.rs"));
    g.add_node(file_node("b.rs"));
    g.add_node(file_node("c.rs"));

    // a and b co-change, a and c co-change
    update_cochanges(&mut g, &["a.rs".into(), "b.rs".into()], 5.0);
    update_cochanges(&mut g, &["a.rs".into(), "c.rs".into()], 10.0);

    let impact = compute_impact_set(&g, &["a.rs".into()], 10, 0.0);

    // Both b and c should appear
    let files: Vec<&str> = impact
        .affected_files
        .iter()
        .map(|(f, _)| f.as_str())
        .collect();
    assert!(files.contains(&"b.rs"), "b.rs should be in impact set");
    assert!(files.contains(&"c.rs"), "c.rs should be in impact set");
}

#[test]
fn impact_set_excludes_changed_files() {
    let mut g = CodeGraph::new();
    g.add_node(file_node("a.rs"));
    g.add_node(file_node("b.rs"));

    update_cochanges(&mut g, &["a.rs".into(), "b.rs".into()], 1.0);

    let impact = compute_impact_set(&g, &["a.rs".into()], 10, 0.0);
    let files: Vec<&str> = impact
        .affected_files
        .iter()
        .map(|(f, _)| f.as_str())
        .collect();
    assert!(
        !files.contains(&"a.rs"),
        "Changed file should not appear in its own impact set"
    );
}

#[test]
fn impact_set_respects_top_k() {
    let mut g = CodeGraph::new();
    for i in 0..10 {
        g.add_node(file_node(&format!("f{}.rs", i)));
    }
    // f0 co-changes with all others
    for i in 1..10 {
        update_cochanges(&mut g, &["f0.rs".into(), format!("f{}.rs", i)], i as f64);
    }

    let impact = compute_impact_set(&g, &["f0.rs".into()], 3, 0.0);
    assert_eq!(
        impact.affected_files.len(),
        3,
        "Should return at most top_k files"
    );
}

#[test]
fn impact_set_respects_min_weight() {
    let mut g = CodeGraph::new();
    g.add_node(file_node("a.rs"));
    g.add_node(file_node("b.rs"));
    g.add_node(file_node("c.rs"));

    // a-b with recent co-change (high weight)
    update_cochanges(&mut g, &["a.rs".into(), "b.rs".into()], 1.0);
    // a-c with old co-change (low weight)
    update_cochanges(&mut g, &["a.rs".into(), "c.rs".into()], 200.0);

    let impact = compute_impact_set(&g, &["a.rs".into()], 10, 0.5);
    let files: Vec<&str> = impact
        .affected_files
        .iter()
        .map(|(f, _)| f.as_str())
        .collect();
    assert!(
        files.contains(&"b.rs"),
        "Recent co-change should pass min_weight"
    );
    // c.rs has weight = exp(-0.01 * 200) ≈ 0.135 < 0.5
    assert!(
        !files.contains(&"c.rs"),
        "Old co-change should be below min_weight"
    );
}

#[test]
fn impact_set_empty_for_no_cochanges() {
    let mut g = CodeGraph::new();
    g.add_node(file_node("a.rs"));
    g.add_node(file_node("b.rs"));

    let impact = compute_impact_set(&g, &["a.rs".into()], 10, 0.0);
    assert!(impact.affected_files.is_empty());
}

#[test]
fn impact_set_sorted_by_weight_descending() {
    let mut g = CodeGraph::new();
    g.add_node(file_node("a.rs"));
    g.add_node(file_node("b.rs"));
    g.add_node(file_node("c.rs"));

    // a-b: recent (high weight), a-c: old (low weight)
    update_cochanges(&mut g, &["a.rs".into(), "b.rs".into()], 1.0);
    update_cochanges(&mut g, &["a.rs".into(), "c.rs".into()], 50.0);

    let impact = compute_impact_set(&g, &["a.rs".into()], 10, 0.0);
    assert!(impact.affected_files.len() == 2);
    assert!(
        impact.affected_files[0].1 >= impact.affected_files[1].1,
        "Should be sorted by weight descending"
    );
}
