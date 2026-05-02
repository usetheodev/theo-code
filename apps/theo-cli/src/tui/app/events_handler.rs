//! Single-purpose slice extracted from `tui/app.rs` (T5.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::time::Instant;

use theo_domain::event::{DomainEvent, EventType};

use super::*;
use super::state_types::*;

pub fn handle_domain_event(state: &mut TuiState, event: DomainEvent) {
    match event.event_type {
        EventType::ContentDelta => handle_content_delta(state, &event),
        EventType::ReasoningDelta => handle_reasoning_delta(state, &event),
        EventType::ToolCallQueued => handle_tool_call_queued(state, &event),
        EventType::ToolCallProgress => handle_tool_call_progress(state, &event),
        EventType::ToolCallCompleted => handle_tool_call_completed(state, &event),
        EventType::RunStateChanged => handle_run_state_changed(state, &event),
        EventType::LlmCallStart => handle_llm_call_start(state, &event),
        EventType::LlmCallEnd => handle_llm_call_end(state, &event),
        EventType::BudgetExceeded => handle_budget_exceeded(state, &event),
        EventType::Error => handle_error_event(state, &event),
        EventType::TodoUpdated => handle_todo_updated(state, &event),
        _ => {}
    }
}

fn handle_content_delta(state: &mut TuiState, event: &DomainEvent) {
    let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) else {
        return;
    };
    if state.streaming_assistant {
        if let Some(TranscriptEntry::Assistant(s)) = state.transcript.last_mut() {
            s.push_str(text);
        }
    } else {
        state
            .transcript
            .push(TranscriptEntry::Assistant(text.to_string()));
        state.streaming_assistant = true;
    }
    state.cursor_visible = true;
}

fn handle_reasoning_delta(state: &mut TuiState, event: &DomainEvent) {
    let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) else {
        return;
    };
    if !state.streaming_assistant {
        state
            .transcript
            .push(TranscriptEntry::Assistant(String::new()));
        state.streaming_assistant = true;
    }
    if let Some(TranscriptEntry::Assistant(s)) = state.transcript.last_mut() {
        s.push_str(text);
    }
}

fn handle_tool_call_queued(state: &mut TuiState, event: &DomainEvent) {
    state.streaming_assistant = false;
    let call_id = event.entity_id.clone();
    let tool_name = event
        .payload
        .get("tool_name")
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

fn handle_tool_call_progress(state: &mut TuiState, event: &DomainEvent) {
    let Some(line) = event.payload.get("line").and_then(|v| v.as_str()) else {
        return;
    };
    let Some(&idx) = state.active_tool_cards.get(&event.entity_id) else {
        return;
    };
    let Some(TranscriptEntry::ToolCard(card)) = state.transcript.get_mut(idx) else {
        return;
    };
    card.stdout_lines.push(line.to_string());
    if card.stdout_lines.len() > 5 {
        card.stdout_lines.drain(..card.stdout_lines.len() - 5);
    }
}

fn handle_tool_call_completed(state: &mut TuiState, event: &DomainEvent) {
    let success = event
        .payload
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let duration_ms = event.payload.get("duration_ms").and_then(|v| v.as_u64());
    if let Some(&idx) = state.active_tool_cards.get(&event.entity_id)
        && let Some(TranscriptEntry::ToolCard(card)) = state.transcript.get_mut(idx)
    {
        card.status = if success {
            ToolCardStatus::Succeeded
        } else {
            ToolCardStatus::Failed
        };
        card.duration_ms = duration_ms;
    }
    let tool_name = event
        .payload
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();
    state.tool_chain.push(ToolChainEntry {
        tool_name,
        reason: String::new(),
        status: if success {
            ToolCardStatus::Succeeded
        } else {
            ToolCardStatus::Failed
        },
        duration_ms,
    });
    state.active_tool_cards.remove(&event.entity_id);
    state.status.tools_running = state.status.tools_running.saturating_sub(1);
}

fn handle_run_state_changed(state: &mut TuiState, event: &DomainEvent) {
    if let Some(to) = event.payload.get("to").and_then(|v| v.as_str()) {
        state.status.phase = to.to_string();
    }
    if let Some(max) = event.payload.get("max_iterations").and_then(|v| v.as_u64()) {
        state.status.max_iterations = max as usize;
    }
}

fn handle_llm_call_start(state: &mut TuiState, event: &DomainEvent) {
    if let Some(iter) = event.payload.get("iteration").and_then(|v| v.as_u64()) {
        state.status.iteration = iter as usize;
    }
}

fn handle_llm_call_end(state: &mut TuiState, event: &DomainEvent) {
    if let Some(t_in) = event.payload.get("tokens_in").and_then(|v| v.as_u64()) {
        state.status.tokens_in += t_in;
    }
    if let Some(t_out) = event.payload.get("tokens_out").and_then(|v| v.as_u64()) {
        state.status.tokens_out += t_out;
    }
    state.budget_tokens_used = state.status.tokens_in + state.status.tokens_out;
    state.agent_running = false;
    state.streaming_assistant = false;
}

fn handle_budget_exceeded(state: &mut TuiState, event: &DomainEvent) {
    let msg = event
        .payload
        .get("violation")
        .and_then(|v| v.as_str())
        .unwrap_or("budget exceeded");
    state
        .transcript
        .push(TranscriptEntry::SystemMessage(format!("⚠ {msg}")));
    state
        .transcript
        .push(TranscriptEntry::SystemMessage(format!("⚠ {msg}")));
}

fn handle_error_event(state: &mut TuiState, event: &DomainEvent) {
    if event.payload.get("type").and_then(|v| v.as_str()) == Some("retry") {
        return;
    }
    let msg = event
        .payload
        .get("error")
        .or(event.payload.get("reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown error");
    state
        .transcript
        .push(TranscriptEntry::SystemMessage(format!("❌ {msg}")));
    state
        .transcript
        .push(TranscriptEntry::SystemMessage(msg.to_string()));
}

fn handle_todo_updated(state: &mut TuiState, event: &DomainEvent) {
    let action = event
        .payload
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let content = event
        .payload
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let id = event
        .payload
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| event.payload.get("id").and_then(|v| v.as_u64()).map(|_| ""))
        .unwrap_or(&event.entity_id);
    let status = event
        .payload
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("pending");
    match action {
        "task_create" => {
            state.todos.push(TodoItem {
                id: id.to_string(),
                content: content.to_string(),
                status: "pending".to_string(),
            });
        }
        "task_update" => {
            if let Some(todo) = state.todos.iter_mut().find(|t| t.id == id) {
                todo.status = status.to_string();
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
