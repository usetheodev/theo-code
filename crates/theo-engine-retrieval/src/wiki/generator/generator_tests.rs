//! Sibling test body of `generator.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `generator.rs` via `#[path = "generator_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.


#![cfg(test)]

    #![allow(unused_imports)]
    use super::*;
    use std::collections::{HashMap, HashSet};
    use crate::wiki::generator::*;
    use crate::wiki::model::*;
    use theo_engine_graph::cluster::Community;
    use theo_engine_graph::model::{CodeGraph, Edge, EdgeType, Node, NodeType, SymbolKind};

    fn test_graph() -> (CodeGraph, Vec<Community>) {
        let mut graph = CodeGraph::new();

        // File: auth.rs
        graph.add_node(Node {
            id: "file:auth.rs".into(),
            name: "auth.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/auth.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 100.0,
        });
        graph.add_node(Node {
            id: "sym:verify".into(),
            name: "verify_token".into(),
            node_type: NodeType::Symbol,
            file_path: Some("src/auth.rs".into()),
            signature: Some("pub fn verify_token(t: &str) -> bool".into()),
            doc: Some("Verify JWT token".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(10),
            line_end: Some(30),
            last_modified: 100.0,
        });
        graph.add_edge(Edge {
            source: "file:auth.rs".into(),
            target: "sym:verify".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        // File: handler.rs (different community)
        graph.add_node(Node {
            id: "file:handler.rs".into(),
            name: "handler.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/handler.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 200.0,
        });
        graph.add_node(Node {
            id: "sym:handle".into(),
            name: "handle_request".into(),
            node_type: NodeType::Symbol,
            file_path: Some("src/handler.rs".into()),
            signature: Some("pub fn handle_request(req: Request) -> Response".into()),
            doc: None,
            kind: Some(SymbolKind::Function),
            line_start: Some(5),
            line_end: Some(20),
            last_modified: 200.0,
        });
        graph.add_edge(Edge {
            source: "file:handler.rs".into(),
            target: "sym:handle".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        // Second file in auth community
        graph.add_node(Node {
            id: "file:auth_utils.rs".into(),
            name: "auth_utils.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/auth_utils.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 100.0,
        });

        // handler calls verify (cross-community)
        graph.add_edge(Edge {
            source: "sym:handle".into(),
            target: "sym:verify".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });

        // Second file in handler community
        graph.add_node(Node {
            id: "file:middleware.rs".into(),
            name: "middleware.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/middleware.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 200.0,
        });

        let communities = vec![
            Community {
                id: "c1".into(),
                name: "auth".into(),
                node_ids: vec![
                    "file:auth.rs".into(),
                    "sym:verify".into(),
                    "file:auth_utils.rs".into(),
                ],
                level: 0,
                parent_id: None,
                version: 0,
            },
            Community {
                id: "c2".into(),
                name: "handler".into(),
                node_ids: vec![
                    "file:handler.rs".into(),
                    "sym:handle".into(),
                    "file:middleware.rs".into(),
                ],
                level: 0,
                parent_id: None,
                version: 0,
            },
        ];

        (graph, communities)
    }

    #[test]
    fn generate_wiki_produces_pages() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test-project");
        assert_eq!(wiki.docs.len(), 2);
        assert_eq!(wiki.manifest.page_count, 2);
    }

    #[test]
    fn wiki_doc_has_provenance() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");
        let auth = wiki.docs.iter().find(|d| d.slug == "auth").unwrap();
        assert!(!auth.source_refs.is_empty());
        assert_eq!(auth.source_refs[0].file_path, "src/auth.rs");
    }

    #[test]
    fn wiki_doc_has_files() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");
        let auth = wiki.docs.iter().find(|d| d.slug == "auth").unwrap();
        assert!(!auth.files.is_empty());
        assert!(auth.files.iter().any(|f| f.path == "src/auth.rs"));
    }

    #[test]
    fn wiki_doc_has_public_api() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");
        let auth = wiki.docs.iter().find(|d| d.slug == "auth").unwrap();
        assert!(!auth.public_api.is_empty());
        assert!(auth.public_api[0].signature.contains("verify_token"));
    }

    #[test]
    fn cross_community_deps_detected() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");
        let handler = wiki.docs.iter().find(|d| d.slug == "handler").unwrap();
        // handler calls verify_token in auth → dependency on auth
        assert!(handler.dependencies.iter().any(|d| d.target_slug == "auth"));
    }

    #[test]
    fn slugify_works() {
        assert_eq!(slugify("My Module"), "my-module");
        assert_eq!(slugify("auth/jwt"), "auth-jwt");
        assert_eq!(slugify("theo-engine-graph (42)"), "theo-engine-graph-42");
    }

    #[test]
    fn empty_community() {
        let graph = CodeGraph::new();
        let communities = vec![Community {
            id: "empty".into(),
            name: "empty".into(),
            node_ids: vec![],
            level: 0,
            parent_id: None,
            version: 0,
        }];
        let wiki = generate_wiki(&communities, &graph, "test");
        assert_eq!(wiki.docs.len(), 0); // Empty community filtered out
    }

    #[test]
    fn graph_hash_deterministic() {
        let (graph, _) = test_graph();
        let h1 = compute_graph_hash(&graph);
        let h2 = compute_graph_hash(&graph);
        assert_eq!(h1, h2);
    }

    #[test]
    fn community_hash_deterministic() {
        let (graph, communities) = test_graph();
        let h1 = compute_community_hash(&communities[0], &graph);
        let h2 = compute_community_hash(&communities[0], &graph);
        assert_eq!(h1, h2);
    }

    #[test]
    fn community_hash_differs_between_communities() {
        let (graph, communities) = test_graph();
        let h1 = compute_community_hash(&communities[0], &graph);
        let h2 = compute_community_hash(&communities[1], &graph);
        assert_ne!(h1, h2);
    }

    #[test]
    fn incremental_zero_change_zero_regen() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");

        // Build page_hashes from the current state
        let mut page_hashes = HashMap::new();
        for c in &communities {
            if c.node_ids.is_empty() {
                continue;
            }
            let key = community_canonical_key(c, &graph);
            let hash = compute_community_hash(c, &graph);
            page_hashes.insert(key, hash);
        }

        let manifest_with_hashes = WikiManifest {
            page_hashes,
            ..wiki.manifest.clone()
        };

        let (_, stats) = generate_wiki_incremental(
            &communities,
            &graph,
            "test",
            &manifest_with_hashes,
            &wiki.docs,
        );
        assert_eq!(stats.changed, 0, "no changes should mean zero regeneration");
        assert_eq!(stats.propagated, 0);
    }

    fn test_graph_modified() -> (CodeGraph, Vec<Community>) {
        // Same as test_graph but auth.rs has different mtime
        let mut graph = CodeGraph::new();
        graph.add_node(Node {
            id: "file:auth.rs".into(),
            name: "auth.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/auth.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 999.0, // CHANGED
        });
        graph.add_node(Node {
            id: "sym:verify".into(),
            name: "verify_token".into(),
            node_type: NodeType::Symbol,
            file_path: Some("src/auth.rs".into()),
            signature: Some("pub fn verify_token(t: &str) -> bool".into()),
            doc: Some("Verify JWT token".into()),
            kind: Some(SymbolKind::Function),
            line_start: Some(10),
            line_end: Some(30),
            last_modified: 999.0,
        });
        graph.add_edge(Edge {
            source: "file:auth.rs".into(),
            target: "sym:verify".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_node(Node {
            id: "file:handler.rs".into(),
            name: "handler.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/handler.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 200.0,
        });
        graph.add_node(Node {
            id: "sym:handle".into(),
            name: "handle_request".into(),
            node_type: NodeType::Symbol,
            file_path: Some("src/handler.rs".into()),
            signature: Some("pub fn handle_request(req: Request) -> Response".into()),
            doc: None,
            kind: Some(SymbolKind::Function),
            line_start: Some(5),
            line_end: Some(20),
            last_modified: 200.0,
        });
        graph.add_edge(Edge {
            source: "file:handler.rs".into(),
            target: "sym:handle".into(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });
        graph.add_edge(Edge {
            source: "sym:handle".into(),
            target: "sym:verify".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        // Second files (same as test_graph)
        graph.add_node(Node {
            id: "file:auth_utils.rs".into(),
            name: "auth_utils.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/auth_utils.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 100.0,
        });
        graph.add_node(Node {
            id: "file:middleware.rs".into(),
            name: "middleware.rs".into(),
            node_type: NodeType::File,
            file_path: Some("src/middleware.rs".into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 200.0,
        });

        let communities = vec![
            Community {
                id: "c1".into(),
                name: "auth".into(),
                node_ids: vec![
                    "file:auth.rs".into(),
                    "sym:verify".into(),
                    "file:auth_utils.rs".into(),
                ],
                level: 0,
                parent_id: None,
                version: 0,
            },
            Community {
                id: "c2".into(),
                name: "handler".into(),
                node_ids: vec![
                    "file:handler.rs".into(),
                    "sym:handle".into(),
                    "file:middleware.rs".into(),
                ],
                level: 0,
                parent_id: None,
                version: 0,
            },
        ];
        (graph, communities)
    }

    #[test]
    fn incremental_detects_changed_community() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");

        // Build initial hashes
        let mut page_hashes = HashMap::new();
        for c in &communities {
            if c.node_ids.is_empty() {
                continue;
            }
            let key = community_canonical_key(c, &graph);
            let hash = compute_community_hash(c, &graph);
            page_hashes.insert(key, hash);
        }
        let manifest = WikiManifest {
            page_hashes,
            ..wiki.manifest.clone()
        };

        // New graph where auth.rs has different mtime
        let (graph2, communities2) = test_graph_modified();

        let (result, stats) =
            generate_wiki_incremental(&communities2, &graph2, "test", &manifest, &wiki.docs);
        assert!(stats.changed > 0, "should detect change in auth community");
        assert_eq!(result.docs.len(), 2, "should still have all pages");
    }

    #[test]
    fn topology_concept_detection_with_cross_deps() {
        // Build docs with enough cross-deps to form topology clusters
        let doc_a = WikiDoc {
            slug: "mod-a".into(),
            title: "Module A".into(),
            community_id: "c1".into(),
            file_count: 5,
            symbol_count: 10,
            primary_language: "rs".into(),
            files: vec![],
            entry_points: vec![],
            public_api: vec![],
            dependencies: vec![
                DepEntry {
                    target_slug: "mod-b".into(),
                    target_name: "B".into(),
                    edge_type: "Calls".into(),
                },
                DepEntry {
                    target_slug: "mod-b".into(),
                    target_name: "B".into(),
                    edge_type: "Imports".into(),
                },
            ],
            call_flow: vec![],
            test_coverage: TestCoverage {
                tested: 0,
                total: 0,
                percentage: 0.0,
                untested: vec![],
            },
            source_refs: vec![],
            summary: String::new(),
            tags: vec![],
            crate_description: None,
            module_doc: None,
            generated_at: "0".into(),
            enriched: false,
        };
        let doc_b = WikiDoc {
            slug: "mod-b".into(),
            title: "Module B".into(),
            community_id: "c2".into(),
            file_count: 5,
            symbol_count: 10,
            primary_language: "rs".into(),
            files: vec![],
            entry_points: vec![],
            public_api: vec![],
            dependencies: vec![DepEntry {
                target_slug: "mod-a".into(),
                target_name: "A".into(),
                edge_type: "Calls".into(),
            }],
            call_flow: vec![],
            test_coverage: TestCoverage {
                tested: 0,
                total: 0,
                percentage: 0.0,
                untested: vec![],
            },
            source_refs: vec![],
            summary: String::new(),
            tags: vec![],
            crate_description: None,
            module_doc: None,
            generated_at: "0".into(),
            enriched: false,
        };
        // C and D are isolated
        let doc_c = WikiDoc {
            slug: "other-c".into(),
            title: "Other C".into(),
            community_id: "c3".into(),
            file_count: 3,
            symbol_count: 5,
            primary_language: "rs".into(),
            files: vec![],
            entry_points: vec![],
            public_api: vec![],
            dependencies: vec![],
            call_flow: vec![],
            test_coverage: TestCoverage {
                tested: 0,
                total: 0,
                percentage: 0.0,
                untested: vec![],
            },
            source_refs: vec![],
            summary: String::new(),
            tags: vec![],
            crate_description: None,
            module_doc: None,
            generated_at: "0".into(),
            enriched: false,
        };

        let docs = vec![doc_a, doc_b, doc_c];
        let concepts = detect_concepts(&docs);
        // A and B have 3 mutual deps → should form a topology cluster
        assert!(
            concepts
                .iter()
                .any(|c| c.related_modules.contains(&"mod-a".to_string())
                    && c.related_modules.contains(&"mod-b".to_string())),
            "A and B should be in same concept cluster, got: {:?}",
            concepts
        );
    }

    #[test]
    fn no_deps_falls_back_to_prefix() {
        // Docs with same prefix but no cross-deps → prefix-based grouping
        let make_doc = |slug: &str, title: &str| WikiDoc {
            slug: slug.into(),
            title: title.into(),
            community_id: "cx".into(),
            file_count: 3,
            symbol_count: 5,
            primary_language: "rs".into(),
            files: vec![],
            entry_points: vec![],
            public_api: vec![],
            dependencies: vec![],
            call_flow: vec![],
            test_coverage: TestCoverage {
                tested: 0,
                total: 0,
                percentage: 0.0,
                untested: vec![],
            },
            source_refs: vec![],
            summary: String::new(),
            tags: vec![],
            crate_description: None,
            module_doc: None,
            generated_at: "0".into(),
            enriched: false,
        };

        let docs = vec![
            make_doc("theo-engine-a", "theo-engine-a (10)"),
            make_doc("theo-engine-b", "theo-engine-b (5)"),
        ];
        let concepts = detect_concepts(&docs);
        // Should fall back to prefix: "theo-engine" groups them
        assert!(!concepts.is_empty(), "should have prefix-based concept");
        assert!(concepts[0].related_modules.len() >= 2);
    }

    #[test]
    fn dep_propagation_regenerates_dependent() {
        let (graph, communities) = test_graph();
        let wiki = generate_wiki(&communities, &graph, "test");

        // Build initial hashes
        let mut page_hashes = HashMap::new();
        for c in &communities {
            if c.node_ids.is_empty() {
                continue;
            }
            let key = community_canonical_key(c, &graph);
            let hash = compute_community_hash(c, &graph);
            page_hashes.insert(key, hash);
        }
        let manifest = WikiManifest {
            page_hashes,
            ..wiki.manifest.clone()
        };

        // Modified graph: auth changed
        let (graph2, communities2) = test_graph_modified();

        let (_, stats) =
            generate_wiki_incremental(&communities2, &graph2, "test", &manifest, &wiki.docs);
        // auth changed + handler propagated (depends on auth)
        assert!(
            stats.changed >= 1,
            "auth should be changed, stats: {}",
            stats
        );
        let total_regen = stats.changed + stats.propagated;
        assert!(
            total_regen >= 1,
            "at least auth should be regenerated, stats: {}",
            stats
        );
    }
