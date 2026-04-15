use std::path::PathBuf;
use std::sync::Arc;

use theo_domain::agent_run::{AgentRun, RunState};
use theo_domain::budget::Budget;
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
        }
    }

    /// Sets the message queues for steering and follow-up injection.
    pub fn with_message_queues(mut self, queues: MessageQueues) -> Self {
        self.message_queues = queues;
        self
    }

    /// Sets the graph context provider for code intelligence injection.
    pub fn with_graph_context(
        mut self,
        provider: Arc<dyn theo_domain::graph_context::GraphContextProvider>,
    ) -> Self {
        self.graph_context = Some(provider);
        self
    }

    /// Returns the run_id.
    pub fn run_id(&self) -> &RunId {
        &self.run.run_id
    }

    /// Returns the current RunState.
    pub fn state(&self) -> RunState {
        self.run.state
    }

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
        self.record_session_exit(&result);
        result
    }

    /// Record session exit: save failure patterns + session progress + context metrics.
    /// Best-effort — never fails, never blocks.
    fn record_session_exit(&mut self, result: &AgentResult) {
        // Save failure pattern tracker
        self.failure_tracker.save();

        // Save context metrics to .theo/metrics/{run_id}.json
        let metrics_dir = self.project_dir.join(".theo").join("metrics");
        if std::fs::create_dir_all(&metrics_dir).is_ok() {
            let report = self.context_metrics.to_report();
            let metrics_path = metrics_dir.join(format!("{}.json", self.run.run_id.as_str()));
            let _ = std::fs::write(
                &metrics_path,
                serde_json::to_string_pretty(&report).unwrap_or_default(),
            );
        }

        // Generate EpisodeSummary from run events and persist to .theo/wiki/episodes/
        let events = self.event_bus.events();
        if !events.is_empty() {
            let task_objective = self
                .task_manager
                .get(&self.task_id)
                .map(|t| t.objective.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let summary = theo_domain::episode::EpisodeSummary::from_events(
                self.run.run_id.as_str(),
                Some(self.task_id.as_str()),
                &task_objective,
                &events,
            );
            let episodes_dir = self.project_dir.join(".theo").join("wiki").join("episodes");
            if std::fs::create_dir_all(&episodes_dir).is_ok() {
                let episode_path = episodes_dir.join(format!("{}.json", summary.summary_id));
                let _ = std::fs::write(
                    &episode_path,
                    serde_json::to_string_pretty(&summary).unwrap_or_default(),
                );
            }
        }

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

        // System prompt: .theo/system-prompt.md replaces default, or use config default
        let system_prompt = if !self.config.is_subagent {
            crate::project_config::load_system_prompt(&self.project_dir)
                .unwrap_or_else(|| self.config.system_prompt.clone())
        } else {
            self.config.system_prompt.clone()
        };

        let mut messages: Vec<Message> = vec![Message::system(&system_prompt)];

        // Project context: .theo/theo.md prepended as separate system message
        if !self.config.is_subagent {
            if let Some(context) = crate::project_config::load_project_context(&self.project_dir) {
                messages.push(Message::system(&format!("## Project Context\n{context}")));
            }
        }

        // GRAPHCTX is available as the `codebase_context` tool — the LLM calls it on-demand.
        // No automatic injection: the LLM decides when it needs code structure context.
        // The graph_context provider is passed to tools via ToolContext.graph_context.

        // Inject memories from previous runs (cross-run memory)
        let memory_root = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
            .join(".config")
            .join("theo")
            .join("memory");

        let memory_store =
            theo_tooling::memory::FileMemoryStore::for_project(&memory_root, &self.project_dir);
        if let Ok(memories) = memory_store.list().await {
            if !memories.is_empty() {
                let memory_context = memories
                    .iter()
                    .map(|m| format!("- **{}**: {}", m.key, m.value))
                    .collect::<Vec<_>>()
                    .join("\n");
                messages.push(Message::system(&format!(
                    "## Memory from previous runs\n{memory_context}"
                )));
            }
        }

        // Boot sequence: inject progress from previous sessions + recent git activity.
        // Inserted after memories, before skills — so the agent knows where it left off.
        if !self.config.is_subagent {
            let mut boot_parts: Vec<String> = Vec::new();

            // Previous session progress
            if let Some(progress_msg) = crate::session_bootstrap::boot_message(&self.project_dir) {
                boot_parts.push(progress_msg);
            }

            // Recent git activity (max 20 commits, best-effort)
            if let Ok(output) = std::process::Command::new("git")
                .args(["log", "--oneline", "-20"])
                .current_dir(&self.project_dir)
                .output()
            {
                if output.status.success() {
                    let log = String::from_utf8_lossy(&output.stdout);
                    let log = log.trim();
                    if !log.is_empty() {
                        boot_parts.push(format!("Recent git commits:\n{log}"));
                    }
                }
            }

            if !boot_parts.is_empty() {
                messages.push(Message::system(&format!(
                    "## Session Boot Context\n{}",
                    boot_parts.join("\n\n")
                )));
            }
        }

        // Planning injection: if GRAPHCTX is Ready, inject top-5 relevant files
        // as system message so the LLM starts with structural orientation.
        // Skip if Building (don't use stale for planning), only use fresh Ready state.
        if !self.config.is_subagent {
            if let Some(ref provider) = self.graph_context {
                if provider.is_ready() {
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

                    if !planning_query.is_empty() {
                        if let Ok(ctx) = provider.query_context(&planning_query, 1000).await {
                            if !ctx.blocks.is_empty() {
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
                                messages.push(Message::system(&format!(
                                    "## Suggested Starting Files\nBased on code graph analysis, these areas are most relevant to your task:\n{}\n\nStart here, but verify with read/grep.",
                                    file_hints.join("\n")
                                )));
                            }
                        }
                    }
                }
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
                messages.push(Message::system(&format!(
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

        // Initialize hook runner for pre/post tool hooks
        let hook_runner = if !self.config.is_subagent {
            Some(crate::hooks::HookRunner::new(
                &self.project_dir,
                crate::hooks::HookConfig::default(),
            ))
        } else {
            None // Sub-agents don't run hooks
        };

        // Doom loop detector — tracks recent tool calls to detect repetition
        let mut doom_tracker = self
            .config
            .doom_loop_threshold
            .map(|t| DoomLoopTracker::new(t));

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
                    success: self.context_loop_state.edits_succeeded > 0,
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
                };
            }

            // ── PLANNING phase ──
            // Context loop injection
            if iteration > 1 && iteration % self.config.context_loop_interval == 0 {
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
            crate::compaction::compact_if_needed_with_context(
                &mut messages,
                self.config.context_window_tokens,
                Some(&compaction_ctx),
            );

            // Record context size for metrics (estimated tokens = chars/4)
            let estimated_context_tokens: usize = messages
                .iter()
                .filter_map(|m| m.content.as_ref())
                .map(|c| (c.len() + 3) / 4)
                .sum();
            self.context_metrics
                .record_context_size(iteration, estimated_context_tokens);

            // LLM call
            self.transition_run(RunState::Planning);

            let mut request = ChatRequest::new(&self.config.model, messages.clone())
                .with_tools(tool_defs.clone())
                .with_max_tokens(self.config.max_tokens)
                .with_temperature(self.config.temperature);

            if let Some(ref effort) = self.config.reasoning_effort {
                request = request.with_reasoning_effort(effort);
            }

            // Publish LLM call start (triggers "Thinking..." in CLI)
            self.event_bus.publish(DomainEvent::new(
                EventType::LlmCallStart,
                self.run.run_id.as_str(),
                serde_json::json!({"iteration": iteration}),
            ));

            let llm_start = std::time::Instant::now();
            let event_bus_for_stream = self.event_bus.clone();
            let run_id_for_stream = self.run.run_id.as_str().to_string();

            // LLM call with retry for retryable errors (429, 503, 504, network)
            let retry_policy = theo_domain::retry_policy::RetryPolicy::default_llm();
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

            let response = match llm_result.unwrap() {
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
                    resp
                }
                Err(e) if e.is_context_overflow() => {
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

            let mut should_return = None;

            for call in tool_calls {
                let name = &call.function.name;

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
                            &format!(
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
                                messages.push(Message::user(&format!(
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
                                &format!(
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
                    });
                    break;
                }

                // Handle `subagent` meta-tool — delegate to sub-agent
                if name == "subagent" {
                    let args = call.parse_arguments().unwrap_or_default();
                    let role_str = args
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("explorer");
                    let objective = args
                        .get("objective")
                        .and_then(|v| v.as_str())
                        .unwrap_or("No objective provided");

                    let role = crate::subagent::SubAgentRole::from_str(role_str)
                        .unwrap_or(crate::subagent::SubAgentRole::Explorer);

                    // Publish spawn event
                    self.event_bus.publish(DomainEvent::new(
                        EventType::RunStateChanged,
                        self.run.run_id.as_str(),
                        serde_json::json!({
                            "from": "Executing",
                            "to": format!("SubAgent:{}", role.display_name()),
                        }),
                    ));

                    let manager = crate::subagent::SubAgentManager::new(
                        self.config.clone(),
                        self.event_bus.clone(),
                        self.project_dir.clone(),
                    );

                    let sub_result = manager.spawn(role, objective, None).await;

                    let result_msg = if sub_result.success {
                        format!(
                            "[{} sub-agent completed] {}",
                            role.display_name(),
                            sub_result.summary
                        )
                    } else {
                        format!(
                            "[{} sub-agent failed] {}",
                            role.display_name(),
                            sub_result.summary
                        )
                    };

                    // Record sub-agent files as parent's edits
                    for file in &sub_result.files_edited {
                        if !file.is_empty() {
                            self.context_loop_state
                                .record_edit_attempt(file, true, None);
                        }
                    }

                    // Aggregate sub-agent tokens into parent budget + metrics
                    self.budget_enforcer.record_tokens(sub_result.tokens_used);
                    self.metrics.record_delegated_tokens(sub_result.tokens_used);

                    messages.push(Message::tool_result(&call.id, "subagent", &result_msg));
                    continue;
                }

                // Handle `subagent_parallel` — run multiple sub-agents concurrently
                if name == "subagent_parallel" {
                    let args = call.parse_arguments().unwrap_or_default();
                    let agents_array = args.get("agents").and_then(|v| v.as_array());

                    if let Some(agents) = agents_array {
                        let tasks: Vec<(crate::subagent::SubAgentRole, String)> = agents
                            .iter()
                            .filter_map(|a| {
                                let role_str = a.get("role").and_then(|v| v.as_str())?;
                                let objective = a.get("objective").and_then(|v| v.as_str())?;
                                let role = crate::subagent::SubAgentRole::from_str(role_str)?;
                                Some((role, objective.to_string()))
                            })
                            .collect();

                        let count = tasks.len();

                        // Publish parallel spawn event
                        self.event_bus.publish(DomainEvent::new(
                            EventType::RunStateChanged,
                            self.run.run_id.as_str(),
                            serde_json::json!({
                                "from": "Executing",
                                "to": format!("SubAgentParallel:{}", count),
                            }),
                        ));

                        let manager = crate::subagent::SubAgentManager::new(
                            self.config.clone(),
                            self.event_bus.clone(),
                            self.project_dir.clone(),
                        );

                        let results = manager.spawn_parallel(tasks).await;

                        // Combine results
                        let mut combined = String::new();
                        for (i, result) in results.iter().enumerate() {
                            combined.push_str(&format!(
                                "[Sub-agent {}] {}: {}\n",
                                i + 1,
                                if result.success { "✅" } else { "❌" },
                                result.summary,
                            ));
                            for file in &result.files_edited {
                                if !file.is_empty() {
                                    self.context_loop_state
                                        .record_edit_attempt(file, true, None);
                                }
                            }
                            // Aggregate parallel sub-agent tokens into parent budget + metrics
                            self.budget_enforcer.record_tokens(result.tokens_used);
                            self.metrics.record_delegated_tokens(result.tokens_used);
                        }

                        messages.push(Message::tool_result(
                            &call.id,
                            "subagent_parallel",
                            &combined,
                        ));
                        continue;
                    } else {
                        messages.push(Message::tool_result(
                            &call.id,
                            "subagent_parallel",
                            "Error: 'agents' array is required",
                        ));
                        continue;
                    }
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
                                    &format!(
                                        "Skill '{}' loaded. Follow the instructions above.",
                                        skill_name
                                    ),
                                ));
                            }
                            crate::skill::SkillMode::SubAgent { role } => {
                                // Spawn sub-agent with skill instructions as prompt
                                self.event_bus.publish(DomainEvent::new(
                                    EventType::RunStateChanged,
                                    self.run.run_id.as_str(),
                                    serde_json::json!({
                                        "from": "Executing",
                                        "to": format!("Skill:{}:{}", skill_name, role.display_name()),
                                    }),
                                ));

                                let manager = crate::subagent::SubAgentManager::new(
                                    self.config.clone(),
                                    self.event_bus.clone(),
                                    self.project_dir.clone(),
                                );

                                let sub_result =
                                    manager.spawn(*role, &skill.instructions, None).await;

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

                                // Aggregate skill sub-agent tokens into parent budget + metrics
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
                            &format!(
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
                                &format!("batch_{}_{}", call.id, i),
                                &tool_name,
                                &tool_args.to_string(),
                            );
                            let batch_ctx = ToolContext {
                                session_id: SessionId::new("batch"),
                                message_id: MessageId::new(&format!("batch_{}", i)),
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
                        self.event_bus.publish(DomainEvent::new(
                            EventType::ToolCallCompleted,
                            call.id.as_str(),
                            serde_json::json!({
                                "tool_name": "batch",
                                "success": true,
                                "input": { "count": total },
                                "output_preview": format!("Batch: {total} calls executed"),
                                "duration_ms": 0,
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
                            &format!("BLOCKED by hook: {}", hook_result.output.trim()),
                        ));
                        continue;
                    }
                }

                // Execute regular tool via ToolCallManager (Invariants 2, 3, 5)
                let tool_args = match call.parse_arguments() {
                    Ok(args) => args,
                    Err(e) => {
                        // Report parse error to LLM so it can fix and retry
                        messages.push(Message::tool_result(
                            &call.id,
                            name,
                            &format!(
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
                    message_id: MessageId::new(&format!("iter_{iteration}")),
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
                            };
                        }
                    }
                }

                let result_msg = Message::tool_result(&call.id, name, &output);

                // Update working set + context metrics with tool interaction data.
                // This feeds the usefulness pipeline (P0: feedback data).
                match name.as_str() {
                    "read" | "edit" | "write" | "apply_patch" => {
                        if let Ok(args) = call.parse_arguments() {
                            if let Some(path) = args
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
                    &format!("tool:{}:iter{}", name, iteration),
                    20, // keep last 20 events
                );

                // Update context-loop diagnostics state
                match name.as_str() {
                    "read" => {
                        if let Ok(args) = call.parse_arguments() {
                            if let Some(path) = args.get("filePath").and_then(|p| p.as_str()) {
                                self.context_loop_state.record_read(path);
                            }
                        }
                    }
                    "grep" | "glob" => self.context_loop_state.record_search(),
                    "edit" | "write" | "apply_patch" => {
                        // RPI sensor: warn if editing without prior research.
                        // Research = codebase_context, grep, glob, or read calls.
                        if self.context_loop_state.searches_done == 0
                            && self.context_loop_state.files_read.is_empty()
                            && self.context_loop_state.edit_attempts == 0
                        {
                            messages.push(Message::user(
                                "⚠️ You are editing files without prior research (no grep, glob, read, or codebase_context calls). \
                                 Consider researching the codebase first to understand the affected area and avoid misdiagnosis."
                            ));
                        }
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
            if let Some(ref store) = self.snapshot_store {
                if let Some(task) = self.task_manager.get(&self.task_id) {
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
            if t.starts_with("name") && t.contains('=') {
                if let Some(val) = t.split('=').nth(1) {
                    let name = val.trim().trim_matches('"').trim_matches('\'');
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
    }
    // Try package.json
    if let Ok(content) = std::fs::read_to_string(project_dir.join("package.json")) {
        for line in content.lines() {
            let t = line.trim().trim_start_matches('{').trim();
            if t.starts_with("\"name\"") {
                if let Some(val) = t.split(':').nth(1) {
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
    }
    None
}

// ---------------------------------------------------------------------------
// Doom Loop Detection
// ---------------------------------------------------------------------------

/// Tracks recent tool calls to detect doom loops (identical calls repeated).
/// Uses a ring buffer of (tool_name, args_hash) tuples.
struct DoomLoopTracker {
    recent: std::collections::VecDeque<(String, u64)>,
    threshold: usize,
    /// How many times the doom loop was detected consecutively.
    /// First detection = warning. Second detection (threshold*2) = hard abort.
    hit_count: usize,
}

impl DoomLoopTracker {
    fn new(threshold: usize) -> Self {
        Self {
            recent: std::collections::VecDeque::with_capacity(threshold + 1),
            threshold,
            hit_count: 0,
        }
    }

    /// Returns true if a hard abort should happen (2x threshold consecutive identical calls).
    fn should_abort(&self) -> bool {
        self.hit_count >= 2
    }

    /// Record a tool call. Returns true if a doom loop is detected.
    fn record(&mut self, tool_name: &str, args: &serde_json::Value) -> bool {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        tool_name.hash(&mut hasher);
        args.to_string().hash(&mut hasher);
        let hash = hasher.finish();

        self.recent.push_back((tool_name.to_string(), hash));
        if self.recent.len() > self.threshold {
            self.recent.pop_front();
        }

        // Detect: all entries in the buffer are identical
        if self.recent.len() == self.threshold {
            let first = &self.recent[0];
            let is_loop = self.recent.iter().all(|entry| entry.1 == first.1);
            if is_loop {
                self.hit_count += 1;
            } else {
                self.hit_count = 0;
            }
            is_loop
        } else {
            self.hit_count = 0;
            false
        }
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_millis() as u64
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
        };
        assert!(result.success);
        assert_eq!(result.summary, "done");
        assert_eq!(result.files_edited.len(), 1);
        assert_eq!(result.iterations_used, 5);
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

    // -----------------------------------------------------------------------
    // Doom loop detection
    // -----------------------------------------------------------------------

    #[test]
    fn doom_loop_detected_after_threshold_identical_calls() {
        let mut tracker = DoomLoopTracker::new(3);
        let args = serde_json::json!({"filePath": "/tmp/test"});
        assert!(!tracker.record("read", &args));
        assert!(!tracker.record("read", &args));
        assert!(
            tracker.record("read", &args),
            "3rd identical call should trigger doom loop"
        );
    }

    #[test]
    fn doom_loop_no_false_positive_same_tool_different_inputs() {
        let mut tracker = DoomLoopTracker::new(3);
        assert!(!tracker.record("read", &serde_json::json!({"filePath": "a.rs"})));
        assert!(!tracker.record("read", &serde_json::json!({"filePath": "b.rs"})));
        assert!(!tracker.record("read", &serde_json::json!({"filePath": "c.rs"})));
        // Different inputs → no doom loop
    }

    #[test]
    fn doom_loop_counter_resets_on_different_tool() {
        let mut tracker = DoomLoopTracker::new(3);
        let args = serde_json::json!({"filePath": "/tmp/test"});
        assert!(!tracker.record("read", &args));
        assert!(!tracker.record("read", &args));
        assert!(!tracker.record("bash", &serde_json::json!({"command": "ls"}))); // different tool
        assert!(!tracker.record("read", &args)); // counter reset by interleaving
    }

    #[test]
    fn doom_loop_threshold_configurable() {
        let config = AgentConfig::default();
        assert_eq!(config.doom_loop_threshold, Some(3));

        let mut tracker = DoomLoopTracker::new(5);
        let args = serde_json::json!({});
        for _ in 0..4 {
            assert!(!tracker.record("bash", &args));
        }
        assert!(
            tracker.record("bash", &args),
            "5th call should trigger with threshold=5"
        );
    }
}
