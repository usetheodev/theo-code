//! Sibling test body of `search.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `search.rs` via `#[path = "search_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use super::*;

    #[test]
    fn test_split_snake_case() {
        let tokens = tokenise("parse_auth_header");
        assert!(tokens.contains(&"parse".to_string()));
        assert!(tokens.contains(&"auth".to_string()));
        assert!(tokens.contains(&"header".to_string()));
    }

    #[test]
    fn test_split_camel_case() {
        let tokens = tokenise("verifyJwtToken");
        assert!(tokens.contains(&"verify".to_string()));
        assert!(tokens.contains(&"jwt".to_string()));
        assert!(tokens.contains(&"token".to_string()));
        // Unsplit form also present
        assert!(tokens.contains(&"verifyjwttoken".to_string()));
    }

    #[test]
    fn test_split_pascal_case() {
        let tokens = tokenise("AuthService");
        assert!(tokens.contains(&"auth".to_string()));
        assert!(tokens.contains(&"service".to_string()));
    }

    #[test]
    fn test_split_screaming_snake() {
        let tokens = tokenise("MAX_RETRY_COUNT");
        assert!(tokens.contains(&"max".to_string()));
        assert!(tokens.contains(&"retry".to_string()));
        assert!(tokens.contains(&"count".to_string()));
    }

    #[test]
    fn test_split_acronym_prefix() {
        let tokens = tokenise("HTMLParser");
        assert!(tokens.contains(&"html".to_string()));
        assert!(tokens.contains(&"parser".to_string()) || tokens.contains(&"pars".to_string()));
    }

    #[test]
    fn test_split_acronym_middle() {
        let tokens = tokenise("getHTTPResponse");
        assert!(tokens.contains(&"get".to_string()));
        assert!(tokens.contains(&"http".to_string()));
        assert!(
            tokens.contains(&"response".to_string()) || tokens.contains(&"respons".to_string())
        );
    }

    #[test]
    fn test_split_mixed_separators() {
        let tokens = tokenise("fn verify_token(jwt: &str)");
        assert!(tokens.contains(&"verify".to_string()));
        assert!(tokens.contains(&"token".to_string()));
        assert!(tokens.contains(&"jwt".to_string()));
    }

    #[test]
    fn test_split_single_word() {
        let tokens = tokenise("auth");
        assert!(tokens.contains(&"auth".to_string()));
    }

    #[test]
    fn test_split_empty() {
        let result: Vec<String> = tokenise("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_all_lowercase() {
        let tokens = tokenise("already lowercase");
        assert!(tokens.contains(&"already".to_string()));
        assert!(tokens.contains(&"lowercase".to_string()));
    }

    /// Debug test: verify BM25 actually works with a simple community.
    #[test]
    fn debug_bm25_community_document() {
        use theo_engine_graph::cluster::Community;
        use theo_engine_graph::model::*;

        let mut graph = CodeGraph::new();

        // Create a File node with Symbol children
        graph.add_node(Node {
            id: "file:crates/auth/src/lib.rs".to_string(),
            node_type: NodeType::File,
            name: "crates/auth/src/lib.rs".to_string(),
            file_path: Some("crates/auth/src/lib.rs".to_string()),
            signature: None,
            kind: None,
            line_start: None,
            line_end: None,
            last_modified: 0.0,
            doc: None,
        });
        graph.add_node(Node {
            id: "sym:verify_token".to_string(),
            node_type: NodeType::Symbol,
            name: "verify_token".to_string(),
            file_path: Some("crates/auth/src/lib.rs".to_string()),
            signature: Some("pub fn verify_token(token: &str) -> Result<Claims>".to_string()),
            kind: Some(SymbolKind::Function),
            line_start: Some(10),
            line_end: Some(25),
            last_modified: 0.0,
            doc: Some("Verify a JWT token and extract claims.".to_string()),
        });
        graph.add_edge(Edge {
            source: "file:crates/auth/src/lib.rs".to_string(),
            target: "sym:verify_token".to_string(),
            edge_type: EdgeType::Contains,
            weight: 1.0,
        });

        let community = Community {
            id: "comm-auth".to_string(),
            name: "authentication".to_string(),
            level: 0,
            node_ids: vec![
                "file:crates/auth/src/lib.rs".to_string(),
                "sym:verify_token".to_string(),
            ],
            parent_id: None,
            version: 1,
        };

        // Check community_document output
        let doc = community_document(&community, &graph);
        eprintln!("COMMUNITY DOCUMENT:\n{}\n", doc);
        assert!(
            doc.contains("verify_token"),
            "Document should contain symbol name"
        );
        assert!(
            doc.contains("verify"),
            "Document should contain 'verify' after tokenization"
        );

        // Check tokenization
        let tokens = tokenise(&doc);
        eprintln!("TOKENS: {:?}\n", tokens);
        assert!(
            tokens.contains(&"verify".to_string()),
            "Tokens should contain 'verify'"
        );
        assert!(
            tokens.contains(&"token".to_string()),
            "Tokens should contain 'token'"
        );

        // Build BM25 index and search
        let communities = vec![community];
        let bm25 = Bm25Index::build(&communities, &graph);
        let results = bm25.search("verify_token", &communities);

        eprintln!("BM25 RESULTS for 'verify_token':");
        for r in &results {
            eprintln!("  {} score={:.4}", r.community.name, r.score);
        }

        assert!(!results.is_empty(), "BM25 should return results");
        assert!(
            results[0].score > 0.0,
            "Top result should have positive score, got {}",
            results[0].score
        );
    }

    // --- S3-T3: ScoringWeights tests ---

    #[test]
    fn scoring_weights_default_sums_to_one() {
        let w = ScoringWeights::default();
        let sum = w.bm25 + w.file_boost + w.centrality + w.recency;
        assert!(
            (sum - 1.0).abs() < 0.001,
            "Default weights must sum to 1.0, got {}",
            sum
        );
    }

    #[test]
    fn scoring_weights_custom_normalizes() {
        let w = ScoringWeights::new(2.0, 1.0, 0.5, 0.5);
        let sum = w.bm25 + w.file_boost + w.centrality + w.recency;
        assert!(
            (sum - 1.0).abs() < 0.001,
            "Custom weights must be normalized to 1.0, got {}",
            sum
        );
        assert!((w.bm25 - 0.5).abs() < 0.001, "2.0/4.0 = 0.5");
        assert!((w.file_boost - 0.25).abs() < 0.001, "1.0/4.0 = 0.25");
    }

    #[test]
    fn scoring_weights_zero_input_uses_default() {
        let w = ScoringWeights::new(0.0, 0.0, 0.0, 0.0);
        assert_eq!(w.bm25, ScoringWeights::default().bm25);
    }

    #[test]
    fn scoring_weights_on_scorer() {
        // Verify ScoringWeights is accessible on the scorer struct
        let graph = CodeGraph::new();
        let communities: Vec<Community> = vec![];
        let scorer = MultiSignalScorer::build(&communities, &graph);
        assert!((scorer.scoring_weights.bm25 - 0.55).abs() < 0.001);
    }
