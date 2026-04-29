//! Sibling test body of `subagent/resume.rs` — split per-area (T3.7 of code-hygiene-5x5).

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::resume_test_helpers::*;
use super::*;
use crate::config::AgentConfig;
use crate::event_bus::EventBus;
use crate::subagent::SubAgentRegistry;
use crate::subagent_runs::{FileSubagentRunStore, SubagentRun};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn reconstruct_history_skips_unknown_event_types() {
    let events = vec![
        SubagentEvent {
            timestamp: 1,
            event_type: "iteration_completed".into(), // ignored
            payload: serde_json::json!({}),
        },
        SubagentEvent {
            timestamp: 2,
            event_type: "user_message".into(),
            payload: serde_json::json!({"text": "ok"}),
        },
        SubagentEvent {
            timestamp: 3,
            event_type: "weird_unknown".into(), // ignored
            payload: serde_json::json!({"text": "x"}),
        },
    ];
    let history = reconstruct_history(&events);
    assert_eq!(history.len(), 1);
}

#[test]
fn reconstruct_history_handles_user_message_event() {
    let events = vec![SubagentEvent {
        timestamp: 1,
        event_type: "user_message".into(),
        payload: serde_json::json!({"text": "input"}),
    }];
    let history = reconstruct_history(&events);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content.as_deref(), Some("input"));
}

#[test]
fn reconstruct_history_handles_assistant_message_event() {
    let events = vec![SubagentEvent {
        timestamp: 1,
        event_type: "assistant_message".into(),
        payload: serde_json::json!({"text": "output"}),
    }];
    let history = reconstruct_history(&events);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content.as_deref(), Some("output"));
}

#[test]
fn reconstruct_history_handles_tool_result_event() {
    let events = vec![SubagentEvent {
        timestamp: 1,
        event_type: "tool_result".into(),
        payload: serde_json::json!({
            "call_id": "c1",
            "name": "read",
            "content": "file contents"
        }),
    }];
    let history = reconstruct_history(&events);
    assert_eq!(history.len(), 1);
}

#[test]
fn reconstruct_history_skips_user_message_without_text() {
    let events = vec![SubagentEvent {
        timestamp: 1,
        event_type: "user_message".into(),
        payload: serde_json::json!({}), // no "text" field
    }];
    let history = reconstruct_history(&events);
    assert!(history.is_empty());
}

