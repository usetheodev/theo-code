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
    /// Phase 18 (sota-gaps-plan): emitted by the parent right BEFORE the
    /// sub-agent spawn happens, after the handoff guardrails have run.
    /// Provides full audit trail of what was allowed/blocked and by whom.
    /// Payload: {
    ///   "source_agent": String,           // parent agent name (or "main")
    ///   "target_agent": String,           // requested agent name
    ///   "objective": String,              // the objective string
    ///   "decision": "allow|block|warn",   // overall outcome
    ///   "reason": Option<String>,         // present when decision != allow
    ///   "guardrails_evaluated": Vec<String>,  // ids of guardrails run
    ///   "blocked_by": Option<String>,     // first blocker id (if any)
    /// }
    HandoffEvaluated,

    /// T1.3 supply-chain audit: emitted when a plugin directory is loaded.
    /// Payload: {
    ///   "name": String,           // manifest.name
    ///   "dir": String,             // plugin directory (display-only)
    ///   "manifest_sha256": String, // sha256 hex of plugin.toml
    ///   "tool_count": u64,
    ///   "hook_count": u64,
    /// }
    PluginLoaded,
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
            // Phase 18 (sota-gaps): handoff guardrail audit trail
            EventType::HandoffEvaluated => EventKind::Lifecycle,
            // T1.3 supply-chain audit: plugin load with sha256 hash
            EventType::PluginLoaded => EventKind::Lifecycle,
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
            EventType::HandoffEvaluated => write!(f, "HandoffEvaluated"),
            EventType::PluginLoaded => write!(f, "PluginLoaded"),
        }
    }
}

/// All EventType variants for iteration in tests.
pub const ALL_EVENT_TYPES: [EventType; 26] = [
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
    EventType::HandoffEvaluated,
    EventType::PluginLoaded,
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
#[path = "event_tests.rs"]
mod tests;
