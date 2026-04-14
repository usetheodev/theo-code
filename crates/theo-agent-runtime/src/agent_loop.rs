use std::path::Path;
use std::sync::Arc;

use theo_domain::session::SessionId;
use theo_domain::task::AgentType;
use theo_infra_llm::LlmClient;
use theo_tooling::registry::ToolRegistry;

use crate::capability_gate::CapabilityGate;
use crate::config::AgentConfig;
use crate::event_bus::{EventBus, EventListener};
use crate::run_engine::AgentRunEngine;
use crate::task_manager::TaskManager;
use crate::tool_call_manager::ToolCallManager;

/// Result of an agent loop execution.
#[derive(Debug, Clone)]
pub struct AgentResult {
    pub success: bool,
    pub summary: String,
    pub files_edited: Vec<String>,
    pub iterations_used: usize,
    /// True when the summary was already displayed via ContentDelta streaming.
    /// The REPL should NOT re-print the summary in this case to avoid duplication.
    /// Only set for text-only responses (no tool calls) where content == summary.
    pub was_streamed: bool,
    /// Total tokens consumed during this run (LLM input + output).
    /// Collected by MetricsCollector, surfaced for display.
    pub tokens_used: u64,
    /// Input (prompt) tokens consumed during this run.
    pub input_tokens: u64,
    /// Output (completion) tokens consumed during this run.
    pub output_tokens: u64,
}

/// The main agent loop that orchestrates LLM ↔ tool execution.
///
/// This is now a thin facade over `AgentRunEngine`. All execution logic
/// lives in `run_engine.rs`.
pub struct AgentLoop {
    client_base_url: String,
    client_api_key: Option<String>,
    client_model: String,
    client_endpoint_override: Option<String>,
    client_extra_headers: Vec<(String, String)>,
    #[allow(dead_code)] // Stored for backward compat; runtime uses create_default_registry()
    registry: ToolRegistry,
    config: AgentConfig,
    listeners: Vec<Arc<dyn EventListener>>,
    graph_context: Option<Arc<dyn theo_domain::graph_context::GraphContextProvider>>,
}

impl AgentLoop {
    pub fn new(config: AgentConfig, registry: ToolRegistry) -> Self {
        Self {
            client_base_url: config.base_url.clone(),
            client_api_key: config.api_key.clone(),
            client_model: config.model.clone(),
            client_endpoint_override: config.endpoint_override.clone(),
            client_extra_headers: config
                .extra_headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            registry,
            config,
            listeners: Vec::new(),
            graph_context: None,
        }
    }

    /// Attach a native domain-event listener to the loop.
    #[must_use]
    pub fn with_event_listener(mut self, listener: Arc<dyn EventListener>) -> Self {
        self.listeners.push(listener);
        self
    }

    /// Set the graph context provider for code intelligence injection.
    pub fn with_graph_context(
        mut self,
        provider: Arc<dyn theo_domain::graph_context::GraphContextProvider>,
    ) -> Self {
        self.graph_context = Some(provider);
        self
    }

    /// Run the agent loop on a task.
    ///
    /// Delegates entirely to `AgentRunEngine::execute()`.
    pub async fn run(&self, task: &str, project_dir: &Path) -> AgentResult {
        let event_bus = Arc::new(EventBus::new());
        self.attach_listeners(&event_bus);

        // Create managers
        let task_manager = Arc::new(TaskManager::new(event_bus.clone()));
        let tcm = ToolCallManager::new(event_bus.clone());
        let tool_call_manager = Arc::new(if let Some(ref caps) = self.config.capability_set {
            let gate = Arc::new(CapabilityGate::new(caps.clone(), event_bus.clone()));
            tcm.with_capability_gate(gate)
        } else {
            tcm
        });

        // Create task
        let task_id =
            task_manager.create_task(SessionId::new("agent"), AgentType::Coder, task.to_string());

        // Build LLM client (same logic as old AgentLoop::new)
        let mut client = LlmClient::new(
            &self.client_base_url,
            self.client_api_key.clone(),
            &self.client_model,
        );
        if let Some(ref endpoint) = self.client_endpoint_override {
            client = client.with_endpoint(endpoint);
        }
        for (k, v) in &self.client_extra_headers {
            client = client.with_header(k, v);
        }

        // Create registry with plugin tools
        let mut registry = theo_tooling::registry::create_default_registry();
        load_plugin_tools(&mut registry, project_dir);
        let registry = Arc::new(registry);

        let mut engine = AgentRunEngine::new(
            task_id,
            task_manager,
            tool_call_manager,
            event_bus,
            client,
            registry,
            self.config.clone(),
            project_dir.to_path_buf(),
        );
        if let Some(ref gc) = self.graph_context {
            engine = engine.with_graph_context(gc.clone());
        }

        engine.execute().await
    }

    /// Run with session history and external EventBus.
    ///
    /// The caller provides an EventBus with listeners already subscribed
    /// (e.g., CliRenderer for real-time output).
    pub async fn run_with_history(
        &self,
        task: &str,
        project_dir: &Path,
        history: Vec<theo_infra_llm::types::Message>,
        external_bus: Option<Arc<EventBus>>,
    ) -> AgentResult {
        let event_bus = external_bus.unwrap_or_else(|| Arc::new(EventBus::new()));
        self.attach_listeners(&event_bus);

        let task_manager = Arc::new(TaskManager::new(event_bus.clone()));
        let tcm = ToolCallManager::new(event_bus.clone());
        let tool_call_manager = Arc::new(if let Some(ref caps) = self.config.capability_set {
            let gate = Arc::new(CapabilityGate::new(caps.clone(), event_bus.clone()));
            tcm.with_capability_gate(gate)
        } else {
            tcm
        });

        let task_id =
            task_manager.create_task(SessionId::new("agent"), AgentType::Coder, task.to_string());

        let mut client = LlmClient::new(
            &self.client_base_url,
            self.client_api_key.clone(),
            &self.client_model,
        );
        if let Some(ref endpoint) = self.client_endpoint_override {
            client = client.with_endpoint(endpoint);
        }
        for (k, v) in &self.client_extra_headers {
            client = client.with_header(k, v);
        }

        let mut registry = theo_tooling::registry::create_default_registry();
        load_plugin_tools(&mut registry, project_dir);
        let registry = Arc::new(registry);

        let mut engine = AgentRunEngine::new(
            task_id,
            task_manager,
            tool_call_manager,
            event_bus,
            client,
            registry,
            self.config.clone(),
            project_dir.to_path_buf(),
        );
        if let Some(ref gc) = self.graph_context {
            engine = engine.with_graph_context(gc.clone());
        }

        engine.execute_with_history(history).await
    }

    fn attach_listeners(&self, event_bus: &Arc<EventBus>) {
        for listener in &self.listeners {
            event_bus.subscribe(listener.clone());
        }
    }
}

/// Check if the project has real uncommitted changes via git diff.
/// Kept as free function for backward compatibility with existing tests.
/// Load plugin tools from .theo/plugins/ into the registry.
fn load_plugin_tools(registry: &mut theo_tooling::registry::ToolRegistry, project_dir: &Path) {
    let plugins = crate::plugin::load_plugins(project_dir);
    let mut tool_specs = Vec::new();
    for plugin in &plugins {
        for (spec, script_path) in &plugin.tool_scripts {
            let params: Vec<theo_domain::tool::ToolParam> = spec
                .params
                .iter()
                .map(|p| theo_domain::tool::ToolParam {
                    name: p.name.clone(),
                    param_type: p.param_type.clone(),
                    description: p.description.clone(),
                    required: p.required,
                })
                .collect();
            tool_specs.push((
                spec.name.clone(),
                spec.description.clone(),
                script_path.clone(),
                params,
            ));
        }
    }
    if !tool_specs.is_empty() {
        theo_tooling::registry::register_plugin_tools(registry, tool_specs);
    }
}

#[allow(dead_code)]
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

/// Generate a phase-specific nudge message if appropriate.
/// Kept as free function for backward compatibility with existing tests.
#[allow(dead_code)]
fn phase_nudge(
    state: &crate::loop_state::ContextLoopState,
    iteration: usize,
    max_iterations: usize,
) -> Option<String> {
    let two_thirds = (max_iterations * 2) / 3;

    match state.phase {
        crate::loop_state::LoopPhase::Edit if iteration >= two_thirds && state.edits_succeeded == 0 => {
            Some("URGENT: You have very few iterations left and NO successful edits. Stop reading/searching and EDIT a file NOW.".to_string())
        }
        crate::loop_state::LoopPhase::Edit if state.edit_attempts > 3 && state.edits_succeeded == 0 => {
            Some("Your edits keep failing. Read the target file again carefully, then try a different edit approach.".to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_result_default() {
        let result = AgentResult {
            success: false,
            summary: "test".to_string(),
            files_edited: vec![],
            iterations_used: 0,
            was_streamed: false,
            tokens_used: 0,
            input_tokens: 0,
            output_tokens: 0,
        };
        assert!(!result.success);
    }

    #[test]
    fn test_phase_nudge_urgent() {
        let mut state = crate::loop_state::ContextLoopState::new();
        state.phase = crate::loop_state::LoopPhase::Edit;
        let nudge = phase_nudge(&state, 10, 15);
        assert!(nudge.is_some());
        assert!(nudge.unwrap().contains("URGENT"));
    }

    #[test]
    fn test_phase_nudge_none_in_explore() {
        let state = crate::loop_state::ContextLoopState::new();
        let nudge = phase_nudge(&state, 1, 15);
        assert!(nudge.is_none());
    }

    #[test]
    fn test_agent_loop_new_preserves_constructor_fields() {
        let config = AgentConfig::default();
        let registry = theo_tooling::registry::create_default_registry();
        let agent_loop = AgentLoop::new(config.clone(), registry);

        // Verify constructor propagates config correctly
        assert_eq!(agent_loop.client_model, config.model);
        assert!(
            agent_loop.graph_context.is_none(),
            "graph_context should be None by default"
        );
        assert!(agent_loop.listeners.is_empty());
    }

    #[test]
    fn test_with_event_listener_registers_listener() {
        let config = AgentConfig::default();
        let registry = theo_tooling::registry::create_default_registry();
        let loop_ = AgentLoop::new(config, registry)
            .with_event_listener(Arc::new(crate::event_bus::NullEventListener));
        assert_eq!(loop_.listeners.len(), 1);
    }
}
