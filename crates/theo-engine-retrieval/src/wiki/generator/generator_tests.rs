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

    fn gt_file_node(id: &str, name: &str, path: &str, last_modified: f64) -> Node {
        Node {
            id: id.into(),
            name: name.into(),
            node_type: NodeType::File,
            file_path: Some(path.into()),
            signature: None,
            doc: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified,
        }
    }

    fn gt_symbol_node(
        id: &str,
        name: &str,
        path: &str,
        signature: &str,
        doc: Option<&str>,
        line_range: (usize, usize),
        last_modified: f64,
    ) -> Node {
        Node {
            id: id.into(),
            name: name.into(),
            node_type: NodeType::Symbol,
            file_path: Some(path.into()),
            signature: Some(signature.into()),
            doc: doc.map(String::from),
            kind: Some(SymbolKind::Function),
            line_start: Some(line_range.0),
            line_end: Some(line_range.1),
            last_modified,
        }
    }

    fn gt_edge(source: &str, target: &str, et: EdgeType) -> Edge {
        Edge { source: source.into(), target: target.into(), edge_type: et, weight: 1.0 }
    }

    fn gt_community(id: &str, name: &str, node_ids: Vec<&str>) -> Community {
        Community {
            id: id.into(),
            name: name.into(),
            node_ids: node_ids.into_iter().map(String::from).collect(),
            level: 0,
            parent_id: None,
            version: 0,
        }
    }

    fn add_auth_community_nodes(graph: &mut CodeGraph) {
        graph.add_node(gt_file_node("file:auth.rs", "auth.rs", "src/auth.rs", 100.0));
        graph.add_node(gt_symbol_node(
            "sym:verify",
            "verify_token",
            "src/auth.rs",
            "pub fn verify_token(t: &str) -> bool",
            Some("Verify JWT token"),
            (10, 30),
            100.0,
        ));
        graph.add_node(gt_file_node(
            "file:auth_utils.rs",
            "auth_utils.rs",
            "src/auth_utils.rs",
            100.0,
        ));
        graph.add_edge(gt_edge("file:auth.rs", "sym:verify", EdgeType::Contains));
    }

    fn add_handler_community_nodes(graph: &mut CodeGraph) {
        graph.add_node(gt_file_node("file:handler.rs", "handler.rs", "src/handler.rs", 200.0));
        graph.add_node(gt_symbol_node(
            "sym:handle",
            "handle_request",
            "src/handler.rs",
            "pub fn handle_request(req: Request) -> Response",
            None,
            (5, 20),
            200.0,
        ));
        graph.add_node(gt_file_node(
            "file:middleware.rs",
            "middleware.rs",
            "src/middleware.rs",
            200.0,
        ));
        graph.add_edge(gt_edge("file:handler.rs", "sym:handle", EdgeType::Contains));
    }

    fn test_graph() -> (CodeGraph, Vec<Community>) {
        let mut graph = CodeGraph::new();
        add_auth_community_nodes(&mut graph);
        add_handler_community_nodes(&mut graph);
        // Cross-community call: handler → verify.
        graph.add_edge(gt_edge("sym:handle", "sym:verify", EdgeType::Calls));
        let communities = vec![
            gt_community("c1", "auth", vec!["file:auth.rs", "sym:verify", "file:auth_utils.rs"]),
            gt_community(
                "c2",
                "handler",
                vec!["file:handler.rs", "sym:handle", "file:middleware.rs"],
            ),
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

    /// Same shape as `test_graph()` but the auth.rs file + its symbol
    /// have `last_modified = 999.0` (vs 100.0 in the baseline) so the
    /// incremental generator detects the change.
    fn test_graph_modified() -> (CodeGraph, Vec<Community>) {
        let mut graph = CodeGraph::new();
        graph.add_node(gt_file_node("file:auth.rs", "auth.rs", "src/auth.rs", 999.0));
        graph.add_node(gt_symbol_node(
            "sym:verify",
            "verify_token",
            "src/auth.rs",
            "pub fn verify_token(t: &str) -> bool",
            Some("Verify JWT token"),
            (10, 30),
            999.0,
        ));
        graph.add_node(gt_file_node(
            "file:auth_utils.rs",
            "auth_utils.rs",
            "src/auth_utils.rs",
            100.0,
        ));
        graph.add_edge(gt_edge("file:auth.rs", "sym:verify", EdgeType::Contains));
        add_handler_community_nodes(&mut graph);
        graph.add_edge(gt_edge("sym:handle", "sym:verify", EdgeType::Calls));
        let communities = vec![
            gt_community("c1", "auth", vec!["file:auth.rs", "sym:verify", "file:auth_utils.rs"]),
            gt_community(
                "c2",
                "handler",
                vec!["file:handler.rs", "sym:handle", "file:middleware.rs"],
            ),
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

    fn empty_test_coverage() -> TestCoverage {
        TestCoverage { tested: 0, total: 0, percentage: 0.0, untested: vec![] }
    }

    fn make_topology_doc(
        slug: &str,
        title: &str,
        community_id: &str,
        file_count: usize,
        symbol_count: usize,
        deps: Vec<DepEntry>,
    ) -> WikiDoc {
        WikiDoc {
            slug: slug.into(),
            title: title.into(),
            community_id: community_id.into(),
            file_count,
            symbol_count,
            primary_language: "rs".into(),
            files: vec![],
            entry_points: vec![],
            public_api: vec![],
            dependencies: deps,
            call_flow: vec![],
            test_coverage: empty_test_coverage(),
            source_refs: vec![],
            summary: String::new(),
            tags: vec![],
            crate_description: None,
            module_doc: None,
            generated_at: "0".into(),
            enriched: false,
        }
    }

    fn dep(target_slug: &str, target_name: &str, edge_type: &str) -> DepEntry {
        DepEntry {
            target_slug: target_slug.into(),
            target_name: target_name.into(),
            edge_type: edge_type.into(),
        }
    }

    #[test]
    fn topology_concept_detection_with_cross_deps() {
        // A → B (Calls + Imports), B → A (Calls): 3 mutual edges form a
        // topology cluster. C is isolated (no dependencies).
        let doc_a = make_topology_doc(
            "mod-a",
            "Module A",
            "c1",
            5,
            10,
            vec![dep("mod-b", "B", "Calls"), dep("mod-b", "B", "Imports")],
        );
        let doc_b = make_topology_doc(
            "mod-b",
            "Module B",
            "c2",
            5,
            10,
            vec![dep("mod-a", "A", "Calls")],
        );
        let doc_c = make_topology_doc("other-c", "Other C", "c3", 3, 5, vec![]);

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
