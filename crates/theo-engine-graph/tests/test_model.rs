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

// --- S3-T1: Symbol-level hashing tests ---

#[test]
fn symbol_content_hash_deterministic() {
    let node = Node {
        id: "sym:foo".into(),
        node_type: NodeType::Symbol,
        name: "foo".into(),
        file_path: None,
        signature: Some("pub fn foo(x: i32) -> i32".into()),
        kind: Some(SymbolKind::Function),
        line_start: Some(1),
        line_end: Some(5),
        last_modified: 0.0,
        doc: Some("Adds one".into()),
    };
    let h1 = CodeGraph::symbol_content_hash(&node);
    let h2 = CodeGraph::symbol_content_hash(&node);
    assert_eq!(h1, h2, "Same node must produce same hash");
}

#[test]
fn symbol_content_hash_changes_when_signature_changes() {
    let mut node = Node {
        id: "sym:foo".into(),
        node_type: NodeType::Symbol,
        name: "foo".into(),
        file_path: None,
        signature: Some("pub fn foo(x: i32) -> i32".into()),
        kind: Some(SymbolKind::Function),
        line_start: Some(1),
        line_end: Some(5),
        last_modified: 0.0,
        doc: None,
    };
    let h1 = CodeGraph::symbol_content_hash(&node);
    node.signature = Some("pub fn foo(x: i32, y: i32) -> i32".into());
    let h2 = CodeGraph::symbol_content_hash(&node);
    assert_ne!(h1, h2, "Hash must change when signature changes");
}

#[test]
fn symbol_content_hash_ignores_line_numbers() {
    let mut node = Node {
        id: "sym:foo".into(),
        node_type: NodeType::Symbol,
        name: "foo".into(),
        file_path: None,
        signature: Some("pub fn foo()".into()),
        kind: Some(SymbolKind::Function),
        line_start: Some(1),
        line_end: Some(5),
        last_modified: 0.0,
        doc: None,
    };
    let h1 = CodeGraph::symbol_content_hash(&node);
    node.line_start = Some(100);
    node.line_end = Some(200);
    let h2 = CodeGraph::symbol_content_hash(&node);
    assert_eq!(h1, h2, "Line number changes should not affect hash");
}

#[test]
fn compute_symbol_hashes_only_symbols() {
    let mut g = CodeGraph::new();
    g.add_node(make_file_node("file:a.rs", "a.rs"));
    g.add_node(make_symbol_node("sym:foo", "foo", SymbolKind::Function));
    g.add_node(make_symbol_node("sym:bar", "bar", SymbolKind::Struct));

    let hashes = g.compute_symbol_hashes();
    assert_eq!(hashes.len(), 2, "Should only hash Symbol nodes");
    assert!(hashes.contains_key("sym:foo"));
    assert!(hashes.contains_key("sym:bar"));
    assert!(!hashes.contains_key("file:a.rs"), "File nodes should not be hashed");
}

#[test]
fn community_content_hash_deterministic() {
    let mut g = CodeGraph::new();
    g.add_node(make_symbol_node("sym:a", "a", SymbolKind::Function));
    g.add_node(make_symbol_node("sym:b", "b", SymbolKind::Function));

    let ids = vec!["sym:a".into(), "sym:b".into()];
    let h1 = g.community_content_hash(&ids);
    let h2 = g.community_content_hash(&ids);
    assert_eq!(h1, h2);
}

#[test]
fn community_content_hash_order_independent() {
    let mut g = CodeGraph::new();
    g.add_node(make_symbol_node("sym:a", "a", SymbolKind::Function));
    g.add_node(make_symbol_node("sym:b", "b", SymbolKind::Function));

    let h1 = g.community_content_hash(&["sym:a".into(), "sym:b".into()]);
    let h2 = g.community_content_hash(&["sym:b".into(), "sym:a".into()]);
    assert_eq!(h1, h2, "Order of node_ids should not affect hash");
}

#[test]
fn community_content_hash_none_for_no_symbols() {
    let mut g = CodeGraph::new();
    g.add_node(make_file_node("file:a.rs", "a.rs"));

    let h = g.community_content_hash(&["file:a.rs".into()]);
    assert!(h.is_none(), "Community with no symbols should return None");
}
