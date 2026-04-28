//! Sibling test body of `episode.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `episode.rs` via `#[path = "episode_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use super::*;
    use crate::event::{DomainEvent, EventType};
    use crate::identifiers::EventId;

    fn make_event(event_type: EventType, payload: serde_json::Value) -> DomainEvent {
        DomainEvent::new(event_type, "run-1", payload)
    }

    #[test]
    fn episode_summary_created_from_events() {
        // Arrange
        let events = vec![
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({"tool_name": "read", "file": "src/lib.rs"}),
            ),
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({"tool_name": "edit", "file": "src/lib.rs"}),
            ),
            make_event(
                EventType::ConstraintLearned,
                serde_json::json!({"constraint": "no unwrap in auth", "scope": "workspace-local"}),
            ),
        ];

        // Act
        let summary = EpisodeSummary::from_events("run-1", Some("task-1"), "fix auth bug", &events);

        // Assert
        assert_eq!(summary.run_id, "run-1");
        assert_eq!(summary.task_id, Some("task-1".to_string()));
        assert_eq!(summary.evidence_event_ids.len(), 3);
        assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(summary.machine_summary.objective, "fix auth bug");
        assert_eq!(summary.machine_summary.key_actions.len(), 2);
        assert!(
            summary
                .machine_summary
                .learned_constraints
                .contains(&"no unwrap in auth".to_string())
        );
        assert_eq!(summary.machine_summary.outcome, EpisodeOutcome::Success);
    }

    #[test]
    fn episode_summary_machine_part_has_structured_fields() {
        let events = vec![make_event(
            EventType::ToolCallCompleted,
            serde_json::json!({"tool_name": "bash"}),
        )];
        let summary = EpisodeSummary::from_events("r-1", None, "test", &events);

        assert!(!summary.machine_summary.objective.is_empty());
        assert_eq!(summary.machine_summary.outcome, EpisodeOutcome::Success);
    }

    #[test]
    fn episode_summary_serde_roundtrip() {
        let events = vec![make_event(
            EventType::RunStateChanged,
            serde_json::json!({"from": "Planning", "to": "Executing"}),
        )];
        let summary = EpisodeSummary::from_events("r-1", None, "plan", &events);

        let json = serde_json::to_string(&summary).unwrap();
        let restored: EpisodeSummary = serde_json::from_str(&json).unwrap();

        assert_eq!(summary.summary_id, restored.summary_id);
        assert_eq!(summary.schema_version, restored.schema_version);
        assert_eq!(summary.run_id, restored.run_id);
        assert_eq!(summary.ttl_policy, restored.ttl_policy);
    }

    #[test]
    fn episode_summary_detects_partial_outcome_on_errors() {
        let events = vec![
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({"tool_name": "edit"}),
            ),
            make_event(
                EventType::Error,
                serde_json::json!({"message": "compile error"}),
            ),
        ];
        let summary = EpisodeSummary::from_events("r-1", None, "fix bug", &events);

        assert_eq!(summary.machine_summary.outcome, EpisodeOutcome::Partial);
    }

    #[test]
    fn episode_summary_tracks_unresolved_hypotheses() {
        let h1 = make_event(
            EventType::HypothesisFormed,
            serde_json::json!({
                "hypothesis": "bug in jwt.rs", "rationale": "test fails"
            }),
        );
        let h1_id = h1.event_id.as_str().to_string();

        let h2 = make_event(
            EventType::HypothesisFormed,
            serde_json::json!({
                "hypothesis": "race condition", "rationale": "flaky test"
            }),
        );

        let invalidation = DomainEvent {
            event_id: EventId::generate(),
            event_type: EventType::HypothesisInvalidated,
            entity_id: "run-1".into(),
            timestamp: 1000,
            payload: serde_json::json!({"prior_event_id": h1_id, "reason": "test passed after revert"}),
            supersedes_event_id: Some(h1.event_id.clone()),
        };

        let events = vec![h1, h2, invalidation];
        let summary = EpisodeSummary::from_events("r-1", None, "investigate", &events);

        // h1 was invalidated, h2 remains unresolved
        assert_eq!(summary.unresolved_hypotheses.len(), 1);
        assert_eq!(summary.unresolved_hypotheses[0], "race condition");
    }

    #[test]
    fn episode_summary_empty_events() {
        let summary = EpisodeSummary::from_events("r-1", None, "empty", &[]);
        assert!(summary.evidence_event_ids.is_empty());
        assert!(summary.machine_summary.key_actions.is_empty());
        assert_eq!(summary.window_start_event_id, "");
        assert_eq!(summary.window_end_event_id, "");
    }

    #[test]
    fn ttl_policy_default_is_run_scoped() {
        assert_eq!(TtlPolicy::default(), TtlPolicy::RunScoped);
    }

    #[test]
    fn ttl_policy_serde_roundtrip() {
        for policy in &[
            TtlPolicy::RunScoped,
            TtlPolicy::TimeScoped { seconds: 3600 },
            TtlPolicy::Permanent,
        ] {
            let json = serde_json::to_string(policy).unwrap();
            let back: TtlPolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(*policy, back);
        }
    }

    #[test]
    fn episode_outcome_serde_roundtrip() {
        for outcome in &[
            EpisodeOutcome::Success,
            EpisodeOutcome::Failure,
            EpisodeOutcome::Partial,
            EpisodeOutcome::Inconclusive,
        ] {
            let json = serde_json::to_string(outcome).unwrap();
            let back: EpisodeOutcome = serde_json::from_str(&json).unwrap();
            assert_eq!(*outcome, back);
        }
    }

    // --- P-1 BF1: TTL promotion tests ---

    #[test]
    fn ttl_promoted_to_permanent_when_workspace_constraint() {
        let events = vec![make_event(
            EventType::ConstraintLearned,
            serde_json::json!({
                "constraint": "no unwrap in auth", "scope": "workspace-local"
            }),
        )];
        let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
        assert_eq!(
            summary.ttl_policy,
            TtlPolicy::Permanent,
            "Workspace constraints must survive run end"
        );
    }

    #[test]
    fn ttl_stays_run_scoped_when_only_run_local() {
        let events = vec![make_event(
            EventType::ConstraintLearned,
            serde_json::json!({
                "constraint": "retry 3 times", "scope": "run-local"
            }),
        )];
        let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
        assert_eq!(summary.ttl_policy, TtlPolicy::RunScoped);
    }

    #[test]
    fn ttl_time_scoped_when_task_local() {
        let events = vec![make_event(
            EventType::ConstraintLearned,
            serde_json::json!({
                "constraint": "auth module fragile", "scope": "task-local"
            }),
        )];
        let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
        assert_eq!(summary.ttl_policy, TtlPolicy::TimeScoped { seconds: 86400 });
    }

    // --- P-1 BF3: successful_steps / failed_attempts ---

    #[test]
    fn from_events_populates_successful_steps() {
        let events = vec![
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({
                    "tool_name": "edit", "file": "src/auth.rs", "success": true
                }),
            ),
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({
                    "tool_name": "bash", "success": true
                }),
            ),
        ];
        let summary = EpisodeSummary::from_events("r-1", None, "fix", &events);
        assert!(
            !summary.machine_summary.successful_steps.is_empty(),
            "Should extract successful tool calls"
        );
    }

    #[test]
    fn from_events_populates_failed_attempts() {
        let events = vec![
            make_event(
                EventType::Error,
                serde_json::json!({"message": "compile error"}),
            ),
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({
                    "tool_name": "edit", "success": false, "error": "file not found"
                }),
            ),
        ];
        let summary = EpisodeSummary::from_events("r-1", None, "fix", &events);
        assert!(
            !summary.machine_summary.failed_attempts.is_empty(),
            "Should extract failures"
        );
    }

    #[test]
    fn from_events_separates_success_from_failure() {
        let events = vec![
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({
                    "tool_name": "read", "success": true, "file": "src/a.rs"
                }),
            ),
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({
                    "tool_name": "edit", "success": false, "error": "permission denied"
                }),
            ),
        ];
        let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
        assert_eq!(summary.machine_summary.successful_steps.len(), 1);
        assert_eq!(summary.machine_summary.failed_attempts.len(), 1);
    }

    // --- P0.5: MemoryLifecycle tests ---

    #[test]
    fn lifecycle_defaults_to_active() {
        let summary = EpisodeSummary::from_events("r-1", None, "task", &[]);
        assert_eq!(summary.lifecycle, MemoryLifecycle::Active);
    }

    #[test]
    fn lifecycle_serde_roundtrip() {
        for lc in &[
            MemoryLifecycle::Active,
            MemoryLifecycle::Cooling,
            MemoryLifecycle::Archived,
        ] {
            let json = serde_json::to_string(lc).unwrap();
            let back: MemoryLifecycle = serde_json::from_str(&json).unwrap();
            assert_eq!(*lc, back);
        }
    }

    #[test]
    fn lifecycle_active_eligible_for_assembly() {
        assert!(MemoryLifecycle::Active.eligible_for_assembly());
        assert!(!MemoryLifecycle::Cooling.eligible_for_assembly());
        assert!(!MemoryLifecycle::Archived.eligible_for_assembly());
    }

    #[test]
    fn lifecycle_cooling_requires_gate() {
        assert!(!MemoryLifecycle::Active.requires_usefulness_gate());
        assert!(MemoryLifecycle::Cooling.requires_usefulness_gate());
        assert!(!MemoryLifecycle::Archived.requires_usefulness_gate());
    }

    #[test]
    fn lifecycle_transitions() {
        assert_eq!(MemoryLifecycle::Active.next(), MemoryLifecycle::Cooling);
        assert_eq!(MemoryLifecycle::Cooling.next(), MemoryLifecycle::Archived);
        assert_eq!(MemoryLifecycle::Archived.next(), MemoryLifecycle::Archived);
    }

    #[test]
    fn lifecycle_backward_compat() {
        let mut val =
            serde_json::to_value(EpisodeSummary::from_events("r-1", None, "t", &[])).unwrap();
        val.as_object_mut().unwrap().remove("lifecycle");
        let back: EpisodeSummary = serde_json::from_value(val).unwrap();
        assert_eq!(back.lifecycle, MemoryLifecycle::Active);
    }

    // --- P2: Hypothesis tests ---

    #[test]
    fn hypothesis_new_default_confidence() {
        let h = Hypothesis::new("h-1", "jwt bug", "test fails");
        assert_eq!(h.confidence, 0.5);
        assert_eq!(h.status, HypothesisStatus::Active);
        assert_eq!(h.source, HypothesisSource::Explicit);
    }

    #[test]
    fn hypothesis_inferred_low_confidence() {
        let h = Hypothesis::inferred("h-2", "repeated edit", "pattern detected");
        assert_eq!(h.confidence, 0.3);
        assert_eq!(h.source, HypothesisSource::Inferred);
    }

    #[test]
    fn hypothesis_degrades_to_stale() {
        let mut h = Hypothesis::new("h-1", "bug", "reason");
        h.mark_stale();
        assert_eq!(h.status, HypothesisStatus::Stale);
        assert!(!h.is_eligible_for_assembly());
    }

    #[test]
    fn hypothesis_superseded_not_eligible() {
        let mut h = Hypothesis::new("h-1", "bug", "reason");
        h.supersede("h-2");
        assert_eq!(h.status, HypothesisStatus::Superseded);
        assert_eq!(h.superseded_by, Some("h-2".to_string()));
        assert!(!h.is_eligible_for_assembly());
    }

    #[test]
    fn hypothesis_serde_roundtrip() {
        let h = Hypothesis::new("h-1", "test", "reason");
        let json = serde_json::to_string(&h).unwrap();
        let back: Hypothesis = serde_json::from_str(&json).unwrap();
        assert_eq!(h.id, back.id);
        assert_eq!(h.status, back.status);
        assert_eq!(h.confidence, back.confidence);
    }

    // --- P1: Failure learning tests ---

    #[test]
    fn recurring_error_generates_constraint() {
        let events = vec![
            make_event(
                EventType::Error,
                serde_json::json!({"message": "file not found"}),
            ),
            make_event(
                EventType::Error,
                serde_json::json!({"message": "file not found"}),
            ),
            make_event(
                EventType::Error,
                serde_json::json!({"message": "file not found"}),
            ),
        ];
        let constraints = extract_failure_constraints(&events, 3);
        assert!(
            !constraints.is_empty(),
            "Should generate constraint for recurring error"
        );
    }

    #[test]
    fn isolated_error_no_constraint() {
        let events = vec![make_event(
            EventType::Error,
            serde_json::json!({"message": "timeout"}),
        )];
        let constraints = extract_failure_constraints(&events, 3);
        assert!(constraints.is_empty());
    }

    #[test]
    fn from_events_includes_failure_constraints() {
        let events = vec![
            make_event(
                EventType::Error,
                serde_json::json!({"message": "compile error"}),
            ),
            make_event(
                EventType::Error,
                serde_json::json!({"message": "compile error"}),
            ),
            make_event(
                EventType::Error,
                serde_json::json!({"message": "compile error"}),
            ),
        ];
        let summary = EpisodeSummary::from_events("r-1", None, "fix", &events);
        assert!(
            summary
                .machine_summary
                .learned_constraints
                .iter()
                .any(|c| c.contains("compile error")),
            "Should include failure-derived constraint in learned_constraints"
        );
    }

    // --- MemoryKind tests ---

    #[test]
    fn memory_kind_default_is_episodic() {
        assert_eq!(MemoryKind::default(), MemoryKind::Episodic);
    }

    #[test]
    fn memory_kind_survives_compaction() {
        assert!(!MemoryKind::Ephemeral.survives_compaction());
        assert!(!MemoryKind::Episodic.survives_compaction());
        assert!(MemoryKind::Reusable.survives_compaction());
        assert!(MemoryKind::Canonical.survives_compaction());
    }

    #[test]
    fn memory_kind_auto_evictable() {
        assert!(MemoryKind::Ephemeral.auto_evictable());
        assert!(MemoryKind::Episodic.auto_evictable());
        assert!(!MemoryKind::Reusable.auto_evictable());
        assert!(!MemoryKind::Canonical.auto_evictable());
    }

    #[test]
    fn memory_kind_serde_roundtrip() {
        for kind in &[
            MemoryKind::Ephemeral,
            MemoryKind::Episodic,
            MemoryKind::Reusable,
            MemoryKind::Canonical,
        ] {
            let json = serde_json::to_string(kind).unwrap();
            let back: MemoryKind = serde_json::from_str(&json).unwrap();
            assert_eq!(*kind, back);
        }
    }

    #[test]
    fn memory_kind_inferred_from_ttl() {
        assert_eq!(
            infer_memory_kind(&TtlPolicy::RunScoped),
            MemoryKind::Episodic
        );
        assert_eq!(
            infer_memory_kind(&TtlPolicy::TimeScoped { seconds: 3600 }),
            MemoryKind::Reusable
        );
        assert_eq!(
            infer_memory_kind(&TtlPolicy::Permanent),
            MemoryKind::Canonical
        );
    }

    #[test]
    fn episode_summary_has_memory_kind() {
        let events = vec![make_event(
            EventType::ConstraintLearned,
            serde_json::json!({"constraint": "no unwrap", "scope": "workspace-local"}),
        )];
        let summary = EpisodeSummary::from_events("r-1", None, "task", &events);
        assert_eq!(summary.memory_kind, MemoryKind::Canonical);
    }

    #[test]
    fn episode_summary_default_memory_kind_is_episodic() {
        let summary = EpisodeSummary::from_events("r-1", None, "task", &[]);
        assert_eq!(summary.memory_kind, MemoryKind::Episodic);
    }

    #[test]
    fn memory_kind_backward_compat_deserialization() {
        let mut val =
            serde_json::to_value(EpisodeSummary::from_events("r-1", None, "t", &[])).unwrap();
        val.as_object_mut().unwrap().remove("memory_kind");
        let back: EpisodeSummary = serde_json::from_value(val).unwrap();
        assert_eq!(back.memory_kind, MemoryKind::Episodic);
    }

    // --- Hypothesis evidence tracking tests ---

    #[test]
    fn hypothesis_record_support_increases_confidence() {
        let mut h = Hypothesis::new("h-1", "bug in auth", "test fails");
        let initial = h.confidence;
        h.record_support("evt-1");
        assert!(h.confidence > initial);
        assert_eq!(h.evidence_for, 1);
        assert_eq!(h.evidence_against, 0);
    }

    #[test]
    fn hypothesis_record_contradiction_decreases_confidence() {
        let mut h = Hypothesis::new("h-1", "bug in auth", "test fails");
        let initial = h.confidence;
        h.record_contradiction("evt-1");
        assert!(h.confidence < initial);
        assert_eq!(h.evidence_for, 0);
        assert_eq!(h.evidence_against, 1);
    }

    #[test]
    fn hypothesis_auto_prunes_on_heavy_contradiction() {
        let mut h = Hypothesis::new("h-1", "bug", "reason");
        // 0 for, 3 against → should auto-prune (3 > 0*2, total >= 3)
        h.record_contradiction("evt-1");
        h.record_contradiction("evt-2");
        assert_eq!(h.status, HypothesisStatus::Active); // not yet
        h.record_contradiction("evt-3");
        assert_eq!(h.status, HypothesisStatus::Stale); // auto-pruned
    }

    #[test]
    fn hypothesis_no_prune_with_balanced_evidence() {
        let mut h = Hypothesis::new("h-1", "bug", "reason");
        h.record_support("evt-1");
        h.record_support("evt-2");
        h.record_contradiction("evt-3");
        h.record_contradiction("evt-4");
        // 2 for, 2 against: 2 > 2*2=4? No → should NOT prune
        assert_eq!(h.status, HypothesisStatus::Active);
    }

    #[test]
    fn hypothesis_confidence_with_laplace_smoothing() {
        let h = Hypothesis::new("h-1", "test", "reason");
        // No evidence: (0+1)/(0+0+2) = 0.5
        assert!((h.confidence - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn hypothesis_evidence_backward_compat() {
        let h = Hypothesis::new("h-1", "test", "reason");
        let json = serde_json::to_string(&h).unwrap();
        // Remove the new fields to simulate old data
        let mut val: serde_json::Value = serde_json::from_str(&json).unwrap();
        val.as_object_mut().unwrap().remove("evidence_for");
        val.as_object_mut().unwrap().remove("evidence_against");
        let back: Hypothesis = serde_json::from_value(val).unwrap();
        assert_eq!(back.evidence_for, 0);
        assert_eq!(back.evidence_against, 0);
    }

    #[test]
    fn infer_hypotheses_from_repeated_actions() {
        let events = vec![
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({"tool_name": "edit", "file": "src/auth.rs"}),
            ),
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({"tool_name": "edit", "file": "src/auth.rs"}),
            ),
            make_event(
                EventType::ToolCallCompleted,
                serde_json::json!({"tool_name": "edit", "file": "src/auth.rs"}),
            ),
        ];
        let hypotheses = infer_hypotheses_from_patterns(&events);
        assert!(
            !hypotheses.is_empty(),
            "Should infer hypothesis from repeated pattern"
        );
        assert_eq!(hypotheses[0].source, HypothesisSource::Inferred);
        assert_eq!(hypotheses[0].confidence, 0.3);
    }
