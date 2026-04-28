//! Sibling test body of `bridge.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `bridge.rs` via `#[path = "bridge_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use super::*;

    fn make_test_files() -> Vec<FileData> {
        vec![
            FileData {
                path: "src/auth/jwt.rs".into(),
                language: "rs".into(),
                line_count: 100,
                last_modified: 1000.0,
                symbols: vec![
                    SymbolData {
                        qualified_name: "auth::jwt::verify_token".into(),
                        name: "verify_token".into(),
                        kind: SymbolKindDto::Function,
                        line_start: 10,
                        line_end: 30,
                        signature: Some("fn verify_token(token: &str) -> Result<Claims>".into()),
                        is_test: false,
                        parent: None,
                        doc: None,
                    },
                    SymbolData {
                        qualified_name: "auth::jwt::decode_header".into(),
                        name: "decode_header".into(),
                        kind: SymbolKindDto::Function,
                        line_start: 35,
                        line_end: 50,
                        signature: Some("fn decode_header(token: &str) -> Header".into()),
                        is_test: false,
                        parent: None,
                        doc: None,
                    },
                ],
                imports: vec![ImportData {
                    source: "crypto::hmac".into(),
                    specifiers: vec!["Hmac".into(), "verify".into()],
                    line: 1,
                }],
                references: vec![ReferenceData {
                    source_symbol: "auth::jwt::verify_token".into(),
                    source_file: "src/auth/jwt.rs".into(),
                    target_symbol: "auth::jwt::decode_header".into(),
                    target_file: Some("src/auth/jwt.rs".into()),
                    kind: ReferenceKindDto::Call,
                }],
                data_models: vec![],
            },
            FileData {
                path: "src/crypto/hmac.rs".into(),
                language: "rs".into(),
                line_count: 50,
                last_modified: 900.0,
                symbols: vec![SymbolData {
                    qualified_name: "crypto::hmac::verify".into(),
                    name: "verify".into(),
                    kind: SymbolKindDto::Function,
                    line_start: 5,
                    line_end: 20,
                    signature: Some("fn verify(key: &[u8], msg: &[u8]) -> bool".into()),
                    is_test: false,
                    parent: None,
                    doc: None,
                }],
                imports: vec![],
                references: vec![],
                data_models: vec![],
            },
            FileData {
                path: "tests/test_jwt.rs".into(),
                language: "rs".into(),
                line_count: 30,
                last_modified: 1100.0,
                symbols: vec![SymbolData {
                    qualified_name: "test_jwt::test_verify_valid".into(),
                    name: "test_verify_valid".into(),
                    kind: SymbolKindDto::Function,
                    line_start: 5,
                    line_end: 20,
                    signature: Some("fn test_verify_valid()".into()),
                    is_test: true,
                    parent: None,
                    doc: None,
                }],
                imports: vec![],
                references: vec![ReferenceData {
                    source_symbol: "test_jwt::test_verify_valid".into(),
                    source_file: "tests/test_jwt.rs".into(),
                    target_symbol: "auth::jwt::verify_token".into(),
                    target_file: Some("src/auth/jwt.rs".into()),
                    kind: ReferenceKindDto::Call,
                }],
                data_models: vec![],
            },
        ]
    }

    #[test]
    fn test_build_graph_creates_file_nodes() {
        let files = make_test_files();
        let (graph, stats) = build_graph(&files);

        assert_eq!(stats.files, 3);
        assert!(graph.get_node("file:src/auth/jwt.rs").is_some());
        assert!(graph.get_node("file:src/crypto/hmac.rs").is_some());
        assert!(graph.get_node("file:tests/test_jwt.rs").is_some());
    }

    #[test]
    fn test_build_graph_creates_symbol_nodes() {
        let files = make_test_files();
        let (graph, stats) = build_graph(&files);

        assert_eq!(stats.symbols, 3); // verify_token, decode_header, verify
        let sym = graph
            .get_node("sym:src/auth/jwt.rs:auth::jwt::verify_token")
            .unwrap();
        assert_eq!(sym.node_type, NodeType::Symbol);
        assert_eq!(sym.name, "verify_token");
    }

    #[test]
    fn test_build_graph_creates_test_nodes() {
        let files = make_test_files();
        let (graph, stats) = build_graph(&files);

        assert_eq!(stats.tests, 1);
        let test = graph
            .get_node("test:tests/test_jwt.rs:test_jwt::test_verify_valid")
            .unwrap();
        assert_eq!(test.node_type, NodeType::Test);
    }

    #[test]
    fn test_build_graph_creates_contains_edges() {
        let files = make_test_files();
        let (_, stats) = build_graph(&files);

        // 3 files: jwt(2 syms), hmac(1 sym), test(1 test) + 1 import node
        assert_eq!(stats.edges_contains, 4);
    }

    #[test]
    fn test_build_graph_resolves_call_edges() {
        let files = make_test_files();
        let (graph, stats) = build_graph(&files);

        assert!(stats.edges_calls > 0);
        // verify_token calls decode_header
        let caller = "sym:src/auth/jwt.rs:auth::jwt::verify_token";
        let callee = "sym:src/auth/jwt.rs:auth::jwt::decode_header";
        let call_edges = graph.edges_between(caller, callee);
        assert!(
            call_edges.iter().any(|e| e.edge_type == EdgeType::Calls),
            "Expected Calls edge from verify_token to decode_header"
        );
    }

    #[test]
    fn test_build_graph_creates_tests_edges() {
        let files = make_test_files();
        let (graph, stats) = build_graph(&files);

        assert!(stats.edges_tests > 0);
        let test_id = "test:tests/test_jwt.rs:test_jwt::test_verify_valid";
        let target = "sym:src/auth/jwt.rs:auth::jwt::verify_token";
        let test_edges = graph.edges_between(test_id, target);
        assert!(
            test_edges.iter().any(|e| e.edge_type == EdgeType::Tests),
            "Expected Tests edge from test to verify_token"
        );
    }

    #[test]
    fn test_build_graph_creates_import_nodes() {
        let files = make_test_files();
        let (_, stats) = build_graph(&files);

        assert_eq!(stats.imports, 1);
        assert_eq!(stats.edges_imports, 1);
    }

    #[test]
    fn test_build_graph_stats_totals() {
        let files = make_test_files();
        let (graph, stats) = build_graph(&files);

        assert_eq!(stats.total_nodes(), graph.node_count());
        assert!(stats.total_edges() > 0);
    }

    #[test]
    fn test_file_node_id_format() {
        assert_eq!(file_node_id("src/main.rs"), "file:src/main.rs");
    }

    #[test]
    fn test_empty_files_produces_empty_graph() {
        let (graph, stats) = build_graph(&[]);
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
        assert_eq!(stats.total_nodes(), 0);
    }

    #[test]
    fn test_data_model_creates_type_node_and_inheritance() {
        let files = vec![FileData {
            path: "src/model.rs".into(),
            language: "rs".into(),
            line_count: 50,
            last_modified: 1000.0,
            symbols: vec![],
            imports: vec![],
            references: vec![],
            data_models: vec![
                DataModelData {
                    name: "BaseEntity".into(),
                    file_path: "src/model.rs".into(),
                    line_start: 1,
                    line_end: 10,
                    parent_type: None,
                    implemented_interfaces: vec![],
                },
                DataModelData {
                    name: "User".into(),
                    file_path: "src/model.rs".into(),
                    line_start: 15,
                    line_end: 30,
                    parent_type: Some("BaseEntity".into()),
                    implemented_interfaces: vec![],
                },
            ],
        }];

        let (graph, stats) = build_graph(&files);

        assert_eq!(stats.types, 2);
        assert!(graph.get_node("type:src/model.rs:User").is_some());
        assert!(graph.get_node("type:src/model.rs:BaseEntity").is_some());

        // User inherits BaseEntity
        assert!(stats.edges_inherits > 0);
    }
