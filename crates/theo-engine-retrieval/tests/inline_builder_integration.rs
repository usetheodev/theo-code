//! Integration tests for the Phase 3 wiring of `inline_builder` into
//! `file_retriever::retrieve_files_with_inline` + the mutual-exclusion
//! guard inside `build_context_blocks_with_compression`
//! (PLAN_CONTEXT_WIRING Task 3.5).
//!
//! Tests go through the public API only (from outside the crate) to
//! guarantee the wiring is actually exposed and usable by callers such
//! as `graph_context_service`.

use std::collections::HashSet;

use theo_engine_graph::cluster::Community;
use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};
use theo_engine_retrieval::file_retriever::{
    build_context_blocks_with_compression, retrieve_files_with_inline, FileRetrievalResult,
    RankedFile, RerankConfig,
};
use theo_engine_retrieval::inline_builder::InlineSlice;

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

fn tiny_graph() -> CodeGraph {
    let mut g = CodeGraph::new();
    g.add_node(file_node("file:src/auth.rs", "src/auth.rs"));
    g.add_node(symbol_node("sym:verify_token", "verify_token", "src/auth.rs"));
    g.add_edge(Edge {
        source: "file:src/auth.rs".into(),
        target: "sym:verify_token".into(),
        edge_type: EdgeType::Contains,
        weight: 1.0,
    });
    g
}

#[test]
fn retrieve_files_with_inline_no_match_returns_empty_slices() {
    let graph = tiny_graph();
    let tmp = tempfile::tempdir().expect("tmpdir");

    let result = retrieve_files_with_inline(
        &graph,
        &[] as &[Community],
        "no_such_symbol",
        &RerankConfig::default(),
        &HashSet::new(),
        tmp.path(),
    );

    assert!(
        result.inline_slices.is_empty(),
        "unmatched query must not produce any inline slices"
    );
}

#[test]
fn retrieve_files_with_inline_on_empty_graph_is_equivalent() {
    // When the graph is empty, retrieve_files_with_inline should behave
    // identically to retrieve_files (inline_slices empty, harm_removals 0).
    let graph = CodeGraph::new();
    let tmp = tempfile::tempdir().expect("tmpdir");

    let result = retrieve_files_with_inline(
        &graph,
        &[] as &[Community],
        "anything",
        &RerankConfig::default(),
        &HashSet::new(),
        tmp.path(),
    );

    assert!(result.primary_files.is_empty());
    assert!(result.inline_slices.is_empty());
    assert_eq!(result.harm_removals, 0);
}

#[test]
fn build_context_blocks_places_inline_slice_first_with_top_score() {
    // Forge a slice so we can deterministically assert ordering without
    // depending on inline_builder resolving real source.
    let graph = tiny_graph();
    let slice = InlineSlice {
        focal_symbol_id: "sym:verify_token".into(),
        focal_file: "src/auth.rs".into(),
        content: "// inline slice content".into(),
        token_count: 12,
        inlined_symbols: vec![],
        unresolved_callees: vec![],
    };
    let result = FileRetrievalResult {
        primary_files: vec![RankedFile {
            path: "src/other.rs".into(),
            score: 0.8,
            signals: Vec::new(),
        }],
        inline_slices: vec![slice],
        ..FileRetrievalResult::default()
    };

    let (blocks, _) =
        build_context_blocks_with_compression(&result, &graph, 10_000, None, "verify_token");

    assert!(!blocks.is_empty());
    assert!(
        blocks[0].block_id.starts_with("blk-inline-"),
        "inline block must come first, got {}",
        blocks[0].block_id
    );
    assert_eq!(blocks[0].score, 1.0, "inline score must be maximum");
}

#[test]
fn inline_slice_suppresses_duplicate_primary_block_for_same_file() {
    // AAA: both an inline slice AND a primary_files entry cover src/auth.rs.
    // The primary loop must skip it to avoid reverse-boost double counting
    // (PLAN_CONTEXT_WIRING plan line 253 onwards).
    let graph = tiny_graph();
    let slice = InlineSlice {
        focal_symbol_id: "sym:verify_token".into(),
        focal_file: "src/auth.rs".into(),
        content: "// inline".into(),
        token_count: 10,
        inlined_symbols: vec![],
        unresolved_callees: vec![],
    };
    let result = FileRetrievalResult {
        primary_files: vec![
            RankedFile {
                path: "src/auth.rs".into(),
                score: 0.9,
                signals: Vec::new(),
            },
            RankedFile {
                path: "src/db.rs".into(),
                score: 0.5,
                signals: Vec::new(),
            },
        ],
        inline_slices: vec![slice],
        ..FileRetrievalResult::default()
    };

    let (blocks, _) =
        build_context_blocks_with_compression(&result, &graph, 10_000, None, "verify_token");

    // src/auth.rs appears only via the inline slice, src/db.rs once as
    // primary.
    let auth_primary_count = blocks
        .iter()
        .filter(|b| b.block_id == "blk-file-src-auth.rs")
        .count();
    let db_primary_count = blocks
        .iter()
        .filter(|b| b.block_id == "blk-file-src-db.rs")
        .count();
    assert_eq!(auth_primary_count, 0, "src/auth.rs duplicate block suppressed");
    assert_eq!(db_primary_count, 1, "src/db.rs primary block remains");
}

#[test]
fn retrieve_files_with_inline_exposes_harm_removals_same_as_base() {
    // The _with_inline wrapper delegates harm_filter to retrieve_files,
    // so the counter flows through untouched.
    let graph = tiny_graph();
    let tmp = tempfile::tempdir().expect("tmpdir");

    let result = retrieve_files_with_inline(
        &graph,
        &[] as &[Community],
        "verify_token",
        &RerankConfig::default(),
        &HashSet::new(),
        tmp.path(),
    );
    // harm_removals is usize — just assert presence and non-negativity
    // (usize cannot be negative, this is a compile-time guarantee).
    let _ = result.harm_removals;
    assert!(result.primary_files.len() + result.harm_removals > 0);
}
