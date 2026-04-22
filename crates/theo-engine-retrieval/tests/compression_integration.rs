//! Integration tests for the Phase 2 wiring of `code_compression` into
//! `file_retriever::build_context_blocks_with_compression`
//! (PLAN_CONTEXT_WIRING Task 2.3).
//!
//! Exercises the public `build_context_blocks_with_compression` entry
//! point from outside the crate, which is the hot path
//! `graph_context_service` uses. Validates:
//! 1. With no workspace_root → graceful fallback to signatures, zero savings.
//! 2. With a workspace_root but missing file → same fallback.
//! 3. With a real rust source file → ratio > 1 (some compression happens).
//! 4. Token budget is respected even after compression.

use std::collections::HashSet;
use std::io::Write;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};
use theo_engine_retrieval::file_retriever::{
    build_context_blocks_with_compression, build_context_blocks_with_compression_mut,
    retrieve_files, FileRetrievalResult, RankedFile, RerankConfig,
};

fn file_node(id: &str, path: &str) -> Node {
    Node {
        id: id.to_string(),
        node_type: NodeType::File,
        name: path.to_string(),
        file_path: Some(path.to_string()),
        signature: None,
        kind: None,
        line_start: Some(1),
        line_end: Some(500),
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

fn graph_with_one_file(path: &str) -> CodeGraph {
    let mut g = CodeGraph::new();
    let file_id = format!("file:{path}");
    let sym_id = format!("sym:relevant_symbol:{path}");
    g.add_node(file_node(&file_id, path));
    g.add_node(symbol_node(&sym_id, "relevant_symbol", path));
    g.add_edge(Edge {
        source: file_id,
        target: sym_id,
        edge_type: EdgeType::Contains,
        weight: 1.0,
    });
    g
}

fn forced_result(path: &str) -> FileRetrievalResult {
    FileRetrievalResult {
        primary_files: vec![RankedFile {
            path: path.to_string(),
            score: 1.0,
            signals: Vec::new(),
        }],
        ..FileRetrievalResult::default()
    }
}

#[test]
fn build_context_blocks_no_workspace_root_keeps_legacy_behavior() {
    // AAA: with None workspace_root, compressor is skipped entirely.
    let path = "demo.rs";
    let graph = graph_with_one_file(path);
    let result = forced_result(path);

    let (blocks, savings) =
        build_context_blocks_with_compression(&result, &graph, 10_000, None, "relevant_symbol");

    assert_eq!(blocks.len(), 1, "one block for the one file");
    assert_eq!(savings, 0, "no compression attempted without workspace_root");
}

#[test]
fn build_context_blocks_with_missing_file_falls_back_to_signatures() {
    // AAA: workspace_root points to an empty temp dir → file missing →
    // fallback to signatures, zero savings.
    let path = "phantom.rs";
    let graph = graph_with_one_file(path);
    let result = forced_result(path);
    let tmp = tempfile::tempdir().expect("tmpdir");

    let (blocks, savings) = build_context_blocks_with_compression(
        &result,
        &graph,
        10_000,
        Some(tmp.path()),
        "relevant_symbol",
    );

    assert_eq!(blocks.len(), 1);
    assert_eq!(savings, 0, "missing file must degrade to zero savings");
}

#[test]
fn build_context_blocks_compresses_real_rust_source() {
    // AAA: write a multi-function Rust file. Mark only one function as
    // relevant (by the query). Compression keeps that body, signature-
    // only for the rest → savings > 0.
    let tmp = tempfile::tempdir().expect("tmpdir");
    let file_name = "calc.rs";
    let full_path = tmp.path().join(file_name);
    let mut f = std::fs::File::create(&full_path).expect("create");
    writeln!(f, "fn relevant_symbol(x: i32) -> i32 {{").unwrap();
    writeln!(f, "    let mut acc = 0;").unwrap();
    writeln!(f, "    for i in 0..x {{ acc += i; }}").unwrap();
    writeln!(f, "    acc").unwrap();
    writeln!(f, "}}").unwrap();
    for i in 0..5 {
        writeln!(f, "\nfn noise_{i}(a: i32, b: i32) -> i32 {{").unwrap();
        writeln!(f, "    let mut r = a + b;").unwrap();
        writeln!(f, "    r *= {i};").unwrap();
        writeln!(f, "    r -= {i};").unwrap();
        writeln!(f, "    r").unwrap();
        writeln!(f, "}}").unwrap();
    }
    drop(f);

    let graph = graph_with_one_file(file_name);
    let result = forced_result(file_name);

    let (blocks, savings) = build_context_blocks_with_compression(
        &result,
        &graph,
        10_000,
        Some(tmp.path()),
        "relevant_symbol",
    );

    assert_eq!(blocks.len(), 1);
    assert!(
        blocks[0].content.contains("relevant_symbol"),
        "relevant symbol body must survive: {}",
        &blocks[0].content
    );
    assert!(
        savings > 0,
        "compression must report savings on a multi-function file, got {savings}"
    );
}

#[test]
fn build_context_blocks_respects_budget_after_compression() {
    // AAA: very tight token budget → compression runs, block still omitted
    // when budget cannot accommodate it (no panic, no over-budget block).
    let tmp = tempfile::tempdir().expect("tmpdir");
    let file_name = "big.rs";
    let full_path = tmp.path().join(file_name);
    let mut f = std::fs::File::create(&full_path).expect("create");
    for i in 0..50 {
        writeln!(f, "fn fn_{i}() {{ /* body {i} */ }}").unwrap();
    }
    drop(f);

    let graph = graph_with_one_file(file_name);
    let result = forced_result(file_name);

    let (blocks, _savings) = build_context_blocks_with_compression(
        &result, &graph, 2, // absurdly small budget
        Some(tmp.path()),
        "relevant_symbol",
    );

    // With a 2-token budget nothing fits → zero blocks, but no panic.
    assert!(blocks.is_empty());
}

#[test]
fn build_context_blocks_mut_writes_savings_to_struct_field() {
    // AAA: with the _mut variant, the savings counter ends up on
    // result.compression_savings_tokens (PLAN_CONTEXT_WIRING Task 2.4),
    // so consumers don't need to juggle the tuple return.
    let tmp = tempfile::tempdir().expect("tmpdir");
    let file_name = "more.rs";
    let full_path = tmp.path().join(file_name);
    let mut f = std::fs::File::create(&full_path).expect("create");
    writeln!(f, "fn relevant_symbol() {{ let _x = 1; }}").unwrap();
    for i in 0..6 {
        writeln!(f, "fn extra_{i}() {{ let _y = {i}; }}").unwrap();
    }
    drop(f);

    let graph = graph_with_one_file(file_name);
    let mut result = forced_result(file_name);
    assert_eq!(result.compression_savings_tokens, 0, "starts at zero");

    let _ = build_context_blocks_with_compression_mut(
        &mut result,
        &graph,
        10_000,
        Some(tmp.path()),
        "relevant_symbol",
    );

    assert!(
        result.compression_savings_tokens > 0,
        "_mut variant must populate compression_savings_tokens, got {}",
        result.compression_savings_tokens
    );
}

#[test]
fn build_context_blocks_end_to_end_through_retrieve_files() {
    // AAA: exercises retrieve_files → build_context_blocks_with_compression
    // end-to-end using the same public entry points graph_context_service
    // uses.
    let graph = graph_with_one_file("auth.rs");
    let communities: Vec<Community> = vec![];
    let config = RerankConfig::default();

    let result = retrieve_files(
        &graph,
        &communities,
        "relevant_symbol",
        &config,
        &HashSet::new(),
    );
    // Without workspace_root → savings stay 0, block is emitted with
    // signature content.
    let (blocks, savings) =
        build_context_blocks_with_compression(&result, &graph, 10_000, None, "relevant_symbol");

    assert!(!blocks.is_empty(), "pipeline must produce at least one block");
    assert_eq!(savings, 0);
}
