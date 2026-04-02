use std::path::PathBuf;
use std::sync::Arc;

use theo_domain::agent_run::{AgentRun, RunState};
use theo_domain::budget::Budget;
use theo_domain::event::{DomainEvent, EventType};
use theo_domain::identifiers::{RunId, TaskId};
use theo_domain::retry_policy::RetryPolicy;
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
use crate::retry::RetryExecutor;
use crate::snapshot::RunSnapshot;
#[allow(deprecated)]
use crate::state::AgentState;
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
    registry: ToolRegistry,
    config: AgentConfig,
    project_dir: PathBuf,
    budget_enforcer: BudgetEnforcer,
    metrics: Arc<MetricsCollector>,
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

        let mut messages: Vec<Message> = vec![
            Message::system(&self.config.system_prompt),
        ];

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

        // Inject session history (previous REPL prompts + responses)
        if !history.is_empty() {
            messages.extend(history);
        }

        // Add the task objective as user message
        if let Some(task) = self.task_manager.get(&self.task_id) {
            messages.push(Message::user(&task.objective));
        }

        let tool_defs = tool_bridge::registry_to_definitions(&self.registry);
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
                    self.transition_run(RunState::Converged);
                    let _ = self.task_manager.transition(&self.task_id, TaskState::Completed);
                    self.metrics.record_run_complete(true);
                    return AgentResult {
                        success: true,
                        summary: content,
                        files_edited: self.agent_state.edits_files.clone(),
                        iterations_used: iteration,
                    };
                }
                // Empty content → LLM gave nothing, continue to next iteration
                messages.push(Message::assistant(content));
                continue;
            }

            // ── EXECUTING phase ──
            self.transition_run(RunState::Executing);

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
                    });
                    break;
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
}
