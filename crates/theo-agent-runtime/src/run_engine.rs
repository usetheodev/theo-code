use std::path::{Path, PathBuf};
use std::sync::Arc;

use theo_domain::agent_run::{AgentRun, RunState};
use theo_domain::event::{DomainEvent, EventType};
use theo_domain::identifiers::{RunId, TaskId};
use theo_domain::session::{MessageId, SessionId};
use theo_domain::task::{AgentType, TaskState};
use theo_domain::tool::ToolContext;
use theo_infra_llm::types::{ChatRequest, Message};
use theo_infra_llm::LlmClient;
use theo_tooling::registry::ToolRegistry;

use crate::agent_loop::AgentResult;
use crate::config::AgentConfig;
use crate::event_bus::EventBus;
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

    /// Execute the full agent run cycle.
    ///
    /// Flow: Initialized → Planning → Executing → Evaluating → Converged/Replanning/Aborted
    pub async fn execute(&mut self) -> AgentResult {
        // Transition to Planning
        self.transition_run(RunState::Planning);

        // Transition task to Running
        let _ = self.task_manager.transition(&self.task_id, TaskState::Ready);
        let _ = self.task_manager.transition(&self.task_id, TaskState::Running);

        let mut messages: Vec<Message> = vec![
            Message::system(&self.config.system_prompt),
        ];

        // Add the task objective as user message
        if let Some(task) = self.task_manager.get(&self.task_id) {
            messages.push(Message::user(&task.objective));
        }

        let tool_defs = tool_bridge::registry_to_definitions(&self.registry);
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);

        loop {
            self.run.iteration += 1;
            let iteration = self.run.iteration;

            if iteration > self.config.max_iterations {
                // Budget exhausted → abort
                self.transition_run(RunState::Aborted);
                let _ = self.task_manager.transition(&self.task_id, TaskState::Failed);

                #[allow(deprecated)]
                let summary = format!(
                    "Max iterations ({}) reached. Edits succeeded: {}. Files: {}",
                    self.config.max_iterations,
                    self.agent_state.edits_succeeded,
                    self.agent_state.edits_files.join(", ")
                );

                return AgentResult {
                    success: self.agent_state.edits_succeeded > 0,
                    summary,
                    files_edited: self.agent_state.edits_files.clone(),
                    iterations_used: self.config.max_iterations,
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

            let request = ChatRequest::new(&self.config.model, messages.clone())
                .with_tools(tool_defs.clone())
                .with_max_tokens(self.config.max_tokens)
                .with_temperature(self.config.temperature);

            let response = match self.client.chat(&request).await {
                Ok(resp) => resp,
                Err(e) => {
                    // Retry once
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    match self.client.chat(&request).await {
                        Ok(resp) => resp,
                        Err(_) => {
                            self.transition_run(RunState::Aborted);
                            let _ = self.task_manager.transition(&self.task_id, TaskState::Failed);
                            return AgentResult {
                                success: false,
                                summary: format!("LLM error: {e}"),
                                files_edited: self.agent_state.edits_files.clone(),
                                iterations_used: iteration,
                            };
                        }
                    }
                }
            };

            let tool_calls = response.tool_calls();

            // No tool calls → text response, continue planning
            if tool_calls.is_empty() {
                let content = response.content().unwrap_or("").to_string();
                messages.push(Message::assistant(content));
                // Stay in planning, next iteration
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

                // Handle `done` meta-tool in EVALUATING phase
                if name == "done" {
                    self.transition_run(RunState::Evaluating);

                    let summary = call
                        .parse_arguments()
                        .ok()
                        .and_then(|args| args.get("summary").and_then(|s| s.as_str()).map(String::from))
                        .unwrap_or_else(|| "Task completed.".to_string());

                    // Promise gate: check real changes
                    if has_real_changes(&self.project_dir).await {
                        self.transition_run(RunState::Converged);
                        let _ = self.task_manager.transition(&self.task_id, TaskState::Completed);

                        should_return = Some(AgentResult {
                            success: true,
                            summary,
                            files_edited: self.agent_state.edits_files.clone(),
                            iterations_used: iteration,
                        });
                        break;
                    } else {
                        // Blocked → replan
                        #[allow(deprecated)]
                        self.agent_state.record_done_blocked();
                        messages.push(Message::tool_result(
                            &call.id,
                            "done",
                            "BLOCKED: No real changes detected (git diff is empty). You must make actual code changes before calling done(). Re-read the task and try again.",
                        ));
                        self.transition_run(RunState::Replanning);
                        continue;
                    }
                }

                // Execute regular tool via ToolCallManager
                let ctx = ToolContext {
                    session_id: SessionId::new("agent"),
                    message_id: MessageId::new(&format!("iter_{iteration}")),
                    call_id: call.id.clone(),
                    agent: "main".to_string(),
                    abort: abort_rx.clone(),
                    project_dir: self.project_dir.clone(),
                };

                let (result_msg, success) =
                    tool_bridge::execute_tool_call(&self.registry, call, &ctx).await;

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
                                args.get("filePath")
                                    .or(args.get("file_path"))
                                    .and_then(|p| p.as_str())
                                    .map(String::from)
                            })
                            .unwrap_or_default();
                        self.agent_state.record_edit_attempt(
                            &file,
                            success,
                            if success { None } else { Some(result_msg.content.clone().unwrap_or_default()) },
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

/// Check if the project has real uncommitted changes via git diff.
async fn has_real_changes(project_dir: &Path) -> bool {
    let output = tokio::process::Command::new("git")
        .args(["diff", "--stat"])
        .current_dir(project_dir)
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            !stdout.trim().is_empty()
        }
        Err(_) => true,
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
