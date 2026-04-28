//! Sibling test body of `event.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `event.rs` via `#[path = "event_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use super::*;

    #[test]
    fn serde_roundtrip_all_event_types() {
        for et in &ALL_EVENT_TYPES {
            let json = serde_json::to_string(et).unwrap();
            let back: EventType = serde_json::from_str(&json).unwrap();
            assert_eq!(*et, back, "serde roundtrip failed for {:?}", et);
        }
    }

    // ────────────────────────────────────────────────────────────────
    // Phase 4 — RetrievalExecuted event reachable and serialized
    // (PLAN_CONTEXT_WIRING Phase 4)
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn retrieval_executed_event_type_round_trips() {
        let et = EventType::RetrievalExecuted;
        let json = serde_json::to_string(&et).expect("serde");
        let back: EventType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(et, back);
    }

    #[test]
    fn retrieval_executed_in_all_event_types() {
        assert!(
            ALL_EVENT_TYPES.contains(&EventType::RetrievalExecuted),
            "ALL_EVENT_TYPES must list the telemetry variant"
        );
    }

    #[test]
    fn retrieval_executed_display_name_is_stable() {
        assert_eq!(
            format!("{}", EventType::RetrievalExecuted),
            "RetrievalExecuted"
        );
    }

    #[test]
    fn retrieval_executed_domain_event_carries_metrics_payload() {
        // The caller in graph_context_service emits a trace line today;
        // this smoke test documents the payload shape we expect once the
        // EventBus is plumbed into the read-only context service.
        let payload = serde_json::json!({
            "primary_files": 8,
            "harm_removals": 2,
            "compression_savings_tokens": 1420,
            "inline_slices_count": 1,
        });
        let event = DomainEvent::new(EventType::RetrievalExecuted, "run-xyz", payload.clone());
        assert_eq!(event.event_type, EventType::RetrievalExecuted);
        assert_eq!(event.payload, payload);
        assert!(!event.event_id.as_str().is_empty());
        assert!(event.timestamp > 0);
    }

    #[test]
    fn display_all_event_types() {
        let expected = [
            "TaskCreated",
            "TaskStateChanged",
            "ToolCallQueued",
            "ToolCallDispatched",
            "ToolCallCompleted",
            "ToolCallProgress",
            "RunInitialized",
            "RunStateChanged",
            "LlmCallStart",
            "LlmCallEnd",
            "BudgetExceeded",
            "Error",
        ];
        for (et, name) in ALL_EVENT_TYPES.iter().zip(expected.iter()) {
            assert_eq!(format!("{}", et), *name);
        }
    }

    #[test]
    fn domain_event_new_generates_id_and_timestamp() {
        let event = DomainEvent::new(
            EventType::TaskCreated,
            "task-1",
            serde_json::json!({"objective": "test"}),
        );
        assert!(!event.event_id.as_str().is_empty());
        assert!(event.timestamp > 0);
        assert_eq!(event.event_type, EventType::TaskCreated);
        assert_eq!(event.entity_id, "task-1");
    }

    #[test]
    fn domain_event_serde_roundtrip() {
        let event = DomainEvent::new(
            EventType::RunStateChanged,
            "run-42",
            serde_json::json!({"from": "Planning", "to": "Executing"}),
        );
        let json = serde_json::to_string(&event).unwrap();
        let back: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_id, event.event_id);
        assert_eq!(back.event_type, event.event_type);
        assert_eq!(back.entity_id, event.entity_id);
        assert_eq!(back.timestamp, event.timestamp);
    }

    #[test]
    fn domain_event_with_timestamp_zero() {
        let event = DomainEvent {
            event_id: EventId::new("evt-0"),
            event_type: EventType::Error,
            entity_id: "test".into(),
            timestamp: 0,
            payload: serde_json::Value::Null,
            supersedes_event_id: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.timestamp, 0);
    }

    #[test]
    fn domain_event_with_large_payload() {
        let big_payload = serde_json::json!({
            "data": "x".repeat(10_000),
        });
        let event = DomainEvent::new(EventType::ToolCallCompleted, "call-1", big_payload);
        let json = serde_json::to_string(&event).unwrap();
        let back: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_id, event.event_id);
    }

    // --- S1-T1: Cognitive event validation tests ---

    #[test]
    fn hypothesis_formed_requires_rationale() {
        let missing_rationale = serde_json::json!({"hypothesis": "auth bug in jwt.rs"});
        assert!(validate_cognitive_event(EventType::HypothesisFormed, &missing_rationale).is_err());

        let valid = serde_json::json!({"hypothesis": "auth bug in jwt.rs", "rationale": "test_verify fails"});
        assert!(validate_cognitive_event(EventType::HypothesisFormed, &valid).is_ok());
    }

    #[test]
    fn hypothesis_invalidated_must_reference_prior() {
        let valid =
            serde_json::json!({"prior_event_id": "evt-123", "reason": "test passed after revert"});
        assert!(validate_cognitive_event(EventType::HypothesisInvalidated, &valid).is_ok());

        let missing_ref = serde_json::json!({"reason": "test passed"});
        assert!(validate_cognitive_event(EventType::HypothesisInvalidated, &missing_ref).is_err());
    }

    #[test]
    fn decision_made_carries_choice_and_evidence() {
        let valid = serde_json::json!({
            "choice": "rewrite verify_token",
            "alternatives_considered": ["patch", "rewrite"],
            "evidence_refs": ["evt-100", "evt-102"]
        });
        assert!(validate_cognitive_event(EventType::DecisionMade, &valid).is_ok());

        let missing_choice = serde_json::json!({"evidence_refs": ["evt-100"]});
        assert!(validate_cognitive_event(EventType::DecisionMade, &missing_choice).is_err());
    }

    #[test]
    fn constraint_learned_has_scope() {
        let valid =
            serde_json::json!({"constraint": "no unwrap in auth", "scope": "workspace-local"});
        assert!(validate_cognitive_event(EventType::ConstraintLearned, &valid).is_ok());

        let no_scope = serde_json::json!({"constraint": "no unwrap in auth"});
        assert!(validate_cognitive_event(EventType::ConstraintLearned, &no_scope).is_err());

        let invalid_scope = serde_json::json!({"constraint": "no unwrap", "scope": "global"});
        assert!(validate_cognitive_event(EventType::ConstraintLearned, &invalid_scope).is_err());
    }

    #[test]
    fn non_cognitive_events_pass_validation() {
        let payload = serde_json::json!({});
        assert!(validate_cognitive_event(EventType::TaskCreated, &payload).is_ok());
        assert!(validate_cognitive_event(EventType::Error, &payload).is_ok());
        assert!(validate_cognitive_event(EventType::RunStateChanged, &payload).is_ok());
    }

    #[test]
    fn constraint_scope_serde_roundtrip() {
        for scope in &[
            ConstraintScope::RunLocal,
            ConstraintScope::TaskLocal,
            ConstraintScope::WorkspaceLocal,
        ] {
            let json = serde_json::to_string(scope).unwrap();
            let back: ConstraintScope = serde_json::from_str(&json).unwrap();
            assert_eq!(*scope, back);
        }
    }

    // --- S1-T4: supersedes_event_id tests ---

    #[test]
    fn domain_event_supersedes_none_by_default() {
        let event = DomainEvent::new(EventType::TaskCreated, "run-1", serde_json::json!({}));
        assert!(event.supersedes_event_id.is_none());
    }

    #[test]
    fn domain_event_new_superseding_carries_reference() {
        let original = DomainEvent::new(
            EventType::HypothesisFormed,
            "run-1",
            serde_json::json!({"hypothesis": "h1", "rationale": "r1"}),
        );
        let invalidation = DomainEvent::new_superseding(
            EventType::HypothesisInvalidated,
            "run-1",
            serde_json::json!({"prior_event_id": original.event_id.as_str(), "reason": "disproved"}),
            original.event_id.clone(),
        );
        assert_eq!(invalidation.supersedes_event_id.unwrap(), original.event_id);
    }

    #[test]
    fn supersedes_event_id_survives_serde_roundtrip() {
        let event = DomainEvent::new_superseding(
            EventType::HypothesisInvalidated,
            "run-1",
            serde_json::json!({"prior_event_id": "evt-1", "reason": "test"}),
            EventId::new("evt-1"),
        );
        let json = serde_json::to_string(&event).unwrap();
        let back: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.supersedes_event_id.unwrap(), EventId::new("evt-1"));
    }

    #[test]
    fn legacy_event_without_supersedes_deserializes_to_none() {
        let mut val: serde_json::Value = serde_json::to_value(DomainEvent::new(
            EventType::TaskCreated,
            "t-1",
            serde_json::json!({}),
        ))
        .unwrap();
        val.as_object_mut().unwrap().remove("supersedes_event_id");
        let json = serde_json::to_string(&val).unwrap();
        let back: DomainEvent = serde_json::from_str(&json).unwrap();
        assert!(back.supersedes_event_id.is_none());
    }

    #[test]
    fn tool_call_progress_in_all_event_types() {
        assert!(ALL_EVENT_TYPES.contains(&EventType::ToolCallProgress));
    }

    #[test]
    fn tool_call_progress_display() {
        assert_eq!(format!("{}", EventType::ToolCallProgress), "ToolCallProgress");
    }

    #[test]
    fn cognitive_event_types_in_all_event_types() {
        assert!(ALL_EVENT_TYPES.contains(&EventType::HypothesisFormed));
        assert!(ALL_EVENT_TYPES.contains(&EventType::HypothesisInvalidated));
        assert!(ALL_EVENT_TYPES.contains(&EventType::DecisionMade));
        assert!(ALL_EVENT_TYPES.contains(&EventType::ConstraintLearned));
        // Track A — Phase 3 added SubagentStarted + SubagentCompleted (was 22).
        // sota-gaps Phase 18 added HandoffEvaluated → 25.
        // T1.3 added PluginLoaded → 26.
        assert_eq!(ALL_EVENT_TYPES.len(), 26);
        assert!(ALL_EVENT_TYPES.contains(&EventType::HandoffEvaluated));
        assert!(ALL_EVENT_TYPES.contains(&EventType::PluginLoaded));
    }

    // --- P-1 BF2: Contextual validation tests ---

    #[test]
    fn validate_in_context_rejects_nonexistent_prior() {
        let known: std::collections::HashSet<String> =
            ["evt-1", "evt-2"].iter().map(|s| s.to_string()).collect();
        let payload = serde_json::json!({"prior_event_id": "evt-999", "reason": "disproved"});
        let result =
            validate_cognitive_event_in_context(EventType::HypothesisInvalidated, &payload, &known);
        assert!(result.is_err(), "Should reject nonexistent prior_event_id");
    }

    #[test]
    fn validate_in_context_accepts_existing_prior() {
        let known: std::collections::HashSet<String> =
            ["evt-1", "evt-2"].iter().map(|s| s.to_string()).collect();
        let payload = serde_json::json!({"prior_event_id": "evt-1", "reason": "test passed"});
        let result =
            validate_cognitive_event_in_context(EventType::HypothesisInvalidated, &payload, &known);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_in_context_passes_non_cognitive_events() {
        let known: std::collections::HashSet<String> = std::collections::HashSet::new();
        let payload = serde_json::json!({"tool_name": "bash"});
        let result =
            validate_cognitive_event_in_context(EventType::ToolCallCompleted, &payload, &known);
        assert!(
            result.is_ok(),
            "Non-cognitive events pass without context check"
        );
    }

    // --- T0.1: EventKind mapping tests ---

    #[test]
    fn test_event_kind_mapping_is_exhaustive() {
        for et in &ALL_EVENT_TYPES {
            let kind = et.kind();
            let _ = kind; // must return without panic for every variant
        }
    }

    #[test]
    fn test_event_kind_is_deterministic() {
        for et in &ALL_EVENT_TYPES {
            assert_eq!(et.kind(), et.kind(), "EventKind not deterministic for {:?}", et);
        }
    }

    #[test]
    fn test_event_kind_lifecycle_variants() {
        assert_eq!(EventType::TaskCreated.kind(), EventKind::Lifecycle);
        assert_eq!(EventType::TaskStateChanged.kind(), EventKind::Lifecycle);
        assert_eq!(EventType::RunInitialized.kind(), EventKind::Lifecycle);
        assert_eq!(EventType::RunStateChanged.kind(), EventKind::Lifecycle);
        assert_eq!(EventType::TodoUpdated.kind(), EventKind::Lifecycle);
    }

    #[test]
    fn test_event_kind_tooling_variants() {
        assert_eq!(EventType::ToolCallQueued.kind(), EventKind::Tooling);
        assert_eq!(EventType::ToolCallDispatched.kind(), EventKind::Tooling);
        assert_eq!(EventType::ToolCallCompleted.kind(), EventKind::Tooling);
        assert_eq!(EventType::ToolCallProgress.kind(), EventKind::Tooling);
        assert_eq!(EventType::SensorExecuted.kind(), EventKind::Tooling);
    }

    #[test]
    fn test_event_kind_reasoning_variants() {
        assert_eq!(EventType::HypothesisFormed.kind(), EventKind::Reasoning);
        assert_eq!(EventType::HypothesisInvalidated.kind(), EventKind::Reasoning);
        assert_eq!(EventType::DecisionMade.kind(), EventKind::Reasoning);
        assert_eq!(EventType::ConstraintLearned.kind(), EventKind::Reasoning);
    }

    #[test]
    fn test_event_kind_context_variants() {
        assert_eq!(EventType::LlmCallStart.kind(), EventKind::Context);
        assert_eq!(EventType::LlmCallEnd.kind(), EventKind::Context);
        assert_eq!(EventType::ContextOverflowRecovery.kind(), EventKind::Context);
        assert_eq!(EventType::RetrievalExecuted.kind(), EventKind::Context);
    }

    #[test]
    fn test_event_kind_failure_variants() {
        assert_eq!(EventType::BudgetExceeded.kind(), EventKind::Failure);
        assert_eq!(EventType::Error.kind(), EventKind::Failure);
    }

    #[test]
    fn test_event_kind_streaming_excluded_from_trajectory() {
        assert_eq!(EventType::ContentDelta.kind(), EventKind::Streaming);
        assert_eq!(EventType::ReasoningDelta.kind(), EventKind::Streaming);
    }

    #[test]
    fn test_event_kind_serde_roundtrip() {
        for kind in &[
            EventKind::Lifecycle,
            EventKind::Tooling,
            EventKind::Reasoning,
            EventKind::Context,
            EventKind::Failure,
            EventKind::Streaming,
        ] {
            let json = serde_json::to_string(kind).unwrap();
            let back: EventKind = serde_json::from_str(&json).unwrap();
            assert_eq!(*kind, back);
        }
    }
