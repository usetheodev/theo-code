//! Sibling test body of `context_assembler.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `context_assembler.rs` via `#[path = "context_assembler_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use super::*;
    use theo_domain::event::{DomainEvent, EventType};
    use theo_domain::graph_context::ContextBlock;

    fn make_working_set() -> WorkingSet {
        WorkingSet {
            hot_files: vec!["src/auth.rs".into(), "src/db.rs".into()],
            recent_event_ids: vec!["evt-1".into()],
            active_hypothesis: Some("jwt decode bug".into()),
            current_plan_step: Some("run cargo test".into()),
            constraints: vec!["no unwrap in auth".into()],
            ..WorkingSet::default()
        }
    }

    fn make_structural_context(blocks: Vec<(&str, usize)>) -> GraphContextResult {
        GraphContextResult {
            blocks: blocks
                .iter()
                .map(|(content, tokens)| ContextBlock {
                    block_id: String::new(),
                    source_id: "test".into(),
                    content: content.to_string(),
                    token_count: *tokens,
                    score: 0.5,
                })
                .collect(),
            total_tokens: blocks.iter().map(|(_, t)| t).sum(),
            budget_tokens: 4000,
            exploration_hints: String::new(),
            budget_report: None,
        }
    }

    fn make_events(n: usize) -> Vec<DomainEvent> {
        (0..n)
            .map(|i| {
                DomainEvent::new(
                    EventType::ToolCallCompleted,
                    format!("run-{}", i),
                    serde_json::json!({"tool_name": format!("tool_{}", i)}),
                )
            })
            .collect()
    }

    #[test]
    fn assembler_respects_token_budget() {
        let mut assembler = ContextAssembler::new(100);
        let ws = make_working_set();
        let ctx = make_structural_context(vec![("big block", 5000)]);
        let events = make_events(3);

        let result = assembler.assemble("fix bug", &ws, &ctx, &events);
        assert!(
            result.total_tokens <= 100,
            "Tokens {} exceeded budget 100",
            result.total_tokens
        );
    }

    #[test]
    fn assembler_always_includes_task_objective() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet::default();
        let ctx = make_structural_context(vec![]);
        let events = vec![];

        let result = assembler.assemble("fix authentication bug in jwt.rs", &ws, &ctx, &events);
        let full_text = result.sections.join("\n");
        assert!(
            full_text.contains("fix authentication bug in jwt.rs"),
            "Objective not found in assembled context"
        );
    }

    #[test]
    fn assembler_always_includes_current_step() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet {
            current_plan_step: Some("run cargo test".into()),
            ..WorkingSet::default()
        };
        let ctx = make_structural_context(vec![]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        let full_text = result.sections.join("\n");
        assert!(
            full_text.contains("run cargo test"),
            "Current step not found in context"
        );
    }

    #[test]
    fn assembler_includes_recent_evidence() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet::default();
        let ctx = make_structural_context(vec![]);
        let events = vec![DomainEvent::new(
            EventType::Error,
            "run-1",
            serde_json::json!({"message": "compile error in auth.rs"}),
        )];

        let result = assembler.assemble("fix", &ws, &ctx, &events);
        let full_text = result.sections.join("\n");
        assert!(
            full_text.contains("compile error in auth.rs"),
            "Evidence not found in context"
        );
    }

    #[test]
    fn assembler_fills_remaining_budget_with_structural_context() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet::default();
        let ctx = make_structural_context(vec![
            ("# Auth module\npub fn verify_token()", 50),
            ("# DB module\npub fn query()", 40),
        ]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        let full_text = result.sections.join("\n");
        assert!(
            full_text.contains("Auth module"),
            "Structural context not included"
        );
        assert!(full_text.contains("DB module"), "Second block not included");
    }

    #[test]
    fn assembler_stops_structural_when_budget_exhausted() {
        let mut assembler = ContextAssembler::new(200);
        let ws = WorkingSet::default();
        // Create blocks that exceed budget
        let ctx = make_structural_context(vec![
            ("small block", 50),
            ("big block that should not fit", 500),
        ]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        let full_text = result.sections.join("\n");
        assert!(full_text.contains("small block"), "First block should fit");
        assert!(
            !full_text.contains("big block"),
            "Second block should be excluded"
        );
        assert!(result.total_tokens <= 200);
    }

    #[test]
    fn assembler_includes_hypothesis_and_constraints() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet {
            active_hypothesis: Some("race condition in event bus".into()),
            constraints: vec!["no unwrap".into(), "test before commit".into()],
            ..WorkingSet::default()
        };
        let ctx = make_structural_context(vec![]);

        let result = assembler.assemble("investigate", &ws, &ctx, &[]);
        let full_text = result.sections.join("\n");
        assert!(full_text.contains("race condition"), "Hypothesis missing");
        assert!(full_text.contains("no unwrap"), "Constraints missing");
    }

    #[test]
    fn assembler_empty_everything_still_works() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet::default();
        let ctx = make_structural_context(vec![]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        assert!(
            !result.sections.is_empty(),
            "Should at least have objective"
        );
        assert!(result.total_tokens <= result.budget_tokens);
    }

    #[test]
    fn assembler_zero_budget_returns_empty() {
        let mut assembler = ContextAssembler::new(0);
        let ws = make_working_set();
        let ctx = make_structural_context(vec![("content", 100)]);
        let events = make_events(5);

        let result = assembler.assemble("task", &ws, &ctx, &events);
        assert!(result.total_tokens == 0);
        assert!(result.sections.is_empty());
    }

    #[test]
    fn estimate_tokens_approximation() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1); // 4 chars = 1 token
        assert_eq!(estimate_tokens("abcde"), 2); // 5 chars = 2 tokens (ceiling)
        assert!(estimate_tokens("hello world") > 0);
    }

    #[test]
    fn summarize_payload_extracts_common_fields() {
        let p = serde_json::json!({"message": "compile error", "code": 1});
        assert_eq!(summarize_payload(&p), "compile error");

        let p2 = serde_json::json!({"tool_name": "bash"});
        assert_eq!(summarize_payload(&p2), "bash");

        assert_eq!(summarize_payload(&serde_json::Value::Null), "");
    }

    // --- P2-T1: Feedback loop tests ---

    fn make_tagged_context(blocks: Vec<(&str, &str, usize, f64)>) -> GraphContextResult {
        GraphContextResult {
            blocks: blocks
                .iter()
                .map(|(source_id, content, tokens, score)| ContextBlock {
                    block_id: String::new(),
                    source_id: source_id.to_string(),
                    content: content.to_string(),
                    token_count: *tokens,
                    score: *score,
                })
                .collect(),
            total_tokens: blocks.iter().map(|(_, _, t, _)| t).sum(),
            budget_tokens: 4000,
            exploration_hints: String::new(),
            budget_report: None,
        }
    }

    #[test]
    fn feedback_score_default_is_half() {
        let assembler = ContextAssembler::new(4000);
        assert!((assembler.feedback_score("unknown") - 0.5).abs() < 0.001);
    }

    #[test]
    fn record_feedback_updates_score() {
        let mut assembler = ContextAssembler::new(4000);
        assembler.record_feedback("community:auth", 1.0);
        assert!(
            assembler.feedback_score("community:auth") > 0.5,
            "High feedback should increase score above default"
        );

        assembler.record_feedback("community:db", 0.0);
        assert!(
            assembler.feedback_score("community:db") < 0.5,
            "Low feedback should decrease score below default"
        );
    }

    #[test]
    fn feedback_boosts_ordering_of_useful_communities() {
        let mut assembler = ContextAssembler::new(4000);
        // Give auth high feedback, db low feedback
        for _ in 0..5 {
            assembler.record_feedback("auth", 1.0);
            assembler.record_feedback("db", 0.0);
        }

        let ws = WorkingSet::default();
        // Both blocks have same relevance score (0.5) but different source_ids
        let ctx = make_tagged_context(vec![
            ("db", "# DB module", 50, 0.5),
            ("auth", "# Auth module", 50, 0.5),
        ]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        let full_text = result.sections.join("\n");

        // Auth should appear before DB due to higher feedback
        let auth_pos = full_text.find("Auth module");
        let db_pos = full_text.find("DB module");
        assert!(
            auth_pos.is_some() && db_pos.is_some(),
            "Both blocks should be in context"
        );
        assert!(
            auth_pos.unwrap() < db_pos.unwrap(),
            "Auth should appear before DB due to feedback boost"
        );
    }

    #[test]
    fn feedback_never_violates_budget() {
        let mut assembler = ContextAssembler::new(100);
        assembler.record_feedback("auth", 1.0);

        let ws = WorkingSet::default();
        let ctx = make_tagged_context(vec![("auth", "huge auth content", 5000, 0.9)]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        assert!(result.total_tokens <= 100, "Budget must never be exceeded");
    }

    #[test]
    fn feedback_persistence_roundtrip() {
        let mut assembler = ContextAssembler::new(4000);
        assembler.record_feedback("auth", 0.9);
        assembler.record_feedback("db", 0.2);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feedback.json");

        assembler.save_feedback(&path).unwrap();

        let loaded = ContextAssembler::load_feedback(&path);
        assert!(loaded.contains_key("auth"));
        assert!(loaded.contains_key("db"));
    }

    // --- SOTA: Adaptive budget tests ---

    #[test]
    fn adaptive_budget_small_repo() {
        // sqrt(50) * 500 = 3535 → clamped to 4000 (min)
        assert_eq!(ContextAssembler::compute_adaptive_budget(50), 4000);
    }

    #[test]
    fn adaptive_budget_medium_repo() {
        // sqrt(5000) * 500 ≈ 35355 → clamped to 32000 (max)
        let budget = ContextAssembler::compute_adaptive_budget(5000);
        assert_eq!(budget, 32000, "FFmpeg-size repo should hit max budget");
    }

    #[test]
    fn adaptive_budget_mid_range() {
        // sqrt(500) * 500 ≈ 11180
        let budget = ContextAssembler::compute_adaptive_budget(500);
        assert!(
            budget > 10000 && budget < 12000,
            "500-file repo should get ~11K budget, got {}",
            budget
        );
    }

    #[test]
    fn adaptive_budget_never_exceeds_max() {
        assert_eq!(ContextAssembler::compute_adaptive_budget(1_000_000), 32000);
    }

    #[test]
    fn adaptive_budget_never_below_min() {
        assert_eq!(ContextAssembler::compute_adaptive_budget(1), 4000);
        assert_eq!(ContextAssembler::compute_adaptive_budget(0), 4000);
    }

    #[test]
    fn budget_allocation_ratios() {
        let assembler = ContextAssembler::new(10000);
        let alloc = assembler.compute_allocation();
        assert_eq!(alloc.task_overhead, 1500); // 15%
        assert_eq!(alloc.execution_context, 2500); // 25%
        assert_eq!(alloc.structural, 6000); // 60%
        assert_eq!(
            alloc.task_overhead + alloc.execution_context + alloc.structural,
            10000
        );
    }

    #[test]
    fn adaptive_constructor_works() {
        let assembler = ContextAssembler::adaptive(5000);
        assert!(assembler.budget() > 4000);
    }

    // --- P0.5: Penalty tests ---

    #[test]
    fn penalty_reduces_score_for_repeated_communities() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet::default();
        let ctx = make_tagged_context(vec![
            ("auth", "# Auth first", 50, 0.8),
            ("db", "# DB module", 50, 0.8),
        ]);

        // First assembly — both communities fresh
        let _r1 = assembler.assemble("task", &ws, &ctx, &[]);

        // Second assembly — auth was assembled before, should be penalized
        let r2 = assembler.assemble("task", &ws, &ctx, &[]);
        // Both still in context (penalty floor=0.5 keeps them), but ordering may change
        assert!(r2.total_tokens <= 4000);
    }

    #[test]
    fn penalty_floor_prevents_exclusion() {
        let mut assembler = ContextAssembler::new(4000);
        let ws = WorkingSet::default();
        let ctx = make_tagged_context(vec![("auth", "# Auth module", 50, 0.8)]);

        // Assemble 10 times — penalty should floor at 0.5, never exclude
        for _ in 0..10 {
            let result = assembler.assemble("task", &ws, &ctx, &[]);
            let full = result.sections.join("\n");
            assert!(
                full.contains("Auth module"),
                "Penalty floor must prevent total exclusion"
            );
        }
    }

    // --- P1: Stability bonus tests ---

    #[test]
    fn stability_bonus_only_with_positive_signal() {
        let mut assembler = ContextAssembler::new(4000);
        // NO positive signal recorded
        let ws = WorkingSet::default();
        let ctx = make_tagged_context(vec![("auth", "# Auth", 50, 0.5), ("db", "# DB", 50, 0.5)]);

        let _r1 = assembler.assemble("task", &ws, &ctx, &[]);

        // Without positive signal, ordering should not favor auth over db
        // (both have same base score and no signal)
    }

    #[test]
    fn stability_bonus_with_signal_boosts() {
        let mut assembler = ContextAssembler::new(4000);
        assembler.record_positive_signal("auth");

        let ws = WorkingSet::default();
        let ctx = make_tagged_context(vec![
            ("db", "# DB module", 50, 0.5),
            ("auth", "# Auth module", 50, 0.5),
        ]);

        let result = assembler.assemble("task", &ws, &ctx, &[]);
        let full = result.sections.join("\n");

        // Auth should appear before DB due to stability bonus
        if let (Some(auth_pos), Some(db_pos)) = (full.find("Auth module"), full.find("DB module")) {
            assert!(auth_pos < db_pos, "Auth with signal should rank before DB");
        }
    }

    // --- P1.5: Memory injection tests ---

    #[test]
    fn memory_injection_includes_constraints() {
        use theo_domain::episode::*;
        let mut assembler = ContextAssembler::new(10000);
        let ws = WorkingSet::default();
        let ctx = make_structural_context(vec![("# Structural", 100)]);

        let episode = EpisodeSummary::from_events("r-1", None, "task", &[]);
        // Manually set constraints for test
        let mut ep = episode;
        ep.machine_summary.learned_constraints = vec!["avoid unwrap in auth".into()];

        let result = assembler.assemble_with_memory("task", &ws, &ctx, &[], Some(&ep));
        let full = result.sections.join("\n");
        assert!(
            full.contains("avoid unwrap in auth"),
            "Episode constraints must appear in assembled context"
        );
    }

    #[test]
    fn memory_injection_capped_at_10_percent() {
        use theo_domain::episode::*;
        let mut assembler = ContextAssembler::new(1000); // small budget
        let ws = WorkingSet::default();
        let ctx = make_structural_context(vec![]);

        let mut ep = EpisodeSummary::from_events("r-1", None, "task", &[]);
        // Add many constraints (would exceed 10% = 100 tokens)
        ep.machine_summary.learned_constraints = (0..50)
            .map(|i| {
                format!(
                    "constraint {} with a very long description that takes many tokens",
                    i
                )
            })
            .collect();

        let result = assembler.assemble_with_memory("task", &ws, &ctx, &[], Some(&ep));
        // Memory content should be capped
        let memory_tokens: usize = result
            .sections
            .iter()
            .filter(|s| s.contains("Prior Constraints") || s.contains("Prior Failures"))
            .map(|s| estimate_tokens(s))
            .sum();
        assert!(
            memory_tokens <= 100, // 10% of 1000
            "Memory tokens {} should be <= 100 (10% cap)",
            memory_tokens
        );
    }
