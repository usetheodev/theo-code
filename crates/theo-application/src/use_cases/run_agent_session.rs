#![allow(deprecated)]

use std::path::Path;
use std::sync::Arc;

use theo_agent_runtime::events::EventSink;
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
    config: AgentConfig,
    task: &str,
    project_dir: &Path,
    event_sink: Arc<dyn EventSink>,
) -> Result<AgentResult, RunSessionError> {
    if config.api_key.is_none() {
        return Err(RunSessionError::MissingApiKey);
    }

    if !project_dir.exists() {
        return Err(RunSessionError::InvalidProjectDir(
            project_dir.display().to_string(),
        ));
    }

    // Initialize GRAPHCTX — build code graph for context injection.
    // Runs in spawn_blocking with timeout; failure is non-fatal.
    let graph_context: Option<Arc<dyn GraphContextProvider>> = {
        let service = Arc::new(GraphContextService::new());
        match service.initialize(project_dir).await {
            Ok(()) => {
                eprintln!("[theo] GRAPHCTX initialized ({} ready)", if service.is_ready() { "graph" } else { "no graph" });
                Some(service)
            }
            Err(e) => {
                eprintln!("[theo] GRAPHCTX init failed (degraded mode): {e}");
                None
            }
        }
    };

    let registry = create_default_registry();
    let mut agent = AgentLoop::new(config, registry, event_sink);
    if let Some(gc) = graph_context {
        agent = agent.with_graph_context(gc);
    }
    let result = agent.run(task, project_dir).await;

    Ok(result)
}
