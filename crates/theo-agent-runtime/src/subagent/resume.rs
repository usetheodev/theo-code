//! `Resumer` — retoma um sub-agent run não-terminal a partir do event log.
//!
//! Resume Resilience. Sub-agent crashado (Running) ou cancelado
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
    /// Set of `call_id`s that have already
    /// been executed in the original run. The resumed AgentLoop consults
    /// this set before invoking a tool — if the call_id is present, the
    /// tool is skipped (replay mode) and the cached result from the event
    /// log is reused. Prevents double-write side effects (gap #3).
    pub executed_tool_calls: std::collections::BTreeSet<String>,
    /// Map call_id → reconstructed
    /// `Message::tool_result` for every tool that already completed in
    /// the original run. AgentLoop dispatch consults this BEFORE
    /// invoking a tool — when a hit is found, the cached message is
    /// pushed in lieu of dispatch. Closes gap #3.
    pub executed_tool_results: std::collections::BTreeMap<String, Message>,
    /// How the resumer should treat the
    /// worktree. Computed from `spec.isolation` + filesystem inspection.
    pub worktree_strategy: WorktreeStrategy,
}

/// Decides what the resumer
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

    /// Returns the cached `Message::tool_result` for a previously
    /// completed tool call. AgentLoop pushes this to the message history
    /// instead of re-dispatching the tool, closing gap #3.
    pub fn cached_tool_result(&self, call_id: &str) -> Option<&Message> {
        self.executed_tool_results.get(call_id)
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
        // T4.10m / find_p4_006 — `start_iteration` is the COUNT of
        // completed iterations (zero-based when empty, becomes the
        // index of the next iteration to run because iterations are
        // 1-based downstream). H3 from the deep-review (off-by-one
        // in resume replay) was REFUTED here: the next-iteration
        // index equals the count of completed iterations exactly,
        // and `executed_tool_results` is keyed by tool_call_id (not
        // by index), so re-execution detection is index-free.
        let start_iteration = events
            .iter()
            .filter(|e| e.event_type == "iteration_completed")
            .count();
        let executed_tool_calls = reconstruct_executed_tool_calls(&events);
        let executed_tool_results = reconstruct_executed_tool_results(&events);
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
            executed_tool_results,
            worktree_strategy,
        })
    }

    /// Resume: re-spawn com history reconstruído.
    pub async fn resume(&self, run_id: &str) -> Result<AgentResult, ResumeError> {
        self.resume_with_objective(run_id, None).await
    }

    /// Resume com objective opcional (override do original).
    ///
    /// Stages the
    /// reconstructed `ResumeContext` on the manager so the spawned
    /// AgentLoop runs in replay-mode. Tool calls whose `call_id` already
    /// completed in the original run replay from `executed_tool_results`
    /// instead of re-executing — prevents double side-effects.
    pub async fn resume_with_objective(
        &self,
        run_id: &str,
        objective_override: Option<&str>,
    ) -> Result<AgentResult, ResumeError> {
        let ctx = self.build_context(run_id)?;
        let history_msgs = ctx.history.clone();
        let spec = ctx.spec.clone();
        let objective = objective_override.map(String::from).unwrap_or_else(|| {
            format!(
                "[resumed iter {}] {}",
                ctx.start_iteration, spec.description
            )
        });
        // Convert the
        // reconstructed WorktreeStrategy into the Override that
        // spawn_with_spec_with_override understands.
        let wt_override = match &ctx.worktree_strategy {
            WorktreeStrategy::None => crate::subagent::WorktreeOverride::None,
            WorktreeStrategy::Reuse(p) => {
                crate::subagent::WorktreeOverride::Reuse(p.clone())
            }
            WorktreeStrategy::Recreate { base_branch } => {
                crate::subagent::WorktreeOverride::Recreate {
                    base_branch: base_branch.clone(),
                }
            }
        };
        // Stage the context so spawn_with_spec consumes it on entry.
        self.manager
            .set_pending_resume_context(std::sync::Arc::new(ctx));
        let result = self
            .manager
            .spawn_with_spec_with_override(&spec, &objective, Some(history_msgs), wt_override)
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

/// Scan the event log for every
/// completed tool call and reconstruct a `Message::tool_result` keyed by
/// `call_id`. AgentLoop dispatch replays from this map to avoid
/// re-executing tools whose side-effects already happened (gap #3).
///
/// Looks at `tool_result` events whose payload contains the triplet
/// `{call_id, name, content}` (the same shape `reconstruct_history`
/// already trusts). Events lacking any of those fields are skipped
/// (best-effort, never panics).
pub fn reconstruct_executed_tool_results(
    events: &[SubagentEvent],
) -> std::collections::BTreeMap<String, Message> {
    let mut out = std::collections::BTreeMap::new();
    for e in events {
        if e.event_type != "tool_result" {
            continue;
        }
        let call_id = match e.payload.get("call_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let name = match e.payload.get("name").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let content = match e.payload.get("content").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        out.insert(
            call_id.to_string(),
            Message::tool_result(call_id, name, content),
        );
    }
    out
}

/// Scan the event log for every tool call
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


// Sibling tests split per-area (T3.7 of code-hygiene-5x5).
#[cfg(test)]
#[path = "resume_test_helpers.rs"]
mod resume_test_helpers;
#[cfg(test)]
#[path = "resume_build_context_tests.rs"]
mod resume_build_context_tests;
#[cfg(test)]
#[path = "resume_reconstruct_history_tests.rs"]
mod resume_reconstruct_history_tests;
#[cfg(test)]
#[path = "resume_resume_tests.rs"]
mod resume_resume_tests;
