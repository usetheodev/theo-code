// Submodules extracted from the original single-file run_engine.rs as
// part of Fase 4 (REMEDIATION_PLAN T4.2). Each needs access to private
// fields of `AgentRunEngine` declared in this module — that is why
// they live as child modules rather than siblings.
mod bootstrap;
mod builders;
mod contexts;
mod delegate_handler;
mod dispatch;
mod execution;
mod handoff;
mod iteration_prelude;
mod lifecycle;
mod llm_call;
mod main_loop;
mod post_dispatch_updates;
mod stream_batcher;
mod text_response;

pub use contexts::{
    LlmContext, ObservabilityContext, RuntimeContext, SubagentContext, TrackingContext,
};
pub use handoff::HandoffOutcome;

use std::path::PathBuf;
use std::sync::Arc;

use theo_domain::agent_run::{AgentRun, RunState};
use theo_domain::budget::Budget;
use theo_domain::event::{DomainEvent, EventType};
use theo_domain::identifiers::{RunId, TaskId};
use theo_infra_llm::LlmClient;
use theo_tooling::registry::ToolRegistry;

#[cfg(test)]
use crate::agent_loop::AgentResult;
use crate::budget_enforcer::BudgetEnforcer;
use crate::config::AgentConfig;
use crate::convergence::{
    ConvergenceEvaluator, ConvergenceMode, EditSuccessConvergence, GitDiffConvergence,
};
use crate::event_bus::EventBus;
use crate::loop_state::ContextLoopState;
use crate::metrics::{MetricsCollector, RuntimeMetrics};
use crate::task_manager::TaskManager;
use crate::tool_call_manager::ToolCallManager;

/// Formal agent run engine with RunState machine.
///
/// Wraps the agent loop logic with formal state transitions and event publishing.
/// Enforces Invariant 6: every execution has a unique run_id.
pub struct AgentRunEngine {
    run: AgentRun,
    task_id: TaskId,
    task_manager: Arc<TaskManager>,
    tool_call_manager: Arc<ToolCallManager>,
    event_bus: Arc<EventBus>,
    config: AgentConfig,
    project_dir: PathBuf,
    /// T3.1 PR5 / find_p3_001 — LLM-execution bundle: client, registry,
    /// convergence, budget_enforcer.
    llm: contexts::LlmContext,
    /// T3.1 PR3 / find_p3_001 — state-tracking bundle (done_attempts,
    /// plan_mode_nudged, failure_tracker, checkpoint_taken_this_turn).
    tracking: contexts::TrackingContext,
    /// T3.1 PR4 / find_p3_001 — runtime helper handles bundle.
    /// Replaces snapshot_store, graph_context, context_loop_state,
    /// message_queues, session_token_usage, memory_nudge_counter,
    /// skill_nudge_counter, skill_created_this_task,
    /// autodream_attempted, resume_context.
    rt: contexts::RuntimeContext,
    /// T3.1 PR2 / find_p3_001 — observability + working-set bundle.
    /// Replaces the previous flat fields:
    /// `metrics`, `working_set`, `context_metrics`, `observability`,
    /// `episodes_injected`, `episodes_created`,
    /// `initial_context_files`, `pre_compaction_hot_files`,
    /// `last_run_report`.
    obs: contexts::ObservabilityContext,
    /// T3.1 PR1 / find_p3_001 — sub-agent integration plumbing
    /// (registry, run_store, hooks, cancellation, checkpoint,
    /// worktree, mcp + discovery + dispatcher, handoff_guardrails,
    /// reloadable) bundled into a single owned struct. Replaces the
    /// previous 10 flat `subagent_*` fields. See
    /// `run_engine/contexts/subagent.rs`.
    subagent: contexts::SubagentContext,
}

impl AgentRunEngine {
    /// Creates a new RunEngine. Generates unique run_id (Invariant 6).
    /// Publishes RunInitialized event.
    pub fn new(
        task_id: TaskId,
        task_manager: Arc<TaskManager>,
        tool_call_manager: Arc<ToolCallManager>,
        event_bus: Arc<EventBus>,
        client: LlmClient,
        registry: Arc<ToolRegistry>,
        config: AgentConfig,
        project_dir: PathBuf,
    ) -> Self {
        let now = now_millis();
        let run = AgentRun {
            run_id: RunId::generate(),
            task_id: task_id.clone(),
            state: RunState::Initialized,
            iteration: 0,
            max_iterations: config.loop_cfg().max_iterations,
            created_at: now,
            updated_at: now,
        };

        // Observability pipeline + LoopDetectingListener (T1.6 + T4.4) installed
        // BEFORE RunInitialized so the event is captured.
        let observability = (!config.loop_cfg().is_subagent).then(|| {
            crate::observability::install_observability(
                &event_bus,
                run.run_id.as_str(),
                project_dir.join(".theo").join("trajectories"),
            )
        });

        event_bus.publish(DomainEvent::new(
            EventType::RunInitialized,
            run.run_id.as_str(),
            serde_json::json!({
                "task_id": task_id.as_str(),
                "max_iterations": config.loop_cfg().max_iterations,
            }),
        ));

        let context_loop_state = ContextLoopState::new();

        let budget = Budget {
            max_iterations: config.loop_cfg().max_iterations,
            ..Budget::default()
        };
        let budget_enforcer = BudgetEnforcer::new(budget, event_bus.clone(), run.run_id.as_str());
        let metrics = Arc::new(MetricsCollector::new());
        let convergence = ConvergenceEvaluator::new(
            vec![
                Box::new(GitDiffConvergence),
                Box::new(EditSuccessConvergence),
            ],
            ConvergenceMode::AllOf,
        );

        let failure_tracker = crate::failure_tracker::FailurePatternTracker::new(&project_dir);

        Self {
            run,
            task_id,
            task_manager,
            tool_call_manager,
            event_bus,
            config,
            project_dir,
            llm: contexts::LlmContext::new(client, registry, convergence, budget_enforcer),
            tracking: contexts::TrackingContext::new(failure_tracker),
            rt: contexts::RuntimeContext::new(context_loop_state),
            obs: contexts::ObservabilityContext::new(metrics, observability),
            subagent: contexts::SubagentContext::default(),
        }
    }

    /// Lazy: build the McpDispatcher from the subagent MCP registry on
    /// first call. Returns `None` if no MCP registry is attached.
    /// T3.1 PR1 — delegates to `SubagentContext::mcp_dispatcher`.
    pub fn mcp_dispatcher(&self) -> Option<Arc<theo_infra_mcp::McpDispatcher>> {
        self.subagent.mcp_dispatcher()
    }

    /// Dispatch a tool call to MCP if its name is in the
    /// `mcp:<server>:<tool>` namespace. Returns `Some(message)` on
    /// dispatch (success or RPC failure → error message). Returns `None`
    /// when the tool is not MCP — the caller falls back to normal dispatch.
    pub async fn try_dispatch_mcp_tool(&self, call: &theo_infra_llm::types::ToolCall) -> Option<theo_infra_llm::types::Message> {
        let name = &call.function.name;
        if !theo_infra_mcp::McpDispatcher::handles(name) {
            return None;
        }
        let dispatcher = self.mcp_dispatcher()?;
        let args = call.parse_arguments().unwrap_or_default();
        let result_text = match dispatcher.dispatch(name, args).await {
            Ok(out) => {
                if out.is_error {
                    format!("[mcp error] {}", out.text)
                } else {
                    out.text
                }
            }
            Err(err) => format!("[mcp dispatch failed] {}", err),
        };
        // T2.2 / find_p6_003 / D5 — MCP responses are remote, untrusted
        // input and must be fenced before reaching the LLM. Same
        // `fence_untrusted` helper as T2.1 (regular tools); the source
        // label keeps `mcp:` so audit trails distinguish remote vs
        // local tool output.
        let fenced = theo_domain::prompt_sanitizer::fence_untrusted(
            &result_text,
            &format!("mcp:{name}"),
            crate::constants::MAX_TOOL_OUTPUT_BYTES,
        );
        Some(theo_infra_llm::types::Message::tool_result(
            &call.id,
            name,
            &fenced,
        ))
    }

    /// At the start of a turn, reset the once-per-turn snapshot flag.
    pub fn reset_turn_checkpoint(&self) {
        // Release: pairs with the Acquire failure-ordering in the CAS
        // below. Ensures any subsequent reads of checkpoint-related state
        // observe a fully-committed reset (T5.4).
        self.tracking.checkpoint_taken_this_turn
            .store(false, std::sync::atomic::Ordering::Release);
    }

    /// Take a snapshot if (a) a checkpoint manager is attached AND
    /// (b) the tool is mutating AND (c) no snapshot was taken this turn.
    /// Idempotent within a turn. Returns the SHA on a fresh snapshot,
    /// None otherwise.
    pub fn maybe_checkpoint_for_tool(&self, tool_name: &str, turn_id: u32) -> Option<String> {
        if !Self::is_mutating_tool(tool_name) {
            return None;
        }
        // Compare-and-swap: only snapshot if not already taken this turn.
        // AcqRel on success publishes the flag; Acquire on failure pairs
        // with the Release in `reset_turn_checkpoint` so the losing racer
        // observes a fully committed reset (T5.4).
        if self
            .tracking
            .checkpoint_taken_this_turn
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_err()
        {
            return None;
        }
        self.checkpoint_before_mutation(&format!("turn-{}-pre-{}", turn_id, tool_name))
    }

    // All `with_*` builders moved to `builders.rs` (Fase 4 — T4.2).

    /// Snapshot the workdir BEFORE a mutating tool fires (edit / write /
    /// apply_patch / bash). Idempotent within a turn — caller tracks
    /// the once-per-turn state. Returns the commit SHA on success, None
    /// if no checkpoint manager is attached or snapshot fails (fail-soft).
    pub fn checkpoint_before_mutation(&self, label: &str) -> Option<String> {
        self.subagent
            .checkpoint
            .as_ref()
            .and_then(|cm| cm.snapshot(label).ok())
    }

    /// Returns true if `tool_name` is a mutating tool that warrants a
    /// pre-mutation checkpoint snapshot.
    pub fn is_mutating_tool(tool_name: &str) -> bool {
        matches!(tool_name, "edit" | "write" | "apply_patch" | "bash")
    }

    /// Accumulated token usage (for CLI display).
    pub fn session_token_usage(&self) -> &theo_domain::budget::TokenUsage {
        &self.rt.session_token_usage
    }

    /// Returns the run_id.
    pub fn run_id(&self) -> &RunId { &self.run.run_id }

    /// Returns the current RunState.
    pub fn state(&self) -> RunState { self.run.state }

    /// Returns the current iteration.
    pub fn iteration(&self) -> usize {
        self.run.iteration
    }

    /// Returns a snapshot of current runtime metrics.
    pub fn metrics(&self) -> RuntimeMetrics {
        self.obs.metrics.snapshot()
    }

    /// Borrow the underlying [`MetricsCollector`] for sites that need
    /// to call its mutating methods (`record_*`). T3.1 PR2.
    #[allow(dead_code)] // retained for upcoming T3.1 callers
    pub(crate) fn metrics_collector(&self) -> &Arc<MetricsCollector> {
        &self.obs.metrics
    }

    /// Exposes the pair `(files_edited, current_iteration)` for
    /// `AgentResult::from_engine_state`. Keeps internal fields private.
    pub fn run_result_context(&self) -> (Vec<String>, usize) {
        (
            self.rt.context_loop_state.edits_files.clone(),
            self.run.iteration,
        )
    }

    /// Takes the RunReport captured by the last finalize_observability call.
    pub fn take_run_report(&mut self) -> Option<crate::observability::report::RunReport> {
        self.obs.last_run_report.take()
    }

    // `execute` + `execute_with_history` moved to `run_engine/execution.rs`.
    // `record_session_exit` + `record_session_exit_public` +
    // `finalize_observability` moved to `lifecycle.rs` (Fase 4 — T4.2).

    /// Attempt a task-state transition and observe genuine failures.
    ///
    /// Replaces the `let _ = self.task_manager.transition(...)` pattern
    /// that silently discarded both no-op transitions (semantically
    /// fine) and *real* invalid transitions (which signal state-machine
    /// divergence and need to be observable).
    ///
    /// T1.4 / find_p4_005 / INV-002. Idempotent for same-state targets;
    /// emits `tracing::error!` + `EventType::Error` for every other
    /// failure so downstream listeners (metrics, OTel, dashboards) can
    /// see the divergence.
    pub(crate) fn try_task_transition(&self, target: theo_domain::task::TaskState) {
        if let Err(e) = self.task_manager.transition(&self.task_id, target) {
            if e.is_already_in_state() {
                // Idempotent no-op — caller's intent is satisfied.
                return;
            }
            tracing::error!(
                run_id = %self.run.run_id,
                target = ?target,
                error = %e,
                "task transition failed unexpectedly"
            );
            self.event_bus.publish(DomainEvent::new(
                EventType::Error,
                self.run.run_id.as_str(),
                serde_json::json!({
                    "kind": "task_transition_failed",
                    "target": format!("{:?}", target),
                    "error": e.to_string(),
                }),
            ));
        }
    }

    /// Publish a failure to persist a message to the state manager.
    ///
    /// Used by `execute_with_history` when `StateManager::append_message`
    /// returns `Err` — historically this was discarded via `let _ = ...`,
    /// leaving the JSONL crash-recovery file inconsistent with no
    /// operational signal. T1.3 / find_p4_002 / INV-002.
    ///
    /// Emits both a `tracing::error!` (for log aggregation) and an
    /// `EventType::Error` event on the bus (for in-process listeners,
    /// metrics, OTel export). The run is allowed to continue —
    /// persistence is best-effort — but the failure is now observable.
    pub(crate) fn publish_state_append_failure(
        &self,
        role: &str,
        err: &crate::session_tree::SessionTreeError,
    ) {
        tracing::error!(
            run_id = %self.run.run_id,
            role = role,
            error = %err,
            "state_manager append failed; resume may be incomplete"
        );
        self.event_bus.publish(DomainEvent::new(
            EventType::Error,
            self.run.run_id.as_str(),
            serde_json::json!({
                "kind": "state_manager_append_failed",
                "role": role,
                "error": err.to_string(),
            }),
        ));
    }

    /// Transition RunState and publish event.
    fn transition_run(&mut self, target: RunState) {
        let from = self.run.state;
        // Allow re-entering same state (Planning after Replanning→Planning)
        if from == target {
            return;
        }
        if let Ok(()) = self.run.transition(target) {
            self.event_bus.publish(DomainEvent::new(
                EventType::RunStateChanged,
                self.run.run_id.as_str(),
                serde_json::json!({
                    "from": format!("{:?}", from),
                    "to": format!("{:?}", target),
                }),
            ));
        }
    }

    // `handle_delegate_task` moved to `run_engine/delegate_handler.rs` —
    // split into `build_subagent_manager`, `resolve_handoff_guardrails`,
    // `delegate_single`, `delegate_parallel`, `apply_handoff_guardrails`.
    //
    // `evaluate_handoff` + `evaluate_handoff_or_refuse` moved to
    // `run_engine/handoff.rs`. See those files for docs.
}

// `llm_error_to_class` (and friends — `truncate_handoff_objective`,
// `truncate_batch_args`, `derive_provider_hint`) were extracted to
// `run_engine_helpers.rs` in Fase 4 — see `use` alias at bottom of
// file. Auto-init + sandbox spawn likewise moved to their own modules.

use theo_domain::clock::now_millis;

// NOTE: `derive_provider_hint`, `llm_error_to_class`,
// `truncate_batch_args`, `truncate_handoff_objective`,
// `auto_init_project_context`, `spawn_done_gate_cargo`, `DoomLoopTracker`
// usage all moved to `run_engine_helpers.rs` / `run_engine_auto_init.rs` /
// `run_engine_sandbox.rs` / `execution.rs` in Fase 4 (REMEDIATION_PLAN T4.2).


// Sibling tests split per area (T3.2 of code-hygiene-5x5).
#[cfg(test)]
#[path = "test_helpers.rs"]
mod test_helpers;
#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
#[cfg(test)]
#[path = "delegate_tests.rs"]
mod delegate_tests;
#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod dispatch_tests;
#[cfg(test)]
#[path = "variants_tests.rs"]
mod variants_tests;
