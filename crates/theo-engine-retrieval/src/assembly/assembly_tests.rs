//! Sibling test body of `assembly.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `assembly.rs` via `#[path = "assembly_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.


#![cfg(test)]

    #![allow(unused_imports)]
    use super::*;
    use crate::assembly::*;
    use crate::search::ScoredCommunity;
    use theo_engine_graph::model::{CodeGraph, NodeType};
    use crate::summary::{CommunityStructuredData, CommunitySummary};
    use std::collections::HashMap;
    use std::io::Write;
    use theo_engine_graph::cluster::Community;
    use theo_engine_graph::model::{Edge, EdgeType, Node, SymbolKind};

    fn at_file_node(id: &str, path: &str) -> Node {
        Node {
            id: id.into(),
            node_type: NodeType::File,
            name: path.into(),
            file_path: Some(path.into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 1000.0,
            doc: None,
        }
    }

    fn at_symbol_node(id: &str, name: &str, file: &str, sig: &str, span: (usize, usize)) -> Node {
        Node {
            id: id.into(),
            node_type: NodeType::Symbol,
            name: name.into(),
            file_path: Some(file.into()),
            signature: Some(sig.into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(span.0),
            line_end: Some(span.1),
            last_modified: 1000.0,
            doc: None,
        }
    }

    fn at_edge(source: &str, target: &str, et: EdgeType) -> Edge {
        Edge { source: source.into(), target: target.into(), edge_type: et, weight: 1.0 }
    }

    fn at_empty_summary(id: &str, name: &str, text: &str, tokens: usize) -> CommunitySummary {
        CommunitySummary {
            community_id: id.into(),
            name: name.into(),
            text: text.into(),
            token_count: tokens,
            structured: CommunityStructuredData {
                top_functions: vec![],
                edge_types_present: vec![],
                cross_community_deps: vec![],
                file_count: 0,
                primary_language: String::new(),
            },
        }
    }

    /// Write the two source files used by the small auth/handler fixture.
    fn write_auth_handler_sources(src_dir: &std::path::Path) {
        let mut f1 = std::fs::File::create(src_dir.join("auth.rs")).unwrap();
        writeln!(
            f1,
            "fn verify_token(token: &str) -> bool {{\n    token.len() > 0\n}}\n\nfn decode(t: &str) -> String {{\n    t.to_string()\n}}"
        )
        .unwrap();
        let mut f2 = std::fs::File::create(src_dir.join("handler.rs")).unwrap();
        writeln!(f2, "fn handle(req: Request) -> Response {{\n    todo!()\n}}").unwrap();
    }

    fn build_auth_handler_graph() -> CodeGraph {
        let mut graph = CodeGraph::new();
        graph.add_node(at_file_node("file:src/auth.rs", "src/auth.rs"));
        graph.add_node(at_symbol_node(
            "sym:verify_token",
            "verify_token",
            "src/auth.rs",
            "fn verify_token(token: &str) -> bool",
            (1, 3),
        ));
        graph.add_node(at_symbol_node(
            "sym:decode",
            "decode",
            "src/auth.rs",
            "fn decode(t: &str) -> String",
            (5, 7),
        ));
        graph.add_node(at_file_node("file:src/handler.rs", "src/handler.rs"));
        graph.add_node(at_symbol_node(
            "sym:handle",
            "handle",
            "src/handler.rs",
            "fn handle(req: Request) -> Response",
            (1, 3),
        ));
        graph.add_edge(at_edge("file:src/auth.rs", "sym:verify_token", EdgeType::Contains));
        graph.add_edge(at_edge("file:src/auth.rs", "sym:decode", EdgeType::Contains));
        graph.add_edge(at_edge("file:src/handler.rs", "sym:handle", EdgeType::Contains));
        graph.add_edge(at_edge("sym:verify_token", "sym:decode", EdgeType::Calls));
        graph
    }

    /// Helper: create a graph with one community containing two files, a summary,
    /// and write the source files to a temp directory.
    fn setup_code_test() -> (
        Vec<ScoredCommunity>,
        HashMap<String, CommunitySummary>,
        CodeGraph,
        tempfile::TempDir,
    ) {
        let tmp_dir = tempfile::tempdir().unwrap();
        let src_dir = tmp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        write_auth_handler_sources(&src_dir);
        let graph = build_auth_handler_graph();
        let community = Community {
            id: "comm_auth".into(),
            name: "auth/jwt".into(),
            level: 0,
            node_ids: vec![
                "file:src/auth.rs".into(),
                "sym:verify_token".into(),
                "sym:decode".into(),
                "file:src/handler.rs".into(),
                "sym:handle".into(),
            ],
            parent_id: None,
            version: 1,
        };
        let scored = vec![ScoredCommunity { community: community.clone(), score: 5.0 }];
        let mut summaries = HashMap::new();
        summaries.insert(
            "comm_auth".into(),
            at_empty_summary(
                "comm_auth",
                "auth/jwt",
                "## auth/jwt (3 funções, 10 linhas, src/auth.rs, src/handler.rs)\n\nFluxo: verify_token → decode",
                20,
            ),
        );
        (scored, summaries, graph, tmp_dir)
    }

    #[test]
    fn test_assemble_with_code_includes_source() {
        let (scored, summaries, graph, tmp_dir) = setup_code_test();

        let payload = assemble_with_code(
            &scored,
            &summaries,
            &graph,
            tmp_dir.path(),
            50_000,
            "test query",
        );

        assert!(
            !payload.items.is_empty(),
            "should produce at least one item"
        );

        let content = &payload.items[0].content;

        // Should contain actual source code, not just summaries
        assert!(
            content.contains("fn verify_token(token: &str) -> bool"),
            "should contain actual source code from auth.rs, got: {}",
            content
        );
        assert!(
            content.contains("fn handle(req: Request) -> Response"),
            "should contain actual source code from handler.rs, got: {}",
            content
        );

        // Should contain fenced code blocks
        assert!(
            content.contains("```rust"),
            "should have rust fenced code blocks, got: {}",
            content
        );

        // Should contain file path headers
        assert!(
            content.contains("### src/auth.rs"),
            "should have file path header, got: {}",
            content
        );

        // Should contain the community header
        assert!(
            content.contains("## auth/jwt -- 2 files"),
            "should have community header, got: {}",
            content
        );
    }

    #[test]
    fn test_assemble_with_code_respects_budget() {
        let (scored, summaries, graph, tmp_dir) = setup_code_test();

        // Use a very small budget — should cap the total tokens
        let tiny_budget = 10;
        let payload = assemble_with_code(
            &scored,
            &summaries,
            &graph,
            tmp_dir.path(),
            tiny_budget,
            "test query",
        );

        assert!(
            payload.total_tokens <= tiny_budget,
            "total_tokens ({}) should be <= budget ({})",
            payload.total_tokens,
            tiny_budget
        );
        assert_eq!(payload.budget_tokens, tiny_budget);
    }

    fn write_big_source(src_dir: &std::path::Path) {
        let mut f = std::fs::File::create(src_dir.join("big.rs")).unwrap();
        for i in 1..=300 {
            writeln!(f, "// line {}", i).unwrap();
        }
    }

    fn build_big_module_graph() -> CodeGraph {
        let mut graph = CodeGraph::new();
        graph.add_node(at_file_node("file:src/big.rs", "src/big.rs"));
        graph.add_node(at_symbol_node(
            "sym:func_a",
            "func_a",
            "src/big.rs",
            "fn func_a()",
            (10, 20),
        ));
        graph.add_node(at_symbol_node(
            "sym:func_b",
            "func_b",
            "src/big.rs",
            "fn func_b()",
            (250, 260),
        ));
        graph.add_edge(at_edge("file:src/big.rs", "sym:func_a", EdgeType::Contains));
        graph.add_edge(at_edge("file:src/big.rs", "sym:func_b", EdgeType::Contains));
        graph
    }

    fn setup_big_module_test() -> (
        Vec<ScoredCommunity>,
        HashMap<String, CommunitySummary>,
        CodeGraph,
        tempfile::TempDir,
    ) {
        let tmp_dir = tempfile::tempdir().unwrap();
        let src_dir = tmp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        write_big_source(&src_dir);
        let graph = build_big_module_graph();
        let community = Community {
            id: "comm_big".into(),
            name: "big/module".into(),
            level: 0,
            node_ids: vec![
                "file:src/big.rs".into(),
                "sym:func_a".into(),
                "sym:func_b".into(),
            ],
            parent_id: None,
            version: 1,
        };
        let scored = vec![ScoredCommunity { community, score: 5.0 }];
        let mut summaries = HashMap::new();
        summaries.insert(
            "comm_big".into(),
            at_empty_summary("comm_big", "big/module", "## big/module", 5),
        );
        (scored, summaries, graph, tmp_dir)
    }

    #[test]
    fn test_assemble_with_code_large_file_truncation() {
        let (scored, summaries, graph, tmp_dir) = setup_big_module_test();
        let payload = assemble_with_code(
            &scored,
            &summaries,
            &graph,
            tmp_dir.path(),
            50_000,
            "test query",
        );
        assert!(!payload.items.is_empty());
        let content = &payload.items[0].content;
        assert!(
            content.contains("lines omitted"),
            "should contain omission markers for large file, got: {}",
            content
        );
        assert!(
            content.contains("// line 10"),
            "should contain lines from func_a range"
        );
        assert!(
            content.contains("// line 250"),
            "should contain lines from func_b range"
        );
    }
