//! Sibling test body of `assembly.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `assembly.rs` via `#[path = "assembly_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use super::*;
    use crate::summary::{CommunityStructuredData, CommunitySummary};
    use std::collections::HashMap;
    use std::io::Write;
    use theo_engine_graph::cluster::Community;
    use theo_engine_graph::model::{Edge, EdgeType, Node, SymbolKind};

    /// Helper: create a graph with one community containing two files, a summary,
    /// and write the source files to a temp directory.
    fn setup_code_test() -> (
        Vec<ScoredCommunity>,
        HashMap<String, CommunitySummary>,
        CodeGraph,
        tempfile::TempDir,
    ) {
        let tmp_dir = tempfile::tempdir().unwrap();

        // Create source files on disk
        let src_dir = tmp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        let mut f1 = std::fs::File::create(src_dir.join("auth.rs")).unwrap();
        writeln!(
            f1,
            "fn verify_token(token: &str) -> bool {{\n    token.len() > 0\n}}\n\nfn decode(t: &str) -> String {{\n    t.to_string()\n}}"
        )
        .unwrap();

        let mut f2 = std::fs::File::create(src_dir.join("handler.rs")).unwrap();
        writeln!(
            f2,
            "fn handle(req: Request) -> Response {{\n    todo!()\n}}"
        )
        .unwrap();

        // Build graph
        let mut graph = CodeGraph::new();

        graph.add_node(Node {
            id: "file:src/auth.rs".into(),
            node_type: NodeType::File,
            name: "src/auth.rs".into(),
            file_path: Some("src/auth.rs".into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "sym:verify_token".into(),
            node_type: NodeType::Symbol,
            name: "verify_token".into(),
            file_path: Some("src/auth.rs".into()),
            signature: Some("fn verify_token(token: &str) -> bool".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(3),
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "sym:decode".into(),
            node_type: NodeType::Symbol,
            name: "decode".into(),
            file_path: Some("src/auth.rs".into()),
            signature: Some("fn decode(t: &str) -> String".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(5),
            line_end: Some(7),
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "file:src/handler.rs".into(),
            node_type: NodeType::File,
            name: "src/handler.rs".into(),
            file_path: Some("src/handler.rs".into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "sym:handle".into(),
            node_type: NodeType::Symbol,
            name: "handle".into(),
            file_path: Some("src/handler.rs".into()),
            signature: Some("fn handle(req: Request) -> Response".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(3),
            last_modified: 1000.0,
            doc: None,
        });

        // Edges
        graph.add_edge(Edge {
            source: "file:src/auth.rs".into(),
            target: "sym:verify_token".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "file:src/auth.rs".into(),
            target: "sym:decode".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "file:src/handler.rs".into(),
            target: "sym:handle".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "sym:verify_token".into(),
            target: "sym:decode".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });

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

        let scored = vec![ScoredCommunity {
            community: community.clone(),
            score: 5.0,
        }];

        let mut summaries = HashMap::new();
        summaries.insert(
            "comm_auth".into(),
            CommunitySummary {
                community_id: "comm_auth".into(),
                name: "auth/jwt".into(),
                text: "## auth/jwt (3 funções, 10 linhas, src/auth.rs, src/handler.rs)\n\nFluxo: verify_token → decode".into(),
                token_count: 20,
                structured: CommunityStructuredData { top_functions: vec![], edge_types_present: vec![], cross_community_deps: vec![], file_count: 0, primary_language: String::new() },
            },
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

    #[test]
    fn test_assemble_with_code_large_file_truncation() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let src_dir = tmp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        // Create a file with 300 lines
        let mut f = std::fs::File::create(src_dir.join("big.rs")).unwrap();
        for i in 1..=300 {
            writeln!(f, "// line {}", i).unwrap();
        }

        let mut graph = CodeGraph::new();
        graph.add_node(Node {
            id: "file:src/big.rs".into(),
            node_type: NodeType::File,
            name: "src/big.rs".into(),
            file_path: Some("src/big.rs".into()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "sym:func_a".into(),
            node_type: NodeType::Symbol,
            name: "func_a".into(),
            file_path: Some("src/big.rs".into()),
            signature: Some("fn func_a()".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(10),
            line_end: Some(20),
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "sym:func_b".into(),
            node_type: NodeType::Symbol,
            name: "func_b".into(),
            file_path: Some("src/big.rs".into()),
            signature: Some("fn func_b()".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(250),
            line_end: Some(260),
            last_modified: 1000.0,
            doc: None,
        });
        graph.add_edge(Edge {
            source: "file:src/big.rs".into(),
            target: "sym:func_a".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "file:src/big.rs".into(),
            target: "sym:func_b".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

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

        let scored = vec![ScoredCommunity {
            community,
            score: 5.0,
        }];

        let mut summaries = HashMap::new();
        summaries.insert(
            "comm_big".into(),
            CommunitySummary {
                community_id: "comm_big".into(),
                name: "big/module".into(),
                text: "## big/module".into(),
                token_count: 5,
                structured: CommunityStructuredData {
                    top_functions: vec![],
                    edge_types_present: vec![],
                    cross_community_deps: vec![],
                    file_count: 0,
                    primary_language: String::new(),
                },
            },
        );

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

        // Should have omission markers since file is > 100 lines
        assert!(
            content.contains("lines omitted"),
            "should contain omission markers for large file, got: {}",
            content
        );

        // Should still contain the symbol ranges
        assert!(
            content.contains("// line 10"),
            "should contain lines from func_a range"
        );
        assert!(
            content.contains("// line 250"),
            "should contain lines from func_b range"
        );
    }
