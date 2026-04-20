//! JSON output mode — emit structured JSONL events to stdout.
//!
//! When `--output json` is used, all agent events are serialized as JSON lines
//! to stdout. Stderr still gets status messages.
//!
//! Pi-mono ref: `packages/coding-agent/src/modes/json-mode.ts`

use serde::Serialize;
use theo_domain::event::{DomainEvent, EventType};

/// A structured event for JSON output mode.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JsonEvent {
    AgentStart {
        run_id: String,
    },
    ContentDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolCallStart {
        tool: String,
        call_id: String,
    },
    ToolCallEnd {
        tool: String,
        call_id: String,
        success: bool,
    },
    AgentEnd {
        success: bool,
        summary: String,
        tokens_used: u64,
    },
}

/// Convert a DomainEvent into an optional JsonEvent.
pub fn domain_event_to_json(event: &DomainEvent) -> Option<JsonEvent> {
    match event.event_type {
        EventType::RunInitialized => Some(JsonEvent::AgentStart {
            run_id: event.entity_id.clone(),
        }),
        EventType::ContentDelta => {
            let text = event.payload.get("text")?.as_str()?.to_string();
            Some(JsonEvent::ContentDelta { text })
        }
        EventType::ReasoningDelta => {
            let text = event.payload.get("text")?.as_str()?.to_string();
            Some(JsonEvent::ReasoningDelta { text })
        }
        EventType::ToolCallDispatched => {
            let tool = event.payload.get("tool_name")?.as_str()?.to_string();
            Some(JsonEvent::ToolCallStart {
                tool,
                call_id: event.entity_id.clone(),
            })
        }
        EventType::ToolCallCompleted => {
            let tool = event.payload.get("tool_name")?.as_str()?.to_string();
            let success = event.payload.get("success")?.as_bool()?;
            Some(JsonEvent::ToolCallEnd {
                tool,
                call_id: event.entity_id.clone(),
                success,
            })
        }
        _ => None,
    }
}

/// Emit a JsonEvent as a single line to stdout.
pub fn emit_json_event(event: &JsonEvent) {
    if let Ok(json) = serde_json::to_string(event) {
        println!("{json}");
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_event_serialization_includes_type_tag() {
        // Arrange
        let event = JsonEvent::AgentStart {
            run_id: "run-42".to_string(),
        };

        // Act
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Assert
        assert_eq!(parsed["type"], "agent_start");
        assert_eq!(parsed["run_id"], "run-42");
    }

    #[test]
    fn test_domain_event_to_json_maps_content_delta() {
        // Arrange
        let event = DomainEvent::new(
            EventType::ContentDelta,
            "run-1",
            serde_json::json!({"text": "hello world"}),
        );

        // Act
        let result = domain_event_to_json(&event);

        // Assert
        let json_event = result.expect("should map ContentDelta");
        match json_event {
            JsonEvent::ContentDelta { text } => assert_eq!(text, "hello world"),
            other => panic!("expected ContentDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_domain_event_to_json_returns_none_for_unknown_events() {
        // Arrange
        let event = DomainEvent::new(
            EventType::TodoUpdated,
            "run-1",
            serde_json::json!({"task": "something"}),
        );

        // Act
        let result = domain_event_to_json(&event);

        // Assert
        assert!(result.is_none(), "TodoUpdated should not map to any JsonEvent");
    }

    #[test]
    fn test_agent_end_serialization_includes_all_fields() {
        // Arrange
        let event = JsonEvent::AgentEnd {
            success: true,
            summary: "task completed".to_string(),
            tokens_used: 4096,
        };

        // Act
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Assert
        assert_eq!(parsed["type"], "agent_end");
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["summary"], "task completed");
        assert_eq!(parsed["tokens_used"], 4096);
    }

    #[test]
    fn test_domain_event_to_json_maps_run_initialized() {
        // Arrange
        let event = DomainEvent::new(
            EventType::RunInitialized,
            "run-99",
            serde_json::json!({}),
        );

        // Act
        let result = domain_event_to_json(&event);

        // Assert
        let json_event = result.expect("should map RunInitialized");
        match json_event {
            JsonEvent::AgentStart { run_id } => assert_eq!(run_id, "run-99"),
            other => panic!("expected AgentStart, got {:?}", other),
        }
    }

    #[test]
    fn test_domain_event_to_json_maps_tool_call_dispatched() {
        // Arrange
        let event = DomainEvent::new(
            EventType::ToolCallDispatched,
            "call-7",
            serde_json::json!({"tool_name": "bash"}),
        );

        // Act
        let result = domain_event_to_json(&event);

        // Assert
        let json_event = result.expect("should map ToolCallDispatched");
        match json_event {
            JsonEvent::ToolCallStart { tool, call_id } => {
                assert_eq!(tool, "bash");
                assert_eq!(call_id, "call-7");
            }
            other => panic!("expected ToolCallStart, got {:?}", other),
        }
    }

    #[test]
    fn test_domain_event_to_json_maps_tool_call_completed() {
        // Arrange
        let event = DomainEvent::new(
            EventType::ToolCallCompleted,
            "call-7",
            serde_json::json!({"tool_name": "read", "success": true}),
        );

        // Act
        let result = domain_event_to_json(&event);

        // Assert
        let json_event = result.expect("should map ToolCallCompleted");
        match json_event {
            JsonEvent::ToolCallEnd {
                tool,
                call_id,
                success,
            } => {
                assert_eq!(tool, "read");
                assert_eq!(call_id, "call-7");
                assert!(success);
            }
            other => panic!("expected ToolCallEnd, got {:?}", other),
        }
    }

    #[test]
    fn test_domain_event_to_json_maps_reasoning_delta() {
        // Arrange
        let event = DomainEvent::new(
            EventType::ReasoningDelta,
            "run-1",
            serde_json::json!({"text": "thinking about approach"}),
        );

        // Act
        let result = domain_event_to_json(&event);

        // Assert
        let json_event = result.expect("should map ReasoningDelta");
        match json_event {
            JsonEvent::ReasoningDelta { text } => assert_eq!(text, "thinking about approach"),
            other => panic!("expected ReasoningDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_content_delta_returns_none_when_text_missing() {
        // Arrange
        let event = DomainEvent::new(
            EventType::ContentDelta,
            "run-1",
            serde_json::json!({"other_field": "no text here"}),
        );

        // Act
        let result = domain_event_to_json(&event);

        // Assert
        assert!(
            result.is_none(),
            "ContentDelta without 'text' field should return None"
        );
    }

    #[test]
    fn test_tool_call_end_serialization_snake_case() {
        // Arrange
        let event = JsonEvent::ToolCallEnd {
            tool: "grep".to_string(),
            call_id: "c-1".to_string(),
            success: false,
        };

        // Act
        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Assert
        assert_eq!(parsed["type"], "tool_call_end");
        assert_eq!(parsed["tool"], "grep");
        assert_eq!(parsed["call_id"], "c-1");
        assert_eq!(parsed["success"], false);
    }
}
