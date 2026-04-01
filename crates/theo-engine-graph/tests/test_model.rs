/// Tests for CodeGraph model (NodeType, EdgeType, Node, Edge, CodeGraph).
use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};

fn make_file_node(id: &str, path: &str) -> Node {
    Node {
        id: id.to_string(),
        node_type: NodeType::File,
        name: path.to_string(),
        file_path: Some(path.to_string()),
        signature: None,
        kind: None,
        line_start: None,
        line_end: None,
        last_modified: 0.0,
        doc: None,
    }
}

fn make_symbol_node(id: &str, name: &str, kind: SymbolKind) -> Node {
    Node {
        id: id.to_string(),
        node_type: NodeType::Symbol,
        name: name.to_string(),
        file_path: None,
        signature: None,
        kind: Some(kind),
        line_start: Some(1),
        line_end: Some(10),
        last_modified: 0.0,
        doc: None,
    }
}

fn make_edge(src: &str, tgt: &str, edge_type: EdgeType) -> Edge {
    let weight = edge_type.default_weight();
    Edge {
        source: src.to_string(),
        target: tgt.to_string(),
        weight,
        edge_type,
    }
}

// --- Empty graph ---

#[test]
fn empty_graph_has_zero_counts() {
    let g = CodeGraph::new();
    assert_eq!(g.node_count(), 0);
    assert_eq!(g.edge_count(), 0);
}

#[test]
fn empty_graph_neighbors_returns_empty() {
    let g = CodeGraph::new();
    assert!(g.neighbors("nonexistent").is_empty());
}

#[test]
fn empty_graph_reverse_neighbors_returns_empty() {
    let g = CodeGraph::new();
    assert!(g.reverse_neighbors("nonexistent").is_empty());
}

#[test]
fn empty_graph_symbol_nodes_returns_empty() {
    let g = CodeGraph::new();
    assert!(g.symbol_nodes().is_empty());
}

#[test]
fn empty_graph_file_nodes_returns_empty() {
    let g = CodeGraph::new();
    assert!(g.file_nodes().is_empty());
}

// --- Add nodes ---

#[test]
fn add_single_node_increments_count() {
    let mut g = CodeGraph::new();
    g.add_node(make_file_node("file1", "src/main.rs"));
    assert_eq!(g.node_count(), 1);
}

#[test]
fn add_duplicate_node_id_overwrites() {
    let mut g = CodeGraph::new();
    g.add_node(make_file_node("file1", "src/main.rs"));
    g.add_node(make_file_node("file1", "src/lib.rs")); // same id, different name
    assert_eq!(g.node_count(), 1);
    let node = g.get_node("file1").expect("node must exist");
    assert_eq!(node.name, "src/lib.rs");
}

#[test]
fn get_node_returns_none_for_missing() {
    let g = CodeGraph::new();
    assert!(g.get_node("missing").is_none());
}

// --- Add edges ---

#[test]
fn add_edge_increments_count() {
    let mut g = CodeGraph::new();
    g.add_node(make_file_node("f1", "src/a.rs"));
    g.add_node(make_symbol_node("s1", "foo", SymbolKind::Function));
    g.add_edge(make_edge("f1", "s1", EdgeType::Contains));
    assert_eq!(g.edge_count(), 1);
}

#[test]
fn add_multiple_edges() {
    let mut g = CodeGraph::new();
    g.add_node(make_symbol_node("a", "foo", SymbolKind::Function));
    g.add_node(make_symbol_node("b", "bar", SymbolKind::Function));
    g.add_node(make_symbol_node("c", "baz", SymbolKind::Function));
    g.add_edge(make_edge("a", "b", EdgeType::Calls));
    g.add_edge(make_edge("b", "c", EdgeType::Calls));
    assert_eq!(g.edge_count(), 2);
}

// --- Adjacency ---

#[test]
fn neighbors_follows_directed_edges() {
    let mut g = CodeGraph::new();
    g.add_node(make_symbol_node("a", "foo", SymbolKind::Function));
    g.add_node(make_symbol_node("b", "bar", SymbolKind::Function));
    g.add_node(make_symbol_node("c", "baz", SymbolKind::Function));
    g.add_edge(make_edge("a", "b", EdgeType::Calls));
    g.add_edge(make_edge("a", "c", EdgeType::Calls));

    let mut nbrs = g.neighbors("a");
    nbrs.sort();
    assert_eq!(nbrs, vec!["b", "c"]);
    assert!(g.neighbors("b").is_empty());
}

#[test]
fn reverse_neighbors_are_inbound() {
    let mut g = CodeGraph::new();
    g.add_node(make_symbol_node("a", "foo", SymbolKind::Function));
    g.add_node(make_symbol_node("b", "bar", SymbolKind::Function));
    g.add_node(make_symbol_node("c", "baz", SymbolKind::Function));
    g.add_edge(make_edge("a", "b", EdgeType::Calls));
    g.add_edge(make_edge("c", "b", EdgeType::Calls));

    let mut rev = g.reverse_neighbors("b");
    rev.sort();
    assert_eq!(rev, vec!["a", "c"]);
    assert!(g.reverse_neighbors("a").is_empty());
}

// --- Filter helpers ---

#[test]
fn symbol_nodes_returns_only_symbols() {
    let mut g = CodeGraph::new();
    g.add_node(make_file_node("f1", "a.rs"));
    g.add_node(make_symbol_node("s1", "foo", SymbolKind::Function));
    g.add_node(make_symbol_node("s2", "Bar", SymbolKind::Struct));

    let syms: Vec<_> = g.symbol_nodes();
    assert_eq!(syms.len(), 2);
    for n in &syms {
        assert!(matches!(n.node_type, NodeType::Symbol));
    }
}

#[test]
fn file_nodes_returns_only_files() {
    let mut g = CodeGraph::new();
    g.add_node(make_file_node("f1", "a.rs"));
    g.add_node(make_file_node("f2", "b.rs"));
    g.add_node(make_symbol_node("s1", "foo", SymbolKind::Function));

    let files: Vec<_> = g.file_nodes();
    assert_eq!(files.len(), 2);
    for n in &files {
        assert!(matches!(n.node_type, NodeType::File));
    }
}

// --- edges_of_type ---

#[test]
fn edges_of_type_filters_correctly() {
    let mut g = CodeGraph::new();
    for id in ["a", "b", "c", "d"] {
        g.add_node(make_symbol_node(id, id, SymbolKind::Function));
    }
    g.add_edge(make_edge("a", "b", EdgeType::Calls));
    g.add_edge(make_edge("a", "c", EdgeType::Calls));
    g.add_edge(make_edge("b", "d", EdgeType::Imports));

    let calls = g.edges_of_type(&EdgeType::Calls);
    assert_eq!(calls.len(), 2);
    let imports = g.edges_of_type(&EdgeType::Imports);
    assert_eq!(imports.len(), 1);
    let contains = g.edges_of_type(&EdgeType::Contains);
    assert!(contains.is_empty());
}

// --- edges_between ---

#[test]
fn edges_between_returns_all_edges_for_pair() {
    let mut g = CodeGraph::new();
    g.add_node(make_symbol_node("a", "a", SymbolKind::Function));
    g.add_node(make_symbol_node("b", "b", SymbolKind::Function));
    g.add_edge(make_edge("a", "b", EdgeType::Calls));
    g.add_edge(make_edge("a", "b", EdgeType::References));

    let edges = g.edges_between("a", "b");
    assert_eq!(edges.len(), 2);
}

#[test]
fn edges_between_no_edges_returns_empty() {
    let mut g = CodeGraph::new();
    g.add_node(make_symbol_node("a", "a", SymbolKind::Function));
    g.add_node(make_symbol_node("b", "b", SymbolKind::Function));
    assert!(g.edges_between("a", "b").is_empty());
}

// --- Edge default weights ---

#[test]
fn edge_type_default_weights() {
    assert_eq!(EdgeType::Contains.default_weight(), 1.0);
    assert_eq!(EdgeType::Calls.default_weight(), 1.0);
    assert_eq!(EdgeType::Imports.default_weight(), 1.0);
    assert_eq!(EdgeType::Inherits.default_weight(), 1.0);
    assert_eq!(EdgeType::TypeDepends.default_weight(), 0.8);
    assert_eq!(EdgeType::Tests.default_weight(), 0.7);
    assert_eq!(EdgeType::References.default_weight(), 1.0);
    // CoChanges weight is dynamic, just check it has a method
    let w = EdgeType::CoChanges.default_weight();
    assert!(w >= 0.0);
}
