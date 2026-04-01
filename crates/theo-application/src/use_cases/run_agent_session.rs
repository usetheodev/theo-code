use std::path::Path;
use std::sync::Arc;

use theo_agent_runtime::events::EventSink;
use theo_agent_runtime::{AgentConfig, AgentLoop, AgentResult};
use theo_tooling::registry::create_default_registry;

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

    let registry = create_default_registry();
    let agent = AgentLoop::new(config, registry, event_sink);
    let result = agent.run(task, project_dir).await;

    Ok(result)
}
