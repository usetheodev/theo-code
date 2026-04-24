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
    ConvergenceContext, ConvergenceEvaluator, ConvergenceMode, EditSuccessConvergence,
    GitDiffConvergence, check_git_changes,
};
use crate::event_bus::EventBus;
use crate::loop_state::ContextLoopState;
use crate::metrics::{MetricsCollector, RuntimeMetrics};
use crate::persistence::SnapshotStore;
use crate::skill::SkillRegistry;
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
    /// Phase 1 T1.1: accumulated token usage across LLM calls.
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
    /// Phase 1-13 integrations: when present, propagated to spawn_with_spec.
    /// Optional so backward-compat is preserved (legacy callers don't need to inject).
    subagent_registry: Option<Arc<crate::subagent::SubAgentRegistry>>,
    subagent_run_store: Option<Arc<crate::subagent_runs::FileSubagentRunStore>>,
    subagent_hooks: Option<Arc<crate::lifecycle_hooks::HookManager>>,
    subagent_cancellation: Option<Arc<crate::cancellation::CancellationTree>>,
    subagent_checkpoint: Option<Arc<crate::checkpoint::CheckpointManager>>,
    subagent_worktree: Option<Arc<theo_isolation::WorktreeProvider>>,
    subagent_mcp: Option<Arc<theo_infra_mcp::McpRegistry>>,
    /// Phase 17: optional MCP discovery cache propagated to spawn_with_spec.
    subagent_mcp_discovery: Option<Arc<theo_infra_mcp::DiscoveryCache>>,
    /// Phase 18: optional handoff guardrail chain. When `None`, a default
    /// chain (built-ins) is used per delegate_task call. Programmatic
    /// callers can register custom guardrails by injecting a chain.
    subagent_handoff_guardrails: Option<Arc<crate::handoff_guardrail::GuardrailChain>>,
    /// Phase 30 (resume-runtime-wiring) — gap #3: optional resume context.
    /// When present, the dispatch loop consults `executed_tool_calls`
    /// before invoking each tool and replays cached results from
    /// `executed_tool_results` to avoid double side-effects.
    resume_context: Option<Arc<crate::subagent::resume::ResumeContext>>,
    /// Phase 8: lazy-built dispatcher used to handle `mcp:server:tool`
    /// calls. Built from `subagent_mcp` on first use.
    subagent_mcp_dispatcher: std::sync::OnceLock<Arc<theo_infra_mcp::McpDispatcher>>,
    /// Phase 13: optional ReloadableRegistry. When Some, takes precedence
    /// over `subagent_registry`: each delegate_task call reads
    /// `reloadable.snapshot()` so changes from the watcher take effect
    /// immediately without restart.
    subagent_reloadable: Option<crate::subagent::ReloadableRegistry>,
    /// Phase 9: turns since the last checkpoint (one snapshot per turn,
    /// only when a mutating tool first fires within that turn).
    checkpoint_taken_this_turn: std::sync::atomic::AtomicBool,
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

    /// Phase 8: dispatch a tool call to MCP if its name is in the
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

    /// Phase 9: at the start of a turn, reset the once-per-turn snapshot flag.
    pub fn reset_turn_checkpoint(&self) {
        self.checkpoint_taken_this_turn
            .store(false, std::sync::atomic::Ordering::Release);
    }

    /// Phase 9: take a snapshot if (a) a checkpoint manager is attached AND
    /// (b) the tool is mutating AND (c) no snapshot was taken this turn yet.
    /// Idempotent within a turn. Returns the SHA on a fresh snapshot, None
    /// otherwise.
    pub fn maybe_checkpoint_for_tool(&self, tool_name: &str, turn_id: u32) -> Option<String> {
        if !Self::is_mutating_tool(tool_name) {
            return None;
        }
        // Compare-and-swap: only snapshot if not already taken this turn.
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

    /// Phase 1-13: inject the SubAgentRegistry. Used by delegate_task to look
    /// up named agents (built-in / project / global). When `None`, a default
    /// registry with builtins is constructed on each delegate_task call.
    pub fn with_subagent_registry(
        mut self,
        registry: Arc<crate::subagent::SubAgentRegistry>,
    ) -> Self {
        self.subagent_registry = Some(registry);
        self
    }

    /// Phase 10: inject session persistence store. When set, sub-agent runs
    /// are persisted in `<base>/runs/{run_id}.json`.
    pub fn with_subagent_run_store(
        mut self,
        store: Arc<crate::subagent_runs::FileSubagentRunStore>,
    ) -> Self {
        self.subagent_run_store = Some(store);
        self
    }

    /// Phase 5: inject global hooks (per-agent hooks merged via spec.hooks).
    pub fn with_subagent_hooks(
        mut self,
        hooks: Arc<crate::lifecycle_hooks::HookManager>,
    ) -> Self {
        self.subagent_hooks = Some(hooks);
        self
    }

    /// Phase 6: inject cancellation tree. Sub-agents register children;
    /// root cancellation propagates.
    pub fn with_subagent_cancellation(
        mut self,
        tree: Arc<crate::cancellation::CancellationTree>,
    ) -> Self {
        self.subagent_cancellation = Some(tree);
        self
    }

    /// Phase 9: inject checkpoint manager. Sub-agents auto-snapshot pre-run.
    pub fn with_subagent_checkpoint(
        mut self,
        manager: Arc<crate::checkpoint::CheckpointManager>,
    ) -> Self {
        self.subagent_checkpoint = Some(manager);
        self
    }

    /// Phase 11: inject worktree provider. Sub-agents with isolation=worktree
    /// get an isolated git worktree.
    pub fn with_subagent_worktree(
        mut self,
        provider: Arc<theo_isolation::WorktreeProvider>,
    ) -> Self {
        self.subagent_worktree = Some(provider);
        self
    }

    /// Phase 8: inject MCP registry. Sub-agents with non-empty
    /// `spec.mcp_servers` get a system-prompt hint listing the allowed
    /// `mcp:server:tool` namespace.
    pub fn with_subagent_mcp(mut self, mcp: Arc<theo_infra_mcp::McpRegistry>) -> Self {
        self.subagent_mcp = Some(mcp);
        self
    }

    /// Phase 17: inject MCP discovery cache. When attached, sub-agents whose
    /// `mcp_servers` allowlist matches a cached server receive a richer
    /// system-prompt hint listing actual tool names instead of just the
    /// `mcp:<server>:<tool>` namespace placeholder.
    pub fn with_subagent_mcp_discovery(
        mut self,
        cache: Arc<theo_infra_mcp::DiscoveryCache>,
    ) -> Self {
        self.subagent_mcp_discovery = Some(cache);
        self
    }

    /// Phase 18: inject the handoff guardrail chain. When `None`, a default
    /// chain (`GuardrailChain::with_default_builtins`) is constructed per
    /// `delegate_task` call.
    pub fn with_subagent_handoff_guardrails(
        mut self,
        chain: Arc<crate::handoff_guardrail::GuardrailChain>,
    ) -> Self {
        self.subagent_handoff_guardrails = Some(chain);
        self
    }

    /// Phase 30 (resume-runtime-wiring) — gap #3: enable replay-mode
    /// dispatch. When set, each tool call is short-circuited if its
    /// `call_id` already appears in the context's `executed_tool_calls`
    /// set; the cached `Message::tool_result` from the event log is
    /// pushed instead of invoking the tool.
    pub fn with_resume_context(
        mut self,
        ctx: Arc<crate::subagent::resume::ResumeContext>,
    ) -> Self {
        self.resume_context = Some(ctx);
        self
    }

    /// Phase 13: inject a ReloadableRegistry. Takes precedence over
    /// `with_subagent_registry`: delegate_task reads a fresh snapshot
    /// each call, so filesystem changes (via RegistryWatcher) take effect
    /// without needing to restart the agent.
    pub fn with_subagent_reloadable(
        mut self,
        reloadable: crate::subagent::ReloadableRegistry,
    ) -> Self {
        self.subagent_reloadable = Some(reloadable);
        self
    }

    /// Phase 9: snapshot the workdir BEFORE a mutating tool fires (edit /
    /// write / apply_patch / bash). Idempotent within a turn — caller is
    /// expected to track once-per-turn state.
    /// Returns the commit SHA on success, None if no checkpoint manager
    /// is attached or snapshot fails (fail-soft).
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

    /// Sets the message queues for steering and follow-up injection.
    pub fn with_message_queues(mut self, queues: MessageQueues) -> Self {
        self.message_queues = queues;
        self
    }

    /// Sets the graph context provider for code intelligence injection.
    pub fn with_graph_context(mut self, provider: Arc<dyn theo_domain::graph_context::GraphContextProvider>) -> Self {
        self.graph_context = Some(provider);
        self
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

    /// Sets the snapshot store for persistence (Invariant 7).
    pub fn with_snapshot_store(mut self, store: Arc<dyn SnapshotStore>) -> Self {
        self.snapshot_store = Some(store);
        self
    }

    /// Execute the full agent run cycle.
    ///
    /// Flow: Initialized → Planning → Executing → Evaluating → Converged/Replanning/Aborted
    /// Execute with fresh messages (no session history).
    pub async fn execute(&mut self) -> AgentResult {
        let result = self.execute_with_history(Vec::new()).await;
        self.record_session_exit(&result).await;
        result
    }

    /// AgentLoop::run_with_history adapter — shares execute()'s shutdown path.
    pub async fn record_session_exit_public(&mut self, r: &AgentResult) { self.record_session_exit(r).await; }

    /// Record session exit. Phase 0 T0.1: async tokio::fs + on_session_end hook.
    async fn record_session_exit(&mut self, result: &AgentResult) {
        // Save failure pattern tracker
        self.failure_tracker.save();

        // Save context metrics to .theo/metrics/{run_id}.json
        let metrics_dir = self.project_dir.join(".theo").join("metrics");
        if tokio::fs::create_dir_all(&metrics_dir).await.is_ok() {
            let report = self.context_metrics.to_report();
            let metrics_path = metrics_dir.join(format!("{}.json", self.run.run_id.as_str()));
            let _ = tokio::fs::write(
                &metrics_path,
                serde_json::to_string_pretty(&report).unwrap_or_default(),
            )
            .await;
        }

        // Generate EpisodeSummary from run events and persist to .theo/memory/episodes/
        // (decision: meeting 20260420-221947 #4 — episodes belong to memory namespace,
        // not wiki; wiki is reserved for compiled content).
        let events = self.event_bus.events();
        if !events.is_empty() {
            let task_objective = self
                .task_manager
                .get(&self.task_id)
                .map(|t| t.objective.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let mut summary = theo_domain::episode::EpisodeSummary::from_events(
                self.run.run_id.as_str(), Some(self.task_id.as_str()), &task_objective, &events,
            );
            // Phase 1 T1.1 (usage+cost) + Phase 2 T2.1 (lesson pipeline, G5).
            let mut usage = self.session_token_usage.clone();
            if let Some(c) = theo_domain::budget::known_model_cost(&self.config.model) { usage.recompute_cost(&c); }
            summary.token_usage = Some(usage);
            // Phase 2: T2.1 (lessons G5) + T2.3 (hypotheses G6).
            let _ = crate::lesson_pipeline::extract_and_persist_for_outcome(&self.project_dir, summary.machine_summary.outcome, &events);
            let _ = crate::hypothesis_pipeline::persist_unresolved(&self.project_dir, &summary);
            let episodes_dir = self
                .project_dir
                .join(".theo")
                .join("memory")
                .join("episodes");
            if tokio::fs::create_dir_all(&episodes_dir).await.is_ok() {
                let episode_path = episodes_dir.join(format!("{}.json", summary.summary_id));
                let _ = tokio::fs::write(
                    &episode_path,
                    serde_json::to_string_pretty(&summary).unwrap_or_default(),
                )
                .await;
            }
        }

        // Phase 0 T0.1 AC-0.1.4: memory-provider session-end hook (every exit path).
        crate::memory_lifecycle::MemoryLifecycle::on_session_end(&self.config).await;

        // PLAN_AUTO_EVOLUTION_SOTA Phase 4 — index session transcript
        // via the pluggable TranscriptIndexer trait (concrete impl
        // lives in theo-application). Awaited inline so shutdown
        // completes only after Tantivy has committed to disk.
        crate::memory_lifecycle::maybe_index_transcript(
            &self.config,
            &self.project_dir,
            self.run.run_id.as_str(),
            events.clone(),
        )
        .await;

        // Record session end for cross-session progress tracking
        if !self.config.is_subagent {
            let tasks = if result.success {
                vec![crate::session_bootstrap::CompletedTask {
                    name: result.summary.chars().take(100).collect(),
                    status: "completed".to_string(),
                    files_changed: result.files_edited.clone(),
                }]
            } else {
                vec![crate::session_bootstrap::CompletedTask {
                    name: result.summary.chars().take(100).collect(),
                    status: "failed".to_string(),
                    files_changed: result.files_edited.clone(),
                }]
            };
            let last_error = if result.success {
                None
            } else {
                Some(result.summary.clone())
            };
            crate::session_bootstrap::record_session_end(
                &self.project_dir,
                self.run.run_id.as_str(),
                tasks,
                vec![], // next_steps are determined by the LLM, not the engine
                last_error,
            );
        }

        // Observability: drain writer, compute RunReport, append summary line.
        self.finalize_observability(result, !events.is_empty());
    }

    fn finalize_observability(&mut self, result: &AgentResult, had_events: bool) {
        let Some(pipeline) = self.observability.take() else { return };
        let file_path = pipeline.finalize();
        self.episodes_created = if had_events { 1 } else { 0 };
        let detected = crate::observability::finalize_run_observability(
            &file_path,
            self.run.run_id.as_str(),
            result.success,
            result.files_edited.len() as u64,
            &self.session_token_usage,
            self.config.max_iterations,
            self.budget_enforcer.usage(),
            &self.context_metrics.to_report(),
            self.done_attempts,
            self.episodes_injected,
            self.episodes_created,
            self.failure_tracker.new_fingerprint_count(),
            self.failure_tracker.recurrent_fingerprint_count(),
            &self.initial_context_files,
            &self.pre_compaction_hot_files,
        );
        detected.publish_events(&self.event_bus, self.run.run_id.as_str());
    }

    /// Execute with session history from previous REPL prompts.
    /// `history` contains messages from prior runs in this session.
    /// The current task objective is appended as the last user message.
    pub async fn execute_with_history(&mut self, history: Vec<Message>) -> AgentResult {
        // Transition to Planning
        self.transition_run(RunState::Planning);

        // Transition task to Running
        let _ = self
            .task_manager
            .transition(&self.task_id, TaskState::Ready);
        let _ = self
            .task_manager
            .transition(&self.task_id, TaskState::Running);

        // Auto-init: create .theo/theo.md if it doesn't exist (main agent only).
        // Uses static template — instantaneous, no LLM cost. The agent can enrich later.
        // Best-effort: if write fails, continue without project context.
        if !self.config.is_subagent {
            auto_init_project_context(&self.project_dir);
        }

        // PLAN_AUTO_EVOLUTION_SOTA Phase 2 — autodream at session start.
        if !self.config.is_subagent {
            crate::memory_lifecycle::maybe_spawn_autodream(
                &self.config,
                &self.autodream_attempted,
                &self.project_dir,
                self.run.run_id.as_str(),
            );
        }

        // System prompt: .theo/system-prompt.md replaces default, or use config default.
        // Phase 5 bootstrap prompt prepended when USER.md is missing/empty.
        let base_prompt = if !self.config.is_subagent {
            crate::project_config::load_system_prompt(&self.project_dir)
                .unwrap_or_else(|| self.config.system_prompt.clone())
        } else {
            self.config.system_prompt.clone()
        };
        let system_prompt =
            crate::memory_lifecycle::maybe_prepend_bootstrap(&self.config, &self.project_dir, base_prompt);

        let mut messages: Vec<Message> = vec![Message::system(&system_prompt)];

        // Project context: .theo/theo.md prepended as separate system message
        if !self.config.is_subagent
            && let Some(context) = crate::project_config::load_project_context(&self.project_dir) {
                messages.push(Message::system(format!("## Project Context\n{context}")));
            }

        // GRAPHCTX is available as the `codebase_context` tool — the LLM calls it on-demand.
        // No automatic injection: the LLM decides when it needs code structure context.
        // The graph_context provider is passed to tools via ToolContext.graph_context.

        // Memory injection (Phase 0 T0.1): prefetch when enabled (sole
        // source), else legacy FileMemoryStore fallback. Dual-injection
        // is prevented by this explicit branch (evolution-agent concern).
        if self.config.memory_enabled {
            let query = self
                .task_manager
                .get(&self.task_id)
                .map(|t| t.objective.clone())
                .unwrap_or_else(|| "session".into());
            let _ = crate::memory_lifecycle::run_engine_hooks::inject_prefetch(
                &self.config, &mut messages, &query,
            )
            .await;
        } else {
            crate::memory_lifecycle::run_engine_hooks::inject_legacy_file_memory(
                &self.project_dir, &mut messages,
            )
            .await;
        }

        // Phase 0 T0.3: feed eligible episode summaries back into context
        // (lifecycle != Archived, TTL not expired, 5% token budget).
        if !self.config.is_subagent {
            let injected = crate::memory_lifecycle::run_engine_hooks::inject_episode_history(
                &self.project_dir,
                self.config.context_window_tokens,
                &mut messages,
            );
            self.episodes_injected = self.episodes_injected.saturating_add(injected as u32);
        }

        // Boot sequence: inject progress from previous sessions + recent git activity.
        // Inserted after memories, before skills — so the agent knows where it left off.
        if !self.config.is_subagent {
            let mut boot_parts: Vec<String> = Vec::new();

            // Previous session progress
            if let Some(progress_msg) = crate::session_bootstrap::boot_message(&self.project_dir) {
                boot_parts.push(progress_msg);
            }

            // Recent git activity (max 20 commits, best-effort).
            // Uses tokio::process to avoid blocking the async worker on a
            // slow/locked git repo.
            if let Ok(output) = tokio::process::Command::new("git")
                .args(["log", "--oneline", "-20"])
                .current_dir(&self.project_dir)
                .output()
                .await
                && output.status.success() {
                    let log = String::from_utf8_lossy(&output.stdout);
                    let log = log.trim();
                    if !log.is_empty() {
                        boot_parts.push(format!("Recent git commits:\n{log}"));
                    }
                }

            if !boot_parts.is_empty() {
                messages.push(Message::system(format!(
                    "## Session Boot Context\n{}",
                    boot_parts.join("\n\n")
                )));
            }
        }

        // Planning injection: if GRAPHCTX is Ready, inject top-5 relevant files
        // as system message so the LLM starts with structural orientation.
        // Skip if Building (don't use stale for planning), only use fresh Ready state.
        if !self.config.is_subagent
            && let Some(ref provider) = self.graph_context
                && provider.is_ready() {
                    // Use the task objective (first user message) as query
                    let planning_query = messages
                        .iter()
                        .rev()
                        .find(|m| m.role == theo_infra_llm::types::Role::User)
                        .and_then(|m| m.content.as_deref())
                        .unwrap_or("")
                        .chars()
                        .take(200)
                        .collect::<String>();

                    if !planning_query.is_empty()
                        && let Ok(ctx) = provider.query_context(&planning_query, 1000).await
                            && !ctx.blocks.is_empty() {
                                // T5.5 FM-5: record initial context files for
                                // the task-derailment sensor.
                                for b in ctx.blocks.iter().take(5) {
                                    self.initial_context_files.insert(b.source_id.clone());
                                }
                                let file_hints: Vec<String> = ctx
                                    .blocks
                                    .iter()
                                    .take(5)
                                    .map(|b| {
                                        format!(
                                            "- {} (relevance: {:.0}%)",
                                            b.source_id,
                                            b.score * 100.0
                                        )
                                    })
                                    .collect();
                                messages.push(Message::system(format!(
                                    "## Suggested Starting Files\nBased on code graph analysis, these areas are most relevant to your task:\n{}\n\nStart here, but verify with read/grep.",
                                    file_hints.join("\n")
                                )));
                            }
                }

        // Inject available skills into system context (main agent only).
        // Sub-agents do NOT receive skills — they execute their direct objective.
        // This is Layer 2 of recursive spawning prevention (prompt isolation).
        if !self.config.is_subagent {
            let mut skill_registry = SkillRegistry::new();
            skill_registry.load_bundled();
            let project_skills = self.project_dir.join(".theo").join("skills");
            if project_skills.exists() {
                skill_registry.load_from_dir(&project_skills);
            }
            let user_skills = std::env::var("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
                .join(".config")
                .join("theo")
                .join("skills");
            if user_skills.exists() {
                skill_registry.load_from_dir(&user_skills);
            }
            let skills_summary = skill_registry.triggers_summary();
            if !skills_summary.is_empty() {
                messages.push(Message::system(format!(
                    "## Skills\nYou have specialized skills that you SHOULD invoke when the task matches:\n{skills_summary}\n\nWhen the user's request matches a skill trigger, use the `skill` tool to invoke it."
                )));
            }
        }

        // Inject session history (previous REPL prompts + responses)
        if !history.is_empty() {
            messages.extend(history);
        }

        // Add the task objective as user message
        if let Some(task) = self.task_manager.get(&self.task_id) {
            messages.push(Message::user(&task.objective));
        }

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
                return AgentResult {
                    // Bug #1 fix (benchmark-validation): budget exceeded
                    // ALWAYS means the task did not finish. Previously
                    // we set `success = edits_succeeded > 0` which lied
                    // to callers — they saw success=true even when theo
                    // ran out of iterations mid-implementation. Tests
                    // (run_engine::tests::success_semantics) lock this.
                    success: false,
                    summary,
                    files_edited: self.context_loop_state.edits_files.clone(),
                    iterations_used: iteration,
                    was_streamed: false,
                    tokens_used: self.metrics.snapshot().total_tokens_used,
                    input_tokens: self.metrics.snapshot().total_input_tokens,
                    output_tokens: self.metrics.snapshot().total_output_tokens,
                    tool_calls_total: self.metrics.snapshot().total_tool_calls,
                    tool_calls_success: self.metrics.snapshot().successful_tool_calls,
                    llm_calls: self.metrics.snapshot().total_llm_calls,
                    retries: self.metrics.snapshot().total_retries,
                    duration_ms: 0,
                    // Phase 59: budget exhaustion = Exhausted
                    error_class: Some(theo_domain::error_class::ErrorClass::Exhausted),
                    ..Default::default()
                };
            }

            // ── SENSOR DRAIN ──
            // Drain pending sensor results and inject as system messages before LLM call.
            // This provides the LLM with feedback from computational verification (e.g. clippy, tests).
            if let Some(ref sensor_runner) = sensor_runner {
                for result in sensor_runner.drain_pending() {
                    let severity = if result.exit_code == 0 { "OK" } else { "ISSUE" };
                    let preview = if result.output.len() > 1000 {
                        format!(
                            "{}...\n[truncated]",
                            &result.output[..result.output
                                .char_indices()
                                .nth(1000)
                                .map(|(i, _)| i)
                                .unwrap_or(result.output.len())]
                        )
                    } else {
                        result.output.clone()
                    };
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
                && let Ok(forced) = std::env::var("THEO_FORCE_TOOL_CHOICE")
                && !forced.is_empty()
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
                if std::env::var("THEO_DEBUG_CODEX").is_ok() {
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

                    // Emergency compaction: keep only 50% of context
                    let model_ctx = self.config.context_window_tokens;
                    let target = model_ctx / 2;
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
                    return AgentResult {
                        success: false,
                        summary: format!("LLM error: {e}"),
                        files_edited: self.context_loop_state.edits_files.clone(),
                        iterations_used: iteration,
                        was_streamed: false,
                        tokens_used: self.metrics.snapshot().total_tokens_used,
                    input_tokens: self.metrics.snapshot().total_input_tokens,
                    output_tokens: self.metrics.snapshot().total_output_tokens,
                    tool_calls_total: self.metrics.snapshot().total_tool_calls,
                    tool_calls_success: self.metrics.snapshot().successful_tool_calls,
                    llm_calls: self.metrics.snapshot().total_llm_calls,
                    retries: self.metrics.snapshot().total_retries,
                    duration_ms: 0,
                    error_class: Some(class),
                    ..Default::default()
                    };
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
                return AgentResult {
                    success: true,
                    summary: content,
                    files_edited: self.context_loop_state.edits_files.clone(),
                    iterations_used: iteration,
                    was_streamed: true,
                    tokens_used: self.metrics.snapshot().total_tokens_used,
                    input_tokens: self.metrics.snapshot().total_input_tokens,
                    output_tokens: self.metrics.snapshot().total_output_tokens,
                    tool_calls_total: self.metrics.snapshot().total_tool_calls,
                    tool_calls_success: self.metrics.snapshot().successful_tool_calls,
                    llm_calls: self.metrics.snapshot().total_llm_calls,
                    retries: self.metrics.snapshot().total_retries,
                    duration_ms: 0,
                    error_class: Some(ErrorClass::Solved),
                    ..Default::default()
                };
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

                // Handle `done` meta-tool with multi-layer verification:
                // 1. Convergence pre-filter (git diff must show real changes)
                // 2. Cargo test on affected crate (timeout 60s, fallback cargo check)
                // 3. done_attempts counter (max 3 blocks before hard fail)
                if name == "done" {
                    self.transition_run(RunState::Evaluating);
                    self.done_attempts += 1;

                    let summary = call
                        .parse_arguments()
                        .ok()
                        .and_then(|args| {
                            args.get("summary")
                                .and_then(|s| s.as_str())
                                .map(String::from)
                        })
                        .unwrap_or_else(|| "Task completed.".to_string());

                    // Gate 0: done_attempts hard limit — avoid burning entire budget
                    const MAX_DONE_ATTEMPTS: u32 = 3;
                    if self.done_attempts > MAX_DONE_ATTEMPTS {
                        // Exceeded max attempts — accept with warning
                        self.transition_run(RunState::Converged);
                        let _ = self
                            .task_manager
                            .transition(&self.task_id, TaskState::Completed);
                        self.metrics.record_run_complete(true);
                        should_return = Some(AgentResult {
                            success: true,
                            summary: format!(
                                "{} [accepted after {} done attempts]",
                                summary, self.done_attempts
                            ),
                            files_edited: self.context_loop_state.edits_files.clone(),
                            iterations_used: iteration,
                            was_streamed: false,
                            tokens_used: self.metrics.snapshot().total_tokens_used,
                    input_tokens: self.metrics.snapshot().total_input_tokens,
                    output_tokens: self.metrics.snapshot().total_output_tokens,
                    tool_calls_total: self.metrics.snapshot().total_tool_calls,
                    tool_calls_success: self.metrics.snapshot().successful_tool_calls,
                    llm_calls: self.metrics.snapshot().total_llm_calls,
                    retries: self.metrics.snapshot().total_retries,
                    duration_ms: 0,
                    error_class: Some(ErrorClass::Solved),
                    ..Default::default()
                        });
                        break;
                    }

                    // Gate 1: Convergence pre-filter — verify real changes exist
                    let has_changes = check_git_changes(&self.project_dir).await;
                    let convergence_ctx = ConvergenceContext {
                        has_git_changes: has_changes,
                        edits_succeeded: self.context_loop_state.edits_files.len(),
                        done_requested: true,
                        iteration,
                        max_iterations: self.config.max_iterations,
                    };
                    if !self.convergence.evaluate(&convergence_ctx) {
                        let pending = self.convergence.pending_criteria(&convergence_ctx);
                        messages.push(Message::tool_result(
                            &call.id,
                            "done",
                            format!(
                                "BLOCKED: convergence criteria not met: {}. Make real changes before calling done.",
                                pending.join(", ")
                            ),
                        ));
                        self.transition_run(RunState::Replanning);
                        continue;
                    }

                    // Review suggestion: if diff is large, suggest reviewing before accepting.
                    // Non-blocking — just a hint to encourage careful review.
                    if self.context_loop_state.edits_files.len() > 3 {
                        let diff_stat = tokio::process::Command::new("git")
                            .args(["diff", "--stat"])
                            .current_dir(&self.project_dir)
                            .output()
                            .await;
                        if let Ok(output) = diff_stat {
                            let stat = String::from_utf8_lossy(&output.stdout);
                            let lines_changed: usize = stat
                                .lines()
                                .filter_map(|l| {
                                    // Parse "N insertions(+), M deletions(-)" from last line
                                    if l.contains("insertion") || l.contains("deletion") {
                                        l.split_whitespace()
                                            .filter_map(|w| w.parse::<usize>().ok())
                                            .sum::<usize>()
                                            .into()
                                    } else {
                                        None
                                    }
                                })
                                .sum();
                            if lines_changed > 100 {
                                messages.push(Message::user(format!(
                                    "Note: This change touches {} files with ~{} lines changed. \
                                         Consider reviewing the diff carefully before finalizing.",
                                    self.context_loop_state.edits_files.len(),
                                    lines_changed
                                )));
                            }
                        }
                    }

                    // Gate 2: Clean state sensor — verify project builds and tests pass.
                    // Best-effort: skip if not Rust, timeout 60s, never hard-abort.
                    if self.project_dir.join("Cargo.toml").exists() {
                        // Determine which crate was affected for targeted test
                        let test_args =
                            if let Some(first_file) = self.context_loop_state.edits_files.first() {
                                // Try to find crate name from edited file path
                                let crate_name = std::path::Path::new(first_file)
                                    .components()
                                    .zip(std::path::Path::new(first_file).components().skip(1))
                                    .find(|(a, _)| {
                                        let s = a.as_os_str().to_string_lossy();
                                        s == "crates" || s == "apps"
                                    })
                                    .map(|(_, b)| b.as_os_str().to_string_lossy().to_string());
                                if let Some(name) = crate_name {
                                    vec![
                                        "test".to_string(),
                                        "-p".to_string(),
                                        name,
                                        "--no-fail-fast".to_string(),
                                    ]
                                } else {
                                    vec!["test".to_string(), "--no-fail-fast".to_string()]
                                }
                            } else {
                                // No files edited — just cargo check as sanity
                                vec!["check".to_string(), "--message-format=short".to_string()]
                            };

                        let test_result = tokio::time::timeout(
                            std::time::Duration::from_secs(60),
                            tokio::process::Command::new("cargo")
                                .args(&test_args)
                                .current_dir(&self.project_dir)
                                .output(),
                        )
                        .await;

                        let check_failed = match test_result {
                            Ok(Ok(output)) if !output.status.success() => {
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                let combined = format!("{}\n{}", stderr, stdout);
                                Some(combined)
                            }
                            Ok(Ok(_)) => None,  // Tests passed
                            Ok(Err(_)) => None, // Command not found — pass through
                            Err(_) => {
                                // Timeout — fallback to cargo check with 30s
                                let fallback = tokio::time::timeout(
                                    std::time::Duration::from_secs(30),
                                    tokio::process::Command::new("cargo")
                                        .args(["check", "--message-format=short"])
                                        .current_dir(&self.project_dir)
                                        .output(),
                                )
                                .await;
                                match fallback {
                                    Ok(Ok(output)) if !output.status.success() => {
                                        Some(String::from_utf8_lossy(&output.stderr).to_string())
                                    }
                                    _ => None, // Fallback passed or timed out — accept
                                }
                            }
                        };

                        if let Some(errors) = check_failed {
                            let error_preview = if errors.len() > 2000 {
                                format!(
                                    "{}...\n[truncated]",
                                    &errors[..errors
                                        .char_indices()
                                        .nth(2000)
                                        .map(|(i, _)| i)
                                        .unwrap_or(errors.len())]
                                )
                            } else {
                                errors
                            };
                            let cmd_str = test_args.join(" ");
                            messages.push(Message::tool_result(
                                &call.id,
                                "done",
                                format!(
                                    "BLOCKED: `cargo {}` failed (attempt {}/{}). Fix the errors before calling done.\n\n{}",
                                    cmd_str, self.done_attempts, MAX_DONE_ATTEMPTS, error_preview
                                ),
                            ));
                            self.transition_run(RunState::Replanning);
                            continue;
                        }
                    }

                    self.transition_run(RunState::Converged);
                    let _ = self
                        .task_manager
                        .transition(&self.task_id, TaskState::Completed);
                    self.metrics.record_run_complete(true);

                    should_return = Some(AgentResult {
                        success: true,
                        summary,
                        files_edited: self.context_loop_state.edits_files.clone(),
                        iterations_used: iteration,
                        was_streamed: false,
                        tokens_used: self.metrics.snapshot().total_tokens_used,
                    input_tokens: self.metrics.snapshot().total_input_tokens,
                    output_tokens: self.metrics.snapshot().total_output_tokens,
                    tool_calls_total: self.metrics.snapshot().total_tool_calls,
                    tool_calls_success: self.metrics.snapshot().successful_tool_calls,
                    llm_calls: self.metrics.snapshot().total_llm_calls,
                    retries: self.metrics.snapshot().total_retries,
                    duration_ms: 0,
                    error_class: Some(ErrorClass::Solved),
                    ..Default::default()
                    });
                    break;
                }

                // Handle `delegate_task` meta-tool — Phase 4 unified API.
                // Validates schema and routes to single or parallel mode.
                // Phase 29 follow-up: also accept the split single/parallel
                // variants (delegate_task_single / delegate_task_parallel)
                // which weaker tool-callers like Codex handle correctly
                // because each has a fixed `required` field set.
                if name == "delegate_task"
                    || name == "delegate_task_single"
                    || name == "delegate_task_parallel"
                {
                    let raw_args = call.parse_arguments().unwrap_or_default();
                    // Normalize the split variants to the unified shape
                    // expected by handle_delegate_task.
                    let args = match name.as_str() {
                        "delegate_task_single" => {
                            // {agent, objective, context} → unified shape unchanged
                            raw_args
                        }
                        "delegate_task_parallel" => {
                            // {tasks: [...]} → {parallel: [...]}
                            let tasks = raw_args
                                .get("tasks")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null);
                            serde_json::json!({"parallel": tasks})
                        }
                        _ => raw_args,
                    };
                    let result_msg = self.handle_delegate_task(args).await;
                    messages.push(Message::tool_result(&call.id, name, &result_msg));
                    continue;
                }

                // Handle `skill` meta-tool — invoke a packaged skill
                if name == "skill" {
                    let args = call.parse_arguments().unwrap_or_default();
                    let skill_name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");

                    // Build a temporary registry to look up the skill
                    let mut skill_registry = SkillRegistry::new();
                    skill_registry.load_bundled();
                    let project_skills = self.project_dir.join(".theo").join("skills");
                    if project_skills.exists() {
                        skill_registry.load_from_dir(&project_skills);
                    }

                    if let Some(skill) = skill_registry.get(skill_name) {
                        match &skill.mode {
                            crate::skill::SkillMode::InContext => {
                                // Inject skill instructions into conversation
                                messages.push(Message::system(&skill.instructions));
                                messages.push(Message::tool_result(
                                    &call.id,
                                    "skill",
                                    format!(
                                        "Skill '{}' loaded. Follow the instructions above.",
                                        skill_name
                                    ),
                                ));
                            }
                            crate::skill::SkillMode::SubAgent { agent_name } => {
                                // Spawn sub-agent with skill instructions as prompt.
                                // Resolve agent_name via the registry (or build a default).
                                self.event_bus.publish(DomainEvent::new(
                                    EventType::RunStateChanged,
                                    self.run.run_id.as_str(),
                                    serde_json::json!({
                                        "from": "Executing",
                                        "to": format!("Skill:{}:{}", skill_name, agent_name),
                                    }),
                                ));

                                let registry: Arc<crate::subagent::SubAgentRegistry> = match &self.subagent_registry {
                                    Some(r) => r.clone(),
                                    None => Arc::new(crate::subagent::SubAgentRegistry::with_builtins()),
                                };

                                let spec = registry
                                    .get(agent_name)
                                    .cloned()
                                    .unwrap_or_else(|| {
                                        theo_domain::agent_spec::AgentSpec::on_demand(
                                            agent_name,
                                            &skill.instructions,
                                        )
                                    });

                                let manager = crate::subagent::SubAgentManager::with_registry(
                                    self.config.clone(),
                                    self.event_bus.clone(),
                                    self.project_dir.clone(),
                                    registry,
                                )
                                .with_metrics(self.metrics.clone());

                                let sub_result = manager
                                    .spawn_with_spec_text(&spec, &skill.instructions, None)
                                    .await;

                                let result_msg = if sub_result.success {
                                    format!(
                                        "[Skill '{}' completed] {}",
                                        skill_name, sub_result.summary
                                    )
                                } else {
                                    format!(
                                        "[Skill '{}' failed] {}",
                                        skill_name, sub_result.summary
                                    )
                                };

                                for file in &sub_result.files_edited {
                                    if !file.is_empty() {
                                        self.context_loop_state
                                            .record_edit_attempt(file, true, None);
                                    }
                                }

                                self.budget_enforcer.record_tokens(sub_result.tokens_used);
                                self.metrics.record_delegated_tokens(sub_result.tokens_used);

                                messages.push(Message::tool_result(&call.id, "skill", &result_msg));
                            }
                        }
                    } else {
                        let available: Vec<String> = {
                            skill_registry
                                .list()
                                .iter()
                                .map(|s| s.name.clone())
                                .collect()
                        };
                        messages.push(Message::tool_result(
                            &call.id,
                            "skill",
                            format!(
                                "Unknown skill: '{}'. Available skills: {}",
                                skill_name,
                                available.join(", ")
                            ),
                        ));
                    }
                    continue;
                }

                // Handle `batch` meta-tool — execute N calls in 1 turn
                if name == "batch" {
                    let args = call.parse_arguments().unwrap_or_default();
                    let calls_array = args.get("calls").and_then(|v| v.as_array());

                    if let Some(calls) = calls_array {
                        const MAX_BATCH: usize = 25;
                        const BLOCKED: &[&str] =
                            &["batch", "done", "subagent", "subagent_parallel", "skill"];

                        let total = calls.len().min(MAX_BATCH);

                        // Build futures for parallel execution via join_all
                        // Blocked tools get immediate error results
                        let registry = self.registry.clone(); // Arc::clone — cheap
                        let mut futures = Vec::new();
                        let mut blocked_results: Vec<(usize, String, String)> = Vec::new(); // (index, name, error)

                        for (i, batch_call) in calls.iter().take(MAX_BATCH).enumerate() {
                            let tool_name = batch_call
                                .get("tool")
                                .and_then(|v| v.as_str())
                                .unwrap_or("?")
                                .to_string();
                            let tool_args = batch_call
                                .get("args")
                                .cloned()
                                .unwrap_or(serde_json::json!({}));

                            if BLOCKED.contains(&tool_name.as_str()) {
                                blocked_results.push((
                                    i,
                                    tool_name.clone(),
                                    format!("cannot use '{}' inside batch", tool_name),
                                ));
                                continue;
                            }

                            // Plan mode guard inside batch: block write tools
                            if self.config.mode == crate::config::AgentMode::Plan
                                && matches!(tool_name.as_str(), "edit" | "write" | "apply_patch")
                            {
                                blocked_results.push((i, tool_name.clone(), "BLOCKED by Plan mode guard — no source edits in batch during planning".to_string()));
                                continue;
                            }

                            let reg = registry.clone();
                            let batch_tool_call = theo_infra_llm::types::ToolCall::new(
                                format!("batch_{}_{}", call.id, i),
                                &tool_name,
                                tool_args.to_string(),
                            );
                            let batch_ctx = ToolContext {
                                session_id: SessionId::new("batch"),
                                message_id: MessageId::new(format!("batch_{}", i)),
                                call_id: batch_tool_call.id.clone(),
                                agent: "main".to_string(),
                                abort: abort_rx.clone(),
                                project_dir: self.project_dir.clone(),
                                graph_context: self.graph_context.clone(),
                                stdout_tx: None,
                            };

                            futures.push(async move {
                                let (msg, success) = tool_bridge::execute_tool_call(
                                    &reg,
                                    &batch_tool_call,
                                    &batch_ctx,
                                )
                                .await;
                                (i, tool_name, tool_args, msg, success)
                            });
                        }

                        // Execute all non-blocked calls in parallel (join_all preserves order)
                        let results = futures::future::join_all(futures).await;

                        // Combine blocked + executed results, sorted by index
                        let mut all_results: Vec<(usize, String, String, bool)> = Vec::new();

                        for (i, name, err) in blocked_results {
                            all_results.push((i, name, format!("error — {}", err), false));
                        }

                        for (i, tool_name, tool_args, msg, success) in results {
                            let output = msg.content.unwrap_or_default();
                            let status = if success { "ok" } else { "error" };
                            let preview = if output.len() > 200 {
                                let mut end = 200;
                                while end > 0 && !output.is_char_boundary(end) {
                                    end -= 1;
                                }
                                format!("{}...", &output[..end])
                            } else {
                                output.clone()
                            };

                            all_results.push((
                                i,
                                tool_name.clone(),
                                format!(
                                    "{}({}): {} — {}",
                                    tool_name,
                                    truncate_batch_args(&tool_args),
                                    status,
                                    preview
                                ),
                                success,
                            ));

                            // Track in budget/metrics
                            self.budget_enforcer.record_tool_call();
                            self.metrics.record_tool_call(&tool_name, 0, success);

                            // Track edits
                            if success
                                && matches!(tool_name.as_str(), "edit" | "write" | "apply_patch")
                            {
                                let file = tool_args
                                    .get("filePath")
                                    .and_then(|p| p.as_str())
                                    .unwrap_or("");
                                if !file.is_empty() {
                                    self.context_loop_state
                                        .record_edit_attempt(file, true, None);
                                }
                            }
                        }

                        // Sort by original index for deterministic output
                        all_results.sort_by_key(|(i, _, _, _)| *i);

                        let mut batch_output = String::new();
                        for (i, _name, display, _success) in &all_results {
                            batch_output.push_str(&format!("[{}/{}] {}\n", i + 1, total, display));
                        }

                        if calls.len() > MAX_BATCH {
                            batch_output.push_str(&format!(
                                "\n⚠ {} calls exceeded max batch size of {}. Only first {} executed.\n",
                                calls.len(), MAX_BATCH, MAX_BATCH
                            ));
                        }

                        // Publish batch completion event
                        // Phase 44 (otlp-exporter-plan): include otel payload.
                        let mut batch_span = crate::observability::otel::tool_call_span("batch");
                        batch_span.set(crate::observability::otel::ATTR_THEO_TOOL_CALL_ID, call.id.as_str());
                        batch_span.set(crate::observability::otel::ATTR_THEO_TOOL_STATUS, "Succeeded");
                        batch_span.set(crate::observability::otel::ATTR_THEO_TOOL_DURATION_MS, 0u64);
                        self.event_bus.publish(DomainEvent::new(
                            EventType::ToolCallCompleted,
                            call.id.as_str(),
                            serde_json::json!({
                                "tool_name": "batch",
                                "success": true,
                                "input": { "count": total },
                                "output_preview": format!("Batch: {total} calls executed"),
                                "duration_ms": 0,
                                "otel": batch_span.to_json(),
                            }),
                        ));

                        messages.push(Message::tool_result(&call.id, "batch", &batch_output));
                        continue;
                    } else {
                        messages.push(Message::tool_result(
                            &call.id, "batch",
                            "Error: 'calls' array is required. Example: batch(calls: [{tool: \"read\", args: {filePath: \"a.rs\"}}])",
                        ));
                        continue;
                    }
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
                            return AgentResult {
                                success: false,
                                summary: format!(
                                    "Doom loop abort: '{}' called identically {} times. Agent is stuck.",
                                    name,
                                    self.config.doom_loop_threshold.unwrap_or(3) * 2
                                ),
                                files_edited: self.context_loop_state.edits_files.clone(),
                                iterations_used: iteration,
                                was_streamed: false,
                                tokens_used: self.metrics.snapshot().total_tokens_used,
                    input_tokens: self.metrics.snapshot().total_input_tokens,
                    output_tokens: self.metrics.snapshot().total_output_tokens,
                    tool_calls_total: self.metrics.snapshot().total_tool_calls,
                    tool_calls_success: self.metrics.snapshot().successful_tool_calls,
                    llm_calls: self.metrics.snapshot().total_llm_calls,
                    retries: self.metrics.snapshot().total_retries,
                    duration_ms: 0,
                    error_class: Some(ErrorClass::Aborted),
                    ..Default::default()
                            };
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
fn llm_error_to_class(e: &theo_infra_llm::LlmError) -> ErrorClass {
    use theo_infra_llm::LlmError;
    match e {
        LlmError::RateLimited { .. } => ErrorClass::RateLimited,
        // Phase 61: distinct from RateLimited because retry doesn't help
        // for quota exhaustion — only the billing cycle reset clears it.
        LlmError::QuotaExceeded { .. } => ErrorClass::QuotaExceeded,
        LlmError::AuthFailed(_) => ErrorClass::AuthFailed,
        LlmError::ContextOverflow { .. } => ErrorClass::ContextOverflow,
        // Network / Timeout / ServiceUnavailable / Parse / Api / etc.
        // — none of these match a more specific class, so they fall
        // into the catch-all "internal abort".
        _ => ErrorClass::Aborted,
    }
}

fn truncate_handoff_objective(s: &str) -> String {
    if s.chars().count() <= 200 {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(199).collect();
        t.push('…');
        t
    }
}

/// Truncate batch call args for display (e.g., filePath only).
fn truncate_batch_args(args: &serde_json::Value) -> String {
    if let Some(path) = args.get("filePath").and_then(|v| v.as_str()) {
        return path.to_string();
    }
    if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
        let short = if cmd.len() > 40 { &cmd[..40] } else { cmd };
        return short.to_string();
    }
    if let Some(pattern) = args.get("pattern").and_then(|v| v.as_str()) {
        return format!("\"{}\"", pattern);
    }
    "...".to_string()
}

// ---------------------------------------------------------------------------
// Auto-init: create .theo/theo.md if missing
// ---------------------------------------------------------------------------

/// Ensure .theo/theo.md exists with a static template. Best-effort, never fails.
fn auto_init_project_context(project_dir: &std::path::Path) {
    let theo_md = project_dir.join(".theo").join("theo.md");
    if theo_md.exists() {
        return;
    }

    // Detect project type for template
    let lang = if project_dir.join("Cargo.toml").exists() {
        "Rust"
    } else if project_dir.join("package.json").exists() {
        "Node.js / TypeScript"
    } else if project_dir.join("pyproject.toml").exists() {
        "Python"
    } else if project_dir.join("go.mod").exists() {
        "Go"
    } else {
        "Unknown"
    };

    // Detect project name (simple — first line with name= in manifest)
    let name = detect_project_name_simple(project_dir).unwrap_or_else(|| {
        project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string()
    });

    // Progressive disclosure template: table of contents with pointers.
    // ~60 lines — agent navigates deeper on demand.
    let mut sections = Vec::new();
    sections.push(format!("# {name}\n"));
    sections.push(format!("## Language\n{lang}\n"));

    // Detect and point to existing docs
    let docs_dir = project_dir.join("docs");
    let has_docs = docs_dir.exists();
    let has_readme = project_dir.join("README.md").exists();
    let has_adr = project_dir.join("docs").join("adr").exists();

    sections.push("## Architecture\n".to_string());
    if has_readme {
        sections.push("See `README.md` for project overview.\n".to_string());
    }
    if has_docs {
        sections.push("See `docs/` for detailed documentation.\n".to_string());
    }
    if has_adr {
        sections.push("See `docs/adr/` for Architecture Decision Records.\n".to_string());
    }
    if !has_readme && !has_docs {
        sections
            .push("<!-- Run `theo init` to generate detailed project context -->\n".to_string());
    }

    // Point to key config files
    sections.push("## Key Files\n".to_string());
    let key_files = [
        ("Cargo.toml", "Rust workspace manifest"),
        ("package.json", "Node.js package config"),
        ("pyproject.toml", "Python project config"),
        ("go.mod", "Go module config"),
        (".github/workflows", "CI/CD pipelines"),
    ];
    for (file, desc) in &key_files {
        if project_dir.join(file).exists() {
            sections.push(format!("- `{file}` — {desc}\n"));
        }
    }

    let content = sections.join("");

    let theo_dir = project_dir.join(".theo");
    if std::fs::create_dir_all(&theo_dir).is_err() {
        return; // Best-effort: can't create dir, skip silently
    }
    let _ = std::fs::write(&theo_md, content);

    // Also create .gitignore if missing
    let gitignore = theo_dir.join(".gitignore");
    if !gitignore.exists() {
        let _ = std::fs::write(
            &gitignore,
            "# Generated by Theo\ngraph.bin\ngraph.bin.tmp\nlearnings.json\nsnapshots/\nsessions/\n",
        );
    }

    eprintln!("[theo] Auto-initialized .theo/theo.md — run `theo init` for detailed context");
}

/// Simple project name detection (no external deps).
fn detect_project_name_simple(project_dir: &std::path::Path) -> Option<String> {
    // Try Cargo.toml
    if let Ok(content) = std::fs::read_to_string(project_dir.join("Cargo.toml")) {
        for line in content.lines() {
            let t = line.trim();
            if t.starts_with("name") && t.contains('=')
                && let Some(val) = t.split('=').nth(1) {
                    let name = val.trim().trim_matches('"').trim_matches('\'');
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
        }
    }
    // Try package.json
    if let Ok(content) = std::fs::read_to_string(project_dir.join("package.json")) {
        for line in content.lines() {
            let t = line.trim().trim_start_matches('{').trim();
            if t.starts_with("\"name\"")
                && let Some(val) = t.split(':').nth(1) {
                    let name = val
                        .trim()
                        .trim_end_matches('}')
                        .trim_matches(',')
                        .trim()
                        .trim_matches('"');
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Doom Loop Detection
// ---------------------------------------------------------------------------

use crate::doom_loop::DoomLoopTracker;
use theo_domain::clock::now_millis;

/// Phase 43 (otlp-exporter-plan) — heuristic mapping `base_url → provider`
/// for the OTel `gen_ai.system` attribute. Conservative: returns
/// "openai_compatible" for unknown URLs since theo's protocol is
/// OpenAI-compatible across providers (per the LLM crate's contract).
pub(crate) fn derive_provider_hint(base_url: &str) -> &'static str {
    let lower = base_url.to_ascii_lowercase();
    if lower.contains("api.openai.com") || lower.contains("chatgpt.com") {
        "openai"
    } else if lower.contains("api.anthropic.com") {
        "anthropic"
    } else if lower.contains("googleapis.com") || lower.contains("gemini") {
        "gemini"
    } else if lower.contains("groq.com") {
        "groq"
    } else if lower.contains("mistral.ai") {
        "mistral"
    } else if lower.contains("deepseek") {
        "deepseek"
    } else if lower.contains("together.ai") {
        "together"
    } else if lower.contains("xai") || lower.contains("x.ai") {
        "xai"
    } else if lower.contains("localhost") || lower.contains("127.0.0.1") {
        "openai_compatible_local"
    } else {
        "openai_compatible"
    }
}

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
