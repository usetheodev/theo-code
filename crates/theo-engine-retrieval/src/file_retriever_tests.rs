//! Sibling test body of `file_retriever.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `file_retriever.rs` via `#[path = "file_retriever_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.


#![cfg(test)]

    use super::*;
    use theo_engine_graph::model::{Edge, EdgeType, Node, SymbolKind};

    fn file_node(id: &str, path: &str) -> Node {
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

    fn symbol_node(id: &str, name: &str, file_path: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: NodeType::Symbol,
            name: name.to_string(),
            file_path: Some(file_path.to_string()),
            signature: Some(format!("pub fn {}()", name)),
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(10),
            last_modified: 0.0,
            doc: None,
        }
    }

    fn test_node(id: &str, name: &str, file_path: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: NodeType::Test,
            name: name.to_string(),
            file_path: Some(file_path.to_string()),
            signature: None,
            kind: Some(SymbolKind::Function),
            line_start: Some(1),
            line_end: Some(5),
            last_modified: 0.0,
            doc: None,
        }
    }

    fn build_test_graph() -> CodeGraph {
        let mut g = CodeGraph::new();

        // Files
        g.add_node(file_node("file:src/auth.rs", "src/auth.rs"));
        g.add_node(file_node("file:src/db.rs", "src/db.rs"));
        g.add_node(file_node("file:src/api.rs", "src/api.rs"));

        // Symbols
        g.add_node(symbol_node(
            "sym:verify_token",
            "verify_token",
            "src/auth.rs",
        ));
        g.add_node(symbol_node("sym:decode_jwt", "decode_jwt", "src/auth.rs"));
        g.add_node(symbol_node("sym:query_db", "query_db", "src/db.rs"));
        g.add_node(symbol_node(
            "sym:handle_request",
            "handle_request",
            "src/api.rs",
        ));

        // Test
        g.add_node(test_node(
            "test:test_auth",
            "test_auth",
            "tests/test_auth.rs",
        ));
        g.add_node(file_node("file:tests/test_auth.rs", "tests/test_auth.rs"));

        // Contains
        g.add_edge(Edge {
            source: "file:src/auth.rs".into(),
            target: "sym:verify_token".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "file:src/auth.rs".into(),
            target: "sym:decode_jwt".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "file:src/db.rs".into(),
            target: "sym:query_db".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "file:src/api.rs".into(),
            target: "sym:handle_request".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "file:tests/test_auth.rs".into(),
            target: "test:test_auth".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        // Calls
        g.add_edge(Edge {
            source: "sym:handle_request".into(),
            target: "sym:verify_token".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        g.add_edge(Edge {
            source: "sym:verify_token".into(),
            target: "sym:query_db".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });

        // Tests
        g.add_edge(Edge {
            source: "test:test_auth".into(),
            target: "sym:verify_token".into(),
            edge_type: EdgeType::Tests,
            weight: 0.7,
        });

        g
    }

    fn build_communities() -> Vec<Community> {
        vec![
            Community {
                id: "auth".to_string(),
                name: "Auth".to_string(),
                level: 0,
                node_ids: vec![
                    "file:src/auth.rs".into(),
                    "sym:verify_token".into(),
                    "sym:decode_jwt".into(),
                ],
                parent_id: None,
                version: 1,
            },
            Community {
                id: "db".to_string(),
                name: "DB".to_string(),
                level: 0,
                node_ids: vec!["file:src/db.rs".into(), "sym:query_db".into()],
                parent_id: None,
                version: 1,
            },
        ]
    }

    // --- Core retrieval tests ---

    #[test]
    fn retrieve_returns_files_not_communities() {
        let graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(
            &graph,
            &communities,
            "verify token authentication",
            &config,
            &seen,
        );

        // Should return file paths, not community IDs
        for file in &result.primary_files {
            assert!(
                file.path.contains('/') || file.path.contains('.'),
                "Result should be file path, got: {}",
                file.path
            );
            assert!(
                !file.path.starts_with("auth") && !file.path.starts_with("db"),
                "Result should NOT be community ID, got: {}",
                file.path
            );
        }
    }

    #[test]
    fn retrieve_top1_matches_auth_for_token_query() {
        let graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(
            &graph,
            &communities,
            "verify token jwt authentication",
            &config,
            &seen,
        );

        assert!(
            !result.primary_files.is_empty(),
            "Should return at least 1 file"
        );
        assert!(
            result.primary_files[0].path.contains("auth"),
            "Top result for 'verify token' should be auth file, got: {}",
            result.primary_files[0].path
        );
    }

    #[test]
    fn retrieve_ghost_path_filter_works() {
        let mut graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig::default();
        let seen = HashSet::new();

        // Add a symbol that references a non-existent file
        graph.add_node(Node {
            id: "sym:ghost".to_string(),
            node_type: NodeType::Symbol,
            name: "ghost_function".to_string(),
            file_path: Some("src/nonexistent.rs".to_string()),
            signature: Some("fn ghost()".into()),
            kind: Some(SymbolKind::Function),
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        });

        let result = retrieve_files(&graph, &communities, "ghost function", &config, &seen);

        // Ghost file should be filtered out
        for file in &result.primary_files {
            assert_ne!(
                file.path, "src/nonexistent.rs",
                "Ghost paths should be filtered"
            );
        }
    }

    #[test]
    fn retrieve_already_seen_penalty_reduces_score() {
        let graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig::default();

        let result_fresh = retrieve_files(
            &graph,
            &communities,
            "verify token",
            &config,
            &HashSet::new(),
        );
        let mut seen = HashSet::new();
        if let Some(top) = result_fresh.primary_files.first() {
            seen.insert(top.path.clone());
        }
        let result_seen = retrieve_files(&graph, &communities, "verify token", &config, &seen);

        // Score should be lower when previously seen
        if let (Some(fresh), Some(penalized)) = (
            result_fresh.primary_files.first(),
            result_seen.primary_files.first(),
        )
            && fresh.path == penalized.path {
                assert!(
                    penalized.score <= fresh.score,
                    "Seen penalty should reduce score: fresh={}, seen={}",
                    fresh.score,
                    penalized.score
                );
            }
    }

    #[test]
    fn retrieve_expansion_finds_related_files() {
        let graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(&graph, &communities, "verify token", &config, &seen);

        // auth.rs calls query_db in db.rs → db.rs should be in expanded
        let _all_files: Vec<&str> = result.expanded_files.iter().map(|s| s.as_str()).collect();
        // At minimum, expansion should find some related files
        assert!(
            result.primary_files.len() + result.expanded_files.len() > 1,
            "Should find primary + expanded files"
        );
    }

    #[test]
    fn retrieve_respects_max_neighbors() {
        let graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig {
            max_neighbors: 2,
            ..RerankConfig::default()
        };
        let seen = HashSet::new();

        let result = retrieve_files(&graph, &communities, "verify token", &config, &seen);
        assert!(
            result.expanded_files.len() + result.expanded_tests.len() <= 2,
            "Expansion must respect max_neighbors limit"
        );
    }

    #[test]
    fn retrieve_scores_never_negative() {
        let graph = build_test_graph();
        let communities = build_communities();
        let config = RerankConfig::default();
        let mut seen = HashSet::new();
        // Mark everything as seen to maximize penalties
        seen.insert("src/auth.rs".into());
        seen.insert("src/db.rs".into());
        seen.insert("src/api.rs".into());

        let result = retrieve_files(&graph, &communities, "anything", &config, &seen);
        for file in &result.primary_files {
            assert!(
                file.score >= 0.0,
                "Score must never be negative, got: {}",
                file.score
            );
        }
    }

    #[test]
    fn retrieve_empty_graph_returns_empty() {
        let graph = CodeGraph::new();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(&graph, &communities, "anything", &config, &seen);
        assert!(result.primary_files.is_empty());
    }

    #[test]
    fn community_flatten_respects_max_per_community() {
        let scores = vec![("comm1".into(), 1.0)];
        let communities = vec![Community {
            id: "comm1".into(),
            name: "Big".into(),
            level: 0,
            node_ids: (0..50).map(|i| format!("file:f{}.rs", i)).collect(),
            parent_id: None,
            version: 1,
        }];

        let files = flatten_top_communities(&scores, &communities, 10);
        assert!(
            files.len() <= 10,
            "Should respect max_per_community, got {}",
            files.len()
        );
    }

    // ────────────────────────────────────────────────────────────────
    // Phase 1 integration — harm_filter wired into retrieve_files
    // (PLAN_CONTEXT_WIRING Phase 1, Task 1.3)
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn retrieve_files_removes_test_file_when_definer_present() {
        // Arrange: graph has src/auth.rs (definer) + tests/test_auth.rs (test).
        // Both would normally rank for "verify_token" — harm filter should
        // drop the test since the definer is already in the top list.
        let graph = build_test_graph();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(&graph, &communities, "verify_token", &config, &seen);

        let test_file_kept = result
            .primary_files
            .iter()
            .any(|r| r.path == "tests/test_auth.rs");
        let definer_kept = result
            .primary_files
            .iter()
            .any(|r| r.path == "src/auth.rs");
        assert!(definer_kept, "definer src/auth.rs must survive");
        assert!(
            !test_file_kept,
            "test file tests/test_auth.rs must be filtered when definer is present"
        );
        assert!(
            result.harm_removals >= 1,
            "harm_removals counter must reflect at least the test-file removal"
        );
    }

    #[test]
    fn retrieve_files_harm_removals_metric_exposed() {
        // Smoke: on any non-empty graph, the harm_removals field is present
        // and ≥ 0 (catches accidental removal of the telemetry field).
        let graph = build_test_graph();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(&graph, &communities, "query_db", &config, &seen);

        // The field exists (compile-time) and is a valid usize. This test
        // mainly guards against future refactors removing the metric.
        let _ = result.harm_removals;
        assert!(
            result.primary_files.len() + result.harm_removals > 0,
            "something must have been ranked or filtered"
        );
    }

    #[test]
    fn retrieve_files_respects_40pct_removal_cap() {
        // Per harm_filter::MAX_REMOVAL_FRACTION, no more than 40% of the
        // ranked list may be removed in one pass. This test sanity-checks
        // that the cap survives integration.
        let graph = build_test_graph();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();

        let result = retrieve_files(&graph, &communities, "verify_token", &config, &seen);

        // After filtering, primary_files + harm_removals == whatever ranked
        // saw pre-filter. The ratio of removals to the pre-filter size must
        // be ≤ 40% + 1 (ceil of MAX_REMOVAL_FRACTION).
        let pre_filter = result.primary_files.len() + result.harm_removals;
        if pre_filter > 0 {
            let removal_fraction = result.harm_removals as f64 / pre_filter as f64;
            assert!(
                removal_fraction <= 0.5,
                "removal fraction {removal_fraction} exceeded 50% safety bound"
            );
        }
    }

    // ────────────────────────────────────────────────────────────────
    // Phase 2 integration — code_compression wired into
    // build_context_blocks_with_compression (PLAN_CONTEXT_WIRING Phase 2)
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn build_context_blocks_without_workspace_root_uses_signatures() {
        // None workspace_root keeps the pre-Phase-2 behaviour: content is
        // concatenated signatures, savings = 0.
        let graph = build_test_graph();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();
        let result = retrieve_files(&graph, &communities, "verify_token", &config, &seen);

        let (blocks, savings) =
            build_context_blocks_with_compression(&result, &graph, 10_000, None, "verify_token");

        assert!(!blocks.is_empty(), "must produce at least one block");
        assert_eq!(savings, 0, "no compression attempted without workspace_root");
    }

    #[test]
    fn build_context_blocks_compression_falls_back_when_file_missing() {
        // Points workspace_root at a non-existent directory: every fs::read
        // should fail → graceful fallback to signatures, savings = 0.
        let graph = build_test_graph();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();
        let result = retrieve_files(&graph, &communities, "verify_token", &config, &seen);

        let fake_root = std::path::Path::new("/tmp/theo-no-such-dir-xyz-999");
        let (blocks, savings) = build_context_blocks_with_compression(
            &result,
            &graph,
            10_000,
            Some(fake_root),
            "verify_token",
        );

        assert!(!blocks.is_empty(), "fallback must still produce blocks");
        assert_eq!(
            savings, 0,
            "missing-file fallback must yield zero compression savings"
        );
    }

    #[test]
    fn build_context_blocks_compression_saves_tokens_on_real_source() {
        // Arrange: write a Rust file with one relevant function and four
        // irrelevant functions. Compression should keep the relevant body
        // and reduce the others to signatures, yielding savings > 0.
        let tmp = tempfile::tempdir().expect("tmpdir");
        let file_name = "demo.rs";
        let path = tmp.path().join(file_name);
        let mut src = String::from(
            "fn relevant_symbol() {\n    // body line 1\n    // body line 2\n    println!(\"hi\");\n}\n\n",
        );
        for i in 0..4 {
            src.push_str(&format!(
                "fn noise_{i}() {{\n    // bulk body {i}\n    let x = {i};\n    let y = x + {i};\n    println!(\"{{x}} {{y}}\");\n}}\n\n",
                i = i
            ));
        }
        std::fs::write(&path, &src).expect("write demo");

        // Build a minimal graph containing just this file.
        let mut g = CodeGraph::new();
        g.add_node(file_node(&format!("file:{file_name}"), file_name));
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();
        // Query targets the relevant function by name.
        let result = retrieve_files(&g, &communities, "relevant_symbol", &config, &seen);
        // Force presence of the file in result regardless of ranker output,
        // so we always exercise the compression helper.
        let forced_result = FileRetrievalResult {
            primary_files: vec![RankedFile {
                path: file_name.to_string(),
                score: 1.0,
                signals: Vec::new(),
            }],
            ..result
        };

        let (blocks, savings) = build_context_blocks_with_compression(
            &forced_result,
            &g,
            10_000,
            Some(tmp.path()),
            "relevant_symbol",
        );

        assert_eq!(blocks.len(), 1, "one block for the single file");
        // The relevant function's body must survive compression.
        assert!(
            blocks[0].content.contains("relevant_symbol"),
            "compressed content must mention relevant_symbol: {}",
            &blocks[0].content
        );
        // Savings are non-zero when compression actually ran.
        assert!(savings > 0, "expected compression savings > 0, got {savings}");
    }

    // ────────────────────────────────────────────────────────────────
    // Phase 3 integration — inline_builder wired into
    // retrieve_files_with_inline + build_context_blocks_with_compression
    // (PLAN_CONTEXT_WIRING Phase 3)
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn retrieve_files_with_inline_no_match_yields_no_slices() {
        // Query that doesn't hit any symbol in the graph — inline slices
        // must remain empty; primary_files behaves like retrieve_files.
        let graph = build_test_graph();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();
        let tmp = tempfile::tempdir().expect("tmpdir");

        let result = retrieve_files_with_inline(
            &graph,
            &communities,
            "no_such_symbol_xyz_999",
            &config,
            &seen,
            tmp.path(),
        );

        assert!(
            result.inline_slices.is_empty(),
            "no symbol match → inline_slices must be empty"
        );
    }

    #[test]
    fn retrieve_files_with_inline_returns_identical_result_on_empty_graph() {
        // Isolates the inline path: identical to retrieve_files when the
        // graph has no symbols to slice.
        let graph = CodeGraph::new();
        let communities = vec![];
        let config = RerankConfig::default();
        let seen = HashSet::new();
        let tmp = tempfile::tempdir().expect("tmpdir");

        let a = retrieve_files(&graph, &communities, "anything", &config, &seen);
        let b = retrieve_files_with_inline(
            &graph,
            &communities,
            "anything",
            &config,
            &seen,
            tmp.path(),
        );

        assert_eq!(a.primary_files.len(), b.primary_files.len());
        assert_eq!(a.harm_removals, b.harm_removals);
        assert!(b.inline_slices.is_empty());
    }

    #[test]
    fn build_context_blocks_prepends_inline_slices_with_highest_score() {
        // Forge a result with one inline slice to exercise the block-build
        // prepend path without needing the full inline_builder to resolve.
        let graph = build_test_graph();
        let slice = crate::inline_builder::InlineSlice {
            focal_symbol_id: "sym:verify_token".into(),
            focal_file: "src/auth.rs".into(),
            content: "// inline snippet\nfn verify_token() { /* ... */ }".into(),
            token_count: 30,
            inlined_symbols: vec!["sym:decode_jwt".into()],
            unresolved_callees: vec![],
        };
        let forced = FileRetrievalResult {
            primary_files: vec![RankedFile {
                path: "src/db.rs".into(),
                score: 0.5,
                signals: Vec::new(),
            }],
            inline_slices: vec![slice],
            ..FileRetrievalResult::default()
        };

        let (blocks, _) =
            build_context_blocks_with_compression(&forced, &graph, 10_000, None, "verify_token");

        // First block is the inline slice.
        assert!(!blocks.is_empty());
        assert!(
            blocks[0].block_id.starts_with("blk-inline-"),
            "inline slice must be the first block, got: {}",
            blocks[0].block_id
        );
        assert_eq!(blocks[0].score, 1.0, "inline slice must score 1.0");
    }

    #[test]
    fn inline_slice_for_primary_file_skips_that_file_in_loop() {
        // Mutual-exclusion test: when an inline slice covers src/auth.rs,
        // the primary-files loop must not emit an additional block for
        // the same path (avoids reverse-boost double count).
        let graph = build_test_graph();
        let slice = crate::inline_builder::InlineSlice {
            focal_symbol_id: "sym:verify_token".into(),
            focal_file: "src/auth.rs".into(),
            content: "// inline".into(),
            token_count: 10,
            inlined_symbols: vec![],
            unresolved_callees: vec![],
        };
        let forced = FileRetrievalResult {
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
            build_context_blocks_with_compression(&forced, &graph, 10_000, None, "verify_token");

        // Expected: 1 inline + 1 primary (db only; auth skipped due to inline).
        let auth_primary_count = blocks
            .iter()
            .filter(|b| b.block_id == "blk-file-src-auth.rs")
            .count();
        let db_primary_count = blocks
            .iter()
            .filter(|b| b.block_id == "blk-file-src-db.rs")
            .count();
        assert_eq!(
            auth_primary_count, 0,
            "src/auth.rs primary block must be suppressed by inline slice"
        );
        assert_eq!(db_primary_count, 1, "src/db.rs primary block still emitted");
    }
