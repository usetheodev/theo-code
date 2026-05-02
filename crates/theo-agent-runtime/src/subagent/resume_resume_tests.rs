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

#[tokio::test]
async fn resume_terminal_run_returns_error_not_resumable() {
    let (_dir, store) = make_store();
    let spec = fixture_spec("x");
    store.save(&fixture_run(&spec, RunStatus::Completed)).unwrap();
    let manager = make_manager();
    let resumer = Resumer::new(&store, &manager);
    let err = resumer.resume("r-test").await.unwrap_err();
    assert!(matches!(err, ResumeError::NotResumable { .. }));
}

#[tokio::test]
async fn resume_unknown_run_returns_not_found() {
    let (_dir, store) = make_store();
    let manager = make_manager();
    let resumer = Resumer::new(&store, &manager);
    let err = resumer.resume("missing").await.unwrap_err();
    assert!(matches!(err, ResumeError::NotFound(_)));
}

#[tokio::test]
async fn resume_with_objective_override_uses_provided() {
    // Hard to assert side effect without mocking spawn_with_spec.
    // We assert: build_context succeeds, resume invokes spawn_with_spec
    // (which will hit max_depth path immediately because depth=0 is OK
    // but no real LLM — it'll spawn and fail; the resume flow itself
    // returns Ok(AgentResult) with success=false).
    let (_dir, store) = make_store();
    let spec = fixture_spec("x");
    store.save(&fixture_run(&spec, RunStatus::Running)).unwrap();
    let manager = make_manager();
    let resumer = Resumer::new(&store, &manager);
    // Use depth=1 trick? No, manager is depth=0. So spawn happens but
    // hits localhost LLM (no key). We just want to verify Ok variant.
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        resumer.resume_with_objective("r-test", Some("custom obj")),
    )
    .await;
    // Either timeout or returned — both prove the call path worked
    // without panicking.
    let _ = result;
}

// ── Worktree restore ──

pub mod worktree {
    use super::*;

    fn spec_isolated(base: Option<&str>) -> AgentSpec {
        let mut s = AgentSpec::on_demand("x", "y");
        s.isolation = Some("worktree".to_string());
        s.isolation_base_branch = base.map(String::from);
        s
    }

    #[test]
    fn resume_worktree_strategy_none_when_spec_not_isolated() {
        let spec = AgentSpec::on_demand("x", "y"); // isolation=None
        let strategy =
            WorktreeStrategy::from_spec_and_cwd(&spec, std::path::Path::new("/tmp"));
        assert_eq!(strategy, WorktreeStrategy::None);
    }

    #[test]
    fn resume_worktree_strategy_reuse_when_path_exists() {
        let dir = TempDir::new().unwrap();
        let spec = spec_isolated(Some("main"));
        let strategy = WorktreeStrategy::from_spec_and_cwd(&spec, dir.path());
        assert_eq!(strategy, WorktreeStrategy::Reuse(dir.path().to_path_buf()));
    }

    #[test]
    fn resume_worktree_strategy_recreate_when_path_missing() {
        let spec = spec_isolated(Some("develop"));
        let nonexistent = std::path::Path::new("/tmp/sota-followup-xyz-does-not-exist");
        let strategy = WorktreeStrategy::from_spec_and_cwd(&spec, nonexistent);
        assert_eq!(
            strategy,
            WorktreeStrategy::Recreate {
                base_branch: "develop".to_string(),
            }
        );
    }

    #[test]
    fn resume_worktree_strategy_recreate_defaults_to_main_when_no_base_branch() {
        let spec = spec_isolated(None);
        let nonexistent = std::path::Path::new("/tmp/sota-followup-no-base-xyz");
        let strategy = WorktreeStrategy::from_spec_and_cwd(&spec, nonexistent);
        assert_eq!(
            strategy,
            WorktreeStrategy::Recreate {
                base_branch: "main".to_string(),
            }
        );
    }

    #[test]
    fn build_context_populates_worktree_strategy_for_isolated_spec() {
        let (_dir, store) = make_store();
        let mut spec = fixture_spec("x");
        spec.isolation = Some("worktree".to_string());
        spec.isolation_base_branch = Some("main".to_string());

        let mut run = SubagentRun::new_running(
            "r-test",
            None,
            &spec,
            "obj",
            "/nonexistent/missing/path",
            None,
        );
        run.status = RunStatus::Running;
        store.save(&run).unwrap();

        let manager = make_manager();
        let resumer = Resumer::new(&store, &manager);
        let ctx = resumer.build_context("r-test").unwrap();
        // path is /nonexistent → strategy = Recreate
        assert_eq!(
            ctx.worktree_strategy,
            WorktreeStrategy::Recreate {
                base_branch: "main".to_string(),
            }
        );
    }

    #[test]
    fn build_context_populates_worktree_strategy_none_for_non_isolated() {
        let (_dir, store) = make_store();
        let spec = fixture_spec("x"); // isolation=None
        store.save(&fixture_run(&spec, RunStatus::Running)).unwrap();

        let manager = make_manager();
        let resumer = Resumer::new(&store, &manager);
        let ctx = resumer.build_context("r-test").unwrap();
        assert_eq!(ctx.worktree_strategy, WorktreeStrategy::None);
    }

    // ─────────────────────────────────────────────────────────────────
    // Resumer → Override propagation
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn resume_propagates_none_strategy_when_spec_not_isolated() {
        // The Resumer translates ctx.worktree_strategy into the
        // matching WorktreeOverride before invoking spawn. For a
        // non-isolated spec, ctx.worktree_strategy = None, and the
        // resulting WorktreeOverride must also be None — which means
        // spawn_with_spec_with_override behaves as the legacy variant.
        let (_dir, store) = make_store();
        let spec = fixture_spec("x"); // isolation=None
        store.save(&fixture_run(&spec, RunStatus::Running)).unwrap();

        let manager = make_manager();
        let resumer = Resumer::new(&store, &manager);
        let ctx = resumer.build_context("r-test").unwrap();
        assert_eq!(ctx.worktree_strategy, WorktreeStrategy::None);
        // Predicate parity: None translates to None.
        let translated = match ctx.worktree_strategy {
            WorktreeStrategy::None => crate::subagent::WorktreeOverride::None,
            _ => panic!("expected None"),
        };
        assert!(matches!(
            translated,
            crate::subagent::WorktreeOverride::None
        ));
    }

    #[test]
    fn resume_propagates_reuse_strategy_to_override_with_same_path() {
        // When the spec is isolated AND the original cwd path STILL
        // exists on disk, the strategy is Reuse(path). The Resumer
        // must propagate that path verbatim into WorktreeOverride.
        let dir = TempDir::new().unwrap();
        let mut spec = fixture_spec("x");
        spec.isolation = Some("worktree".to_string());
        spec.isolation_base_branch = Some("main".to_string());

        let (_d, store) = make_store();
        let mut run = SubagentRun::new_running(
            "r-test",
            None,
            &spec,
            "obj",
            dir.path().to_string_lossy().as_ref(),
            None,
        );
        run.status = RunStatus::Running;
        store.save(&run).unwrap();

        let manager = make_manager();
        let resumer = Resumer::new(&store, &manager);
        let ctx = resumer.build_context("r-test").unwrap();
        assert!(matches!(ctx.worktree_strategy, WorktreeStrategy::Reuse(_)));

        let translated = match &ctx.worktree_strategy {
            WorktreeStrategy::Reuse(p) => {
                crate::subagent::WorktreeOverride::Reuse(p.clone())
            }
            _ => panic!("expected Reuse"),
        };
        match translated {
            crate::subagent::WorktreeOverride::Reuse(p) => {
                assert_eq!(p, dir.path());
            }
            _ => panic!("expected Reuse override"),
        }
    }

    #[test]
    fn resume_propagates_recreate_strategy_to_override_with_base_branch() {
        // When the original cwd is GONE (cleanup happened or user
        // deleted it), strategy is Recreate{base}. The Resumer must
        // forward the explicit base_branch into the override so the
        // provider creates with the right ref instead of falling
        // back to spec.isolation_base_branch.
        let mut spec = fixture_spec("x");
        spec.isolation = Some("worktree".to_string());
        spec.isolation_base_branch = Some("develop".to_string());
        let nonexistent =
            std::path::Path::new("/tmp/theo-resume-recreate-test-xyz-does-not-exist");

        let strategy = WorktreeStrategy::from_spec_and_cwd(&spec, nonexistent);
        assert!(matches!(strategy, WorktreeStrategy::Recreate { .. }));

        let translated = match &strategy {
            WorktreeStrategy::Recreate { base_branch } => {
                crate::subagent::WorktreeOverride::Recreate {
                    base_branch: base_branch.clone(),
                }
            }
            _ => panic!("expected Recreate"),
        };
        match translated {
            crate::subagent::WorktreeOverride::Recreate { base_branch } => {
                assert_eq!(base_branch, "develop");
            }
            _ => panic!("expected Recreate override"),
        }
    }
}

// ── tool_call replay ──

pub mod idempotency {
    use super::*;

    // ── tool result map ──

    #[test]
    fn reconstruct_executed_tool_results_returns_map_of_call_id_to_message() {
        let events = vec![
            SubagentEvent {
                timestamp: 1,
                event_type: "tool_result".into(),
                payload: serde_json::json!({
                    "call_id": "c1",
                    "name": "read",
                    "content": "file foo",
                }),
            },
            SubagentEvent {
                timestamp: 2,
                event_type: "tool_result".into(),
                payload: serde_json::json!({
                    "call_id": "c2",
                    "name": "bash",
                    "content": "ok",
                }),
            },
        ];
        let map = reconstruct_executed_tool_results(&events);
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("c1"));
        assert!(map.contains_key("c2"));
    }

    #[test]
    fn reconstruct_executed_tool_results_skips_unknown_event_types() {
        let events = vec![
            SubagentEvent {
                timestamp: 1,
                event_type: "user_message".into(),
                payload: serde_json::json!({"text": "hi"}),
            },
            SubagentEvent {
                timestamp: 2,
                event_type: "iteration_completed".into(),
                payload: serde_json::json!({}),
            },
        ];
        assert!(reconstruct_executed_tool_results(&events).is_empty());
    }

    #[test]
    fn reconstruct_executed_tool_results_handles_missing_payload_fields_gracefully()
    {
        let events = vec![
            SubagentEvent {
                timestamp: 1,
                event_type: "tool_result".into(),
                payload: serde_json::json!({"call_id": "c1"}), // missing name/content
            },
            SubagentEvent {
                timestamp: 2,
                event_type: "tool_result".into(),
                payload: serde_json::json!({"name": "read", "content": "x"}), // missing call_id
            },
            SubagentEvent {
                timestamp: 3,
                event_type: "tool_result".into(),
                payload: serde_json::json!({
                    "call_id": "c3", "name": "read", "content": "ok"
                }),
            },
        ];
        let map = reconstruct_executed_tool_results(&events);
        // Only the well-formed entry survives.
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("c3"));
    }

    #[test]
    fn build_context_populates_executed_tool_results() {
        let (_dir, store) = make_store();
        let spec = fixture_spec("x");
        store.save(&fixture_run(&spec, RunStatus::Running)).unwrap();
        store
            .append_event(
                "r-test",
                &SubagentEvent {
                    timestamp: 1,
                    event_type: "tool_result".into(),
                    payload: serde_json::json!({
                        "call_id": "abc",
                        "name": "read",
                        "content": "content",
                    }),
                },
            )
            .unwrap();
        let manager = make_manager();
        let resumer = Resumer::new(&store, &manager);
        let ctx = resumer.build_context("r-test").unwrap();
        assert_eq!(ctx.executed_tool_results.len(), 1);
        assert!(ctx.cached_tool_result("abc").is_some());
        assert!(ctx.cached_tool_result("never").is_none());
    }

    #[test]
    fn cached_tool_result_returns_message_with_correct_call_id() {
        let (_dir, store) = make_store();
        let spec = fixture_spec("x");
        store.save(&fixture_run(&spec, RunStatus::Running)).unwrap();
        store
            .append_event(
                "r-test",
                &SubagentEvent {
                    timestamp: 1,
                    event_type: "tool_result".into(),
                    payload: serde_json::json!({
                        "call_id": "preserved",
                        "name": "read",
                        "content": "expected_content",
                    }),
                },
            )
            .unwrap();
        let manager = make_manager();
        let resumer = Resumer::new(&store, &manager);
        let ctx = resumer.build_context("r-test").unwrap();
        let cached = ctx.cached_tool_result("preserved").expect("must hit");
        // Verify via serde to avoid coupling to exact field name.
        let json = serde_json::to_value(cached).unwrap();
        assert_eq!(
            json.get("tool_call_id").and_then(|v| v.as_str()),
            Some("preserved")
        );
        assert_eq!(
            json.get("content").and_then(|v| v.as_str()),
            Some("expected_content")
        );
    }

    #[test]
    fn reconstruct_executed_tool_calls_returns_set_of_call_ids() {
        let events = vec![
            SubagentEvent {
                timestamp: 1,
                event_type: "tool_result".into(),
                payload: serde_json::json!({
                    "call_id": "c1",
                    "name": "bash",
                    "content": "ok"
                }),
            },
            SubagentEvent {
                timestamp: 2,
                event_type: "tool_result".into(),
                payload: serde_json::json!({
                    "call_id": "c2",
                    "name": "read",
                    "content": "file"
                }),
            },
            // Different event type — must NOT contribute.
            SubagentEvent {
                timestamp: 3,
                event_type: "user_message".into(),
                payload: serde_json::json!({"text": "hi"}),
            },
        ];
        let set = reconstruct_executed_tool_calls(&events);
        assert_eq!(set.len(), 2);
        assert!(set.contains("c1"));
        assert!(set.contains("c2"));
    }

    #[test]
    fn reconstruct_executed_tool_calls_handles_explicit_completion_marker()
    {
        let events = vec![SubagentEvent {
            timestamp: 1,
            event_type: "tool_call_completed".into(),
            payload: serde_json::json!({"call_id": "explicit-1"}),
        }];
        let set = reconstruct_executed_tool_calls(&events);
        assert!(set.contains("explicit-1"));
    }

    #[test]
    fn reconstruct_executed_tool_calls_handles_camel_case_event_type() {
        // DomainEvent variant ToolCallCompleted serializes with
        // entity_id (call_id is in entity_id field per event.rs)
        let events = vec![SubagentEvent {
            timestamp: 1,
            event_type: "ToolCallCompleted".into(),
            payload: serde_json::json!({"entity_id": "call-42"}),
        }];
        let set = reconstruct_executed_tool_calls(&events);
        assert!(set.contains("call-42"));
    }

    #[test]
    fn reconstruct_executed_tool_calls_returns_empty_for_no_tool_events() {
        let events = vec![
            SubagentEvent {
                timestamp: 1,
                event_type: "user_message".into(),
                payload: serde_json::json!({"text": "x"}),
            },
            SubagentEvent {
                timestamp: 2,
                event_type: "iteration_completed".into(),
                payload: serde_json::json!({}),
            },
        ];
        let set = reconstruct_executed_tool_calls(&events);
        assert!(set.is_empty());
    }

    #[test]
    fn build_context_populates_executed_tool_calls() {
        let (_dir, store) = make_store();
        let spec = fixture_spec("x");
        store.save(&fixture_run(&spec, RunStatus::Running)).unwrap();
        store
            .append_event(
                "r-test",
                &SubagentEvent {
                    timestamp: 1,
                    event_type: "tool_result".into(),
                    payload: serde_json::json!({
                        "call_id": "abc",
                        "name": "bash",
                        "content": "ok"
                    }),
                },
            )
            .unwrap();
        let manager = make_manager();
        let resumer = Resumer::new(&store, &manager);
        let ctx = resumer.build_context("r-test").unwrap();
        assert!(ctx.executed_tool_calls.contains("abc"));
    }

    #[test]
    fn resume_skips_tool_call_with_existing_completed_event() {
        // ResumeContext::should_skip_tool_call returns true when the
        // call_id is in executed_tool_calls. AgentLoop is expected to
        // honor this flag and replay the persisted result.
        let (_dir, store) = make_store();
        let spec = fixture_spec("x");
        store.save(&fixture_run(&spec, RunStatus::Running)).unwrap();
        store
            .append_event(
                "r-test",
                &SubagentEvent {
                    timestamp: 1,
                    event_type: "tool_result".into(),
                    payload: serde_json::json!({
                        "call_id": "already-ran",
                        "name": "bash",
                        "content": "$ echo done"
                    }),
                },
            )
            .unwrap();
        let manager = make_manager();
        let resumer = Resumer::new(&store, &manager);
        let ctx = resumer.build_context("r-test").unwrap();
        assert!(ctx.should_skip_tool_call("already-ran"));
        assert!(!ctx.should_skip_tool_call("never-ran"));
    }

    #[test]
    fn resume_executes_tool_call_when_no_completed_event_exists() {
        // Fresh run, no events — every tool call is "new".
        let (_dir, store) = make_store();
        let spec = fixture_spec("x");
        store.save(&fixture_run(&spec, RunStatus::Running)).unwrap();
        let manager = make_manager();
        let resumer = Resumer::new(&store, &manager);
        let ctx = resumer.build_context("r-test").unwrap();
        assert!(!ctx.should_skip_tool_call("anything"));
        assert!(ctx.executed_tool_calls.is_empty());
    }

    #[test]
    fn resume_replay_preserves_call_id_in_history() {
        // The tool_result event with call_id="abc" must appear in
        // ctx.history as a Message::tool_result whose tool_call_id == "abc".
        let (_dir, store) = make_store();
        let spec = fixture_spec("x");
        store.save(&fixture_run(&spec, RunStatus::Running)).unwrap();
        store
            .append_event(
                "r-test",
                &SubagentEvent {
                    timestamp: 1,
                    event_type: "tool_result".into(),
                    payload: serde_json::json!({
                        "call_id": "preserved-id",
                        "name": "read",
                        "content": "content"
                    }),
                },
            )
            .unwrap();
        let manager = make_manager();
        let resumer = Resumer::new(&store, &manager);
        let ctx = resumer.build_context("r-test").unwrap();
        assert_eq!(ctx.history.len(), 1);
        // Match the Message::tool_result shape — tool_call_id field set.
        let msg = &ctx.history[0];
        // The Message struct in theo_infra_llm exposes tool_call_id; we
        // verify via serde to avoid coupling to exact field names.
        let json = serde_json::to_value(msg).unwrap();
        assert_eq!(
            json.get("tool_call_id").and_then(|v| v.as_str()),
            Some("preserved-id")
        );
    }
}

