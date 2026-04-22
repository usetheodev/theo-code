use std::path::Path;
use std::sync::Arc;

use theo_agent_runtime::event_bus::EventListener;
use theo_agent_runtime::{AgentConfig, AgentLoop, AgentResult};
use theo_domain::graph_context::GraphContextProvider;
use theo_tooling::registry::create_default_registry;

use super::graph_context_service::GraphContextService;

/// Errors that can occur when running an agent session.
#[derive(Debug, thiserror::Error)]
pub enum RunSessionError {
    #[error("No API key configured")]
    MissingApiKey,
    #[error("Project directory does not exist: {0}")]
    InvalidProjectDir(String),
}

/// Run a complete agent session: validate config, create registry, execute loop.
///
/// This is the primary entry point for any surface (CLI, desktop, API)
/// to run the agent. Surfaces should NOT call AgentLoop directly.
///
/// Initializes GRAPHCTX (code intelligence) before running the agent.
/// If graph build fails, the agent runs without code context (graceful degradation).
pub async fn run_agent_session(
    mut config: AgentConfig,
    task: &str,
    project_dir: &Path,
    event_listener: Arc<dyn EventListener>,
) -> Result<AgentResult, RunSessionError> {
    if config.api_key.is_none() {
        return Err(RunSessionError::MissingApiKey);
    }

    if !project_dir.exists() {
        return Err(RunSessionError::InvalidProjectDir(
            project_dir.display().to_string(),
        ));
    }

    // Phase 0 T0.2: when memory is enabled, attach a MemoryEngine with
    // BuiltinMemoryProvider. No-op when disabled.
    super::memory_factory::attach_memory_to_config(&mut config, project_dir);

    // PLAN_CONTEXT_WIRING Phase 4 — build a shared EventBus so retrieval
    // telemetry emitted by the graph-context service flows through the
    // same broadcast channel the agent loop uses. Listeners subscribed
    // to the bus (e.g. TUI renderer, benchmark collectors) observe
    // `RetrievalExecuted` events side-by-side with LLM/tool events.
    let shared_bus = Arc::new(theo_agent_runtime::event_bus::EventBus::new());
    shared_bus.subscribe(event_listener.clone());

    // Initialize GRAPHCTX — fire-and-forget background build.
    // Disabled entirely when THEO_NO_GRAPHCTX=1.
    let graph_context: Option<Arc<dyn GraphContextProvider>> =
        if std::env::var("THEO_NO_GRAPHCTX").is_ok() {
            None // Enabled by default. Set THEO_NO_GRAPHCTX=1 to disable.
        } else {
            let sink: Arc<dyn theo_domain::graph_context::EventSink> = Arc::new(
                theo_agent_runtime::event_bus::EventBusSink::new(shared_bus.clone()),
            );
            let service = Arc::new(GraphContextService::new().with_event_sink(sink));
            let _ = service.initialize(project_dir).await;
            eprintln!("[theo] GRAPHCTX building in background");
            Some(service)
        };

    let registry = create_default_registry();
    let mut agent = AgentLoop::new(config, registry).with_event_listener(event_listener);
    if let Some(gc) = graph_context {
        agent = agent.with_graph_context(gc);
    }
    let result = agent.run(task, project_dir).await;

    Ok(result)
}
