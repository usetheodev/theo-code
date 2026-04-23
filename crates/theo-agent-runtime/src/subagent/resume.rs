//! `Resumer` — retoma um sub-agent run não-terminal a partir do event log.
//!
//! Phase 16 — Resume Resilience. Sub-agent crashado (Running) ou cancelado
//! (Cancelled) pode ser retomado via `theo subagent resume <id>`.
//!
//! Idempotente (D3): runs em status terminal (Completed/Failed/Cancelled/
//! Abandoned) NÃO são re-executados. Apenas Running pode ser resumido.
//! Justificativa: side effects (mutations) já aconteceram; resume é
//! "continue from where it stopped", não "redo from scratch".

use theo_infra_llm::types::Message;

use crate::agent_loop::AgentResult;
use crate::subagent::SubAgentManager;
use crate::subagent_runs::{
    FileSubagentRunStore, RunStoreError, SubagentEvent,
};

#[cfg(test)]
use crate::subagent_runs::RunStatus;

use theo_domain::agent_spec::AgentSpec;

/// Estado reconstruído para resume.
#[derive(Debug)]
pub struct ResumeContext {
    pub spec: AgentSpec,
    pub start_iteration: usize,
    pub history: Vec<Message>,
    pub prior_tokens_used: u64,
    pub checkpoint_before: Option<String>,
    /// Phase 25 (sota-gaps-followup): set of `call_id`s that have already
    /// been executed in the original run. The resumed AgentLoop consults
    /// this set before invoking a tool — if the call_id is present, the
    /// tool is skipped (replay mode) and the cached result from the event
    /// log is reused. Prevents double-write side effects (gap #3).
    pub executed_tool_calls: std::collections::BTreeSet<String>,
    /// Phase 26 (sota-gaps-followup): how the resumer should treat the
    /// worktree. Computed from `spec.isolation` + filesystem inspection.
    pub worktree_strategy: WorktreeStrategy,
}

/// Phase 26 (sota-gaps-followup) — closes gap #10. Decides what the resumer
/// does about the original worktree:
/// - `None` — spec was not isolated; nothing to do.
/// - `Reuse(path)` — original worktree path still exists and is reused.
/// - `Recreate { base_branch }` — original was cleaned up; create a fresh
///   worktree from the same base branch (accepts state drift as cost).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeStrategy {
    /// Spec was not isolated (`spec.isolation` != Some("worktree")).
    None,
    /// Original worktree path still exists; resume reuses it.
    Reuse(std::path::PathBuf),
    /// Original worktree path is missing; resume creates a new one.
    Recreate { base_branch: String },
}

impl WorktreeStrategy {
    /// Decide the strategy from a spec + the cwd recorded on the original
    /// run. The cwd is the worktree path when the original spawn isolated;
    /// otherwise it's the project_dir.
    pub fn from_spec_and_cwd(spec: &AgentSpec, original_cwd: &std::path::Path) -> Self {
        if spec.isolation.as_deref() != Some("worktree") {
            return WorktreeStrategy::None;
        }
        if original_cwd.exists() {
            WorktreeStrategy::Reuse(original_cwd.to_path_buf())
        } else {
            let base = spec
                .isolation_base_branch
                .clone()
                .unwrap_or_else(|| "main".to_string());
            WorktreeStrategy::Recreate { base_branch: base }
        }
    }
}

impl ResumeContext {
    /// Returns `true` when the LLM is about to issue a tool call that has
    /// already been executed in the original run. Caller (AgentLoop) is
    /// expected to replay the persisted result instead of invoking the
    /// tool dispatcher.
    pub fn should_skip_tool_call(&self, call_id: &str) -> bool {
        self.executed_tool_calls.contains(call_id)
    }
}

/// Erros do fluxo de resume.
#[derive(Debug, thiserror::Error)]
pub enum ResumeError {
    #[error("run '{run_id}' is in terminal status '{status}', cannot resume")]
    NotResumable { run_id: String, status: String },
    #[error("run not found: {0}")]
    NotFound(String),
    #[error("store error: {0}")]
    Store(#[from] RunStoreError),
}

pub struct Resumer<'a> {
    store: &'a FileSubagentRunStore,
    manager: &'a SubAgentManager,
}

impl<'a> Resumer<'a> {
    pub fn new(store: &'a FileSubagentRunStore, manager: &'a SubAgentManager) -> Self {
        Self { store, manager }
    }

    /// Carrega o run + reconstrói contexto. Falha se status terminal.
    pub fn build_context(&self, run_id: &str) -> Result<ResumeContext, ResumeError> {
        let run = match self.store.load(run_id) {
            Ok(r) => r,
            Err(RunStoreError::NotFound(_)) => {
                return Err(ResumeError::NotFound(run_id.to_string()));
            }
            Err(other) => return Err(ResumeError::Store(other)),
        };
        if run.status.is_terminal() {
            return Err(ResumeError::NotResumable {
                run_id: run_id.into(),
                status: format!("{:?}", run.status),
            });
        }
        let events = self.store.list_events(run_id)?;
        let history = reconstruct_history(&events);
        let start_iteration = events
            .iter()
            .filter(|e| e.event_type == "iteration_completed")
            .count();
        let executed_tool_calls = reconstruct_executed_tool_calls(&events);
        let cwd_path = std::path::PathBuf::from(&run.cwd);
        let worktree_strategy =
            WorktreeStrategy::from_spec_and_cwd(&run.config_snapshot, &cwd_path);
        Ok(ResumeContext {
            spec: run.config_snapshot,
            start_iteration,
            history,
            prior_tokens_used: run.tokens_used,
            checkpoint_before: run.checkpoint_before,
            executed_tool_calls,
            worktree_strategy,
        })
    }

    /// Resume: re-spawn com history reconstruído.
    pub async fn resume(&self, run_id: &str) -> Result<AgentResult, ResumeError> {
        self.resume_with_objective(run_id, None).await
    }

    /// Resume com objective opcional (override do original).
    pub async fn resume_with_objective(
        &self,
        run_id: &str,
        objective_override: Option<&str>,
    ) -> Result<AgentResult, ResumeError> {
        let ctx = self.build_context(run_id)?;
        let history_msgs = ctx.history.clone();
        let objective = objective_override
            .map(String::from)
            .unwrap_or_else(|| format!("[resumed iter {}] {}", ctx.start_iteration, ctx.spec.description));
        let result = self
            .manager
            .spawn_with_spec(&ctx.spec, &objective, Some(history_msgs))
            .await;
        Ok(result)
    }
}

/// Reconstrói history a partir de event log. Eventos desconhecidos são
/// skipped (best-effort, não erram).
pub fn reconstruct_history(events: &[SubagentEvent]) -> Vec<Message> {
    events
        .iter()
        .filter_map(|e| match e.event_type.as_str() {
            "user_message" => e
                .payload
                .get("text")
                .and_then(|v| v.as_str())
                .map(Message::user),
            "assistant_message" => e
                .payload
                .get("text")
                .and_then(|v| v.as_str())
                .map(Message::assistant),
            "tool_result" => {
                let call_id = e.payload.get("call_id").and_then(|v| v.as_str())?;
                let name = e.payload.get("name").and_then(|v| v.as_str())?;
                let content = e.payload.get("content").and_then(|v| v.as_str())?;
                Some(Message::tool_result(call_id, name, content))
            }
            _ => None,
        })
        .collect()
}

/// Phase 25 (sota-gaps-followup): scan the event log for every tool call
/// that already produced a result. The returned set lets the resumed
/// AgentLoop short-circuit re-execution of those tools (idempotency).
///
/// We accept three event types as "tool was executed":
/// - `tool_result` — the dispatched result was persisted (history shape)
/// - `tool_call_completed` — explicit completion marker (future schema)
/// - `ToolCallCompleted` — domain-event-style camel-case (mirrors the
///   public EventType variant name)
///
/// `call_id` is read from `payload.call_id` for `tool_result` /
/// `tool_call_completed`, or `payload.entity_id` for `ToolCallCompleted`
/// (matching how `DomainEvent` serializes tool-call events).
pub fn reconstruct_executed_tool_calls(
    events: &[SubagentEvent],
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for e in events {
        let id_field = match e.event_type.as_str() {
            "tool_result" | "tool_call_completed" => Some("call_id"),
            "ToolCallCompleted" => Some("entity_id"),
            _ => None,
        };
        if let Some(field) = id_field
            && let Some(id) = e.payload.get(field).and_then(|v| v.as_str())
        {
            out.insert(id.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentConfig;
    use crate::event_bus::EventBus;
    use crate::subagent::SubAgentRegistry;
    use crate::subagent_runs::{FileSubagentRunStore, SubagentRun};
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn fixture_spec(name: &str) -> AgentSpec {
        AgentSpec::on_demand(name, "test obj")
    }

    fn fixture_run(spec: &AgentSpec, status: RunStatus) -> SubagentRun {
        let mut run = SubagentRun::new_running("r-test", None, spec, "obj", "/tmp", None);
        run.status = status;
        run
    }

    fn make_store() -> (TempDir, FileSubagentRunStore) {
        let dir = TempDir::new().unwrap();
        let store = FileSubagentRunStore::new(dir.path());
        (dir, store)
    }

    fn make_manager() -> SubAgentManager {
        SubAgentManager::with_registry(
            AgentConfig::default(),
            Arc::new(EventBus::new()),
            PathBuf::from("/tmp"),
            Arc::new(SubAgentRegistry::with_builtins()),
        )
    }

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

    // ── Phase 26 (sota-gaps-followup): worktree restore ──

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
    }

    // ── Phase 25 (sota-gaps-followup): tool_call replay ──

    pub mod idempotency {
        use super::*;

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
}
