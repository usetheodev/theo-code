//! Single-purpose slice extracted from `tui/app.rs` (T5.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::time::Instant;

use theo_domain::event::{DomainEvent, EventType};

use super::*;
use super::state_types::*;

pub fn handle_domain_event(state: &mut TuiState, event: DomainEvent) {
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
        EventType::ToolCallProgress => {
            if let Some(line) = event.payload.get("line").and_then(|v| v.as_str())
                && let Some(&idx) = state.active_tool_cards.get(&event.entity_id)
                    && let Some(TranscriptEntry::ToolCard(card)) = state.transcript.get_mut(idx) {
                        card.stdout_lines.push(line.to_string());
                        // Keep only last 5 lines visible
                        if card.stdout_lines.len() > 5 {
                            card.stdout_lines.drain(..card.stdout_lines.len() - 5);
                        }
                    }
        }
        EventType::ToolCallCompleted => {
            let success = event.payload.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
            let duration_ms = event.payload.get("duration_ms").and_then(|v| v.as_u64());

            if let Some(&idx) = state.active_tool_cards.get(&event.entity_id)
                && let Some(TranscriptEntry::ToolCard(card)) = state.transcript.get_mut(idx) {
                    card.status = if success { ToolCardStatus::Succeeded } else { ToolCardStatus::Failed };
                    card.duration_ms = duration_ms;
                }
            // Track in tool chain for timeline
            let tool_name = event.payload.get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            state.tool_chain.push(ToolChainEntry {
                tool_name,
                reason: String::new(), // TODO: extract from LLM reasoning
                status: if success { ToolCardStatus::Succeeded } else { ToolCardStatus::Failed },
                duration_ms,
            });

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
            state.budget_tokens_used = state.status.tokens_in + state.status.tokens_out;
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
            state.transcript.push(TranscriptEntry::SystemMessage(format!("⚠ {msg}")));
        }
        EventType::Error => {
            if event.payload.get("type").and_then(|v| v.as_str()) == Some("retry") {
                return; // Don't show retry errors as toasts
            }
            let msg = event.payload.get("error")
                .or(event.payload.get("reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            state.transcript.push(TranscriptEntry::SystemMessage(
                format!("❌ {msg}"),
            ));
            state.transcript.push(TranscriptEntry::SystemMessage(msg.to_string()));
        }
        EventType::TodoUpdated => {
            let action = event.payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let content = event.payload.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let id = event.payload.get("id").and_then(|v| v.as_str())
                .or_else(|| event.payload.get("id").and_then(|v| v.as_u64()).map(|_| ""))
                .unwrap_or(&event.entity_id);
            let status = event.payload.get("status").and_then(|v| v.as_str()).unwrap_or("pending");

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
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
