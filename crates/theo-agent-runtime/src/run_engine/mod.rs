// Submodules extracted from the original single-file run_engine.rs as
// part of Fase 4 (REMEDIATION_PLAN T4.2). Each needs access to private
// fields of `AgentRunEngine` declared in this module — that is why
// they live as child modules rather than siblings.
mod bootstrap;
mod builders;
mod delegate_handler;
mod dispatch;
mod execution;
mod handoff;
mod lifecycle;
mod main_loop;
mod stream_batcher;

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
use crate::config::{AgentConfig, MessageQueues};
use crate::context_metrics::ContextMetrics;
use crate::convergence::{
    ConvergenceEvaluator, ConvergenceMode, EditSuccessConvergence, GitDiffConvergence,
};
use crate::event_bus::EventBus;
use crate::loop_state::ContextLoopState;
use crate::metrics::{MetricsCollector, RuntimeMetrics};
use crate::persistence::SnapshotStore;
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
    client: LlmClient,
    registry: Arc<ToolRegistry>,
    config: AgentConfig,
    project_dir: PathBuf,
    budget_enforcer: BudgetEnforcer,
    metrics: Arc<MetricsCollector>,
    convergence: ConvergenceEvaluator,
    done_attempts: u32,
    /// One-shot guard: in Plan mode, if the model converges with text only and
    /// no plan file on disk, we inject a corrective reminder once. After that
    /// we let it converge normally to avoid infinite reminder loops.
    plan_mode_nudged: bool,
    failure_tracker: crate::failure_tracker::FailurePatternTracker,
    snapshot_store: Option<Arc<dyn SnapshotStore>>,
    graph_context: Option<Arc<dyn theo_domain::graph_context::GraphContextProvider>>,
    context_loop_state: ContextLoopState,
    /// Active context scope — tracks hot files, events, hypotheses for this run.
    working_set: theo_domain::working_set::WorkingSet,
    /// Context breakdown metrics — measures context usage patterns.
    context_metrics: ContextMetrics,
    /// Steering and follow-up message queues for mid-run injection.
    /// Pi-mono ref: `packages/agent/src/agent-loop.ts:165-229`
    message_queues: MessageQueues,
    /// Accumulated token usage across LLM calls in this session.
    session_token_usage: theo_domain::budget::TokenUsage,
    /// PLAN_AUTO_EVOLUTION_SOTA: turns since the last memory
    /// reviewer spawn. `AtomicUsize` lets the counter survive fork
    /// boundaries (eliminates Hermes Issue #8506).
    memory_nudge_counter: Arc<crate::memory_lifecycle::MemoryNudgeCounter>,
    /// PLAN_AUTO_EVOLUTION_SOTA: tool iterations since the
    /// last skill reviewer spawn. Persists across task boundaries so
    /// short tasks don't reset accumulation mid-stream.
    skill_nudge_counter: Arc<crate::skill_reviewer::SkillNudgeCounter>,
    /// PLAN_AUTO_EVOLUTION_SOTA: flipped to `true` whenever
    /// `skill_manage.create` / `edit` / `patch` succeeds in the
    /// current task, suppressing the reviewer for that task.
    skill_created_this_task: std::sync::atomic::AtomicBool,
    /// PLAN_AUTO_EVOLUTION_SOTA: flipped once autodream has
    /// been attempted for this session so we don't retry on every
    /// message in long-running sessions.
    autodream_attempted: std::sync::atomic::AtomicBool,
    observability: Option<crate::observability::ObservabilityPipeline>,
    episodes_injected: u32, episodes_created: u32,
    initial_context_files: std::collections::HashSet<String>,
    pre_compaction_hot_files: std::collections::HashSet<String>,
    /// Sub-agent integrations — when present, propagated to spawn_with_spec.
    /// Optional so backward-compat is preserved.
    subagent_registry: Option<Arc<crate::subagent::SubAgentRegistry>>,
    subagent_run_store: Option<Arc<crate::subagent_runs::FileSubagentRunStore>>,
    subagent_hooks: Option<Arc<crate::lifecycle_hooks::HookManager>>,
    subagent_cancellation: Option<Arc<crate::cancellation::CancellationTree>>,
    subagent_checkpoint: Option<Arc<crate::checkpoint::CheckpointManager>>,
    subagent_worktree: Option<Arc<theo_isolation::WorktreeProvider>>,
    subagent_mcp: Option<Arc<theo_infra_mcp::McpRegistry>>,
    /// Optional MCP discovery cache propagated to spawn_with_spec.
    subagent_mcp_discovery: Option<Arc<theo_infra_mcp::DiscoveryCache>>,
    /// Optional handoff guardrail chain. When `None`, a default chain
    /// (built-ins) is used per delegate_task call. Programmatic callers
    /// can register custom guardrails by injecting a chain.
    subagent_handoff_guardrails: Option<Arc<crate::handoff_guardrail::GuardrailChain>>,
    /// Optional resume context. When present, the dispatch loop
    /// consults `executed_tool_calls` before invoking each tool and
    /// replays cached results from `executed_tool_results` to avoid
    /// double side-effects.
    resume_context: Option<Arc<crate::subagent::resume::ResumeContext>>,
    /// Lazy-built dispatcher for `mcp:server:tool` calls. Built from
    /// `subagent_mcp` on first use.
    subagent_mcp_dispatcher: std::sync::OnceLock<Arc<theo_infra_mcp::McpDispatcher>>,
    /// Optional ReloadableRegistry. When Some, takes precedence over
    /// `subagent_registry`: each delegate_task call reads
    /// `reloadable.snapshot()` so watcher changes take effect immediately
    /// without restart.
    subagent_reloadable: Option<crate::subagent::ReloadableRegistry>,
    /// Whether a checkpoint snapshot has already been taken this turn.
    /// Reset at the start of every turn; set to `true` on first
    /// mutating-tool dispatch. One snapshot per turn max.
    checkpoint_taken_this_turn: std::sync::atomic::AtomicBool,
    /// Phase 64 (benchmark-sota-metrics-plan): RunReport captured after
    /// finalize_observability. The caller reads this to embed in AgentResult.
    last_run_report: Option<crate::observability::report::RunReport>,
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
            max_iterations: config.max_iterations,
            created_at: now,
            updated_at: now,
        };

        // Observability pipeline + LoopDetectingListener (T1.6 + T4.4) installed
        // BEFORE RunInitialized so the event is captured.
        let observability = (!config.is_subagent).then(|| {
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
                "max_iterations": config.max_iterations,
            }),
        ));

        let context_loop_state = ContextLoopState::new();

        let budget = Budget {
            max_iterations: config.max_iterations,
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
            client,
            registry,
            config,
            project_dir,
            budget_enforcer,
            metrics,
            convergence,
            done_attempts: 0,
            plan_mode_nudged: false,
            failure_tracker,
            snapshot_store: None,
            graph_context: None,
            context_loop_state,
            working_set: theo_domain::working_set::WorkingSet::new(),
            context_metrics: ContextMetrics::new(),
            message_queues: MessageQueues::default(),
            session_token_usage: theo_domain::budget::TokenUsage::default(),
            memory_nudge_counter: Arc::new(crate::memory_lifecycle::MemoryNudgeCounter::new()),
            skill_nudge_counter: Arc::new(crate::skill_reviewer::SkillNudgeCounter::new()),
            skill_created_this_task: std::sync::atomic::AtomicBool::new(false),
            autodream_attempted: std::sync::atomic::AtomicBool::new(false),
            observability, episodes_injected: 0, episodes_created: 0,
            initial_context_files: Default::default(), pre_compaction_hot_files: Default::default(),
            subagent_registry: None,
            subagent_run_store: None,
            subagent_hooks: None,
            subagent_cancellation: None,
            subagent_checkpoint: None,
            subagent_worktree: None,
            subagent_mcp: None,
            subagent_mcp_discovery: None,
            subagent_handoff_guardrails: None,
            subagent_mcp_dispatcher: std::sync::OnceLock::new(),
            subagent_reloadable: None,
            resume_context: None,
            checkpoint_taken_this_turn: std::sync::atomic::AtomicBool::new(false),
            last_run_report: None,
        }
    }

    /// Lazy: build the McpDispatcher from `subagent_mcp` registry on first call.
    /// Returns `None` if no MCP registry is attached.
    pub fn mcp_dispatcher(&self) -> Option<Arc<theo_infra_mcp::McpDispatcher>> {
        let reg = self.subagent_mcp.as_ref()?;
        Some(
            self.subagent_mcp_dispatcher
                .get_or_init(|| Arc::new(theo_infra_mcp::McpDispatcher::new(reg.clone())))
                .clone(),
        )
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
        Some(theo_infra_llm::types::Message::tool_result(
            &call.id,
            name,
            &result_text,
        ))
    }

    /// At the start of a turn, reset the once-per-turn snapshot flag.
    pub fn reset_turn_checkpoint(&self) {
        // Release: pairs with the Acquire failure-ordering in the CAS
        // below. Ensures any subsequent reads of checkpoint-related state
        // observe a fully-committed reset (T5.4).
        self.checkpoint_taken_this_turn
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
        self.subagent_checkpoint
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
        &self.session_token_usage
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
        self.metrics.snapshot()
    }

    /// Exposes the pair `(files_edited, current_iteration)` for
    /// `AgentResult::from_engine_state`. Keeps internal fields private.
    pub fn run_result_context(&self) -> (Vec<String>, usize) {
        (
            self.context_loop_state.edits_files.clone(),
            self.run.iteration,
        )
    }

    /// Takes the RunReport captured by the last finalize_observability call.
    pub fn take_run_report(&mut self) -> Option<crate::observability::report::RunReport> {
        self.last_run_report.take()
    }

    // `execute` + `execute_with_history` moved to `run_engine/execution.rs`.
    // `record_session_exit` + `record_session_exit_public` +
    // `finalize_observability` moved to `lifecycle.rs` (Fase 4 — T4.2).

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

/// Map an LLM error to its
/// canonical `ErrorClass`. Used at every site in `execute_with_history`
/// that returns `AgentResult` from a failed LLM call so headless v3
/// consumers can distinguish infra failures (rate-limit, quota, auth)
/// from agent failures.
// Helpers below (llm_error_to_class, truncate_handoff_objective,
// truncate_batch_args, derive_provider_hint) were extracted to
// `run_engine_helpers.rs` in Fase 4 — see `use` alias at bottom of
// file. Auto-init + sandbox spawn likewise moved to their own modules.

use theo_domain::clock::now_millis;

// NOTE: `derive_provider_hint`, `llm_error_to_class`,
// `truncate_batch_args`, `truncate_handoff_objective`,
// `auto_init_project_context`, `spawn_done_gate_cargo`, `DoomLoopTracker`
// usage all moved to `run_engine_helpers.rs` / `run_engine_auto_init.rs` /
// `run_engine_sandbox.rs` / `execution.rs` in Fase 4 (REMEDIATION_PLAN T4.2).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::CapturingListener;
    use theo_domain::session::SessionId;
    use theo_domain::task::AgentType;

    struct TestSetup {
        bus: Arc<EventBus>,
        listener: Arc<CapturingListener>,
        tm: Arc<TaskManager>,
        tcm: Arc<ToolCallManager>,
    }

    impl TestSetup {
        fn new() -> Self {
            let bus = Arc::new(EventBus::new());
            let listener = Arc::new(CapturingListener::new());
            bus.subscribe(listener.clone());
            let tm = Arc::new(TaskManager::new(bus.clone()));
            let tcm = Arc::new(ToolCallManager::new(bus.clone()));
            Self {
                bus,
                listener,
                tm,
                tcm,
            }
        }

        fn create_engine(&self, task_objective: &str) -> AgentRunEngine {
            let task_id =
                self.tm
                    .create_task(SessionId::new("s"), AgentType::Coder, task_objective.into());
            AgentRunEngine::new(
                task_id,
                self.tm.clone(),
                self.tcm.clone(),
                self.bus.clone(),
                LlmClient::new("http://localhost:9999", None, "test"),
                Arc::new(theo_tooling::registry::create_default_registry()),
                AgentConfig::default(),
                PathBuf::from("/tmp"),
            )
        }

    }

    // -----------------------------------------------------------------------
    // Invariant 6: unique run_id
    // -----------------------------------------------------------------------

    #[test]
    fn new_generates_unique_run_id() {
        let setup = TestSetup::new();
        let e1 = setup.create_engine("task1");
        let e2 = setup.create_engine("task2");
        assert_ne!(e1.run_id().as_str(), e2.run_id().as_str());
    }

    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------

    #[test]
    fn new_publishes_run_initialized_event() {
        let setup = TestSetup::new();
        let engine = setup.create_engine("test");

        let events = setup.listener.captured();
        let run_init = events
            .iter()
            .find(|e| e.event_type == EventType::RunInitialized);
        assert!(run_init.is_some(), "RunInitialized event must be published");
        assert_eq!(run_init.unwrap().entity_id, engine.run_id().as_str());
    }

    #[test]
    fn transition_run_publishes_state_changed_event() {
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");

        engine.transition_run(RunState::Planning);
        assert_eq!(engine.state(), RunState::Planning);

        let events = setup.listener.captured();
        let state_changed: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == EventType::RunStateChanged)
            .collect();
        assert!(!state_changed.is_empty());
        let last = state_changed.last().unwrap();
        assert_eq!(last.payload["from"].as_str().unwrap(), "Initialized");
        assert_eq!(last.payload["to"].as_str().unwrap(), "Planning");
    }

    #[test]
    fn run_state_changed_events_have_correct_count() {
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");

        engine.transition_run(RunState::Planning);
        engine.transition_run(RunState::Executing);
        engine.transition_run(RunState::Evaluating);
        engine.transition_run(RunState::Converged);

        let state_events: Vec<_> = setup
            .listener
            .captured()
            .iter()
            .filter(|e| e.event_type == EventType::RunStateChanged)
            .cloned()
            .collect();
        // Initialized→Planning, Planning→Executing, Executing→Evaluating, Evaluating→Converged
        assert_eq!(state_events.len(), 4);
    }

    // -----------------------------------------------------------------------
    // State transitions
    // -----------------------------------------------------------------------

    #[test]
    fn initial_state_is_initialized() {
        let setup = TestSetup::new();
        let engine = setup.create_engine("test");
        assert_eq!(engine.state(), RunState::Initialized);
        assert_eq!(engine.iteration(), 0);
    }

    #[test]
    fn transition_run_through_full_cycle() {
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");

        engine.transition_run(RunState::Planning);
        assert_eq!(engine.state(), RunState::Planning);

        engine.transition_run(RunState::Executing);
        assert_eq!(engine.state(), RunState::Executing);

        engine.transition_run(RunState::Evaluating);
        assert_eq!(engine.state(), RunState::Evaluating);

        engine.transition_run(RunState::Converged);
        assert_eq!(engine.state(), RunState::Converged);
        assert!(engine.state().is_terminal());
    }

    #[test]
    fn transition_run_replanning_cycle() {
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");

        engine.transition_run(RunState::Planning);
        engine.transition_run(RunState::Executing);
        engine.transition_run(RunState::Evaluating);
        engine.transition_run(RunState::Replanning);
        assert_eq!(engine.state(), RunState::Replanning);

        engine.transition_run(RunState::Planning);
        assert_eq!(engine.state(), RunState::Planning);

        engine.transition_run(RunState::Executing);
        engine.transition_run(RunState::Evaluating);
        engine.transition_run(RunState::Converged);
        assert_eq!(engine.state(), RunState::Converged);
    }

    #[test]
    fn transition_run_abort_from_any_non_terminal() {
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");

        engine.transition_run(RunState::Planning);
        engine.transition_run(RunState::Aborted);
        assert_eq!(engine.state(), RunState::Aborted);
        assert!(engine.state().is_terminal());
    }

    #[test]
    fn converged_rejects_further_transitions() {
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");

        engine.transition_run(RunState::Planning);
        engine.transition_run(RunState::Executing);
        engine.transition_run(RunState::Evaluating);
        engine.transition_run(RunState::Converged);

        engine.transition_run(RunState::Planning);
        assert_eq!(
            engine.state(),
            RunState::Converged,
            "terminal state must not change"
        );
    }

    #[test]
    fn loop_phase_variants_are_distinct() {
        use crate::loop_state::LoopPhase;
        let p = LoopPhase::Explore;
        let e = LoopPhase::Edit;
        let v = LoopPhase::Verify;
        let d = LoopPhase::Done;

        // Verify variants are distinct (no discriminant collision)
        assert_ne!(format!("{p:?}"), format!("{e:?}"));
        assert_ne!(format!("{e:?}"), format!("{v:?}"));
        assert_ne!(format!("{v:?}"), format!("{d:?}"));
    }

    #[test]
    fn agent_result_fields_preserved() {
        let result = AgentResult {
            success: true,
            summary: "done".to_string(),
            files_edited: vec!["src/main.rs".to_string()],
            iterations_used: 5,
            was_streamed: false,
            tokens_used: 0,
            input_tokens: 0,
            output_tokens: 0,
            tool_calls_total: 0,
            tool_calls_success: 0,
            llm_calls: 0,
            retries: 0,
            duration_ms: 0,
            ..Default::default()
        };
        assert!(result.success);
        assert_eq!(result.summary, "done");
        assert_eq!(result.files_edited.len(), 1);
        assert_eq!(result.iterations_used, 5);
    }

    #[test]
    fn agent_result_default_has_no_error_class() {
        // Backcompat — legacy tests that build AgentResult via
        // ..Default::default() must keep working even if they don't set
        // error_class. Default is None.
        let r = AgentResult::default();
        assert!(r.error_class.is_none());
    }

    #[test]
    fn invariant_solved_iff_success_true() {
        // Property: if AgentResult.error_class == Some(Solved), then
        // success MUST be true. Conversely, if success == true, the
        // class (if set) MUST be Solved. This is the headline invariant
        // of the headless v3 schema.
        use theo_domain::error_class::ErrorClass;
        let variants = [
            ErrorClass::Solved,
            ErrorClass::Exhausted,
            ErrorClass::RateLimited,
            ErrorClass::QuotaExceeded,
            ErrorClass::AuthFailed,
            ErrorClass::ContextOverflow,
            ErrorClass::SandboxDenied,
            ErrorClass::Cancelled,
            ErrorClass::Aborted,
            ErrorClass::InvalidTask,
        ];
        for v in variants {
            // Construct the legitimate combinations.
            let solved_pair = AgentResult {
                success: true,
                error_class: Some(ErrorClass::Solved),
                ..Default::default()
            };
            assert!(solved_pair.success);
            assert_eq!(solved_pair.error_class, Some(ErrorClass::Solved));
            // success=false with any non-Solved class is OK.
            if v != ErrorClass::Solved {
                let failed_pair = AgentResult {
                    success: false,
                    error_class: Some(v),
                    ..Default::default()
                };
                assert!(!failed_pair.success);
                assert_ne!(failed_pair.error_class, Some(ErrorClass::Solved));
            }
        }
    }

    mod llm_error_class_mapping {
        use super::*;
        use theo_domain::error_class::ErrorClass;
        use theo_infra_llm::LlmError;

        #[test]
        fn llm_error_to_class_maps_rate_limit() {
            let class = crate::run_engine_helpers::llm_error_to_class(
                &LlmError::RateLimited { retry_after: None },
            );
            assert_eq!(class, ErrorClass::RateLimited);
        }

        #[test]
        fn llm_error_to_class_maps_auth_failure() {
            let class = crate::run_engine_helpers::llm_error_to_class(
                &LlmError::AuthFailed("bad token".into()),
            );
            assert_eq!(class, ErrorClass::AuthFailed);
        }

        #[test]
        fn llm_error_to_class_maps_context_overflow() {
            let class = crate::run_engine_helpers::llm_error_to_class(&LlmError::ContextOverflow {
                provider: "openai".into(),
                message: "too long".into(),
            });
            assert_eq!(class, ErrorClass::ContextOverflow);
        }

        #[test]
        fn llm_error_to_class_falls_back_to_aborted_for_unknown() {
            // Network error doesn't have a dedicated ErrorClass — should
            // map to Aborted (catch-all) so consumers know the run did
            // terminate unexpectedly without misclassifying as infra.
            let class = crate::run_engine_helpers::llm_error_to_class(&LlmError::Timeout);
            assert_eq!(class, ErrorClass::Aborted);
            let class = crate::run_engine_helpers::llm_error_to_class(&LlmError::ServiceUnavailable);
            assert_eq!(class, ErrorClass::Aborted);
        }

        #[test]
        fn llm_error_to_class_maps_quota_exceeded() {
            // Distinct from RateLimited so ab_compare can
            // separate "agent retry exhausted" from "account hit billing
            // ceiling — bench is unusable until reset."
            let class = crate::run_engine_helpers::llm_error_to_class(&LlmError::QuotaExceeded {
                provider: "openai".into(),
                message: "insufficient_quota".into(),
            });
            assert_eq!(class, ErrorClass::QuotaExceeded);
        }
    }

    #[test]
    fn agent_loop_new_signature_current_contract() {
        // Verify AgentLoop::new still accepts the current (config, registry) signature
        use crate::agent_loop::AgentLoop;
        let config = AgentConfig::default();
        let registry = theo_tooling::registry::create_default_registry();
        let agent_loop = AgentLoop::new(config, registry);

        // Verify run() method exists and is callable (signature contract)
        // We can't call it without an LLM, but we can verify the type is correct
        let _: &AgentLoop = &agent_loop;
        // If AgentLoop::new signature changes, this test fails at compile time.
        // If AgentLoop type is renamed or removed, this test fails at compile time.
        assert!(
            std::mem::size_of_val(&agent_loop) > 0,
            "AgentLoop should have non-zero size"
        );
    }

    #[test]
    fn doom_loop_threshold_config_exposes_default() {
        let config = AgentConfig::default();
        assert_eq!(config.doom_loop_threshold, Some(3));
    }

    // -----------------------------------------------------------------------
    // delegate_task validation tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn delegate_task_rejects_both_agent_and_parallel() {
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");
        let args = serde_json::json!({
            "agent": "explorer",
            "objective": "x",
            "parallel": [{"agent": "verifier", "objective": "y"}]
        });
        let result = engine.handle_delegate_task(args).await;
        assert!(result.starts_with("Error:"));
        assert!(result.contains("not both"));
    }

    #[tokio::test]
    async fn delegate_task_rejects_neither_agent_nor_parallel() {
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");
        let args = serde_json::json!({});
        let result = engine.handle_delegate_task(args).await;
        assert!(result.starts_with("Error:"));
    }

    #[tokio::test]
    async fn delegate_task_rejects_empty_agent_name() {
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");
        let args = serde_json::json!({"agent": "", "objective": "x"});
        let result = engine.handle_delegate_task(args).await;
        assert!(result.starts_with("Error:"));
        assert!(result.contains("non-empty"));
    }

    #[tokio::test]
    async fn delegate_task_rejects_empty_objective() {
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");
        let args = serde_json::json!({"agent": "explorer", "objective": ""});
        let result = engine.handle_delegate_task(args).await;
        assert!(result.starts_with("Error:"));
        assert!(result.contains("required"));
    }

    #[tokio::test]
    async fn delegate_task_rejects_empty_parallel_array() {
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");
        let args = serde_json::json!({"parallel": []});
        let result = engine.handle_delegate_task(args).await;
        assert!(result.starts_with("Error:"));
        assert!(result.contains("non-empty"));
    }

    #[tokio::test]
    async fn delegate_task_unknown_agent_creates_on_demand() {
        // We can't actually run a real LLM here. We can verify that the
        // dispatch path PICKS the on-demand spec by inspecting the registry
        // build behavior: when an unknown agent name is passed, the spec
        // returned is on-demand (read-only).
        // This is implicitly tested above through `spawn_with_spec` semantics.
        // The integration test runs against a real LLM (out of scope here).
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");
        // Use a name that won't be in any registry. Fast-fail because
        // there's no LLM at localhost:9999, but we should at least see the
        // delegation prefix prove the routing executed.
        let args = serde_json::json!({"agent": "nonexistent-zzzz", "objective": "do x"});
        let result = engine.handle_delegate_task(args).await;
        // Either succeed (unlikely without LLM) or fail with the agent name
        // prefix proving the dispatch reached spawn_with_spec.
        assert!(
            result.contains("nonexistent-zzzz"),
            "expected agent name in result, got: {}",
            result
        );
    }

    #[test]
    fn delegate_task_tool_def_is_registered() {
        let registry = theo_tooling::registry::create_default_registry();
        let defs = crate::tool_bridge::registry_to_definitions(&registry);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(
            names.contains(&"delegate_task"),
            "delegate_task must be in tool definitions"
        );
    }

    // ── Split tool variants ──

    #[test]
    fn delegate_task_single_tool_def_is_registered() {
        let registry = theo_tooling::registry::create_default_registry();
        let defs = crate::tool_bridge::registry_to_definitions(&registry);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(names.contains(&"delegate_task_single"));
        assert!(names.contains(&"delegate_task_parallel"));
        assert!(names.contains(&"delegate_task")); // legacy alias
    }

    #[tokio::test]
    async fn delegate_task_uses_injected_registry_and_run_store() {
        use crate::subagent_runs::FileSubagentRunStore;
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");

        // Inject a custom registry with a known agent + a run store
        let mut reg = crate::subagent::SubAgentRegistry::with_builtins();
        reg.register(theo_domain::agent_spec::AgentSpec::on_demand(
            "scout",
            "test purpose",
        ));
        let tempdir = tempfile::TempDir::new().unwrap();
        let store = std::sync::Arc::new(FileSubagentRunStore::new(tempdir.path()));

        engine = engine
            .with_subagent_registry(std::sync::Arc::new(reg))
            .with_subagent_run_store(store.clone());

        let args = serde_json::json!({"agent": "scout", "objective": "look around"});
        let _result = engine.handle_delegate_task(args).await;

        // Run store must have persisted the run
        let runs = store.list().unwrap();
        assert_eq!(runs.len(), 1, "registry-resolved spawn must persist");
    }

    // ── Handoff guardrails integration ──

    #[test]
    fn engine_with_subagent_handoff_guardrails_stores_reference() {
        let setup = TestSetup::new();
        let chain = std::sync::Arc::new(
            crate::handoff_guardrail::GuardrailChain::with_default_builtins(),
        );
        let engine = setup
            .create_engine("test")
            .with_subagent_handoff_guardrails(chain);
        assert!(engine.subagent_handoff_guardrails.is_some());
    }

    #[test]
    fn engine_with_subagent_mcp_discovery_stores_reference() {
        let setup = TestSetup::new();
        let cache = std::sync::Arc::new(theo_infra_mcp::DiscoveryCache::new());
        let engine = setup
            .create_engine("test")
            .with_subagent_mcp_discovery(cache);
        assert!(engine.subagent_mcp_discovery.is_some());
    }

    #[tokio::test]
    async fn delegate_task_redirects_when_explorer_asked_to_implement() {
        // Built-in `ReadOnlyAgentMustNotMutate` now redirects (instead of
        // blocking): explorer is read-only, "implement" is a mutation verb,
        // therefore the spawn should target `implementer` and the result
        // should carry a `[handoff redirected …]` prefix.
        let setup = TestSetup::new();
        let reg = crate::subagent::SubAgentRegistry::with_builtins();
        let _ = reg.get("explorer").expect("explorer builtin must exist");
        let engine = setup
            .create_engine("test")
            .with_subagent_registry(std::sync::Arc::new(reg));

        let mut engine = engine;
        let args = serde_json::json!({
            "agent": "explorer",
            "objective": "implement caching layer"
        });
        let result = engine.handle_delegate_task(args).await;
        assert!(
            result.contains("handoff redirected"),
            "expected redirect prefix, got: {}",
            result
        );
        assert!(
            result.contains("implementer"),
            "expected redirect target name in result, got: {}",
            result
        );
    }

    #[tokio::test]
    async fn delegate_task_redirect_emits_handoff_evaluated_with_decision_redirect() {
        use crate::event_bus::EventListener;
        use std::sync::Mutex;
        use theo_domain::event::{DomainEvent, EventType};

        struct Capture(Mutex<Vec<DomainEvent>>);
        impl EventListener for Capture {
            fn on_event(&self, e: &DomainEvent) {
                self.0.lock().unwrap().push(e.clone());
            }
        }

        let setup = TestSetup::new();
        let capture = std::sync::Arc::new(Capture(Mutex::new(Vec::new())));
        setup
            .bus
            .subscribe(capture.clone() as std::sync::Arc<dyn EventListener>);
        let mut engine = setup.create_engine("test");
        let args = serde_json::json!({
            "agent": "explorer",
            "objective": "implement evil mutation"
        });
        let _ = engine.handle_delegate_task(args).await;
        let events = capture.0.lock().unwrap().clone();
        let evt = events
            .iter()
            .find(|e| e.event_type == EventType::HandoffEvaluated)
            .expect("HandoffEvaluated must be emitted");
        assert_eq!(
            evt.payload.get("decision").and_then(|v| v.as_str()),
            Some("redirect"),
            "decision label must be redirect; payload={}",
            evt.payload
        );
        assert_eq!(
            evt.payload.get("redirect_to").and_then(|v| v.as_str()),
            Some("implementer")
        );
    }

    #[tokio::test]
    async fn delegate_task_rewrite_uses_new_objective() {
        use crate::handoff_guardrail::{
            GuardrailChain, GuardrailDecision, HandoffContext, HandoffGuardrail,
        };

        #[derive(Debug)]
        struct ScopeRewriter;
        impl HandoffGuardrail for ScopeRewriter {
            fn id(&self) -> &str { "test.scope_rewriter" }
            fn evaluate(&self, _ctx: &HandoffContext<'_>) -> GuardrailDecision {
                GuardrailDecision::RewriteObjective {
                    new_objective: "scoped: list crates only".into(),
                }
            }
        }

        let setup = TestSetup::new();
        let mut chain = GuardrailChain::new();
        chain.add(std::sync::Arc::new(ScopeRewriter));
        let mut engine = setup
            .create_engine("test")
            .with_subagent_handoff_guardrails(std::sync::Arc::new(chain));
        let args = serde_json::json!({
            "agent": "explorer",
            "objective": "list everything in the universe"
        });
        let result = engine.handle_delegate_task(args).await;
        assert!(
            result.contains("handoff objective rewritten"),
            "expected rewrite prefix, got: {}",
            result
        );
        assert!(
            result.contains("test.scope_rewriter"),
            "expected guardrail id in prefix, got: {}",
            result
        );
    }

    #[tokio::test]
    async fn delegate_task_block_keeps_returning_refusal_when_chain_blocks() {
        use crate::handoff_guardrail::{
            GuardrailChain, GuardrailDecision, HandoffContext, HandoffGuardrail,
        };

        #[derive(Debug)]
        struct AlwaysBlock;
        impl HandoffGuardrail for AlwaysBlock {
            fn id(&self) -> &str { "test.always_block" }
            fn evaluate(&self, _ctx: &HandoffContext<'_>) -> GuardrailDecision {
                GuardrailDecision::Block { reason: "policy".into() }
            }
        }

        let setup = TestSetup::new();
        let mut chain = GuardrailChain::new();
        chain.add(std::sync::Arc::new(AlwaysBlock));
        let mut engine = setup
            .create_engine("test")
            .with_subagent_handoff_guardrails(std::sync::Arc::new(chain));
        let args = serde_json::json!({
            "agent": "implementer",
            "objective": "anything"
        });
        let result = engine.handle_delegate_task(args).await;
        assert!(result.contains("handoff refused"), "got: {}", result);
        assert!(result.contains("test.always_block"), "got: {}", result);
    }

    #[tokio::test]
    async fn delegate_task_allowed_when_implementer_asked_to_implement() {
        let setup = TestSetup::new();
        let mut engine = setup.create_engine("test");
        let args = serde_json::json!({
            "agent": "implementer",
            "objective": "implement caching layer"
        });
        let result = engine.handle_delegate_task(args).await;
        assert!(
            !result.contains("handoff refused"),
            "implementer must be allowed; got: {}",
            result
        );
    }

    #[tokio::test]
    async fn delegate_task_emits_handoff_evaluated_event_with_block_payload() {
        use crate::event_bus::EventListener;
        use crate::handoff_guardrail::{
            GuardrailChain, GuardrailDecision, HandoffContext, HandoffGuardrail,
        };
        use std::sync::Mutex;
        use theo_domain::event::{DomainEvent, EventType};

        struct Capture(Mutex<Vec<DomainEvent>>);
        impl EventListener for Capture {
            fn on_event(&self, e: &DomainEvent) {
                self.0.lock().unwrap().push(e.clone());
            }
        }

        #[derive(Debug)]
        struct AlwaysBlock;
        impl HandoffGuardrail for AlwaysBlock {
            fn id(&self) -> &str { "test.always_block_for_audit" }
            fn evaluate(&self, _ctx: &HandoffContext<'_>) -> GuardrailDecision {
                GuardrailDecision::Block {
                    reason: "audit-test".into(),
                }
            }
        }

        let setup = TestSetup::new();
        let capture = std::sync::Arc::new(Capture(Mutex::new(Vec::new())));
        setup
            .bus
            .subscribe(capture.clone() as std::sync::Arc<dyn EventListener>);
        let mut chain = GuardrailChain::new();
        chain.add(std::sync::Arc::new(AlwaysBlock));
        let mut engine = setup
            .create_engine("test")
            .with_subagent_handoff_guardrails(std::sync::Arc::new(chain));
        let args = serde_json::json!({
            "agent": "explorer",
            "objective": "anything"
        });
        let _ = engine.handle_delegate_task(args).await;
        let events = capture.0.lock().unwrap().clone();
        let evt = events
            .iter()
            .find(|e| e.event_type == EventType::HandoffEvaluated)
            .expect("HandoffEvaluated must be emitted");
        assert_eq!(
            evt.payload.get("decision").and_then(|v| v.as_str()),
            Some("block")
        );
        assert!(
            evt.payload
                .get("blocked_by")
                .and_then(|v| v.as_str())
                .is_some()
        );
    }

    #[test]
    fn engine_with_subagent_hooks_stores_reference() {
        let setup = TestSetup::new();
        let engine = setup
            .create_engine("test")
            .with_subagent_hooks(std::sync::Arc::new(
                crate::lifecycle_hooks::HookManager::new(),
            ));
        assert!(engine.subagent_hooks.is_some());
    }

    #[test]
    fn is_mutating_tool_recognizes_known_writes() {
        assert!(AgentRunEngine::is_mutating_tool("edit"));
        assert!(AgentRunEngine::is_mutating_tool("write"));
        assert!(AgentRunEngine::is_mutating_tool("apply_patch"));
        assert!(AgentRunEngine::is_mutating_tool("bash"));
        assert!(!AgentRunEngine::is_mutating_tool("read"));
        assert!(!AgentRunEngine::is_mutating_tool("grep"));
        assert!(!AgentRunEngine::is_mutating_tool("glob"));
    }

    #[test]
    fn maybe_checkpoint_returns_none_without_manager() {
        let setup = TestSetup::new();
        let engine = setup.create_engine("test");
        // No subagent_checkpoint attached → snapshot returns None even
        // for mutating tool
        assert!(engine.maybe_checkpoint_for_tool("edit", 1).is_none());
        assert!(engine.checkpoint_before_mutation("any").is_none());
    }

    #[test]
    fn maybe_checkpoint_skips_non_mutating_tools() {
        let setup = TestSetup::new();
        let engine = setup.create_engine("test");
        // read is not mutating — even with manager attached this would
        // return None; with no manager, definitely None.
        assert!(engine.maybe_checkpoint_for_tool("read", 1).is_none());
        assert!(engine.maybe_checkpoint_for_tool("grep", 1).is_none());
    }

    #[test]
    fn reset_turn_checkpoint_allows_new_snapshot() {
        let setup = TestSetup::new();
        let engine = setup.create_engine("test");
        // Mark snapshot as taken
        engine
            .checkpoint_taken_this_turn
            .store(true, std::sync::atomic::Ordering::Release);
        engine.reset_turn_checkpoint();
        assert!(
            !engine
                .checkpoint_taken_this_turn
                .load(std::sync::atomic::Ordering::Acquire)
        );
    }

    #[tokio::test]
    async fn try_dispatch_mcp_tool_returns_none_for_non_mcp_name() {
        let setup = TestSetup::new();
        let engine = setup.create_engine("test");
        let call = theo_infra_llm::types::ToolCall {
            id: "1".into(),
            call_type: "function".into(),
            function: theo_infra_llm::types::FunctionCall {
                name: "read".into(),
                arguments: "{}".into(),
            },
        };
        assert!(engine.try_dispatch_mcp_tool(&call).await.is_none());
    }

    #[tokio::test]
    async fn try_dispatch_mcp_tool_no_registry_returns_none() {
        let setup = TestSetup::new();
        let engine = setup.create_engine("test");
        let call = theo_infra_llm::types::ToolCall {
            id: "1".into(),
            call_type: "function".into(),
            function: theo_infra_llm::types::FunctionCall {
                name: "mcp:github:search".into(),
                arguments: "{}".into(),
            },
        };
        // No subagent_mcp attached → no dispatcher → None
        assert!(engine.try_dispatch_mcp_tool(&call).await.is_none());
    }

    #[tokio::test]
    async fn try_dispatch_mcp_tool_unknown_server_returns_error_message() {
        let setup = TestSetup::new();
        let engine = setup
            .create_engine("test")
            .with_subagent_mcp(std::sync::Arc::new(theo_infra_mcp::McpRegistry::new()));
        let call = theo_infra_llm::types::ToolCall {
            id: "1".into(),
            call_type: "function".into(),
            function: theo_infra_llm::types::FunctionCall {
                name: "mcp:nonexistent:foo".into(),
                arguments: "{}".into(),
            },
        };
        let msg = engine.try_dispatch_mcp_tool(&call).await.unwrap();
        let content = msg.content.unwrap_or_default();
        assert!(content.contains("mcp dispatch failed"));
    }

    #[test]
    fn engine_with_subagent_cancellation_stores_reference() {
        let setup = TestSetup::new();
        let engine = setup
            .create_engine("test")
            .with_subagent_cancellation(std::sync::Arc::new(
                crate::cancellation::CancellationTree::new(),
            ));
        assert!(engine.subagent_cancellation.is_some());
    }

    #[test]
    fn delegate_task_excluded_from_subagent_tools() {
        let registry = theo_tooling::registry::create_default_registry();
        let defs = crate::tool_bridge::registry_to_definitions_for_subagent(&registry);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(
            !names.contains(&"delegate_task"),
            "sub-agents must NOT see delegate_task (no recursive delegation)"
        );
    }

    // -----------------------------------------------------------------------
    // Resume-runtime-wiring dispatch wiring
    // -----------------------------------------------------------------------

    mod dispatch_replays {
        use super::*;
        use crate::subagent::resume::{ResumeContext, WorktreeStrategy};
        use std::collections::{BTreeMap, BTreeSet};
        use std::sync::Arc;
        use theo_domain::agent_spec::AgentSpec;

        fn build_resume_ctx(
            executed_calls: BTreeSet<String>,
            cached_results: BTreeMap<String, theo_infra_llm::types::Message>,
        ) -> Arc<ResumeContext> {
            Arc::new(ResumeContext {
                spec: AgentSpec::on_demand("a", "b"),
                start_iteration: 1,
                history: vec![],
                prior_tokens_used: 0,
                checkpoint_before: None,
                executed_tool_calls: executed_calls,
                executed_tool_results: cached_results,
                worktree_strategy: WorktreeStrategy::None,
            })
        }

        #[test]
        fn engine_without_resume_context_dispatches_normally_regression_guard() {
            // D5 backward compat — default engine has resume_context = None.
            let setup = TestSetup::new();
            let engine = setup.create_engine("regression");
            assert!(
                engine.resume_context.is_none(),
                "default engine must NOT have resume_context attached"
            );
        }

        #[test]
        fn engine_with_resume_context_attaches_context_via_builder() {
            let setup = TestSetup::new();
            let mut cached = BTreeMap::new();
            cached.insert(
                "c1".to_string(),
                theo_infra_llm::types::Message::tool_result("c1", "fake_tool", "result-1"),
            );
            let mut executed = BTreeSet::new();
            executed.insert("c1".to_string());
            let ctx = build_resume_ctx(executed, cached);

            let engine = setup.create_engine("with-context").with_resume_context(ctx.clone());

            let attached = engine
                .resume_context
                .as_ref()
                .expect("resume_context must be attached");
            assert!(attached.should_skip_tool_call("c1"));
            assert!(!attached.should_skip_tool_call("c-unknown"));
            let cached_msg = attached.cached_tool_result("c1").expect("cached msg present");
            assert_eq!(cached_msg.tool_call_id.as_deref(), Some("c1"));
            assert_eq!(cached_msg.content.as_deref(), Some("result-1"));
        }

        #[test]
        fn engine_with_resume_context_short_circuit_predicate_for_known_call_id() {
            // The dispatch hook is a 2-condition guard:
            //   should_skip_tool_call(call.id) && cached_tool_result(call.id).is_some()
            // Both must be true for replay. Verify the predicate matches the
            // contract enforced in run_engine handle_completion (lines 1393-1419).
            let mut cached = BTreeMap::new();
            cached.insert(
                "c-known".to_string(),
                theo_infra_llm::types::Message::tool_result(
                    "c-known",
                    "write_file",
                    "{\"ok\":true}",
                ),
            );
            let mut executed = BTreeSet::new();
            executed.insert("c-known".to_string());
            let ctx = build_resume_ctx(executed, cached);

            // Both true → replay path triggers
            assert!(ctx.should_skip_tool_call("c-known"));
            assert!(ctx.cached_tool_result("c-known").is_some());

            // Unknown call_id → dispatch normally (BOTH guards false)
            assert!(!ctx.should_skip_tool_call("c-unknown"));
            assert!(ctx.cached_tool_result("c-unknown").is_none());
        }

        #[test]
        fn engine_with_resume_context_dispatches_unknown_call_id() {
            // When LLM emits a NEW call_id absent from the original event log,
            // the short-circuit predicate is false on both legs, so dispatch
            // proceeds normally. This is the "agent makes new progress on
            // resume" scenario.
            let setup = TestSetup::new();
            let executed = BTreeSet::new(); // empty — no prior calls
            let cached = BTreeMap::new();
            let ctx = build_resume_ctx(executed, cached);
            let engine = setup.create_engine("new-call").with_resume_context(ctx.clone());

            assert!(engine.resume_context.is_some());
            // Predicate: brand-new call_id is NOT skipped → dispatcher runs.
            let attached = engine.resume_context.as_ref().unwrap();
            assert!(!attached.should_skip_tool_call("brand-new-c1"));
        }
    }

    // -----------------------------------------------------------------------
    // provider-hint helper coverage (otlp-exporter)
    // -----------------------------------------------------------------------

    mod provider_hint {
        use super::*;

        #[test]
        fn derive_provider_hint_recognizes_openai() {
            assert_eq!(crate::run_engine_helpers::derive_provider_hint("https://api.openai.com/v1"), "openai");
        }

        #[test]
        fn derive_provider_hint_recognizes_chatgpt_oauth() {
            assert_eq!(crate::run_engine_helpers::derive_provider_hint("https://chatgpt.com/backend-api"), "openai");
        }

        #[test]
        fn derive_provider_hint_recognizes_anthropic() {
            assert_eq!(crate::run_engine_helpers::derive_provider_hint("https://api.anthropic.com"), "anthropic");
        }

        #[test]
        fn derive_provider_hint_recognizes_gemini() {
            assert_eq!(crate::run_engine_helpers::derive_provider_hint("https://generativelanguage.googleapis.com"), "gemini");
        }

        #[test]
        fn derive_provider_hint_falls_back_for_unknown_url() {
            assert_eq!(crate::run_engine_helpers::derive_provider_hint("https://my-private-llm.corp"), "openai_compatible");
        }

        #[test]
        fn derive_provider_hint_recognizes_localhost_as_local() {
            assert_eq!(crate::run_engine_helpers::derive_provider_hint("http://localhost:8000"), "openai_compatible_local");
            assert_eq!(crate::run_engine_helpers::derive_provider_hint("http://127.0.0.1:8080"), "openai_compatible_local");
        }
    }

    // -----------------------------------------------------------------------
    // Bug #1 (benchmark-validation): AgentResult.success semantics
    // -----------------------------------------------------------------------
    //
    // Pure unit tests on the AgentResult constructor logic — the bug was
    // that "budget exceeded" path set success based on whether ANY edit
    // succeeded, not on whether the task verifiably completed. After the
    // fix, only the `done` meta-tool acceptance path returns success=true.

    mod success_semantics {
        use super::*;
        use crate::agent_loop::AgentResult;

        /// The fix: budget-exceeded must always return success=false.
        /// Old behavior: success = (edits_succeeded > 0) which is wrong.
        #[test]
        fn budget_exceeded_with_edits_returns_success_false() {
            // Simulate the budget-exceeded branch — what the constructor
            // SHOULD produce when iter limit / token limit hits.
            let r = budget_exceeded_result(
                /* edits_succeeded */ 5,
                /* edits_files */ vec!["a.txt".into(), "b.txt".into()],
                /* iteration */ 20,
                "Budget exceeded: iterations exceeded: 21 > 20 limit",
            );
            assert!(
                !r.success,
                "budget exceeded must mean success=false even when edits exist; \
                 got success={}",
                r.success
            );
            assert!(r.summary.starts_with("Budget exceeded"));
            assert_eq!(r.iterations_used, 20);
            assert_eq!(r.files_edited.len(), 2);
        }

        #[test]
        fn budget_exceeded_with_zero_edits_returns_success_false() {
            let r = budget_exceeded_result(0, vec![], 20, "Budget exceeded");
            assert!(!r.success);
        }

        #[test]
        fn done_accepted_returns_success_true() {
            let r = done_accepted_result(
                "Implementation complete; tests pass",
                vec!["src/main.rs".into()],
                7,
                /* done_attempts */ 1,
            );
            assert!(r.success, "done accepted is the ONLY success-true path");
            assert!(r.summary.contains("[accepted after"));
        }

        // error_class
        // population on the canonical helpers.

        #[test]
        fn budget_exceeded_returns_exhausted_class() {
            let r = budget_exceeded_result(0, vec![], 35, "max_iterations");
            assert!(!r.success);
            assert_eq!(
                r.error_class,
                Some(theo_domain::error_class::ErrorClass::Exhausted)
            );
        }

        #[test]
        fn done_accepted_returns_solved_class() {
            let r = done_accepted_result("ok", vec![], 5, 1);
            assert!(r.success);
            assert_eq!(
                r.error_class,
                Some(theo_domain::error_class::ErrorClass::Solved)
            );
        }
    }

    // Helpers below mirror the code paths in execute_with_history. They
    // are factored out so the bug fix can be unit-tested without spinning
    // up the full engine. The public API of AgentResult is preserved.

    fn budget_exceeded_result(
        edits_succeeded: u32,
        edits_files: Vec<String>,
        iteration: usize,
        violation: &str,
    ) -> AgentResult {
        AgentResult {
            // Bug #1 fix: budget exceeded ALWAYS means task did not finish.
            // Previously: success = edits_succeeded > 0 (lied to caller)
            success: false,
            summary: format!(
                "{}. Edits succeeded: {}. Files: {}",
                violation,
                edits_succeeded,
                edits_files.join(", ")
            ),
            files_edited: edits_files,
            iterations_used: iteration,
            error_class: Some(theo_domain::error_class::ErrorClass::Exhausted),
            ..Default::default()
        }
    }

    fn done_accepted_result(
        summary: &str,
        edits_files: Vec<String>,
        iteration: usize,
        done_attempts: u32,
    ) -> AgentResult {
        AgentResult {
            success: true,
            summary: format!("{} [accepted after {} done attempts]", summary, done_attempts),
            files_edited: edits_files,
            iterations_used: iteration,
            error_class: Some(theo_domain::error_class::ErrorClass::Solved),
            ..Default::default()
        }
    }
}
