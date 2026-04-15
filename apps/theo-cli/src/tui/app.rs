//! TUI state and update logic — pure Elm/Redux pattern.
//!
//! `TuiState` holds all UI state. `Msg` represents all possible state transitions.
//! `update()` is a pure function: (state, msg) → mutated state, no IO.

use std::collections::HashMap;
use std::time::Instant;

use theo_domain::event::{DomainEvent, EventType};

// ---------------------------------------------------------------------------
// Transcript entries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum TranscriptEntry {
    User(String),
    Assistant(String),
    ToolCard(ToolCardState),
    SystemMessage(String),
}

#[derive(Debug, Clone)]
pub struct ToolCardState {
    pub call_id: String,
    pub tool_name: String,
    pub status: ToolCardStatus,
    pub started_at: Instant,
    pub duration_ms: Option<u64>,
    pub stdout_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolCardStatus {
    Running,
    Succeeded,
    Failed,
}

// ---------------------------------------------------------------------------
// StatusLine state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct StatusLineState {
    pub mode: String,
    pub model: String,
    pub provider: String,
    pub phase: String,
    pub iteration: usize,
    pub max_iterations: usize,
    pub tools_running: usize,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

// ---------------------------------------------------------------------------
// TUI State
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct TuiState {
    pub should_quit: bool,
    pub transcript: Vec<TranscriptEntry>,
    pub active_tool_cards: HashMap<String, usize>, // call_id → transcript index
    pub input_text: String,
    pub input_cursor: usize,
    pub status: StatusLineState,
    pub cursor_visible: bool,
    pub size: (u16, u16),
    pub agent_running: bool,
    pub events_lost: u64,
    pub scroll_offset: usize,
    pub scroll_locked_to_bottom: bool,
    pub streaming_assistant: bool,
    pub show_help: bool,
}

impl TuiState {
    pub fn new(
        provider: String,
        model: String,
        max_iterations: usize,
        width: u16,
        height: u16,
    ) -> Self {
        Self {
            should_quit: false,
            transcript: Vec::new(),
            active_tool_cards: HashMap::new(),
            input_text: String::new(),
            input_cursor: 0,
            status: StatusLineState {
                mode: "AGENT".to_string(),
                model,
                provider,
                phase: "READY".to_string(),
                iteration: 0,
                max_iterations,
                tools_running: 0,
                tokens_in: 0,
                tokens_out: 0,
            },
            cursor_visible: true,
            size: (width, height),
            agent_running: false,
            events_lost: 0,
            scroll_offset: 0,
            scroll_locked_to_bottom: true,
            streaming_assistant: false,
            show_help: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum Msg {
    Quit,
    Resize(u16, u16),
    DomainEvent(DomainEvent),
    DomainEventBatch(Vec<DomainEvent>),
    EventsLost(u64),
    InputChar(char),
    InputBackspace,
    InputDelete,
    InputLeft,
    InputRight,
    InputHome,
    InputEnd,
    Submit(String),
    CursorBlink,
    ScrollUp(usize),
    ScrollDown(usize),
    ScrollToBottom,
    ToggleHelp,
    CycleMode,
}

// ---------------------------------------------------------------------------
// Update — pure function, no IO
// ---------------------------------------------------------------------------

pub fn update(state: &mut TuiState, msg: Msg) {
    match msg {
        Msg::Quit => {
            state.should_quit = true;
        }
        Msg::Resize(w, h) => {
            state.size = (w, h);
        }
        Msg::DomainEvent(event) => {
            handle_domain_event(state, event);
        }
        Msg::DomainEventBatch(events) => {
            for event in events {
                handle_domain_event(state, event);
            }
        }
        Msg::EventsLost(n) => {
            state.events_lost += n;
            state.transcript.push(TranscriptEntry::SystemMessage(
                format!("[{n} events lost — display may be incomplete]"),
            ));
        }
        Msg::InputChar(c) => {
            state.input_text.insert(state.input_cursor, c);
            state.input_cursor += c.len_utf8();
        }
        Msg::InputBackspace => {
            if state.input_cursor > 0 {
                // Find previous char boundary
                let prev = state.input_text[..state.input_cursor]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                state.input_text.drain(prev..state.input_cursor);
                state.input_cursor = prev;
            }
        }
        Msg::InputDelete => {
            if state.input_cursor < state.input_text.len() {
                let next = state.input_text[state.input_cursor..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| state.input_cursor + i)
                    .unwrap_or(state.input_text.len());
                state.input_text.drain(state.input_cursor..next);
            }
        }
        Msg::InputLeft => {
            if state.input_cursor > 0 {
                state.input_cursor = state.input_text[..state.input_cursor]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
            }
        }
        Msg::InputRight => {
            if state.input_cursor < state.input_text.len() {
                state.input_cursor = state.input_text[state.input_cursor..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| state.input_cursor + i)
                    .unwrap_or(state.input_text.len());
            }
        }
        Msg::InputHome => {
            state.input_cursor = 0;
        }
        Msg::InputEnd => {
            state.input_cursor = state.input_text.len();
        }
        Msg::Submit(text) => {
            if !text.is_empty() {
                state.transcript.push(TranscriptEntry::User(text));
                state.input_text.clear();
                state.input_cursor = 0;
                state.agent_running = true;
                state.streaming_assistant = false;
                if state.scroll_locked_to_bottom {
                    state.scroll_offset = 0;
                }
            }
        }
        Msg::CursorBlink => {
            state.cursor_visible = !state.cursor_visible;
        }
        Msg::ScrollUp(n) => {
            state.scroll_offset = state.scroll_offset.saturating_add(n);
            state.scroll_locked_to_bottom = false;
        }
        Msg::ScrollDown(n) => {
            state.scroll_offset = state.scroll_offset.saturating_sub(n);
            if state.scroll_offset == 0 {
                state.scroll_locked_to_bottom = true;
            }
        }
        Msg::ScrollToBottom => {
            state.scroll_offset = 0;
            state.scroll_locked_to_bottom = true;
        }
        Msg::ToggleHelp => {
            state.show_help = !state.show_help;
        }
        Msg::CycleMode => {
            state.status.mode = match state.status.mode.as_str() {
                "AGENT" => "PLAN".to_string(),
                "PLAN" => "ASK".to_string(),
                _ => "AGENT".to_string(),
            };
        }
    }
}

fn handle_domain_event(state: &mut TuiState, event: DomainEvent) {
    match event.event_type {
        EventType::ContentDelta => {
            if let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) {
                if state.streaming_assistant {
                    // Append to last assistant entry
                    if let Some(TranscriptEntry::Assistant(s)) = state.transcript.last_mut() {
                        s.push_str(text);
                    }
                } else {
                    // Start new assistant entry
                    state.transcript.push(TranscriptEntry::Assistant(text.to_string()));
                    state.streaming_assistant = true;
                }
                state.cursor_visible = true;
            }
        }
        EventType::ReasoningDelta => {
            // Reasoning is dimmed — we append to assistant message with marker
            if let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) {
                if !state.streaming_assistant {
                    state.transcript.push(TranscriptEntry::Assistant(String::new()));
                    state.streaming_assistant = true;
                }
                // Reasoning is handled at render time via prefix
                if let Some(TranscriptEntry::Assistant(s)) = state.transcript.last_mut() {
                    s.push_str(text);
                }
            }
        }
        EventType::ToolCallQueued => {
            state.streaming_assistant = false;
            let call_id = event.entity_id.clone();
            let tool_name = event.payload.get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();

            let card = ToolCardState {
                call_id: call_id.clone(),
                tool_name,
                status: ToolCardStatus::Running,
                started_at: Instant::now(),
                duration_ms: None,
                stdout_lines: Vec::new(),
            };

            let idx = state.transcript.len();
            state.transcript.push(TranscriptEntry::ToolCard(card));
            state.active_tool_cards.insert(call_id, idx);
            state.status.tools_running += 1;
        }
        EventType::ToolCallStdoutDelta => {
            if let Some(line) = event.payload.get("line").and_then(|v| v.as_str()) {
                if let Some(&idx) = state.active_tool_cards.get(&event.entity_id) {
                    if let Some(TranscriptEntry::ToolCard(card)) = state.transcript.get_mut(idx) {
                        card.stdout_lines.push(line.to_string());
                        // Keep only last 5 lines visible
                        if card.stdout_lines.len() > 5 {
                            card.stdout_lines.drain(..card.stdout_lines.len() - 5);
                        }
                    }
                }
            }
        }
        EventType::ToolCallCompleted => {
            let success = event.payload.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
            let duration_ms = event.payload.get("duration_ms").and_then(|v| v.as_u64());

            if let Some(&idx) = state.active_tool_cards.get(&event.entity_id) {
                if let Some(TranscriptEntry::ToolCard(card)) = state.transcript.get_mut(idx) {
                    card.status = if success { ToolCardStatus::Succeeded } else { ToolCardStatus::Failed };
                    card.duration_ms = duration_ms;
                }
            }
            state.active_tool_cards.remove(&event.entity_id);
            state.status.tools_running = state.status.tools_running.saturating_sub(1);
        }
        EventType::RunStateChanged => {
            if let Some(to) = event.payload.get("to").and_then(|v| v.as_str()) {
                state.status.phase = to.to_string();
            }
            if let Some(max) = event.payload.get("max_iterations").and_then(|v| v.as_u64()) {
                state.status.max_iterations = max as usize;
            }
        }
        EventType::LlmCallStart => {
            if let Some(iter) = event.payload.get("iteration").and_then(|v| v.as_u64()) {
                state.status.iteration = iter as usize;
            }
        }
        EventType::LlmCallEnd => {
            if let Some(t_in) = event.payload.get("tokens_in").and_then(|v| v.as_u64()) {
                state.status.tokens_in += t_in;
            }
            if let Some(t_out) = event.payload.get("tokens_out").and_then(|v| v.as_u64()) {
                state.status.tokens_out += t_out;
            }
            state.agent_running = false;
            state.streaming_assistant = false;
        }
        EventType::BudgetExceeded => {
            let msg = event.payload.get("violation")
                .and_then(|v| v.as_str())
                .unwrap_or("budget exceeded");
            state.transcript.push(TranscriptEntry::SystemMessage(
                format!("⚠ {msg}"),
            ));
        }
        EventType::Error => {
            let msg = event.payload.get("error")
                .or(event.payload.get("reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            state.transcript.push(TranscriptEntry::SystemMessage(
                format!("❌ {msg}"),
            ));
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::event::DomainEvent;

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
        let delta = make_event(EventType::ToolCallStdoutDelta, "c-1", serde_json::json!({"line": "Compiling..."}));
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
        assert!(state.agent_running);
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
}
