use serde::{Deserialize, Serialize};

use crate::identifiers::EventId;

/// High-level classification of domain events for observability filtering.
///
/// The observability pipeline uses `EventKind` to decide which events to
/// persist into a trajectory. `Streaming` events are explicitly excluded from
/// trajectories because they carry volatile partial output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EventKind {
    /// Lifecycle transitions for tasks, runs, and todos.
    Lifecycle,
    /// Tool and sensor invocations.
    Tooling,
    /// Agent cognition — hypotheses, decisions, constraints.
    Reasoning,
    /// Context retrieval, LLM calls, overflow recovery.
    Context,
    /// Explicit failure signals — budget exhaustion, errors.
    Failure,
    /// Partial streaming output (excluded from trajectories by default).
    Streaming,
}

/// Type-safe classification of domain events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EventType {
    // Task lifecycle
    TaskCreated,
    TaskStateChanged,

    // Tool call lifecycle
    ToolCallQueued,
    ToolCallDispatched,
    ToolCallCompleted,
    /// Partial progress emitted during tool execution (streaming tool output).
    ToolCallProgress,

    // Agent run lifecycle
    RunInitialized,
    RunStateChanged,

    // Operational
    LlmCallStart,
    LlmCallEnd,
    BudgetExceeded,
    Error,

    // Streaming
    ReasoningDelta,
    ContentDelta,

    // Task management
    TodoUpdated,

    // Context management
    /// Context overflow detected; emergency compaction triggered.
    ContextOverflowRecovery,
    /// Retrieval pipeline emitted context blocks. Payload carries the
    /// PLAN_CONTEXT_WIRING Phase 4 telemetry: `primary_files`,
    /// `harm_removals`, `compression_savings_tokens`, `inline_slices_count`.
    RetrievalExecuted,

    // Cognitive events — agent reasoning state
    /// Agent formed a hypothesis. Payload MUST contain "hypothesis" and "rationale".
    HypothesisFormed,
    /// Agent invalidated a prior hypothesis. Payload MUST contain "prior_event_id" and "reason".
    HypothesisInvalidated,
    /// Agent made a deliberate decision. Payload MUST contain "choice" and "evidence_refs".
    DecisionMade,
    /// Agent learned a constraint from execution. Payload MUST contain "constraint" and "scope".
    ConstraintLearned,

    // Sensors
    /// Computational sensor executed after a write tool (e.g., clippy, cargo test).
    /// Payload contains "file", "exit_code", "output_preview".
    SensorExecuted,

    // Sub-agent lifecycle (Track A — Phase 3)
    /// Emitted when a sub-agent starts. Payload:
    /// {
    ///   "agent_name": String,
    ///   "agent_source": "builtin|project|global|on_demand",
    ///   "objective": String,
    /// }
    SubagentStarted,
    /// Emitted when a sub-agent finishes. Payload includes per-agent cost metrics (D4):
    /// {
    ///   "agent_name": String,
    ///   "agent_source": String,
    ///   "success": bool,
    ///   "summary": String,
    ///   "duration_ms": u64,
    ///   "tokens_used": u64,
    ///   "input_tokens": u64,
    ///   "output_tokens": u64,
    ///   "llm_calls": u64,
    ///   "iterations_used": u64,
    /// }
    SubagentCompleted,
}

/// Scope of a learned constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstraintScope {
    /// Only applies within the current run.
    RunLocal,
    /// Applies to the current task (may span multiple runs).
    TaskLocal,
    /// Applies to the entire workspace/project.
    WorkspaceLocal,
}

/// Validation error for cognitive event payloads.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EventValidationError {
    #[error("missing required field '{field}' in {event_type} payload")]
    MissingField { event_type: String, field: String },
    #[error("invalid value for field '{field}' in {event_type}: {reason}")]
    InvalidValue {
        event_type: String,
        field: String,
        reason: String,
    },
}

/// Validates that cognitive event payloads satisfy their causal invariants.
///
/// Non-cognitive events always pass validation (no constraints).
pub fn validate_cognitive_event(
    event_type: EventType,
    payload: &serde_json::Value,
) -> Result<(), EventValidationError> {
    match event_type {
        EventType::HypothesisFormed => {
            require_field(payload, "hypothesis", "HypothesisFormed")?;
            require_field(payload, "rationale", "HypothesisFormed")?;
            Ok(())
        }
        EventType::HypothesisInvalidated => {
            require_field(payload, "prior_event_id", "HypothesisInvalidated")?;
            require_field(payload, "reason", "HypothesisInvalidated")?;
            Ok(())
        }
        EventType::DecisionMade => {
            require_field(payload, "choice", "DecisionMade")?;
            require_field(payload, "evidence_refs", "DecisionMade")?;
            Ok(())
        }
        EventType::ConstraintLearned => {
            require_field(payload, "constraint", "ConstraintLearned")?;
            let scope_val = require_field(payload, "scope", "ConstraintLearned")?;
            let scope_str =
                scope_val
                    .as_str()
                    .ok_or_else(|| EventValidationError::InvalidValue {
                        event_type: "ConstraintLearned".into(),
                        field: "scope".into(),
                        reason: "must be a string".into(),
                    })?;
            if !matches!(scope_str, "run-local" | "task-local" | "workspace-local") {
                return Err(EventValidationError::InvalidValue {
                    event_type: "ConstraintLearned".into(),
                    field: "scope".into(),
                    reason: format!(
                        "invalid scope '{}', expected run-local|task-local|workspace-local",
                        scope_str
                    ),
                });
            }
            Ok(())
        }
        _ => Ok(()), // Non-cognitive events pass without checks
    }
}

fn require_field<'a>(
    payload: &'a serde_json::Value,
    field: &str,
    event_type: &str,
) -> Result<&'a serde_json::Value, EventValidationError> {
    payload
        .get(field)
        .ok_or_else(|| EventValidationError::MissingField {
            event_type: event_type.into(),
            field: field.into(),
        })
}

/// Validates cognitive event payloads with referential integrity checking.
///
/// Extends `validate_cognitive_event` by verifying that `prior_event_id` in
/// `HypothesisInvalidated` actually references a known event.
pub fn validate_cognitive_event_in_context(
    event_type: EventType,
    payload: &serde_json::Value,
    known_event_ids: &std::collections::HashSet<String>,
) -> Result<(), EventValidationError> {
    // Basic structural validation first
    validate_cognitive_event(event_type, payload)?;

    // Referential integrity for invalidation events
    if event_type == EventType::HypothesisInvalidated
        && let Some(prior_id) = payload.get("prior_event_id").and_then(|v| v.as_str())
            && !known_event_ids.contains(prior_id) {
                return Err(EventValidationError::InvalidValue {
                    event_type: "HypothesisInvalidated".into(),
                    field: "prior_event_id".into(),
                    reason: format!("referenced event '{}' not found in known events", prior_id),
                });
            }
    Ok(())
}

impl EventType {
    /// Returns the `EventKind` classification of this event type.
    ///
    /// The mapping is total (every variant has a kind) and deterministic.
    pub fn kind(&self) -> EventKind {
        match self {
            // Lifecycle — state transitions and task management
            EventType::TaskCreated => EventKind::Lifecycle,
            EventType::TaskStateChanged => EventKind::Lifecycle,
            EventType::RunInitialized => EventKind::Lifecycle,
            EventType::RunStateChanged => EventKind::Lifecycle,
            EventType::TodoUpdated => EventKind::Lifecycle,

            // Tooling — tool invocations and sensors
            EventType::ToolCallQueued => EventKind::Tooling,
            EventType::ToolCallDispatched => EventKind::Tooling,
            EventType::ToolCallCompleted => EventKind::Tooling,
            EventType::ToolCallProgress => EventKind::Tooling,
            EventType::SensorExecuted => EventKind::Tooling,

            // Reasoning — cognitive events
            EventType::HypothesisFormed => EventKind::Reasoning,
            EventType::HypothesisInvalidated => EventKind::Reasoning,
            EventType::DecisionMade => EventKind::Reasoning,
            EventType::ConstraintLearned => EventKind::Reasoning,

            // Context — retrieval and LLM
            EventType::LlmCallStart => EventKind::Context,
            EventType::LlmCallEnd => EventKind::Context,
            EventType::ContextOverflowRecovery => EventKind::Context,
            EventType::RetrievalExecuted => EventKind::Context,

            // Failure — budget exhaustion, errors
            EventType::BudgetExceeded => EventKind::Failure,
            EventType::Error => EventKind::Failure,

            // Streaming — partial output (excluded from trajectories)
            EventType::ReasoningDelta => EventKind::Streaming,
            EventType::ContentDelta => EventKind::Streaming,

            // Sub-agent lifecycle (Phase 3)
            EventType::SubagentStarted => EventKind::Lifecycle,
            EventType::SubagentCompleted => EventKind::Lifecycle,
        }
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventType::TaskCreated => write!(f, "TaskCreated"),
            EventType::TaskStateChanged => write!(f, "TaskStateChanged"),
            EventType::ToolCallQueued => write!(f, "ToolCallQueued"),
            EventType::ToolCallDispatched => write!(f, "ToolCallDispatched"),
            EventType::ToolCallCompleted => write!(f, "ToolCallCompleted"),
            EventType::ToolCallProgress => write!(f, "ToolCallProgress"),
            EventType::RunInitialized => write!(f, "RunInitialized"),
            EventType::RunStateChanged => write!(f, "RunStateChanged"),
            EventType::LlmCallStart => write!(f, "LlmCallStart"),
            EventType::LlmCallEnd => write!(f, "LlmCallEnd"),
            EventType::BudgetExceeded => write!(f, "BudgetExceeded"),
            EventType::Error => write!(f, "Error"),
            EventType::ReasoningDelta => write!(f, "ReasoningDelta"),
            EventType::ContentDelta => write!(f, "ContentDelta"),
            EventType::ContextOverflowRecovery => write!(f, "ContextOverflowRecovery"),
            EventType::RetrievalExecuted => write!(f, "RetrievalExecuted"),
            EventType::TodoUpdated => write!(f, "TodoUpdated"),
            EventType::HypothesisFormed => write!(f, "HypothesisFormed"),
            EventType::HypothesisInvalidated => write!(f, "HypothesisInvalidated"),
            EventType::DecisionMade => write!(f, "DecisionMade"),
            EventType::ConstraintLearned => write!(f, "ConstraintLearned"),
            EventType::SensorExecuted => write!(f, "SensorExecuted"),
            EventType::SubagentStarted => write!(f, "SubagentStarted"),
            EventType::SubagentCompleted => write!(f, "SubagentCompleted"),
        }
    }
}

/// All EventType variants for iteration in tests.
pub const ALL_EVENT_TYPES: [EventType; 24] = [
    EventType::TaskCreated,
    EventType::TaskStateChanged,
    EventType::ToolCallQueued,
    EventType::ToolCallDispatched,
    EventType::ToolCallCompleted,
    EventType::ToolCallProgress,
    EventType::RunInitialized,
    EventType::RunStateChanged,
    EventType::LlmCallStart,
    EventType::LlmCallEnd,
    EventType::BudgetExceeded,
    EventType::Error,
    EventType::ReasoningDelta,
    EventType::ContentDelta,
    EventType::ContextOverflowRecovery,
    EventType::RetrievalExecuted,
    EventType::TodoUpdated,
    EventType::HypothesisFormed,
    EventType::HypothesisInvalidated,
    EventType::DecisionMade,
    EventType::ConstraintLearned,
    EventType::SensorExecuted,
    EventType::SubagentStarted,
    EventType::SubagentCompleted,
];

/// A domain event representing a significant occurrence in the system.
///
/// Pure data type — no async, no IO. Persistence and dispatch are handled
/// by the EventBus in theo-agent-runtime.
///
/// Invariant 5: every state transition generates a persisted DomainEvent.
/// This invariant is enforced by TaskManager (Phase 03) and RunEngine (Phase 05),
/// NOT by the transition() function itself. The domain type is pure; the
/// orchestrator enforces the contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainEvent {
    pub event_id: EventId,
    pub event_type: EventType,
    /// The entity this event relates to (TaskId, CallId, or RunId as string).
    pub entity_id: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// Additional structured data about the event.
    pub payload: serde_json::Value,
    /// Optional reference to an event this one supersedes.
    /// Used for minimal causal tracking (e.g. HypothesisInvalidated supersedes HypothesisFormed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes_event_id: Option<EventId>,
}

impl DomainEvent {
    /// Creates a new DomainEvent with an auto-generated event_id and current timestamp.
    pub fn new(
        event_type: EventType,
        entity_id: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            event_id: EventId::generate(),
            event_type,
            entity_id: entity_id.into(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock before UNIX epoch")
                .as_millis() as u64,
            payload,
            supersedes_event_id: None,
        }
    }

    /// Creates a new DomainEvent that supersedes a previous event.
    pub fn new_superseding(
        event_type: EventType,
        entity_id: impl Into<String>,
        payload: serde_json::Value,
        supersedes: EventId,
    ) -> Self {
        let mut event = Self::new(event_type, entity_id, payload);
        event.supersedes_event_id = Some(supersedes);
        event
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(ALL_EVENT_TYPES.len(), 24);
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
}
