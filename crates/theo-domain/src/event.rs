use serde::{Deserialize, Serialize};

use crate::identifiers::EventId;

/// Type-safe classification of domain events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    // Task lifecycle
    TaskCreated,
    TaskStateChanged,

    // Tool call lifecycle
    ToolCallQueued,
    ToolCallDispatched,
    ToolCallCompleted,

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
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventType::TaskCreated => write!(f, "TaskCreated"),
            EventType::TaskStateChanged => write!(f, "TaskStateChanged"),
            EventType::ToolCallQueued => write!(f, "ToolCallQueued"),
            EventType::ToolCallDispatched => write!(f, "ToolCallDispatched"),
            EventType::ToolCallCompleted => write!(f, "ToolCallCompleted"),
            EventType::RunInitialized => write!(f, "RunInitialized"),
            EventType::RunStateChanged => write!(f, "RunStateChanged"),
            EventType::LlmCallStart => write!(f, "LlmCallStart"),
            EventType::LlmCallEnd => write!(f, "LlmCallEnd"),
            EventType::BudgetExceeded => write!(f, "BudgetExceeded"),
            EventType::Error => write!(f, "Error"),
            EventType::ReasoningDelta => write!(f, "ReasoningDelta"),
            EventType::ContentDelta => write!(f, "ContentDelta"),
            EventType::TodoUpdated => write!(f, "TodoUpdated"),
        }
    }
}

/// All EventType variants for iteration in tests.
pub const ALL_EVENT_TYPES: [EventType; 14] = [
    EventType::TaskCreated,
    EventType::TaskStateChanged,
    EventType::ToolCallQueued,
    EventType::ToolCallDispatched,
    EventType::ToolCallCompleted,
    EventType::RunInitialized,
    EventType::RunStateChanged,
    EventType::LlmCallStart,
    EventType::LlmCallEnd,
    EventType::BudgetExceeded,
    EventType::Error,
    EventType::ReasoningDelta,
    EventType::ContentDelta,
    EventType::TodoUpdated,
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
}

impl DomainEvent {
    /// Creates a new DomainEvent with an auto-generated event_id and current timestamp.
    pub fn new(event_type: EventType, entity_id: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            event_id: EventId::generate(),
            event_type,
            entity_id: entity_id.into(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock before UNIX epoch")
                .as_millis() as u64,
            payload,
        }
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

    #[test]
    fn display_all_event_types() {
        let expected = [
            "TaskCreated", "TaskStateChanged",
            "ToolCallQueued", "ToolCallDispatched", "ToolCallCompleted",
            "RunInitialized", "RunStateChanged",
            "LlmCallStart", "LlmCallEnd", "BudgetExceeded", "Error",
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
}
