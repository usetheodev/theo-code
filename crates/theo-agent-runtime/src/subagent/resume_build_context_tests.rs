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
fn build_context_terminal_run_returns_not_resumable() {
    let (_dir, store) = make_store();
    let spec = fixture_spec("x");
    store.save(&fixture_run(&spec, RunStatus::Completed)).unwrap();
    let manager = make_manager();
    let resumer = Resumer::new(&store, &manager);
    let err = resumer.build_context("r-test").unwrap_err();
    match err {
        ResumeError::NotResumable { status, .. } => assert!(status.contains("Completed")),
        _ => panic!("expected NotResumable"),
    }
}

#[test]
fn build_context_failed_run_is_not_resumable() {
    let (_dir, store) = make_store();
    let spec = fixture_spec("x");
    store.save(&fixture_run(&spec, RunStatus::Failed)).unwrap();
    let manager = make_manager();
    let resumer = Resumer::new(&store, &manager);
    assert!(matches!(
        resumer.build_context("r-test").unwrap_err(),
        ResumeError::NotResumable { .. }
    ));
}

#[test]
fn build_context_cancelled_run_is_not_resumable() {
    // Cancelled is terminal — user must use abandon then re-spawn fresh
    let (_dir, store) = make_store();
    let spec = fixture_spec("x");
    store.save(&fixture_run(&spec, RunStatus::Cancelled)).unwrap();
    let manager = make_manager();
    let resumer = Resumer::new(&store, &manager);
    assert!(matches!(
        resumer.build_context("r-test").unwrap_err(),
        ResumeError::NotResumable { .. }
    ));
}

#[test]
fn build_context_running_run_returns_context() {
    let (_dir, store) = make_store();
    let spec = fixture_spec("x");
    store.save(&fixture_run(&spec, RunStatus::Running)).unwrap();
    let manager = make_manager();
    let resumer = Resumer::new(&store, &manager);
    let ctx = resumer.build_context("r-test").unwrap();
    assert_eq!(ctx.spec.name, "x");
    assert_eq!(ctx.start_iteration, 0); // no events
}

#[test]
fn build_context_unknown_run_returns_not_found() {
    let (_dir, store) = make_store();
    let manager = make_manager();
    let resumer = Resumer::new(&store, &manager);
    let err = resumer.build_context("missing").unwrap_err();
    assert!(matches!(err, ResumeError::NotFound(_)));
}

#[test]
fn build_context_start_iteration_counts_completed_events() {
    let (_dir, store) = make_store();
    let spec = fixture_spec("x");
    store.save(&fixture_run(&spec, RunStatus::Running)).unwrap();
    for i in 0..3 {
        store
            .append_event(
                "r-test",
                &SubagentEvent {
                    timestamp: i,
                    event_type: "iteration_completed".into(),
                    payload: serde_json::json!({}),
                },
            )
            .unwrap();
    }
    // Plus one event of a different type that should be ignored
    store
        .append_event(
            "r-test",
            &SubagentEvent {
                timestamp: 99,
                event_type: "user_message".into(),
                payload: serde_json::json!({"text": "hi"}),
            },
        )
        .unwrap();
    let manager = make_manager();
    let resumer = Resumer::new(&store, &manager);
    let ctx = resumer.build_context("r-test").unwrap();
    assert_eq!(ctx.start_iteration, 3);
}

#[test]
fn build_context_reconstructs_history_from_events() {
    let (_dir, store) = make_store();
    let spec = fixture_spec("x");
    store.save(&fixture_run(&spec, RunStatus::Running)).unwrap();
    store
        .append_event(
            "r-test",
            &SubagentEvent {
                timestamp: 1,
                event_type: "user_message".into(),
                payload: serde_json::json!({"text": "hello"}),
            },
        )
        .unwrap();
    store
        .append_event(
            "r-test",
            &SubagentEvent {
                timestamp: 2,
                event_type: "assistant_message".into(),
                payload: serde_json::json!({"text": "hi back"}),
            },
        )
        .unwrap();
    let manager = make_manager();
    let resumer = Resumer::new(&store, &manager);
    let ctx = resumer.build_context("r-test").unwrap();
    assert_eq!(ctx.history.len(), 2);
}

#[test]
fn build_context_preserves_checkpoint_before() {
    let (_dir, store) = make_store();
    let spec = fixture_spec("x");
    let mut run = fixture_run(&spec, RunStatus::Running);
    run.checkpoint_before = Some("abc123def".into());
    store.save(&run).unwrap();
    let manager = make_manager();
    let resumer = Resumer::new(&store, &manager);
    let ctx = resumer.build_context("r-test").unwrap();
    assert_eq!(ctx.checkpoint_before.as_deref(), Some("abc123def"));
}

#[test]
fn build_context_preserves_prior_tokens_used() {
    let (_dir, store) = make_store();
    let spec = fixture_spec("x");
    let mut run = fixture_run(&spec, RunStatus::Running);
    run.tokens_used = 12_345;
    store.save(&run).unwrap();
    let manager = make_manager();
    let resumer = Resumer::new(&store, &manager);
    let ctx = resumer.build_context("r-test").unwrap();
    assert_eq!(ctx.prior_tokens_used, 12_345);
}

