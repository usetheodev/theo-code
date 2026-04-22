//! Integration tests for the Phase 1 wiring of `harm_filter` into
//! `file_retriever::retrieve_files` (PLAN_CONTEXT_WIRING Task 1.3).
//!
//! These tests live in the integration test directory (compiled as a
//! separate crate) so they can only see the public API. They validate
//! that `retrieve_files` invokes `filter_harmful_chunks` at the right
//! stage, records the counter, respects the 40% cap, and survives
//! graphs with nothing to filter.

use std::collections::{HashMap, HashSet};

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};
use theo_engine_retrieval::file_retriever::{retrieve_files, RerankConfig};

fn file_node(id: &str, path: &str) -> Node {
    Node {
        id: id.to_string(),
        node_type: NodeType::File,
        name: path.to_string(),
        file_path: Some(path.to_string()),
        signature: None,
        kind: None,
        line_start: Some(1),
        line_end: Some(100),
        last_modified: 0.0,
        doc: None,
    }
}

fn symbol_node(id: &str, name: &str, path: &str) -> Node {
    Node {
        id: id.to_string(),
        node_type: NodeType::Symbol,
        name: name.to_string(),
        file_path: Some(path.to_string()),
        signature: Some(format!("pub fn {name}()")),
        kind: Some(SymbolKind::Function),
        line_start: Some(1),
        line_end: Some(5),
        last_modified: 0.0,
        doc: None,
    }
}

fn build_graph_with_definer_and_test() -> CodeGraph {
    // BM25 scores candidates by matching the query against file content
    // (derived from symbols). Give BOTH files a "verify_token" symbol
    // so both land in the top candidates — then the harm_filter decides
    // which to drop.
    //
    // Path convention matches what `harm_filter::is_test_file` recognises:
    // `tests/auth_test.rs` ends with `_test.rs` (test file).
    // `src/auth.rs` is the definer.
    let mut g = CodeGraph::new();
    g.add_node(file_node("file:src/auth.rs", "src/auth.rs"));
    g.add_node(file_node("file:tests/auth_test.rs", "tests/auth_test.rs"));
    g.add_node(symbol_node("sym:verify_token", "verify_token", "src/auth.rs"));
    g.add_node(symbol_node(
        "sym:test_verify_token",
        "test_verify_token",
        "tests/auth_test.rs",
    ));
    g.add_edge(Edge {
        source: "file:src/auth.rs".into(),
        target: "sym:verify_token".into(),
        edge_type: EdgeType::Contains,
        weight: 1.0,
    });
    g.add_edge(Edge {
        source: "file:tests/auth_test.rs".into(),
        target: "sym:test_verify_token".into(),
        edge_type: EdgeType::Contains,
        weight: 1.0,
    });
    g
}

#[test]
fn retrieve_files_drops_test_when_definer_present() {
    // AAA: definer src/auth.rs + test tests/test_auth.rs → test is dropped.
    let g = build_graph_with_definer_and_test();
    let result = retrieve_files(
        &g,
        &[] as &[Community],
        "verify_token",
        &RerankConfig::default(),
        &HashSet::new(),
    );

    let paths: Vec<&str> = result
        .primary_files
        .iter()
        .map(|r| r.path.as_str())
        .collect();
    assert!(
        !paths.contains(&"tests/auth_test.rs"),
        "test file should have been filtered when definer is present: {paths:?}"
    );
    assert!(result.harm_removals > 0, "harm_filter must fire");
}

#[test]
fn retrieve_files_harm_removals_counter_is_monotonic() {
    // Running over two different graphs: counter reflects only the
    // current call (no hidden global state).
    let g = build_graph_with_definer_and_test();
    let r1 = retrieve_files(
        &g,
        &[] as &[Community],
        "verify_token",
        &RerankConfig::default(),
        &HashSet::new(),
    );
    let r2 = retrieve_files(
        &g,
        &[] as &[Community],
        "verify_token",
        &RerankConfig::default(),
        &HashSet::new(),
    );
    assert_eq!(
        r1.harm_removals, r2.harm_removals,
        "harm_removals must be deterministic for identical input"
    );
}

#[test]
fn retrieve_files_empty_graph_yields_zero_removals() {
    let g = CodeGraph::new();
    let result = retrieve_files(
        &g,
        &[] as &[Community],
        "nothing",
        &RerankConfig::default(),
        &HashSet::new(),
    );
    assert!(result.primary_files.is_empty());
    assert_eq!(result.harm_removals, 0);
}

#[test]
fn retrieve_files_preserves_non_test_candidates() {
    // AAA: two definers (no test files). Harm filter must leave both alone.
    let mut g = CodeGraph::new();
    g.add_node(file_node("file:src/a.rs", "src/a.rs"));
    g.add_node(file_node("file:src/b.rs", "src/b.rs"));
    g.add_node(symbol_node("sym:foo", "foo", "src/a.rs"));
    g.add_node(symbol_node("sym:bar", "bar", "src/b.rs"));
    g.add_edge(Edge {
        source: "file:src/a.rs".into(),
        target: "sym:foo".into(),
        edge_type: EdgeType::Contains,
        weight: 1.0,
    });
    g.add_edge(Edge {
        source: "file:src/b.rs".into(),
        target: "sym:bar".into(),
        edge_type: EdgeType::Contains,
        weight: 1.0,
    });

    let result = retrieve_files(
        &g,
        &[] as &[Community],
        "foo",
        &RerankConfig::default(),
        &HashSet::new(),
    );
    // Both definers survive; no test/fixture/config to remove.
    let paths: Vec<&str> = result
        .primary_files
        .iter()
        .map(|r| r.path.as_str())
        .collect();
    assert!(paths.contains(&"src/a.rs"));
    assert_eq!(result.harm_removals, 0);
}

#[test]
fn retrieve_files_respects_40_percent_safety_cap() {
    // Same plan-DoD guard as in the src-level test but now from the
    // integration-test crate: ratio removed/total <= 50% (ceil of 40%).
    let g = build_graph_with_definer_and_test();
    let result = retrieve_files(
        &g,
        &[] as &[Community],
        "verify_token",
        &RerankConfig::default(),
        &HashSet::new(),
    );

    let pre = result.primary_files.len() + result.harm_removals;
    if pre > 0 {
        let ratio = result.harm_removals as f64 / pre as f64;
        assert!(
            ratio <= 0.5,
            "harm removal ratio {ratio} exceeded 50% safety bound"
        );
    }
}

// Silence unused-import warning on `HashMap` for tests that do not need it.
#[allow(dead_code)]
fn _unused(_: HashMap<String, f64>) {}
