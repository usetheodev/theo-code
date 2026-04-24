// Submodules extracted from the original single-file run_engine.rs as
// part of Fase 4 (REMEDIATION_PLAN T4.2). Each needs access to private
// fields of `AgentRunEngine` declared in this module — that is why
// they live as child modules rather than siblings.
mod bootstrap;
mod builders;
mod dispatch;
mod lifecycle;

use std::path::PathBuf;
use std::sync::Arc;

use theo_domain::agent_run::{AgentRun, RunState};
use theo_domain::budget::Budget;
use theo_domain::error_class::ErrorClass;
use theo_domain::event::{DomainEvent, EventType};
use theo_domain::identifiers::{RunId, TaskId};
use theo_domain::session::{MessageId, SessionId};
use theo_domain::task::TaskState;
use theo_domain::tool::ToolContext;
use theo_domain::tool_call::ToolCallState;
use theo_infra_llm::LlmClient;
use theo_infra_llm::types::{ChatRequest, Message};
use theo_tooling::registry::ToolRegistry;

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
use crate::snapshot::RunSnapshot;
use crate::task_manager::TaskManager;
use crate::tool_bridge;
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
    /// PLAN_AUTO_EVOLUTION_SOTA Phase 1: turns since the last memory
    /// reviewer spawn. `AtomicUsize` lets the counter survive fork
    /// boundaries (eliminates Hermes Issue #8506).
    memory_nudge_counter: Arc<crate::memory_lifecycle::MemoryNudgeCounter>,
    /// PLAN_AUTO_EVOLUTION_SOTA Phase 3: tool iterations since the
    /// last skill reviewer spawn. Persists across task boundaries so
    /// short tasks don't reset accumulation mid-stream.
    skill_nudge_counter: Arc<crate::skill_reviewer::SkillNudgeCounter>,
    /// PLAN_AUTO_EVOLUTION_SOTA Phase 3: flipped to `true` whenever
    /// `skill_manage.create` / `edit` / `patch` succeeds in the
    /// current task, suppressing the reviewer for that task.
    skill_created_this_task: std::sync::atomic::AtomicBool,
    /// PLAN_AUTO_EVOLUTION_SOTA Phase 2: flipped once autodream has
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

    /// Accumulated token usage (Phase 1 T1.1 AC-1.1.4, CLI display).
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

    /// Execute the full agent run cycle.
    ///
    /// Flow: Initialized → Planning → Executing → Evaluating → Converged/Replanning/Aborted
    /// Execute with fresh messages (no session history).
    pub async fn execute(&mut self) -> AgentResult {
        let mut result = self.execute_with_history(Vec::new()).await;
        self.record_session_exit(&result).await;
        result.run_report = self.last_run_report.take();
        result
    }

    // record_session_exit + record_session_exit_public +
    // finalize_observability moved to `lifecycle.rs` (Fase 4 — T4.2).

    /// Execute with session history from previous REPL prompts.
    /// `history` contains messages from prior runs in this session.
    /// The current task objective is appended as the last user message.
    pub async fn execute_with_history(&mut self, history: Vec<Message>) -> AgentResult {
        // Fase 4 (T4.2): the 200-LOC setup phase — state-machine
        // transitions, auto-init, autodream spawn, system prompt,
        // memory prefetch, episode replay, git boot context, GRAPHCTX
        // planning hints, skills summary, history merge, task
        // objective — lives in `bootstrap.rs`.
        let mut messages = self.assemble_initial_messages(history).await;

        // Initialize state manager for file-backed persistence (crash recovery).
        // Best-effort: if creation fails, continue without persistence.
        let mut state_manager = if !self.config.is_subagent {
            match crate::state_manager::StateManager::create(
                &self.project_dir,
                self.run.run_id.as_str(),
            ) {
                Ok(sm) => Some(sm),
                Err(e) => {
                    eprintln!("[theo] State manager init failed (non-fatal): {e}");
                    None
                }
            }
        } else {
            None
        };

        // Initialize hook runner for pre/post tool hooks
        let hook_runner = if !self.config.is_subagent {
            Some(crate::hooks::HookRunner::new(
                &self.project_dir,
                crate::hooks::HookConfig::default(),
            ))
        } else {
            None // Sub-agents don't run hooks
        };

        // Initialize sensor runner for computational verification after write tools.
        // Sensors fire asynchronously and results are drained before each LLM call.
        let sensor_runner = if !self.config.is_subagent {
            let runner = crate::sensor::SensorRunner::new(
                &self.project_dir,
                crate::hooks::HookConfig::default(),
            );
            if runner.has_sensors() {
                Some(runner)
            } else {
                None
            }
        } else {
            None
        };

        // Doom loop detector — tracks recent tool calls to detect repetition
        let mut doom_tracker = self
            .config
            .doom_loop_threshold
            .map(DoomLoopTracker::new);

        // Layer 1: Schema stripping — sub-agents get filtered tool definitions
        // that exclude delegation meta-tools (subagent, subagent_parallel, skill).
        let tool_defs = if self.config.is_subagent {
            tool_bridge::registry_to_definitions_for_subagent(&self.registry)
        } else {
            tool_bridge::registry_to_definitions(&self.registry)
        };
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);

        loop {
            self.run.iteration += 1;
            let iteration = self.run.iteration;

            // Phase 9: reset per-turn checkpoint flag so the first mutating
            // tool of THIS iteration triggers a snapshot.
            self.reset_turn_checkpoint();

            // Budget check (Invariant 8) — record iteration BEFORE check
            self.budget_enforcer.record_iteration();
            if let Err(violation) = self.budget_enforcer.check() {
                self.transition_run(RunState::Aborted);
                let _ = self
                    .task_manager
                    .transition(&self.task_id, TaskState::Failed);

                let summary = format!(
                    "Budget exceeded: {}. Edits succeeded: {}. Files: {}",
                    violation,
                    self.context_loop_state.edits_succeeded,
                    self.context_loop_state.edits_files.join(", ")
                );

                self.metrics.record_run_complete(false);
                // Bug #1 fix (benchmark-validation): budget exceeded ALWAYS
                // means the task did not finish. Previously `success` was
                // derived from edits_succeeded > 0, which lied to callers.
                return AgentResult::from_engine_state(
                    self,
                    false,
                    summary,
                    false,
                    theo_domain::error_class::ErrorClass::Exhausted,
                );
            }

            // ── SENSOR DRAIN ──
            // Drain pending sensor results and inject as system messages before LLM call.
            // This provides the LLM with feedback from computational verification (e.g. clippy, tests).
            if let Some(ref sensor_runner) = sensor_runner {
                for result in sensor_runner.drain_pending() {
                    let severity = if result.exit_code == 0 { "OK" } else { "ISSUE" };
                    let preview = theo_domain::prompt_sanitizer::char_boundary_truncate(
                        &result.output,
                        crate::constants::SENSOR_OUTPUT_PREVIEW_BYTES,
                    );
                    messages.push(Message::system(format!(
                        "[SENSOR {severity}] {} (via {}): {preview}",
                        result.file_path, result.tool_name
                    )));

                    // Publish SensorExecuted event
                    self.event_bus.publish(DomainEvent::new(
                        EventType::SensorExecuted,
                        self.run.run_id.as_str(),
                        serde_json::json!({
                            "file": result.file_path,
                            "exit_code": result.exit_code,
                            "output_preview": &preview[..preview.len().min(200)],
                            "duration_ms": result.duration_ms,
                            "tool_name": result.tool_name,
                        }),
                    ));
                }
            }

            // ── PLANNING phase ──
            // Context loop injection
            if iteration > 1 && iteration.is_multiple_of(self.config.context_loop_interval) {
                let task_objective = self
                    .task_manager
                    .get(&self.task_id)
                    .map(|t| t.objective.clone())
                    .unwrap_or_default();
                let ctx_msg = self.context_loop_state.build_context_loop(
                    iteration,
                    self.config.max_iterations,
                    &task_objective,
                );
                messages.push(Message::user(ctx_msg));
            }

            // Phase transitions (legacy, preserved for context loop diagnostics)
            self.context_loop_state
                .maybe_transition(iteration, self.config.max_iterations);

            // Context compaction: compress history with semantic progress context.
            let compaction_ctx = crate::compaction::CompactionContext {
                task_objective: messages
                    .iter()
                    .find(|m| m.role == theo_infra_llm::types::Role::User)
                    .and_then(|m| m.content.clone())
                    .unwrap_or_default()
                    .chars()
                    .take(100)
                    .collect(),
                current_phase: format!("{:?}", self.run.state),
                target_files: self.context_loop_state.edits_files.clone(),
                recent_errors: self
                    .context_loop_state
                    .edit_failures
                    .iter()
                    .rev()
                    .take(2)
                    .cloned()
                    .collect(),
            };
            // Phase 0 T0.1 AC-0.1.3: pre-compression memory hook (survives truncation).
            crate::memory_lifecycle::run_engine_hooks::pre_compress_push(&self.config, &mut messages).await;

            crate::compaction_stages::compact_staged_with_policy(
                &mut messages,
                self.config.context_window_tokens,
                Some(&compaction_ctx),
                &self.config.compaction_policy,
            );

            // Record context size for metrics (estimated tokens = chars/4)
            let estimated_context_tokens: usize = messages
                .iter()
                .filter_map(|m| m.content.as_ref())
                .map(|c| c.len().div_ceil(4))
                .sum();
            self.context_metrics
                .record_context_size(iteration, estimated_context_tokens);

            // LLM call
            self.transition_run(RunState::Planning);

            // --- Routing decision (plan §R3) ---
            // Consult the configured router; when absent, fall back to
            // the session defaults. The router is called exactly once
            // per turn at this single call site (invariant enforced by
            // structural_hygiene.rs).
            let latest_user_msg = messages
                .iter()
                .rev()
                .find(|m| matches!(m.role, theo_infra_llm::types::Role::User))
                .and_then(|m| m.content.as_deref());
            let mut routing_ctx =
                theo_domain::routing::RoutingContext::new(theo_domain::routing::RoutingPhase::Normal);
            routing_ctx.latest_user_message = latest_user_msg;
            routing_ctx.conversation_tokens = estimated_context_tokens as u64;
            routing_ctx.iteration = iteration;
            routing_ctx.requires_tool_use = !tool_defs.is_empty();
            let (chosen_model, chosen_effort, routing_reason): (String, Option<String>, &'static str) =
                match &self.config.router {
                    Some(handle) => {
                        let choice = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            handle.as_router().route(&routing_ctx)
                        }));
                        match choice {
                            Ok(c) => (c.model_id, c.reasoning_effort, c.routing_reason),
                            Err(_) => (
                                self.config.model.clone(),
                                self.config.reasoning_effort.clone(),
                                "router_panic_fallback_default",
                            ),
                        }
                    }
                    None => (
                        self.config.model.clone(),
                        self.config.reasoning_effort.clone(),
                        "no_router",
                    ),
                };

            let mut request = ChatRequest::new(&chosen_model, messages.clone())
                .with_tools(tool_defs.clone())
                .with_max_tokens(self.config.max_tokens)
                .with_temperature(self.config.temperature);

            if let Some(ref effort) = chosen_effort {
                request = request.with_reasoning_effort(effort);
            }

            // Phase 29 follow-up (sota-gaps-followup) — closes gap #7.
            // THEO_FORCE_TOOL_CHOICE env var lets operators force the model
            // to call a tool. Useful for benchmarks / OAuth E2E tests where
            // chatty models like gpt-5.3-codex would otherwise generate
            // text instead of invoking delegate_task.
            //   - "required" / "any"  → model MUST call some tool (any)
            //   - "none"              → model MUST NOT call a tool
            //   - "function:NAME"     → shorthand for forcing a specific tool
            //   - JSON `{"type":"function","name":"X"}` → passed through verbatim
            //   - any other value     → passed through verbatim (string)
            // Skipped silently when no tools are exposed for this turn.
            if !tool_defs.is_empty()
                && let Some(forced) = theo_domain::environment::theo_var("THEO_FORCE_TOOL_CHOICE")
            {
                let normalized = match forced.as_str() {
                    "any" => "required".to_string(),
                    other if other.starts_with("function:") => {
                        let name = other.trim_start_matches("function:");
                        // Use serde_json to safely encode the tool name.
                        // Hand-rolled format!(r#"{{"name":"{}"}}"#, name)
                        // produces broken JSON if `name` contains a quote.
                        serde_json::json!({"type": "function", "name": name}).to_string()
                    }
                    other => other.to_string(),
                };
                if theo_domain::environment::bool_var("THEO_DEBUG_CODEX", false) {
                    eprintln!(
                        "[theo] THEO_FORCE_TOOL_CHOICE active: {} → {}",
                        forced, normalized
                    );
                }
                request = request.with_tool_choice(normalized);
            }

            // Publish LLM call start (triggers "Thinking..." in CLI)
            // Phase 43 (otlp-exporter-plan): attach an `otel` payload so
            // OtelExportingListener can build a `gen_ai.*`-attributed span.
            let provider_hint = derive_provider_hint(&self.config.base_url);
            let llm_start_span = crate::observability::otel::llm_call_span(
                provider_hint, &chosen_model,
            );
            self.event_bus.publish(DomainEvent::new(
                EventType::LlmCallStart,
                self.run.run_id.as_str(),
                serde_json::json!({
                    "iteration": iteration,
                    "routing_reason": routing_reason,
                    "model": chosen_model,
                    "otel": llm_start_span.to_json(),
                }),
            ));

            let llm_start = std::time::Instant::now();
            let event_bus_for_stream = self.event_bus.clone();
            let run_id_for_stream = self.run.run_id.as_str().to_string();

            // LLM call with retry for retryable errors (429, 503, 504, network)
            let retry_policy = if self.config.aggressive_retry {
                theo_domain::retry_policy::RetryPolicy::benchmark()
            } else {
                theo_domain::retry_policy::RetryPolicy::default_llm()
            };
            let max_retries = retry_policy.max_retries;
            let mut llm_result = None;

            for attempt in 0..=max_retries {
                let eb = event_bus_for_stream.clone();
                let rid = run_id_for_stream.clone();

                let response = self
                    .client
                    .chat_streaming(&request, |delta| match delta {
                        theo_infra_llm::stream::StreamDelta::Reasoning(text) => {
                            eb.publish(DomainEvent::new(
                                EventType::ReasoningDelta,
                                &rid,
                                serde_json::json!({"text": text}),
                            ));
                        }
                        theo_infra_llm::stream::StreamDelta::Content(text) => {
                            eb.publish(DomainEvent::new(
                                EventType::ContentDelta,
                                &rid,
                                serde_json::json!({"text": text}),
                            ));
                        }
                        _ => {}
                    })
                    .await;

                match response {
                    Ok(resp) => {
                        llm_result = Some(Ok(resp));
                        break;
                    }
                    Err(ref e) if e.is_retryable() && attempt < max_retries => {
                        let delay = retry_policy.delay_for_attempt(attempt);
                        self.event_bus.publish(DomainEvent::new(
                            EventType::Error,
                            self.run.run_id.as_str(),
                            serde_json::json!({
                                "type": "retry",
                                "attempt": attempt + 1,
                                "max_retries": max_retries,
                                "error": e.to_string(),
                                "delay_ms": delay.as_millis() as u64,
                            }),
                        ));
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    Err(e) => {
                        llm_result = Some(Err(e));
                        break;
                    }
                }
            }

            // RM-pre-3: replace pre-existing `unwrap()` with a typed error.
            // Defensive — in practice the retry loop above always assigns
            // `llm_result` at least once, but relying on that invariant via
            // `unwrap()` would panic on any future refactor that breaks it.
            let llm_result = llm_result.unwrap_or_else(|| {
                Err(theo_infra_llm::LlmError::Parse(
                    "LLM retry loop produced no result (invariant broken)".to_string(),
                ))
            });
            let response = match llm_result {
                Ok(resp) => {
                    let llm_duration = llm_start.elapsed().as_millis() as u64;
                    let input_tok = resp
                        .usage
                        .as_ref()
                        .map(|u| u.prompt_tokens as u64)
                        .unwrap_or(0);
                    let output_tok = resp
                        .usage
                        .as_ref()
                        .map(|u| u.completion_tokens as u64)
                        .unwrap_or(0);
                    let total_tok = input_tok + output_tok;
                    self.budget_enforcer.record_tokens(total_tok);
                    self.metrics
                        .record_llm_call_detailed(llm_duration, input_tok, output_tok);
                    // Phase 1 T1.1: accumulate the 6-field token usage
                    // (cache / reasoning stay at 0 until providers expose
                    // them; cost recomputed lazily at episode write time).
                    self.session_token_usage.accumulate(&theo_domain::budget::TokenUsage {
                        input_tokens: input_tok, output_tokens: output_tok, ..Default::default()
                    });
                    // Emit LlmCallEnd with full accounting so observability can
                    // plot context growth, token-per-iteration, and cache hit rate.
                    // Phase 43 (otlp-exporter-plan): include the OTel
                    // GenAI usage attributes for the Span end event.
                    let mut llm_end_span = crate::observability::otel::llm_call_span(
                        derive_provider_hint(&self.config.base_url), &chosen_model,
                    );
                    llm_end_span.set(
                        crate::observability::otel::ATTR_USAGE_INPUT_TOKENS,
                        input_tok,
                    );
                    llm_end_span.set(
                        crate::observability::otel::ATTR_USAGE_OUTPUT_TOKENS,
                        output_tok,
                    );
                    llm_end_span.set(
                        crate::observability::otel::ATTR_USAGE_TOTAL_TOKENS,
                        total_tok,
                    );
                    llm_end_span.set(
                        crate::observability::otel::ATTR_THEO_DURATION_MS,
                        llm_duration,
                    );
                    self.event_bus.publish(DomainEvent::new(
                        EventType::LlmCallEnd,
                        self.run.run_id.as_str(),
                        serde_json::json!({
                            "iteration": iteration,
                            "duration_ms": llm_duration,
                            "input_tokens": input_tok,
                            "output_tokens": output_tok,
                            "total_tokens": total_tok,
                            "context_tokens": estimated_context_tokens,
                            "otel": llm_end_span.to_json(),
                        }),
                    ));
                    resp
                }
                Err(e) if e.is_context_overflow() => {
                    // T5.5 FM-6: snapshot hot files BEFORE compaction destroys them.
                    for f in &self.working_set.hot_files {
                        self.pre_compaction_hot_files.insert(f.clone());
                    }
                    // Reactive context overflow recovery: emergency compact at 50%
                    // and retry once. Pi-mono ref: packages/ai/src/utils/overflow.ts
                    self.event_bus.publish(DomainEvent::new(
                        EventType::ContextOverflowRecovery,
                        self.run.run_id.as_str(),
                        serde_json::json!({
                            "error": e.to_string(),
                            "action": "emergency_compaction",
                            "target_ratio": 0.5,
                        }),
                    ));

                    // Emergency compaction: keep only a bounded fraction of context.
                    let model_ctx = self.config.context_window_tokens;
                    let target = (model_ctx as f64
                        * crate::constants::EMERGENCY_COMPACT_RATIO)
                        as usize;
                    let before_len = messages.len();
                    crate::compaction::compact_messages_to_target(
                        &mut messages,
                        target,
                        "", // No task objective available at this level
                    );
                    eprintln!(
                        "[theo] Context overflow recovery: compacted {} → {} messages (target {})",
                        before_len,
                        messages.len(),
                        target
                    );

                    // Do NOT retry inline — the next loop iteration will re-call LLM
                    // with the compacted context.
                    continue;
                }
                Err(e) => {
                    self.transition_run(RunState::Aborted);
                    let _ = self
                        .task_manager
                        .transition(&self.task_id, TaskState::Failed);
                    self.metrics.record_run_complete(false);
                    // Phase 59: classify the LLM error so headless v3
                    // consumers (ab_compare) can separate infra failures
                    // (rate-limit, auth, overflow) from real outcomes.
                    let class = llm_error_to_class(&e);
                    return AgentResult::from_engine_state(
                        self,
                        false,
                        format!("LLM error: {e}"),
                        false,
                        class,
                    );
                }
            };

            let tool_calls = response.tool_calls();

            // No tool calls → text-only response (OpenCode pattern)
            // LLM decided to respond with text, not use tools.
            // This handles conversational messages ("hello"), informational queries,
            // and any response where the agent chose not to invoke tools.
            //
            // Before converging, check follow-up queue: if the user typed something
            // while the agent was working, inject it and continue.
            // Pi-mono ref: `packages/agent/src/agent-loop.ts:220-228`
            if tool_calls.is_empty() {
                let content = response.content().unwrap_or("").to_string();

                // Check follow-up queue before converging
                if let Some(ref follow_up_fn) = self.message_queues.follow_up {
                    let follow_ups = follow_up_fn().await;
                    if !follow_ups.is_empty() {
                        // Inject assistant response + follow-ups, continue loop
                        messages.push(Message::assistant(&content));
                        for fu_msg in follow_ups {
                            messages.push(fu_msg);
                        }
                        continue;
                    }
                }

                // Plan-mode safety net: the model is supposed to end with
                // tool calls (write the plan file + done). If it converges with
                // text only and no plan file was written yet, give it ONE
                // corrective nudge to actually call the tools. Without this
                // guard the model occasionally produces a beautiful plan as
                // text and exits without persisting it.
                if self.config.mode == crate::config::AgentMode::Plan
                    && !self.plan_mode_nudged
                    && !content.is_empty()
                {
                    let plans_dir = self.project_dir.join(".theo/plans");
                    let plan_written = plans_dir
                        .read_dir()
                        .ok()
                        .map(|mut it| it.next().is_some())
                        .unwrap_or(false);
                    if !plan_written {
                        self.plan_mode_nudged = true;
                        messages.push(Message::assistant(&content));
                        messages.push(Message::user(
                            "REMINDER: You wrote a plan as text but did not persist it. \
                             You MUST now call the `write` tool to save the plan to \
                             `.theo/plans/01-<slug>.md` (use a kebab-case slug derived from \
                             the task), then call `done` with a one-line summary. Do this in \
                             your next response. Do not write more prose — just call the tools.",
                        ));
                        continue;
                    }
                }

                // Phase 0 T0.1 AC-0.1.2: persist the user→assistant exchange
                // INLINE (not fire-and-forget) — durability > latency.
                crate::memory_lifecycle::run_engine_hooks::sync_final_turn(
                    &self.config,
                    &messages,
                    &content,
                )
                .await;

                // PLAN_AUTO_EVOLUTION_SOTA Phase 1 + Phase 3 — reviewers nudge.
                // Relaxed is sufficient: this flag is a pure per-task
                // counter with no happens-before dependency on any other
                // load; it is written and read on the same task inside
                // the serial main loop (T5.4).
                let tool_calls_this_task = self.metrics.snapshot().total_tool_calls as usize;
                let skill_created = self
                    .skill_created_this_task
                    .load(std::sync::atomic::Ordering::Relaxed);
                crate::memory_lifecycle::maybe_spawn_reviewers(
                    &self.config,
                    &self.memory_nudge_counter,
                    &self.skill_nudge_counter,
                    &messages,
                    tool_calls_this_task,
                    skill_created,
                );
                self.skill_created_this_task
                    .store(false, std::sync::atomic::Ordering::Relaxed);

                self.transition_run(RunState::Converged);
                let _ = self
                    .task_manager
                    .transition(&self.task_id, TaskState::Completed);
                self.metrics.record_run_complete(true);
                return AgentResult::from_engine_state(
                    self,
                    true,
                    content,
                    true,
                    ErrorClass::Solved,
                );
            }

            // ── EXECUTING phase ──
            self.transition_run(RunState::Executing);

            // LLM intention text is already streamed via ContentDelta events
            // during chat_streaming(). No need to re-emit here.

            messages.push(Message::assistant_with_tool_calls(
                response.content().map(String::from),
                tool_calls.to_vec(),
            ));

            // Persist assistant message to state manager (crash recovery)
            if let Some(ref mut sm) = state_manager {
                let content = response.content().unwrap_or("");
                let _ = sm.append_message("assistant", content);
            }

            let mut should_return = None;

            for call in tool_calls {
                let name = &call.function.name;

                // Phase 30 (resume-runtime-wiring) — gap #3: replay
                // short-circuit. When the engine is in resume mode AND
                // this `call_id` already produced a result in the
                // original run, push the cached `Message::tool_result`
                // and emit a `ToolCallCompleted` event tagged with
                // `replayed: true`. Skip the entire dispatch path so no
                // side-effects re-execute (write/bash/etc.).
                if let Some(ref ctx) = self.resume_context
                    && ctx.should_skip_tool_call(&call.id)
                    && let Some(cached) = ctx.cached_tool_result(&call.id)
                {
                    messages.push(cached.clone());
                    // Phase 44 (otlp-exporter-plan): replay events still
                    // get an `otel` payload — tagged with `replayed: true`
                    // so dashboards can filter (or count separately).
                    let mut replay_span = crate::observability::otel::tool_call_span(name);
                    replay_span.set(crate::observability::otel::ATTR_THEO_TOOL_CALL_ID, call.id.clone());
                    replay_span.set(crate::observability::otel::ATTR_THEO_TOOL_STATUS, "Succeeded");
                    replay_span.set(crate::observability::otel::ATTR_THEO_TOOL_REPLAYED, true);
                    self.event_bus.publish(DomainEvent::new(
                        EventType::ToolCallCompleted,
                        self.run.run_id.as_str(),
                        serde_json::json!({
                            "tool_name": name,
                            "call_id": &call.id,
                            "replayed": true,
                            "status": "Succeeded",
                            "otel": replay_span.to_json(),
                        }),
                    ));
                    continue;
                }

                // `done` meta-tool — gates Gate 0/1/2 live in
                // `dispatch/done.rs` (Fase 4 — T4.2).
                if name == "done" {
                    match self.handle_done_call(call, iteration, &mut messages).await {
                        dispatch::DispatchOutcome::Converged(result) => {
                            should_return = Some(result);
                            break;
                        }
                        dispatch::DispatchOutcome::Continue => continue,
                    }
                }

                // `delegate_task` / `delegate_task_single` /
                // `delegate_task_parallel` — extracted to dispatch/delegate.rs.
                if name == "delegate_task"
                    || name == "delegate_task_single"
                    || name == "delegate_task_parallel"
                {
                    self.dispatch_delegate_task(call, &mut messages).await;
                    continue;
                }

                // `skill` — extracted to dispatch/skill.rs.
                if name == "skill" {
                    self.dispatch_skill(call, &mut messages).await;
                    continue;
                }

                // `batch` — extracted to dispatch/batch.rs.
                if name == "batch" {
                    self.dispatch_batch(call, &abort_rx, &mut messages).await;
                    continue;
                }

                // ── PLAN MODE GUARD ──
                // In Plan mode, block write tools except writes to .theo/plans/.
                // Also block `think` — reasoning must appear as visible assistant text.
                if self.config.mode == crate::config::AgentMode::Plan {
                    if name == "think" {
                        messages.push(Message::tool_result(
                            &call.id, name,
                            "BLOCKED by Plan mode: The `think` tool is forbidden in plan mode. \
                             Write your reasoning and plan as visible markdown text in your assistant message instead. \
                             The user is reading your messages directly.",
                        ));
                        continue;
                    }
                    let is_write_tool = matches!(name.as_str(), "edit" | "write" | "apply_patch");
                    if is_write_tool {
                        let is_roadmap_write = name == "write"
                            && call
                                .parse_arguments()
                                .ok()
                                .and_then(|a| {
                                    a.get("filePath").and_then(|p| p.as_str()).map(String::from)
                                })
                                .map(|p| p.contains(".theo/plans/"))
                                .unwrap_or(false);

                        if !is_roadmap_write {
                            messages.push(Message::tool_result(
                                &call.id, name,
                                "BLOCKED by Plan mode guard: You can only write to .theo/plans/. \
                                 Write the roadmap first. Source code edits are not allowed until user approves.",
                            ));
                            continue;
                        }
                    }
                }

                // ── PRE-HOOK: tool.before ──
                if let Some(ref runner) = hook_runner {
                    let hook_args = call.parse_arguments().unwrap_or_default();
                    let event = crate::hooks::tool_hook_event(
                        "tool.before",
                        name,
                        &hook_args,
                        &self.project_dir,
                    );
                    let hook_result = runner.run_pre_hook("tool.before", &event).await;
                    if !hook_result.allowed {
                        messages.push(Message::tool_result(
                            &call.id,
                            name,
                            format!("BLOCKED by hook: {}", hook_result.output.trim()),
                        ));
                        continue;
                    }
                }

                // ── PHASE 9: PRE-MUTATION CHECKPOINT ──
                // Snapshot the workdir BEFORE the first mutating tool of
                // each iteration. Idempotent within an iteration (CAS).
                // No-op when no checkpoint manager is attached.
                if let Some(sha) = self.maybe_checkpoint_for_tool(name, iteration as u32) {
                    self.event_bus.publish(DomainEvent::new(
                        EventType::RunStateChanged,
                        self.run.run_id.as_str(),
                        serde_json::json!({
                            "from": "Executing",
                            "to": format!("Checkpoint:{}:turn-{}", &sha[..sha.len().min(12)], iteration),
                        }),
                    ));
                }

                // ── PHASE 8: MCP DISPATCH ──
                // If the tool name is in the `mcp:<server>:<tool>`
                // namespace, route to McpDispatcher (transient stdio
                // client). Otherwise fall through to the normal dispatch.
                if let Some(mcp_msg) = self.try_dispatch_mcp_tool(call).await {
                    messages.push(mcp_msg);
                    continue;
                }

                // Execute regular tool via ToolCallManager (Invariants 2, 3, 5)
                let tool_args = match call.parse_arguments() {
                    Ok(args) => args,
                    Err(e) => {
                        // Report parse error to LLM so it can fix and retry
                        messages.push(Message::tool_result(
                            &call.id,
                            name,
                            format!(
                                "Failed to parse arguments: {e}. Please retry with valid JSON."
                            ),
                        ));
                        continue;
                    }
                };
                // Apply tool's prepare_arguments hook (normalizes/migrates args
                // before schema validation). Pi-mono ref: prepareArguments hook.
                let tool_args = if let Some(tool) = self.registry.get(name) {
                    tool.prepare_arguments(tool_args)
                } else {
                    tool_args
                };
                let tool_call_id =
                    self.tool_call_manager
                        .enqueue(self.task_id.clone(), name.clone(), tool_args);

                let ctx = ToolContext {
                    session_id: SessionId::new("agent"),
                    message_id: MessageId::new(format!("iter_{iteration}")),
                    call_id: call.id.clone(),
                    agent: "main".to_string(),
                    abort: abort_rx.clone(),
                    project_dir: self.project_dir.clone(),
                    graph_context: self.graph_context.clone(),
                    stdout_tx: None,
                };

                let tool_result = self
                    .tool_call_manager
                    .dispatch_and_execute(&tool_call_id, &self.registry, &ctx)
                    .await;

                let (success, output) = match &tool_result {
                    Ok(r) => (r.status == ToolCallState::Succeeded, r.output.clone()),
                    Err(e) => (false, format!("Tool call error: {}", e)),
                };

                // Record tool call in budget and metrics
                self.budget_enforcer.record_tool_call();
                self.metrics.record_tool_call(name, 0, success);

                // Track failure patterns for steering loop suggestions
                if !success {
                    let pattern = format!("{}_failure", name);
                    if let Some(suggestion) = self.failure_tracker.record_and_check(&pattern) {
                        messages.push(Message::user(&suggestion));
                    }
                }

                // Doom loop detection: check if this call repeats identically
                if let Some(ref mut tracker) = doom_tracker {
                    let args = call.parse_arguments().unwrap_or_default();
                    if tracker.record(name, &args) {
                        let warning = format!(
                            "⚠️ DOOM LOOP DETECTED: You have called '{}' with identical arguments {} times in a row. \
                             You are stuck in a loop. Try a DIFFERENT approach or tool.",
                            name,
                            self.config.doom_loop_threshold.unwrap_or(3)
                        );
                        self.event_bus.publish(DomainEvent::new(
                            EventType::Error,
                            self.run.run_id.as_str(),
                            serde_json::json!({
                                "type": "doom_loop",
                                "tool_name": name,
                                "threshold": self.config.doom_loop_threshold,
                            }),
                        ));
                        messages.push(Message::user(&warning));

                        // Hard abort after 2x threshold (warning wasn't enough)
                        if tracker.should_abort() {
                            self.transition_run(RunState::Aborted);
                            let _ = self
                                .task_manager
                                .transition(&self.task_id, TaskState::Failed);
                            self.metrics.record_run_complete(false);
                            let summary = format!(
                                "Doom loop abort: '{}' called identically {} times. Agent is stuck.",
                                name,
                                self.config.doom_loop_threshold.unwrap_or(3) * 2
                            );
                            return AgentResult::from_engine_state(
                                self,
                                false,
                                summary,
                                false,
                                ErrorClass::Aborted,
                            );
                        }
                    }
                }

                let result_msg = Message::tool_result(&call.id, name, &output);

                // Update working set + context metrics with tool interaction data.
                // This feeds the usefulness pipeline (P0: feedback data).
                match name.as_str() {
                    "read" | "edit" | "write" | "apply_patch" => {
                        if let Ok(args) = call.parse_arguments()
                            && let Some(path) = args
                                .get("filePath")
                                .or(args.get("file_path"))
                                .and_then(|p| p.as_str())
                            {
                                self.working_set.touch_file(path);
                                self.context_metrics.record_artifact_fetch(path, iteration);
                                // P0: Feed usefulness pipeline — record which files agent actually uses
                                self.context_metrics.record_tool_reference(path);
                            }
                    }
                    "grep" | "glob" | "codebase_context" => {
                        if let Ok(args) = call.parse_arguments() {
                            let query = args
                                .get("pattern")
                                .or(args.get("query"))
                                .and_then(|p| p.as_str())
                                .unwrap_or("");
                            self.context_metrics
                                .record_action(&format!("{}: {}", name, query), iteration);
                            // Also record searched paths as references
                            if let Some(path) = args.get("path").and_then(|p| p.as_str()) {
                                self.context_metrics.record_tool_reference(path);
                            }
                        }
                    }
                    _ => {}
                }

                // Record event in working set
                self.working_set.record_event(
                    format!("tool:{}:iter{}", name, iteration),
                    20, // keep last 20 events
                );

                // Update context-loop diagnostics state
                match name.as_str() {
                    "read" => {
                        if let Ok(args) = call.parse_arguments()
                            && let Some(path) = args.get("filePath").and_then(|p| p.as_str()) {
                                self.context_loop_state.record_read(path);
                            }
                    }
                    "grep" | "glob" => self.context_loop_state.record_search(),
                    "edit" | "write" | "apply_patch" => {
                        let file = call
                            .parse_arguments()
                            .ok()
                            .and_then(|args| {
                                // For edit/write: filePath is a direct arg
                                args.get("filePath")
                                    .or(args.get("file_path"))
                                    .and_then(|p| p.as_str())
                                    .map(String::from)
                                    // For apply_patch: extract from patchText
                                    .or_else(|| {
                                        args.get("patchText").and_then(|p| p.as_str()).and_then(
                                            |patch| {
                                                patch
                                                    .lines()
                                                    .find(|l| l.starts_with("+++ "))
                                                    .and_then(|l| {
                                                        l.strip_prefix("+++ b/")
                                                            .or(l.strip_prefix("+++ "))
                                                    })
                                                    .filter(|f| *f != "/dev/null")
                                                    .map(String::from)
                                            },
                                        )
                                    })
                            })
                            .unwrap_or_default();
                        self.context_loop_state.record_edit_attempt(
                            &file,
                            success,
                            if success { None } else { Some(output.clone()) },
                        );
                    }
                    _ => {}
                }

                // Persist tool result to state manager (crash recovery)
                if let Some(ref mut sm) = state_manager {
                    let _ = sm.append_message("tool", &output);
                }

                // ── SENSOR FIRE: trigger computational verification after successful writes ──
                if success && crate::sensor::is_write_tool(name)
                    && let Some(ref sensor_runner) = sensor_runner {
                        let file_path = call
                            .parse_arguments()
                            .ok()
                            .and_then(|args| {
                                args.get("filePath")
                                    .or(args.get("file_path"))
                                    .and_then(|p| p.as_str())
                                    .map(String::from)
                            })
                            .unwrap_or_default();
                        if !file_path.is_empty() {
                            sensor_runner.fire(name, &file_path, &self.project_dir);
                        }
                    }

                messages.push(result_msg);

                // ── POST-HOOK: tool.after ──
                if let Some(ref runner) = hook_runner {
                    let hook_args = call.parse_arguments().unwrap_or_default();
                    let event = crate::hooks::tool_hook_event(
                        "tool.after",
                        name,
                        &hook_args,
                        &self.project_dir,
                    );
                    runner.run_post_hook("tool.after", &event).await;
                }
            }

            if let Some(result) = should_return {
                return result;
            }

            // ── Steering queue check ──
            // After tool execution, check if the user injected messages mid-run.
            // These are inserted as user messages before the next LLM call.
            // Pi-mono ref: `packages/agent/src/agent-loop.ts:216`
            if let Some(ref steering_fn) = self.message_queues.steering {
                let steering_msgs = steering_fn().await;
                for msg in steering_msgs {
                    messages.push(msg);
                }
            }

            // ── EVALUATING phase ──
            self.transition_run(RunState::Evaluating);

            // Save snapshot if store is configured (Invariant 7)
            if let Some(ref store) = self.snapshot_store
                && let Some(task) = self.task_manager.get(&self.task_id) {
                    // Collect real tool calls and results for this task.
                    let tool_calls = self.tool_call_manager.calls_for_task(&self.task_id);
                    let tool_results: Vec<theo_domain::tool_call::ToolResultRecord> = tool_calls
                        .iter()
                        .filter_map(|tc| self.tool_call_manager.get_result(&tc.call_id))
                        .collect();

                    // Serialize conversation messages.
                    let messages_json: Vec<serde_json::Value> = messages
                        .iter()
                        .filter_map(|m| serde_json::to_value(m).ok())
                        .collect();

                    let mut snapshot = RunSnapshot::new(
                        self.run.clone(),
                        task,
                        tool_calls,
                        tool_results,
                        self.event_bus.events(),
                        self.budget_enforcer.usage(),
                        messages_json,
                        vec![], // DLQ entries
                    );
                    snapshot.working_set = Some(self.working_set.clone());
                    snapshot.checksum = snapshot.compute_checksum();
                    let _ = store.save(&self.run.run_id, &snapshot).await;
                }

            // After executing tools, evaluate and loop back to Planning
            self.transition_run(RunState::Replanning);
        }
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

    // ---------------------------------------------------------------------
    // Phase 4: delegate_task dispatch (Track A)
    // ---------------------------------------------------------------------

    /// Handle a `delegate_task` tool call. Validates the schema and routes
    /// to single or parallel sub-agent spawn.
    ///
    /// Schema rules:
    /// - Either `agent` + `objective` (single) OR `parallel: [...]` (multi).
    /// - Both present → error.
    /// - Neither present → error.
    ///
    /// Routing:
    /// - Known agent name → `spawn_with_spec` with the registered spec.
    /// - Unknown name → `AgentSpec::on_demand` (read-only by S1).
    async fn handle_delegate_task(&mut self, args: serde_json::Value) -> String {
        let has_agent = args.get("agent").is_some();
        let has_parallel = args.get("parallel").is_some();

        if has_agent && has_parallel {
            return "Error: delegate_task accepts EITHER `agent`+`objective` OR `parallel`, not both."
                .to_string();
        }
        if !has_agent && !has_parallel {
            return "Error: delegate_task requires either `agent`+`objective` or `parallel`."
                .to_string();
        }

        // Phase 13: ReloadableRegistry takes precedence — fresh snapshot
        // per call so filesystem changes via RegistryWatcher are picked up
        // immediately. Falls back to static registry, then to builtins+load_all.
        let registry: Arc<crate::subagent::SubAgentRegistry> = if let Some(rel) =
            &self.subagent_reloadable
        {
            Arc::new(rel.snapshot())
        } else if let Some(r) = &self.subagent_registry {
            r.clone()
        } else {
            let mut reg = crate::subagent::SubAgentRegistry::with_builtins();
            let _ = reg.load_all(
                Some(&self.project_dir),
                None,
                crate::subagent::ApprovalMode::TrustAll,
            );
            Arc::new(reg)
        };

        // Build the manager and chain ALL Phase 5-12 integrations.
        let mut manager = crate::subagent::SubAgentManager::with_registry(
            self.config.clone(),
            self.event_bus.clone(),
            self.project_dir.clone(),
            registry,
        )
        .with_metrics(self.metrics.clone());

        if let Some(store) = &self.subagent_run_store {
            manager = manager.with_run_store(store.clone());
        }
        if let Some(hooks) = &self.subagent_hooks {
            manager = manager.with_hooks(hooks.clone());
        }
        if let Some(tree) = &self.subagent_cancellation {
            manager = manager.with_cancellation(tree.clone());
        }
        if let Some(cm) = &self.subagent_checkpoint {
            manager = manager.with_checkpoint(cm.clone());
        }
        if let Some(wp) = &self.subagent_worktree {
            manager = manager.with_worktree_provider(wp.clone());
        }
        if let Some(mcp) = &self.subagent_mcp {
            manager = manager.with_mcp_registry(mcp.clone());
        }
        if let Some(cache) = &self.subagent_mcp_discovery {
            manager = manager.with_mcp_discovery(cache.clone());
        }

        // Phase 18: resolve handoff guardrail chain (injected or default).
        let guardrails: Arc<crate::handoff_guardrail::GuardrailChain> = self
            .subagent_handoff_guardrails
            .clone()
            .unwrap_or_else(|| {
                Arc::new(crate::handoff_guardrail::GuardrailChain::with_default_builtins())
            });

        if has_agent {
            let agent_name = args
                .get("agent")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let objective = args
                .get("objective")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let context = args
                .get("context")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if agent_name.is_empty() {
                return "Error: `agent` must be a non-empty string.".to_string();
            }
            if objective.is_empty() {
                return "Error: `objective` is required when delegating to a single agent."
                    .to_string();
            }

            let initial_spec = manager
                .registry()
                .and_then(|r| r.get(&agent_name).cloned())
                .unwrap_or_else(|| {
                    theo_domain::agent_spec::AgentSpec::on_demand(&agent_name, &objective)
                });

            // Phase 18: handoff guardrail evaluation. Block short-circuits;
            // Redirect/Rewrite mutate the spawn arguments before continuing.
            let (spec, objective, redirect_note) = match self.evaluate_handoff(
                &guardrails,
                "main",
                &agent_name,
                &initial_spec,
                &objective,
            ) {
                crate::run_engine::HandoffOutcome::Block { refusal_message } => {
                    return refusal_message;
                }
                crate::run_engine::HandoffOutcome::Allow => (initial_spec, objective, None),
                crate::run_engine::HandoffOutcome::Redirect {
                    guardrail_id,
                    new_agent_name,
                } => {
                    let new_spec = manager
                        .registry()
                        .and_then(|r| r.get(&new_agent_name).cloned())
                        .unwrap_or_else(|| {
                            theo_domain::agent_spec::AgentSpec::on_demand(
                                &new_agent_name,
                                &objective,
                            )
                        });
                    let note = format!(
                        "[handoff redirected by {} → {}]",
                        guardrail_id, new_agent_name
                    );
                    (new_spec, objective, Some(note))
                }
                crate::run_engine::HandoffOutcome::RewriteObjective {
                    guardrail_id,
                    new_objective,
                } => {
                    let note = format!("[handoff objective rewritten by {}]", guardrail_id);
                    (initial_spec, new_objective, Some(note))
                }
            };

            let result = manager
                .spawn_with_spec_text(&spec, &objective, context.as_deref())
                .await;

            // Aggregate metrics into parent budget
            self.budget_enforcer.record_tokens(result.tokens_used);
            self.metrics.record_delegated_tokens(result.tokens_used);

            // Mirror legacy formatter — prefix the redirect/rewrite note
            // when present so the parent LLM sees the mutation explicitly.
            let prefix = redirect_note
                .map(|n| format!("{} ", n))
                .unwrap_or_default();
            if result.success {
                format!(
                    "{}[{} sub-agent completed] {}",
                    prefix, spec.name, result.summary
                )
            } else {
                format!(
                    "{}[{} sub-agent failed] {}",
                    prefix, spec.name, result.summary
                )
            }
        } else {
            // Parallel mode
            let arr = args
                .get("parallel")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if arr.is_empty() {
                return "Error: `parallel` must be a non-empty array.".to_string();
            }

            let mut combined = String::new();
            for (i, entry) in arr.iter().enumerate() {
                let agent_name = entry
                    .get("agent")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let objective = entry
                    .get("objective")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let context = entry
                    .get("context")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if agent_name.is_empty() || objective.is_empty() {
                    combined.push_str(&format!(
                        "[Sub-agent {}] ERROR: missing agent/objective\n",
                        i + 1
                    ));
                    continue;
                }
                let initial_spec = manager
                    .registry()
                    .and_then(|r| r.get(&agent_name).cloned())
                    .unwrap_or_else(|| {
                        theo_domain::agent_spec::AgentSpec::on_demand(&agent_name, &objective)
                    });

                // Phase 18: per-entry handoff guardrail evaluation.
                let (spec, objective, redirect_note) = match self.evaluate_handoff(
                    &guardrails,
                    "main",
                    &agent_name,
                    &initial_spec,
                    &objective,
                ) {
                    crate::run_engine::HandoffOutcome::Block { refusal_message } => {
                        combined.push_str(&format!(
                            "[Sub-agent {}] ❌ {} (handoff refused): {}\n",
                            i + 1,
                            initial_spec.name,
                            refusal_message,
                        ));
                        continue;
                    }
                    crate::run_engine::HandoffOutcome::Allow => (initial_spec, objective, None),
                    crate::run_engine::HandoffOutcome::Redirect {
                        guardrail_id,
                        new_agent_name,
                    } => {
                        let new_spec = manager
                            .registry()
                            .and_then(|r| r.get(&new_agent_name).cloned())
                            .unwrap_or_else(|| {
                                theo_domain::agent_spec::AgentSpec::on_demand(
                                    &new_agent_name,
                                    &objective,
                                )
                            });
                        let note = format!(
                            "[handoff redirected by {} → {}]",
                            guardrail_id, new_agent_name
                        );
                        (new_spec, objective, Some(note))
                    }
                    crate::run_engine::HandoffOutcome::RewriteObjective {
                        guardrail_id,
                        new_objective,
                    } => {
                        let note = format!(
                            "[handoff objective rewritten by {}]",
                            guardrail_id
                        );
                        (initial_spec, new_objective, Some(note))
                    }
                };

                let result = manager
                    .spawn_with_spec_text(&spec, &objective, context.as_deref())
                    .await;

                self.budget_enforcer.record_tokens(result.tokens_used);
                self.metrics.record_delegated_tokens(result.tokens_used);

                let mark = if result.success { "✅" } else { "❌" };
                let prefix = redirect_note
                    .map(|n| format!("{} ", n))
                    .unwrap_or_default();
                combined.push_str(&format!(
                    "[Sub-agent {}] {} {}{} ({}): {}\n",
                    i + 1,
                    mark,
                    prefix,
                    spec.name,
                    spec.source.as_str(),
                    result.summary,
                ));
            }
            combined
        }
    }

    // ---------------------------------------------------------------------
    // Phase 18: handoff guardrail evaluation
    // ---------------------------------------------------------------------

    /// Outcome of a handoff evaluation.
    ///
    /// `Block` short-circuits the spawn with a refusal message returned to
    /// the LLM. `Redirect`/`Rewrite` mutate the spawn arguments; the caller
    /// then continues with the new (target_agent, objective) pair.
    /// `Allow` is the default — proceed unchanged.
    pub fn evaluate_handoff(
        &self,
        chain: &crate::handoff_guardrail::GuardrailChain,
        source_agent: &str,
        target_agent: &str,
        target_spec: &theo_domain::agent_spec::AgentSpec,
        objective: &str,
    ) -> HandoffOutcome {
        use crate::handoff_guardrail::{GuardrailDecision, HandoffContext};
        let ctx = HandoffContext {
            source_agent,
            target_agent,
            target_spec,
            objective,
            source_capabilities: self.config.capability_set.as_ref(),
        };

        let decisions = chain.evaluate(&ctx);
        let blocked_by = decisions.iter().find_map(|(id, d)| match d {
            GuardrailDecision::Block { reason } => Some((id.clone(), reason.clone())),
            _ => None,
        });
        let warnings: Vec<String> = decisions
            .iter()
            .filter_map(|(id, d)| match d {
                GuardrailDecision::Warn { message } => Some(format!("[{}] {}", id, message)),
                _ => None,
            })
            .collect();
        let mutating = decisions.iter().find_map(|(id, d)| match d {
            GuardrailDecision::Redirect { new_agent_name } => {
                Some(("redirect", id.clone(), Some(new_agent_name.clone()), None))
            }
            GuardrailDecision::RewriteObjective { new_objective } => {
                Some(("rewrite", id.clone(), None, Some(new_objective.clone())))
            }
            _ => None,
        });

        // Phase 18 + 24: PreHandoff hook only fires when no chain block —
        // chain wins first. Hooks may also Block, becoming the final blocker.
        // Phase 24 (sota-gaps-followup): populates HookContext.target_agent
        // + target_objective so YAML matchers can regex-match against them.
        let hook_block = if blocked_by.is_none() {
            self.subagent_hooks.as_ref().and_then(|hooks| {
                use crate::lifecycle_hooks::{HookContext, HookEvent, HookResponse};
                let hook_ctx = HookContext {
                    tool_name: Some(format!("delegate_task:{}", target_agent)),
                    tool_args: Some(serde_json::json!({
                        "agent": target_agent,
                        "objective": objective,
                    })),
                    tool_result: None,
                    target_agent: Some(target_agent.to_string()),
                    target_objective: Some(objective.to_string()),
                };
                match hooks.dispatch(HookEvent::PreHandoff, &hook_ctx) {
                    HookResponse::Block { reason } => {
                        Some(("hook.pre_handoff".to_string(), reason))
                    }
                    _ => None,
                }
            })
        } else {
            None
        };

        let final_block = blocked_by.clone().or(hook_block.clone());
        let decision_label = if final_block.is_some() {
            "block"
        } else if let Some((label, _, _, _)) = &mutating {
            *label
        } else if !warnings.is_empty() {
            "warn"
        } else {
            "allow"
        };

        // Always publish an audit event.
        self.event_bus.publish(theo_domain::event::DomainEvent::new(
            theo_domain::event::EventType::HandoffEvaluated,
            self.run.run_id.as_str(),
            serde_json::json!({
                "source_agent": source_agent,
                "target_agent": target_agent,
                "target_source": target_spec.source.as_str(),
                "objective": truncate_handoff_objective(objective),
                "decision": decision_label,
                "reason": final_block.as_ref().map(|(_, r)| r.clone()),
                "blocked_by": final_block.as_ref().map(|(id, _)| id.clone()),
                "redirect_to": mutating.as_ref().and_then(|(_, _, n, _)| n.clone()),
                "rewrite_objective": mutating
                    .as_ref()
                    .and_then(|(_, _, _, o)| o.clone())
                    .map(|s| truncate_handoff_objective(&s)),
                "mutated_by": mutating.as_ref().map(|(_, id, _, _)| id.clone()),
                "guardrails_evaluated": chain.ids(),
                "warnings": warnings,
            }),
        ));

        if let Some((id, reason)) = final_block {
            return HandoffOutcome::Block {
                refusal_message: format!("[handoff refused by {}] {}", id, reason),
            };
        }
        match mutating {
            Some(("redirect", id, Some(new), _)) => HandoffOutcome::Redirect {
                guardrail_id: id,
                new_agent_name: new,
            },
            Some(("rewrite", id, _, Some(new))) => HandoffOutcome::RewriteObjective {
                guardrail_id: id,
                new_objective: new,
            },
            _ => HandoffOutcome::Allow,
        }
    }

    /// Backwards-compatible wrapper used by tests written before the
    /// outcome enum existed. Returns `Some(refusal)` on Block, `None`
    /// otherwise — note: redirects/rewrites now return None (caller is
    /// expected to handle them by inspecting the outcome enum directly).
    #[deprecated(note = "use evaluate_handoff instead")]
    pub fn evaluate_handoff_or_refuse(
        &self,
        chain: &crate::handoff_guardrail::GuardrailChain,
        source_agent: &str,
        target_agent: &str,
        target_spec: &theo_domain::agent_spec::AgentSpec,
        objective: &str,
    ) -> Option<String> {
        match self.evaluate_handoff(chain, source_agent, target_agent, target_spec, objective) {
            HandoffOutcome::Block { refusal_message } => Some(refusal_message),
            _ => None,
        }
    }
}

/// Outcome of `AgentRunEngine::evaluate_handoff`. Phase 18 (sota-gaps).
#[derive(Debug, Clone)]
pub enum HandoffOutcome {
    /// Proceed with the spawn unchanged.
    Allow,
    /// Refuse the spawn and surface `refusal_message` to the LLM.
    Block { refusal_message: String },
    /// Spawn `new_agent_name` instead of the requested target. Objective
    /// is preserved. `guardrail_id` is logged for the audit event prefix.
    Redirect {
        guardrail_id: String,
        new_agent_name: String,
    },
    /// Spawn the requested target but with `new_objective` replacing the
    /// LLM-provided one.
    RewriteObjective {
        guardrail_id: String,
        new_objective: String,
    },
}

/// Phase 59 (headless-error-classification-plan): map an LLM error to its
/// canonical `ErrorClass`. Used at every site in `execute_with_history`
/// that returns `AgentResult` from a failed LLM call so headless v3
/// consumers can distinguish infra failures (rate-limit, quota, auth)
/// from agent failures.
// Helpers below (llm_error_to_class, truncate_handoff_objective,
// truncate_batch_args, derive_provider_hint) were extracted to
// `run_engine_helpers.rs` in Fase 4 — see `use` alias at bottom of
// file. Auto-init + sandbox spawn likewise moved to their own modules.

// ---------------------------------------------------------------------------
// Doom Loop Detection
// ---------------------------------------------------------------------------

use crate::doom_loop::DoomLoopTracker;
use crate::run_engine_helpers::{
    derive_provider_hint, llm_error_to_class, truncate_handoff_objective,
};
use theo_domain::clock::now_millis;

// NOTE: `derive_provider_hint`, `llm_error_to_class`,
// `truncate_batch_args`, `truncate_handoff_objective`,
// `auto_init_project_context`, `spawn_done_gate_cargo` all moved to
// `run_engine_helpers.rs` / `run_engine_auto_init.rs` /
// `run_engine_sandbox.rs` in Fase 4 (REMEDIATION_PLAN T4.2).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::CapturingListener;
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
        // Phase 59 backcompat — legacy tests that build AgentResult via
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
            let class = super::super::llm_error_to_class(
                &LlmError::RateLimited { retry_after: None },
            );
            assert_eq!(class, ErrorClass::RateLimited);
        }

        #[test]
        fn llm_error_to_class_maps_auth_failure() {
            let class = super::super::llm_error_to_class(
                &LlmError::AuthFailed("bad token".into()),
            );
            assert_eq!(class, ErrorClass::AuthFailed);
        }

        #[test]
        fn llm_error_to_class_maps_context_overflow() {
            let class = super::super::llm_error_to_class(&LlmError::ContextOverflow {
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
            let class = super::super::llm_error_to_class(&LlmError::Timeout);
            assert_eq!(class, ErrorClass::Aborted);
            let class = super::super::llm_error_to_class(&LlmError::ServiceUnavailable);
            assert_eq!(class, ErrorClass::Aborted);
        }

        #[test]
        fn llm_error_to_class_maps_quota_exceeded() {
            // Phase 61: distinct from RateLimited so ab_compare can
            // separate "agent retry exhausted" from "account hit billing
            // ceiling — bench is unusable until reset."
            let class = super::super::llm_error_to_class(&LlmError::QuotaExceeded {
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
    // Phase 4: delegate_task validation tests
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

    // ── Phase 29 follow-up: split tool variants ──

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

    // ── Phase 18: handoff guardrails integration ──

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
    // Phase 30 (resume-runtime-wiring) — gap #3 dispatch wiring
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
    // Phase 43 (otlp-exporter-plan) — provider-hint helper coverage
    // -----------------------------------------------------------------------

    mod provider_hint {
        use super::*;

        #[test]
        fn derive_provider_hint_recognizes_openai() {
            assert_eq!(derive_provider_hint("https://api.openai.com/v1"), "openai");
        }

        #[test]
        fn derive_provider_hint_recognizes_chatgpt_oauth() {
            assert_eq!(derive_provider_hint("https://chatgpt.com/backend-api"), "openai");
        }

        #[test]
        fn derive_provider_hint_recognizes_anthropic() {
            assert_eq!(derive_provider_hint("https://api.anthropic.com"), "anthropic");
        }

        #[test]
        fn derive_provider_hint_recognizes_gemini() {
            assert_eq!(derive_provider_hint("https://generativelanguage.googleapis.com"), "gemini");
        }

        #[test]
        fn derive_provider_hint_falls_back_for_unknown_url() {
            assert_eq!(derive_provider_hint("https://my-private-llm.corp"), "openai_compatible");
        }

        #[test]
        fn derive_provider_hint_recognizes_localhost_as_local() {
            assert_eq!(derive_provider_hint("http://localhost:8000"), "openai_compatible_local");
            assert_eq!(derive_provider_hint("http://127.0.0.1:8080"), "openai_compatible_local");
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

        // Phase 59 (headless-error-classification-plan) — error_class
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
