use std::path::PathBuf;
use std::sync::Arc;

use theo_domain::agent_run::{AgentRun, RunState};
use theo_domain::budget::Budget;
use theo_domain::event::{DomainEvent, EventType};
use theo_domain::identifiers::{RunId, TaskId};
use theo_domain::session::{MessageId, SessionId};
use theo_domain::task::TaskState;
use theo_domain::tool_call::ToolCallState;
use theo_domain::tool::ToolContext;
use theo_infra_llm::types::{ChatRequest, Message};
use theo_infra_llm::LlmClient;
use theo_tooling::registry::ToolRegistry;

use crate::agent_loop::AgentResult;
use crate::budget_enforcer::BudgetEnforcer;
use crate::config::AgentConfig;
use crate::convergence::{
    ConvergenceEvaluator, ConvergenceMode,
    EditSuccessConvergence, GitDiffConvergence,
};
use crate::event_bus::EventBus;
use crate::metrics::{MetricsCollector, RuntimeMetrics};
use crate::persistence::SnapshotStore;
use crate::snapshot::RunSnapshot;
#[allow(deprecated)]
use crate::state::AgentState;
use crate::task_manager::TaskManager;
use crate::skill::SkillRegistry;
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
    registry: ToolRegistry,
    config: AgentConfig,
    project_dir: PathBuf,
    budget_enforcer: BudgetEnforcer,
    metrics: Arc<MetricsCollector>,
    #[allow(dead_code)] // Will be used when GRAPHCTX is wired to agent
    convergence: ConvergenceEvaluator,
    snapshot_store: Option<Arc<dyn SnapshotStore>>,
    #[allow(deprecated)]
    agent_state: AgentState,
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
        registry: ToolRegistry,
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

        #[allow(deprecated)]
        let agent_state = AgentState::new();

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
            snapshot_store: None,
            agent_state,
        }
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
        self.execute_with_history(Vec::new()).await
    }

    /// Execute with session history from previous REPL prompts.
    /// `history` contains messages from prior runs in this session.
    /// The current task objective is appended as the last user message.
    pub async fn execute_with_history(&mut self, history: Vec<Message>) -> AgentResult {
        // Transition to Planning
        self.transition_run(RunState::Planning);

        // Transition task to Running
        let _ = self.task_manager.transition(&self.task_id, TaskState::Ready);
        let _ = self.task_manager.transition(&self.task_id, TaskState::Running);

        // System prompt: .theo/system-prompt.md replaces default, or use config default
        let system_prompt = if !self.config.is_subagent {
            crate::project_config::load_system_prompt(&self.project_dir)
                .unwrap_or_else(|| self.config.system_prompt.clone())
        } else {
            self.config.system_prompt.clone()
        };

        let mut messages: Vec<Message> = vec![
            Message::system(&system_prompt),
        ];

        // Project context: .theo/theo.md prepended as separate system message
        if !self.config.is_subagent {
            if let Some(context) = crate::project_config::load_project_context(&self.project_dir) {
                messages.push(Message::system(&format!(
                    "## Project Context\n{context}"
                )));
            }
        }

        // Inject memories from previous runs (cross-run memory)
        let memory_root = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
            .join(".config")
            .join("theo")
            .join("memory");

        let memory_store = theo_tooling::memory::FileMemoryStore::for_project(
            &memory_root,
            &self.project_dir,
        );
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

        // Doom loop detector — tracks recent tool calls to detect repetition
        let mut doom_tracker = self.config.doom_loop_threshold
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
                let _ = self.task_manager.transition(&self.task_id, TaskState::Failed);

                #[allow(deprecated)]
                let summary = format!(
                    "Budget exceeded: {}. Edits succeeded: {}. Files: {}",
                    violation,
                    self.agent_state.edits_succeeded,
                    self.agent_state.edits_files.join(", ")
                );

                self.metrics.record_run_complete(false);
                return AgentResult {
                    success: self.agent_state.edits_succeeded > 0,
                    summary,
                    files_edited: self.agent_state.edits_files.clone(),
                    iterations_used: iteration,
                    was_streamed: false,
                    tokens_used: self.metrics.snapshot().total_tokens_used,
                };
            }

            // ── PLANNING phase ──
            // Context loop injection
            #[allow(deprecated)]
            if iteration > 1 && iteration % self.config.context_loop_interval == 0 {
                let task_objective = self.task_manager.get(&self.task_id)
                    .map(|t| t.objective.clone())
                    .unwrap_or_default();
                let ctx_msg = self.agent_state.build_context_loop(
                    iteration,
                    self.config.max_iterations,
                    &task_objective,
                );
                messages.push(Message::user(ctx_msg));
            }

            // Phase transitions (legacy, preserved for context loop diagnostics)
            #[allow(deprecated)]
            self.agent_state.maybe_transition(iteration, self.config.max_iterations);

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

            // Use streaming with delta callback for real-time reasoning display
            let response = self.client.chat_streaming(&request, |delta| {
                match delta {
                    theo_infra_llm::stream::StreamDelta::Reasoning(text) => {
                        event_bus_for_stream.publish(DomainEvent::new(
                            EventType::ReasoningDelta,
                            &run_id_for_stream,
                            serde_json::json!({"text": text}),
                        ));
                    }
                    theo_infra_llm::stream::StreamDelta::Content(text) => {
                        event_bus_for_stream.publish(DomainEvent::new(
                            EventType::ContentDelta,
                            &run_id_for_stream,
                            serde_json::json!({"text": text}),
                        ));
                    }
                    _ => {}
                }
            }).await;

            let response = match response {
                Ok(resp) => {
                    let llm_duration = llm_start.elapsed().as_millis() as u64;
                    let tokens = resp.usage.as_ref().map(|u| u.total_tokens as u64).unwrap_or(0);
                    // Record tokens in budget and metrics
                    self.budget_enforcer.record_tokens(tokens);
                    self.metrics.record_llm_call(llm_duration, tokens);
                    resp
                }
                Err(e) => {
                    self.transition_run(RunState::Aborted);
                    let _ = self.task_manager.transition(&self.task_id, TaskState::Failed);
                    self.metrics.record_run_complete(false);
                    return AgentResult {
                        success: false,
                        summary: format!("LLM error: {e}"),
                        files_edited: self.agent_state.edits_files.clone(),
                        iterations_used: iteration,
                        was_streamed: false,
                        tokens_used: self.metrics.snapshot().total_tokens_used,
                    };
                }
            };

            let tool_calls = response.tool_calls();

            // No tool calls → text-only response (OpenCode pattern)
            // LLM decided to respond with text, not use tools.
            // This handles conversational messages ("hello") and informational queries.
            if tool_calls.is_empty() {
                let content = response.content().unwrap_or("").to_string();
                if !content.is_empty() {
                    // Text-only response → return as result (like OpenCode finish_reason="stop")
                    // was_streamed=true: this content was already displayed via ContentDelta
                    // during chat_streaming(). The REPL must NOT re-print it.
                    self.transition_run(RunState::Converged);
                    let _ = self.task_manager.transition(&self.task_id, TaskState::Completed);
                    self.metrics.record_run_complete(true);
                    return AgentResult {
                        success: true,
                        summary: content,
                        files_edited: self.agent_state.edits_files.clone(),
                        iterations_used: iteration,
                        was_streamed: true,
                        tokens_used: self.metrics.snapshot().total_tokens_used,
                    };
                }
                // Empty content → LLM gave nothing, continue to next iteration
                messages.push(Message::assistant(content));
                continue;
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

                // Handle `done` meta-tool — always accept (OpenCode pattern)
                // No git diff gate. The LLM decides when the task is complete.
                // Works in projects with or without git.
                if name == "done" {
                    self.transition_run(RunState::Evaluating);

                    let summary = call
                        .parse_arguments()
                        .ok()
                        .and_then(|args| args.get("summary").and_then(|s| s.as_str()).map(String::from))
                        .unwrap_or_else(|| "Task completed.".to_string());

                    self.transition_run(RunState::Converged);
                    let _ = self.task_manager.transition(&self.task_id, TaskState::Completed);
                    self.metrics.record_run_complete(true);

                    should_return = Some(AgentResult {
                        success: true,
                        summary,
                        files_edited: self.agent_state.edits_files.clone(),
                        iterations_used: iteration,
                        was_streamed: false,
                        tokens_used: self.metrics.snapshot().total_tokens_used,
                    });
                    break;
                }

                // Handle `subagent` meta-tool — delegate to sub-agent
                if name == "subagent" {
                    let args = call.parse_arguments().unwrap_or_default();
                    let role_str = args.get("role").and_then(|v| v.as_str()).unwrap_or("explorer");
                    let objective = args.get("objective").and_then(|v| v.as_str()).unwrap_or("No objective provided");

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
                        format!("[{} sub-agent completed] {}", role.display_name(), sub_result.summary)
                    } else {
                        format!("[{} sub-agent failed] {}", role.display_name(), sub_result.summary)
                    };

                    // Record sub-agent files as parent's edits
                    for file in &sub_result.files_edited {
                        if !file.is_empty() {
                            self.agent_state.record_edit_attempt(file, true, None);
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
                                    self.agent_state.record_edit_attempt(file, true, None);
                                }
                            }
                            // Aggregate parallel sub-agent tokens into parent budget + metrics
                            self.budget_enforcer.record_tokens(result.tokens_used);
                            self.metrics.record_delegated_tokens(result.tokens_used);
                        }

                        messages.push(Message::tool_result(&call.id, "subagent_parallel", &combined));
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
                                    &format!("Skill '{}' loaded. Follow the instructions above.", skill_name),
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

                                let sub_result = manager.spawn(*role, &skill.instructions, None).await;

                                let result_msg = if sub_result.success {
                                    format!("[Skill '{}' completed] {}", skill_name, sub_result.summary)
                                } else {
                                    format!("[Skill '{}' failed] {}", skill_name, sub_result.summary)
                                };

                                for file in &sub_result.files_edited {
                                    if !file.is_empty() {
                                        self.agent_state.record_edit_attempt(file, true, None);
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
                            skill_registry.list().iter().map(|s| s.name.clone()).collect()
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
                        const BLOCKED: &[&str] = &["batch", "done", "subagent", "subagent_parallel", "skill"];

                        let mut batch_output = String::new();
                        let total = calls.len().min(MAX_BATCH);

                        for (i, batch_call) in calls.iter().take(MAX_BATCH).enumerate() {
                            let tool_name = batch_call.get("tool").and_then(|v| v.as_str()).unwrap_or("?");
                            let tool_args = batch_call.get("args").cloned().unwrap_or(serde_json::json!({}));

                            // Block meta-tools and self-recursion
                            if BLOCKED.contains(&tool_name) {
                                batch_output.push_str(&format!(
                                    "[{}/{}] {}: error — cannot use '{}' inside batch\n",
                                    i + 1, total, tool_name, tool_name
                                ));
                                continue;
                            }

                            // Execute via tool_bridge (sequential, no LLM round-trip)
                            let batch_tool_call = theo_infra_llm::types::ToolCall::new(
                                &format!("batch_{}_{}", call.id, i),
                                tool_name,
                                &tool_args.to_string(),
                            );
                            let batch_ctx = ToolContext {
                                session_id: SessionId::new("batch"),
                                message_id: MessageId::new(&format!("batch_{}", i)),
                                call_id: batch_tool_call.id.clone(),
                                agent: "main".to_string(),
                                abort: abort_rx.clone(),
                                project_dir: self.project_dir.clone(),
                            };

                            let (msg, success) = tool_bridge::execute_tool_call(
                                &self.registry, &batch_tool_call, &batch_ctx,
                            ).await;

                            let output = msg.content.unwrap_or_default();
                            let status = if success { "ok" } else { "error" };
                            let preview = if output.len() > 200 {
                                let mut end = 200;
                                while end > 0 && !output.is_char_boundary(end) { end -= 1; }
                                format!("{}...", &output[..end])
                            } else {
                                output.clone()
                            };

                            batch_output.push_str(&format!(
                                "[{}/{}] {}({}): {} — {}\n",
                                i + 1, total, tool_name,
                                truncate_batch_args(&tool_args), status, preview
                            ));

                            // Track in budget/metrics
                            self.budget_enforcer.record_tool_call();
                            self.metrics.record_tool_call(tool_name, 0, success);

                            // Track edits
                            #[allow(deprecated)]
                            if success && matches!(tool_name, "edit" | "write" | "apply_patch") {
                                let file = tool_args.get("filePath")
                                    .and_then(|p| p.as_str())
                                    .unwrap_or("");
                                if !file.is_empty() {
                                    self.agent_state.record_edit_attempt(file, true, None);
                                }
                            }
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

                // Execute regular tool via ToolCallManager (Invariants 2, 3, 5)
                let tool_call_id = self.tool_call_manager.enqueue(
                    self.task_id.clone(),
                    name.clone(),
                    call.parse_arguments().unwrap_or_default(),
                );

                let ctx = ToolContext {
                    session_id: SessionId::new("agent"),
                    message_id: MessageId::new(&format!("iter_{iteration}")),
                    call_id: call.id.clone(),
                    agent: "main".to_string(),
                    abort: abort_rx.clone(),
                    project_dir: self.project_dir.clone(),
                };

                let tool_result = self.tool_call_manager
                    .dispatch_and_execute(&tool_call_id, &self.registry, &ctx)
                    .await;

                let (success, output) = match &tool_result {
                    Ok(r) => (r.status == ToolCallState::Succeeded, r.output.clone()),
                    Err(e) => (false, format!("Tool call error: {}", e)),
                };

                // Record tool call in budget and metrics
                self.budget_enforcer.record_tool_call();
                self.metrics.record_tool_call(name, 0, success);

                // Doom loop detection: check if this call repeats identically
                if let Some(ref mut tracker) = doom_tracker {
                    let args = call.parse_arguments().unwrap_or_default();
                    if tracker.record(name, &args) {
                        let warning = format!(
                            "⚠️ DOOM LOOP DETECTED: You have called '{}' with identical arguments {} times in a row. \
                             You are stuck in a loop. Try a DIFFERENT approach or tool.",
                            name, self.config.doom_loop_threshold.unwrap_or(3)
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
                    }
                }

                let result_msg = Message::tool_result(&call.id, name, &output);

                // Update agent state (preserved for context loop diagnostics)
                #[allow(deprecated)]
                match name.as_str() {
                    "read" => {
                        if let Ok(args) = call.parse_arguments() {
                            if let Some(path) = args.get("filePath").and_then(|p| p.as_str()) {
                                self.agent_state.record_read(path);
                            }
                        }
                    }
                    "grep" | "glob" => self.agent_state.record_search(),
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
                                        args.get("patchText")
                                            .and_then(|p| p.as_str())
                                            .and_then(|patch| {
                                                patch.lines()
                                                    .find(|l| l.starts_with("+++ "))
                                                    .and_then(|l| l.strip_prefix("+++ b/").or(l.strip_prefix("+++ ")))
                                                    .filter(|f| *f != "/dev/null")
                                                    .map(String::from)
                                            })
                                    })
                            })
                            .unwrap_or_default();
                        self.agent_state.record_edit_attempt(
                            &file,
                            success,
                            if success { None } else { Some(output.clone()) },
                        );
                    }
                    _ => {}
                }

                messages.push(result_msg);
            }

            if let Some(result) = should_return {
                return result;
            }

            // ── EVALUATING phase ──
            self.transition_run(RunState::Evaluating);

            // Save snapshot if store is configured (Invariant 7)
            if let Some(ref store) = self.snapshot_store {
                if let Some(task) = self.task_manager.get(&self.task_id) {
                    let snapshot = RunSnapshot::new(
                        self.run.clone(),
                        task,
                        vec![], // tool_calls aggregated by ToolCallManager, not stored here
                        vec![], // tool_results same
                        self.event_bus.events(),
                        self.budget_enforcer.usage(),
                        vec![], // messages not persisted in snapshot for now
                        vec![], // DLQ entries
                    );
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
// Doom Loop Detection
// ---------------------------------------------------------------------------

/// Tracks recent tool calls to detect doom loops (identical calls repeated).
/// Uses a ring buffer of (tool_name, args_hash) tuples.
struct DoomLoopTracker {
    recent: std::collections::VecDeque<(String, u64)>,
    threshold: usize,
}

impl DoomLoopTracker {
    fn new(threshold: usize) -> Self {
        Self {
            recent: std::collections::VecDeque::with_capacity(threshold + 1),
            threshold,
        }
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
            self.recent.iter().all(|entry| entry.1 == first.1)
        } else {
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
            Self { bus, listener, tm, tcm }
        }

        fn create_engine(&self, task_objective: &str) -> AgentRunEngine {
            let task_id = self.tm.create_task(
                SessionId::new("s"), AgentType::Coder, task_objective.into(),
            );
            AgentRunEngine::new(
                task_id, self.tm.clone(), self.tcm.clone(), self.bus.clone(),
                LlmClient::new("http://localhost:9999", None, "test"),
                theo_tooling::registry::create_default_registry(),
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
        let run_init = events.iter().find(|e| e.event_type == EventType::RunInitialized);
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
        let state_changed: Vec<_> = events.iter()
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

        let state_events: Vec<_> = setup.listener.captured().iter()
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
        assert_eq!(engine.state(), RunState::Converged, "terminal state must not change");
    }

    // -----------------------------------------------------------------------
    // Backward compat
    // -----------------------------------------------------------------------

    #[allow(deprecated)]
    #[test]
    fn phase_enum_still_compiles_with_deprecated() {
        use crate::state::Phase;
        let _p = Phase::Explore;
        let _e = Phase::Edit;
        let _v = Phase::Verify;
        let _d = Phase::Done;
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
        };
        assert!(result.success);
        assert_eq!(result.summary, "done");
        assert_eq!(result.files_edited.len(), 1);
        assert_eq!(result.iterations_used, 5);
    }

    #[allow(deprecated)]
    #[test]
    fn agent_loop_new_signature_backward_compat() {
        // Verify AgentLoop::new still accepts the old signature
        use crate::agent_loop::AgentLoop;
        use crate::events::{NullEventSink, EventSink};
        let config = AgentConfig::default();
        let registry = theo_tooling::registry::create_default_registry();
        let sink: Arc<dyn EventSink> = Arc::new(NullEventSink);
        let _loop = AgentLoop::new(config, registry, sink);
        // Compilation is the test — if this compiles, backward compat is preserved
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
        assert!(tracker.record("read", &args), "3rd identical call should trigger doom loop");
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
        assert!(tracker.record("bash", &args), "5th call should trigger with threshold=5");
    }
}
