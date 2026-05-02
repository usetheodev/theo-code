//! Sibling test body of `runtime.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `runtime.rs` via `#[path = "runtime_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.


#![cfg(test)]

    use super::*;

    fn sample_insight(success: bool, command: &str, error: Option<&str>) -> RuntimeInsight {
        RuntimeInsight {
            timestamp: 1000,
            source: "cargo_test".into(),
            command: command.into(),
            exit_code: if success { 0 } else { 1 },
            success,
            duration_ms: 500,
            error_summary: error.map(|s| s.into()),
            stdout_excerpt: Some("test output".into()),
            stderr_excerpt: error.map(|s| s.into()),
            affected_files: vec!["src/auth.rs".into()],
            affected_symbols: vec!["auth::tests::verify_token".into()],
            graph_hash: 12345,
        }
    }

    #[test]
    fn ingest_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");

        let insight = sample_insight(true, "cargo test -p auth", None);
        ingest_insight(&wiki_dir, insight.clone()).unwrap();
        ingest_insight(
            &wiki_dir,
            sample_insight(false, "cargo test", Some("error[E0308]")),
        )
        .unwrap();

        let all = load_all_insights(&wiki_dir);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].command, "cargo test -p auth");
        assert!(all[0].success);
        assert!(!all[1].success);
    }

    #[test]
    fn query_filters_by_keyword() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");

        ingest_insight(&wiki_dir, sample_insight(true, "cargo test -p auth", None)).unwrap();

        let mut build_insight = sample_insight(true, "cargo build", None);
        build_insight.affected_files = vec!["src/main.rs".into()]; // different module
        build_insight.affected_symbols = vec![];
        ingest_insight(&wiki_dir, build_insight).unwrap();

        let mut router_insight = sample_insight(false, "cargo test -p router", Some("error"));
        router_insight.affected_files = vec!["src/router.rs".into()];
        router_insight.affected_symbols = vec!["router::tests::basic".into()];
        ingest_insight(&wiki_dir, router_insight).unwrap();

        let auth_results = query_insights(&wiki_dir, "auth", 10);
        assert_eq!(
            auth_results.len(),
            1,
            "only 1 insight mentions 'auth': {:?}",
            auth_results.iter().map(|r| &r.command).collect::<Vec<_>>()
        );

        let all_test = query_insights(&wiki_dir, "cargo test", 10);
        assert!(
            all_test.len() >= 2,
            "should match 'cargo test' in command: got {}",
            all_test.len()
        );
    }

    #[test]
    fn extract_entities_from_rust_error() {
        let stderr = r#"
error[E0308]: mismatched types
  --> src/auth.rs:42:5
   |
42 |     verify_token(t)
   |     ^^^^^^^^^^^^^^^ expected `bool`, found `()`

error: test auth::tests::verify_token ... FAILED
        "#;
        let (files, symbols) = extract_affected_entities("", stderr);
        assert!(
            files.contains(&"src/auth.rs".to_string()),
            "files: {:?}",
            files
        );
        assert!(
            symbols.iter().any(|s| s.contains("verify_token")),
            "symbols: {:?}",
            symbols
        );
    }

    #[test]
    fn extract_entities_from_test_output() {
        let stdout = r#"
running 5 tests
test auth::tests::verify_token ... ok
test auth::tests::refresh_token ... FAILED
test router::tests::basic_route ... ok
        "#;
        let (_, symbols) = extract_affected_entities(stdout, "");
        assert!(symbols.contains(&"auth::tests::verify_token".to_string()));
        assert!(symbols.contains(&"auth::tests::refresh_token".to_string()));
        assert!(symbols.contains(&"router::tests::basic_route".to_string()));
    }

    #[test]
    fn extract_error_summary_picks_first_error() {
        let stderr =
            "warning: unused variable\nerror[E0308]: mismatched types\nnote: see full error";
        let summary = extract_error_summary(stderr);
        assert!(summary.unwrap().contains("error[E0308]"));
    }

    #[test]
    fn aggregate_for_module_groups_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");

        // 2 successes, 1 failure for auth
        ingest_insight(&wiki_dir, sample_insight(true, "cargo test -p auth", None)).unwrap();
        ingest_insight(&wiki_dir, sample_insight(true, "cargo test -p auth", None)).unwrap();
        ingest_insight(
            &wiki_dir,
            sample_insight(false, "cargo test -p auth", Some("error[E0308]")),
        )
        .unwrap();

        let ops = aggregate_for_module(&wiki_dir, "auth");
        assert_eq!(ops.insight_count, 3);
        assert!(!ops.successful_recipes.is_empty());
        assert!(!ops.common_failures.is_empty());
        assert_eq!(ops.common_failures[0].count, 1);
    }

    #[test]
    fn flaky_test_detection() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");

        // Same test: passed once, failed once
        ingest_insight(&wiki_dir, sample_insight(true, "cargo test", None)).unwrap();
        ingest_insight(
            &wiki_dir,
            sample_insight(false, "cargo test", Some("failed")),
        )
        .unwrap();

        let ops = aggregate_for_module(&wiki_dir, "auth");
        assert!(
            !ops.flaky_tests.is_empty(),
            "should detect flaky test: {:?}",
            ops
        );
    }

    #[test]
    fn distill_learnings_promotes_repeated_errors() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");

        // Same error 4 times (>= 3 threshold)
        for _ in 0..4 {
            ingest_insight(
                &wiki_dir,
                sample_insight(false, "cargo build", Some("error[E0308]: mismatched types")),
            )
            .unwrap();
        }
        // Different error 1 time
        ingest_insight(
            &wiki_dir,
            sample_insight(false, "cargo build", Some("error[E0412]: cannot find type")),
        )
        .unwrap();

        let learnings = distill_learnings(&wiki_dir);
        assert!(!learnings.is_empty(), "should promote repeated pattern");
        assert!(learnings.iter().any(|l| l.occurrences >= 3));
    }

    #[test]
    fn empty_wiki_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");

        assert!(load_all_insights(&wiki_dir).is_empty());
        assert!(query_insights(&wiki_dir, "auth", 10).is_empty());
        let ops = aggregate_for_module(&wiki_dir, "auth");
        assert_eq!(ops.insight_count, 0);
    }

    // --- S3-T4: WAL and archival tests ---

    #[test]
    fn promotion_wal_append_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");

        let entry = PromotionEntry {
            timestamp: 1000,
            action: PromotionAction::Promoted,
            source_path: "cache/auth-flow.md".into(),
            target_tier: "promoted".into(),
            reason: "validated by human".into(),
        };
        append_promotion(&wiki_dir, entry.clone()).unwrap();
        append_promotion(
            &wiki_dir,
            PromotionEntry {
                timestamp: 2000,
                action: PromotionAction::Archived,
                source_path: "cache/old.md".into(),
                target_tier: "archive".into(),
                reason: "TTL expired".into(),
            },
        )
        .unwrap();

        let entries = load_promotions(&wiki_dir);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].source_path, "cache/auth-flow.md");
        assert_eq!(entries[1].source_path, "cache/old.md");
    }

    #[test]
    fn validate_wal_detects_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");
        let runtime_dir = wiki_dir.join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();

        // Write valid + corrupted lines
        let wal_path = runtime_dir.join("promotions.jsonl");
        let valid = serde_json::to_string(&PromotionEntry {
            timestamp: 1000,
            action: PromotionAction::Promoted,
            source_path: "test.md".into(),
            target_tier: "promoted".into(),
            reason: "test".into(),
        })
        .unwrap();
        std::fs::write(
            &wal_path,
            format!("{}\n{{invalid json}}\n{}\n", valid, valid),
        )
        .unwrap();

        let (valid_count, corrupted) = validate_wal(&wiki_dir);
        assert_eq!(valid_count, 2);
        assert_eq!(corrupted, 1);
    }

    #[test]
    fn archive_old_insights_moves_old_entries() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");

        // Insert insights: one very old, one recent
        let mut old_insight = sample_insight(true, "cargo test old", None);
        old_insight.timestamp = 1000; // very old (epoch + 1s)
        ingest_insight(&wiki_dir, old_insight).unwrap();

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let mut recent_insight = sample_insight(true, "cargo test recent", None);
        recent_insight.timestamp = now_ms; // just now
        ingest_insight(&wiki_dir, recent_insight).unwrap();

        // Archive entries older than 1 hour
        let archived = archive_old_insights(&wiki_dir, 3600).unwrap();
        assert_eq!(archived, 1, "Should archive 1 old insight");

        // Verify only recent remains
        let remaining = load_all_insights(&wiki_dir);
        assert_eq!(remaining.len(), 1, "Only recent insight should remain");
        assert_eq!(remaining[0].command, "cargo test recent");

        // Verify archive file exists
        let archive_dir = wiki_dir.join("runtime").join("archive");
        let archives: Vec<_> = std::fs::read_dir(&archive_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(archives.len(), 1, "Archive file should exist");
    }

    #[test]
    fn archive_empty_insights_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");
        let archived = archive_old_insights(&wiki_dir, 3600).unwrap();
        assert_eq!(archived, 0);
    }

    // --- P1-T1: Promotion policy tests ---

    #[test]
    fn evaluate_promotion_promotes_with_references() {
        let action = evaluate_promotion(&["community:auth".into()], false, 0.5);
        assert!(matches!(action, PromotionAction::Promoted));
    }

    #[test]
    fn evaluate_promotion_promotes_with_workspace_constraints() {
        let action = evaluate_promotion(&[], true, 0.5);
        assert!(matches!(action, PromotionAction::Promoted));
    }

    #[test]
    fn evaluate_promotion_evicts_without_signal() {
        let action = evaluate_promotion(&[], false, 0.5);
        assert!(matches!(action, PromotionAction::Evicted));
    }

    // --- P1-T2: Hard limits + health check tests ---

    #[test]
    fn enforce_limits_on_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");
        let report = enforce_limits(&wiki_dir, &OperationalLimits::default()).unwrap();
        assert!(!report.raw_events_rotated);
        assert_eq!(report.summaries_archived, 0);
    }

    #[test]
    fn health_check_empty_is_healthy() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");
        let health = check_health(&wiki_dir, &OperationalLimits::default());
        assert!(health.is_healthy);
        assert!(health.warnings.is_empty());
        assert_eq!(health.raw_events_bytes, 0);
    }

    #[test]
    fn health_check_warns_near_limit() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");
        let runtime_dir = wiki_dir.join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();

        // Write enough data to trigger 80% warning (tiny limit for test)
        let jsonl_path = runtime_dir.join("insights.jsonl");
        std::fs::write(&jsonl_path, "x".repeat(900)).unwrap(); // 900 bytes

        let limits = OperationalLimits {
            max_raw_event_bytes: 1000, // 1KB limit
            max_active_summaries: 10,
            archival_ttl_days: 30,
        };
        let health = check_health(&wiki_dir, &limits);
        assert!(!health.is_healthy, "Should warn at 90% capacity");
        assert!(!health.warnings.is_empty());
    }

    #[test]
    fn enforce_limits_rotates_when_over_size() {
        let dir = tempfile::tempdir().unwrap();
        let wiki_dir = dir.path().join(".theo").join("wiki");
        let runtime_dir = wiki_dir.join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();

        // Write data exceeding tiny limit
        let jsonl_path = runtime_dir.join("insights.jsonl");
        let mut content = String::new();
        for i in 0..20 {
            content.push_str(&format!("line {}\n", i));
        }
        std::fs::write(&jsonl_path, &content).unwrap();

        let limits = OperationalLimits {
            max_raw_event_bytes: 50, // tiny limit
            max_active_summaries: 500,
            archival_ttl_days: 30,
        };
        let report = enforce_limits(&wiki_dir, &limits).unwrap();
        assert!(report.raw_events_rotated, "Should rotate when over limit");

        // Verify file is smaller now
        let after = std::fs::read_to_string(&jsonl_path).unwrap();
        assert!(
            after.lines().count() < 20,
            "Should have fewer lines after rotation"
        );
    }
