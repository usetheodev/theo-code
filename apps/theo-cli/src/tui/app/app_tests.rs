//! Sibling test body of `app.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `app.rs` via `#[path = "app_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use super::*;
    use theo_domain::event::{DomainEvent, EventType};

    fn make_event(event_type: EventType, entity: &str, payload: serde_json::Value) -> DomainEvent {
        DomainEvent::new(event_type, entity, payload)
    }

    fn new_state() -> TuiState {
        TuiState::new("test".into(), "gpt-4o".into(), 40, 80, 24)
    }

    #[test]
    fn update_quit_sets_should_quit() {
        let mut state = new_state();
        update(&mut state, Msg::Quit);
        assert!(state.should_quit);
    }

    #[test]
    fn update_resize_updates_dimensions() {
        let mut state = new_state();
        update(&mut state, Msg::Resize(200, 50));
        assert_eq!(state.size, (200, 50));
    }

    #[test]
    fn update_content_delta_appends_assistant_message() {
        let mut state = new_state();
        let event = make_event(EventType::ContentDelta, "r-1", serde_json::json!({"text": "hello"}));
        update(&mut state, Msg::DomainEvent(event));

        assert_eq!(state.transcript.len(), 1);
        match &state.transcript[0] {
            TranscriptEntry::Assistant(text) => assert_eq!(text, "hello"),
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn update_content_delta_streaming_appends() {
        let mut state = new_state();
        let e1 = make_event(EventType::ContentDelta, "r-1", serde_json::json!({"text": "hel"}));
        let e2 = make_event(EventType::ContentDelta, "r-1", serde_json::json!({"text": "lo"}));
        update(&mut state, Msg::DomainEvent(e1));
        update(&mut state, Msg::DomainEvent(e2));

        assert_eq!(state.transcript.len(), 1);
        match &state.transcript[0] {
            TranscriptEntry::Assistant(text) => assert_eq!(text, "hello"),
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn update_tool_queued_creates_running_card() {
        let mut state = new_state();
        let event = make_event(EventType::ToolCallQueued, "c-1", serde_json::json!({"tool_name": "bash"}));
        update(&mut state, Msg::DomainEvent(event));

        assert_eq!(state.transcript.len(), 1);
        match &state.transcript[0] {
            TranscriptEntry::ToolCard(card) => {
                assert_eq!(card.tool_name, "bash");
                assert_eq!(card.status, ToolCardStatus::Running);
            }
            _ => panic!("expected ToolCard"),
        }
        assert_eq!(state.status.tools_running, 1);
    }

    #[test]
    fn update_tool_stdout_delta_appends_line() {
        let mut state = new_state();
        // Create card first
        let queued = make_event(EventType::ToolCallQueued, "c-1", serde_json::json!({"tool_name": "bash"}));
        update(&mut state, Msg::DomainEvent(queued));
        // Send stdout
        let delta = make_event(EventType::ToolCallProgress, "c-1", serde_json::json!({"line": "Compiling..."}));
        update(&mut state, Msg::DomainEvent(delta));

        match &state.transcript[0] {
            TranscriptEntry::ToolCard(card) => {
                assert_eq!(card.stdout_lines, vec!["Compiling..."]);
            }
            _ => panic!("expected ToolCard"),
        }
    }

    #[test]
    fn update_tool_completed_sets_status() {
        let mut state = new_state();
        let queued = make_event(EventType::ToolCallQueued, "c-1", serde_json::json!({"tool_name": "bash"}));
        update(&mut state, Msg::DomainEvent(queued));

        let completed = make_event(EventType::ToolCallCompleted, "c-1", serde_json::json!({
            "success": true, "duration_ms": 3200
        }));
        update(&mut state, Msg::DomainEvent(completed));

        match &state.transcript[0] {
            TranscriptEntry::ToolCard(card) => {
                assert_eq!(card.status, ToolCardStatus::Succeeded);
                assert_eq!(card.duration_ms, Some(3200));
            }
            _ => panic!("expected ToolCard"),
        }
        assert_eq!(state.status.tools_running, 0);
    }

    #[test]
    fn update_cursor_blink_toggles() {
        let mut state = new_state();
        let initial = state.cursor_visible;
        update(&mut state, Msg::CursorBlink);
        assert_ne!(state.cursor_visible, initial);
        update(&mut state, Msg::CursorBlink);
        assert_eq!(state.cursor_visible, initial);
    }

    #[test]
    fn update_events_lost_increments() {
        let mut state = new_state();
        update(&mut state, Msg::EventsLost(5));
        assert_eq!(state.events_lost, 5);
        assert_eq!(state.transcript.len(), 1);
    }

    #[test]
    fn update_submit_adds_user_message() {
        let mut state = new_state();
        state.input_text = "fix the bug".to_string();
        state.input_cursor = 11;
        update(&mut state, Msg::Submit("fix the bug".to_string()));
        assert!(state.input_text.is_empty());
        assert_eq!(state.input_cursor, 0);
        match &state.transcript[0] {
            TranscriptEntry::User(text) => assert_eq!(text, "fix the bug"),
            _ => panic!("expected User"),
        }
        // agent_running is set by mod.rs (the task spawner), not by the
        // pure update function — so we do NOT assert it here.
    }

    #[test]
    fn update_llm_call_end_accumulates_tokens() {
        let mut state = new_state();
        let e1 = make_event(EventType::LlmCallEnd, "r-1", serde_json::json!({
            "iteration": 1, "tokens_in": 100, "tokens_out": 50, "duration_ms": 500
        }));
        update(&mut state, Msg::DomainEvent(e1));
        assert_eq!(state.status.tokens_in, 100);
        assert_eq!(state.status.tokens_out, 50);

        let e2 = make_event(EventType::LlmCallEnd, "r-1", serde_json::json!({
            "iteration": 2, "tokens_in": 200, "tokens_out": 100, "duration_ms": 300
        }));
        update(&mut state, Msg::DomainEvent(e2));
        assert_eq!(state.status.tokens_in, 300);
        assert_eq!(state.status.tokens_out, 150);
    }
