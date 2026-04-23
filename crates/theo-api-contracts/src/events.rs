use serde::{Deserialize, Serialize};

/// Event payload sent to frontend surfaces (desktop, CLI, etc.).
///
/// Deserialize is derived so that consumers (e.g. IPC bridges) can
/// round-trip events without reinventing the schema. Every variant
/// is tagged by `"type"` on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FrontendEvent {
    #[serde(rename = "token")]
    Token { text: String },
    #[serde(rename = "tool_start")]
    ToolStart {
        name: String,
        args: serde_json::Value,
    },
    #[serde(rename = "tool_end")]
    ToolEnd {
        name: String,
        success: bool,
        output: String,
    },
    #[serde(rename = "phase_change")]
    PhaseChange { from: String, to: String },
    #[serde(rename = "done")]
    Done { success: bool, summary: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "llm_call_start")]
    LlmCallStart { iteration: usize },
    #[serde(rename = "llm_call_end")]
    LlmCallEnd { iteration: usize },
}

#[cfg(test)]
mod tests {
    //! Wire-format tests for FrontendEvent.
    //!
    //! The schema is crossed by both the Rust backend (serializer) and
    //! external consumers (CLI / Desktop / tests). Changing any rename
    //! or field order is a breaking change — these tests pin the shape
    //! down so a silent rename cannot slip through review.

    use super::FrontendEvent;
    use serde_json::json;

    fn roundtrip(event: &FrontendEvent) -> FrontendEvent {
        let raw = serde_json::to_string(event).expect("serialize");
        serde_json::from_str(&raw).expect("deserialize")
    }

    #[test]
    fn token_event_uses_snake_case_type_tag() {
        let event = FrontendEvent::Token {
            text: "hello".into(),
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value, json!({"type": "token", "text": "hello"}));
    }

    #[test]
    fn token_event_round_trips_via_json() {
        let original = FrontendEvent::Token {
            text: "streamed token".into(),
        };
        assert_eq!(roundtrip(&original), original);
    }

    #[test]
    fn tool_start_event_preserves_args_payload() {
        let event = FrontendEvent::ToolStart {
            name: "bash".into(),
            args: json!({"command": "echo hi"}),
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["type"], "tool_start");
        assert_eq!(value["name"], "bash");
        assert_eq!(value["args"]["command"], "echo hi");
    }

    #[test]
    fn tool_start_round_trips_with_arbitrary_args() {
        let original = FrontendEvent::ToolStart {
            name: "fetch".into(),
            args: json!({"url": "https://example.org", "method": "GET", "retries": 3}),
        };
        assert_eq!(roundtrip(&original), original);
    }

    #[test]
    fn tool_end_wire_format_has_success_and_output_fields() {
        let event = FrontendEvent::ToolEnd {
            name: "bash".into(),
            success: false,
            output: "error: no such file".into(),
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(
            value,
            json!({
                "type": "tool_end",
                "name": "bash",
                "success": false,
                "output": "error: no such file",
            })
        );
    }

    #[test]
    fn phase_change_wire_format_exposes_from_and_to() {
        let event = FrontendEvent::PhaseChange {
            from: "planning".into(),
            to: "executing".into(),
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(
            value,
            json!({"type": "phase_change", "from": "planning", "to": "executing"})
        );
    }

    #[test]
    fn done_event_round_trips_both_flags() {
        let success_case = FrontendEvent::Done {
            success: true,
            summary: "ok".into(),
        };
        let failure_case = FrontendEvent::Done {
            success: false,
            summary: "aborted".into(),
        };
        assert_eq!(roundtrip(&success_case), success_case);
        assert_eq!(roundtrip(&failure_case), failure_case);
    }

    #[test]
    fn error_event_round_trips() {
        let original = FrontendEvent::Error {
            message: "boom".into(),
        };
        assert_eq!(roundtrip(&original), original);
    }

    #[test]
    fn llm_call_start_event_wire_format() {
        let event = FrontendEvent::LlmCallStart { iteration: 3 };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value, json!({"type": "llm_call_start", "iteration": 3}));
    }

    #[test]
    fn llm_call_end_event_wire_format() {
        let event = FrontendEvent::LlmCallEnd { iteration: 7 };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value, json!({"type": "llm_call_end", "iteration": 7}));
    }

    #[test]
    fn unknown_type_tag_rejects_deserialization() {
        let raw = json!({"type": "does_not_exist", "x": 1}).to_string();
        let result: Result<FrontendEvent, _> = serde_json::from_str(&raw);
        assert!(result.is_err(), "unknown variant should fail to deserialize");
    }

    #[test]
    fn missing_required_field_rejects_deserialization() {
        // ToolStart requires both `name` and `args` — omitting `args` must fail.
        let raw = json!({"type": "tool_start", "name": "bash"}).to_string();
        let result: Result<FrontendEvent, _> = serde_json::from_str(&raw);
        assert!(
            result.is_err(),
            "missing `args` field should fail to deserialize"
        );
    }

    #[test]
    fn every_variant_is_distinguishable_on_the_wire() {
        let variants = vec![
            FrontendEvent::Token { text: "t".into() },
            FrontendEvent::ToolStart {
                name: "n".into(),
                args: json!({}),
            },
            FrontendEvent::ToolEnd {
                name: "n".into(),
                success: true,
                output: "".into(),
            },
            FrontendEvent::PhaseChange {
                from: "a".into(),
                to: "b".into(),
            },
            FrontendEvent::Done {
                success: true,
                summary: "".into(),
            },
            FrontendEvent::Error {
                message: "".into(),
            },
            FrontendEvent::LlmCallStart { iteration: 0 },
            FrontendEvent::LlmCallEnd { iteration: 0 },
        ];
        let mut tags: Vec<String> = variants
            .iter()
            .map(|v| {
                serde_json::to_value(v).unwrap()["type"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        tags.sort();
        tags.dedup();
        assert_eq!(
            tags.len(),
            variants.len(),
            "each variant must serialise to a unique `type` tag"
        );
    }
}
