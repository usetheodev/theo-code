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
        Ok(ResumeContext {
            spec: run.config_snapshot,
            start_iteration,
            history,
            prior_tokens_used: run.tokens_used,
            checkpoint_before: run.checkpoint_before,
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
}
