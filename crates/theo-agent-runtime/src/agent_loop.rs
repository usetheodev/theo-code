use std::path::Path;
use std::sync::Arc;

use theo_domain::event::DomainEvent;
use theo_domain::session::SessionId;
use theo_domain::task::AgentType;
use theo_infra_llm::LlmClient;
use theo_tooling::registry::ToolRegistry;

use crate::config::AgentConfig;
use crate::event_bus::{EventBus, EventListener};
#[allow(deprecated)]
use crate::events::{AgentEvent, EventSink};
use crate::run_engine::AgentRunEngine;
use crate::task_manager::TaskManager;
use crate::tool_call_manager::ToolCallManager;

// Keep these imports for backward compat of existing tests
#[allow(deprecated)]
use crate::state::{AgentState, Phase};

/// Result of an agent loop execution.
#[derive(Debug, Clone)]
pub struct AgentResult {
    pub success: bool,
    pub summary: String,
    pub files_edited: Vec<String>,
    pub iterations_used: usize,
}

/// The main agent loop that orchestrates LLM ↔ tool execution.
///
/// This is now a thin facade over `AgentRunEngine`. All execution logic
/// lives in `run_engine.rs`. This struct preserves the original API
/// for backward compatibility with CLI and desktop binaries.
pub struct AgentLoop {
    client_base_url: String,
    client_api_key: Option<String>,
    client_model: String,
    client_endpoint_override: Option<String>,
    client_extra_headers: Vec<(String, String)>,
    registry: ToolRegistry,
    config: AgentConfig,
    #[allow(deprecated)]
    event_sink: Arc<dyn EventSink>,
}

impl AgentLoop {
    #[allow(deprecated)]
    pub fn new(
        config: AgentConfig,
        registry: ToolRegistry,
        event_sink: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            client_base_url: config.base_url.clone(),
            client_api_key: config.api_key.clone(),
            client_model: config.model.clone(),
            client_endpoint_override: config.endpoint_override.clone(),
            client_extra_headers: config.extra_headers.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            registry,
            config,
            event_sink,
        }
    }

    /// Run the agent loop on a task.
    ///
    /// Delegates entirely to `AgentRunEngine::execute()`.
    #[allow(deprecated)]
    pub async fn run(&self, task: &str, project_dir: &Path) -> AgentResult {
        // Create EventBus and bridge old EventSink
        let event_bus = Arc::new(EventBus::new());
        let bridge = Arc::new(EventSinkBridge::new(self.event_sink.clone()));
        event_bus.subscribe(bridge);

        // Create managers
        let task_manager = Arc::new(TaskManager::new(event_bus.clone()));
        let tool_call_manager = Arc::new(ToolCallManager::new(event_bus.clone()));

        // Create task
        let task_id = task_manager.create_task(
            SessionId::new("agent"),
            AgentType::Coder,
            task.to_string(),
        );

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

        // Create and execute RunEngine
        // ToolRegistry doesn't impl Clone, so we create a fresh default registry
        let registry = theo_tooling::registry::create_default_registry();
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

        engine.execute().await
    }

    /// Run with session history and external EventBus.
    ///
    /// The caller provides an EventBus with listeners already subscribed
    /// (e.g., CliRenderer for real-time output). The EventSinkBridge is
    /// also subscribed for backward compat.
    #[allow(deprecated)]
    pub async fn run_with_history(
        &self,
        task: &str,
        project_dir: &Path,
        history: Vec<theo_infra_llm::types::Message>,
        external_bus: Option<Arc<EventBus>>,
    ) -> AgentResult {
        let event_bus = external_bus.unwrap_or_else(|| Arc::new(EventBus::new()));
        let bridge = Arc::new(EventSinkBridge::new(self.event_sink.clone()));
        event_bus.subscribe(bridge);

        let task_manager = Arc::new(TaskManager::new(event_bus.clone()));
        let tool_call_manager = Arc::new(ToolCallManager::new(event_bus.clone()));

        let task_id = task_manager.create_task(
            SessionId::new("agent"),
            AgentType::Coder,
            task.to_string(),
        );

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

        let registry = theo_tooling::registry::create_default_registry();
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

        engine.execute_with_history(history).await
    }
}

/// Bridge that maps DomainEvent → AgentEvent for backward compatibility.
///
/// Implements EventListener (new system) and forwards to EventSink (old system).
#[allow(deprecated)]
struct EventSinkBridge {
    sink: Arc<dyn EventSink>,
}

#[allow(deprecated)]
impl EventSinkBridge {
    fn new(sink: Arc<dyn EventSink>) -> Self {
        Self { sink }
    }
}

#[allow(deprecated)]
impl EventListener for EventSinkBridge {
    fn on_event(&self, event: &DomainEvent) {
        // Map DomainEvent to AgentEvent for backward compat
        let agent_event = match event.event_type {
            theo_domain::event::EventType::LlmCallStart => {
                let iteration = event.payload.get("iteration")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                Some(AgentEvent::LlmCallStart { iteration })
            }
            theo_domain::event::EventType::LlmCallEnd => {
                let iteration = event.payload.get("iteration")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                Some(AgentEvent::LlmCallEnd { iteration })
            }
            theo_domain::event::EventType::RunStateChanged => {
                // Map to PhaseChange for display purposes
                None // RunState changes don't map 1:1 to old Phase changes
            }
            theo_domain::event::EventType::Error => {
                let msg = event.payload.get("error")
                    .or(event.payload.get("reason"))
                    .or(event.payload.get("violation"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error")
                    .to_string();
                Some(AgentEvent::Error(msg))
            }
            _ => None, // Other domain events don't have AgentEvent equivalents
        };

        if let Some(ae) = agent_event {
            self.sink.emit(ae);
        }
    }
}

/// Check if the project has real uncommitted changes via git diff.
/// Kept as free function for backward compatibility with existing tests.
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
#[allow(deprecated, dead_code)]
fn phase_nudge(state: &AgentState, iteration: usize, max_iterations: usize) -> Option<String> {
    let two_thirds = (max_iterations * 2) / 3;

    match state.phase {
        Phase::Edit if iteration >= two_thirds && state.edits_succeeded == 0 => {
            Some("URGENT: You have very few iterations left and NO successful edits. Stop reading/searching and EDIT a file NOW.".to_string())
        }
        Phase::Edit if state.edit_attempts > 3 && state.edits_succeeded == 0 => {
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
        };
        assert!(!result.success);
    }

    #[allow(deprecated)]
    #[test]
    fn test_phase_nudge_urgent() {
        let mut state = AgentState::new();
        state.phase = Phase::Edit;
        let nudge = phase_nudge(&state, 10, 15);
        assert!(nudge.is_some());
        assert!(nudge.unwrap().contains("URGENT"));
    }

    #[allow(deprecated)]
    #[test]
    fn test_phase_nudge_none_in_explore() {
        let state = AgentState::new();
        let nudge = phase_nudge(&state, 1, 15);
        assert!(nudge.is_none());
    }

    #[allow(deprecated)]
    #[test]
    fn test_agent_loop_new_backward_compat() {
        use crate::events::NullEventSink;
        let config = AgentConfig::default();
        let registry = theo_tooling::registry::create_default_registry();
        let sink: Arc<dyn EventSink> = Arc::new(NullEventSink);
        let _loop = AgentLoop::new(config, registry, sink);
        // Compilation is the test
    }

    #[test]
    fn test_event_sink_bridge_does_not_panic() {
        use crate::events::NullEventSink;
        use theo_domain::event::{EventType, ALL_EVENT_TYPES};

        let sink: Arc<dyn EventSink> = Arc::new(NullEventSink);
        let bridge = EventSinkBridge::new(sink);

        // All event types should not panic
        for et in &ALL_EVENT_TYPES {
            let event = DomainEvent::new(*et, "test", serde_json::Value::Null);
            bridge.on_event(&event);
        }
    }
}
