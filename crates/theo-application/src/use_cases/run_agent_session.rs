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

/// Optional runtime injections that surfaces (CLI/Desktop) can build from
/// flags and pass into the agent session. Each field is forwarded to
/// `AgentLoop` and propagated to the underlying `AgentRunEngine`, which
/// activates the corresponding sub-agent feature in `delegate_task`.
#[derive(Default, Clone)]
pub struct SubagentInjections {
    pub registry: Option<Arc<theo_agent_runtime::subagent::SubAgentRegistry>>,
    pub run_store: Option<Arc<theo_agent_runtime::subagent_runs::FileSubagentRunStore>>,
    pub hooks: Option<Arc<theo_agent_runtime::lifecycle_hooks::HookManager>>,
    pub cancellation: Option<Arc<theo_agent_runtime::cancellation::CancellationTree>>,
    pub checkpoint: Option<Arc<theo_agent_runtime::checkpoint::CheckpointManager>>,
    pub worktree: Option<Arc<theo_isolation::WorktreeProvider>>,
    pub mcp: Option<Arc<theo_infra_mcp::McpRegistry>>,
    /// Phase 17 (sota-gaps): pre-populated discovery cache so sub-agents
    /// receive concrete MCP tool definitions in their tool array (not only
    /// a textual hint).
    pub mcp_discovery: Option<Arc<theo_infra_mcp::DiscoveryCache>>,
    /// Phase 18 (sota-gaps): handoff guardrail chain. When `None`, a
    /// default chain (built-ins) is constructed per `delegate_task` call.
    pub handoff_guardrails:
        Option<Arc<theo_agent_runtime::handoff_guardrail::GuardrailChain>>,
    /// Phase 27 follow-up (sota-gaps-followup gap #4): the production
    /// `AutomaticModelRouter` resolved from `.theo/config.toml`. Mounted
    /// onto `AgentConfig.router` by `apply_to`.
    pub router: Option<Arc<dyn theo_domain::routing::ModelRouter>>,
    pub reloadable: Option<theo_agent_runtime::subagent::ReloadableRegistry>,
}

impl SubagentInjections {
    /// Returns a clone of the router handle, if injected. Used by the
    /// CLI to seed `AgentConfig.router` before spawning the loop.
    pub fn router_clone(&self) -> Option<Arc<dyn theo_domain::routing::ModelRouter>> {
        self.router.clone()
    }

    /// Apply all present injections to the AgentLoop.
    ///
    /// REMEDIATION_PLAN T5.2 — translates this app-level injection
    /// container into the runtime's `SubAgentIntegrations` bundle and
    /// applies it via the single `with_subagent_integrations` builder.
    /// The previous chain of 10 individual `with_subagent_*` calls is
    /// gone; the runtime now sees ONE build site, so adding a new
    /// integration field is one struct change instead of two.
    pub fn apply_to(&self, loop_: AgentLoop) -> AgentLoop {
        let integrations = theo_agent_runtime::SubAgentIntegrations {
            registry: self.registry.clone(),
            run_store: self.run_store.clone(),
            hooks: self.hooks.clone(),
            cancellation: self.cancellation.clone(),
            checkpoint: self.checkpoint.clone(),
            worktree: self.worktree.clone(),
            mcp: self.mcp.clone(),
            mcp_discovery: self.mcp_discovery.clone(),
            handoff_guardrails: self.handoff_guardrails.clone(),
            reloadable: self.reloadable.clone(),
            resume_context: None,
        };
        loop_.with_subagent_integrations(integrations)
    }
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
    event_listener: Arc<dyn EventListener>,
) -> Result<AgentResult, RunSessionError> {
    run_agent_session_with_injections(
        config,
        task,
        project_dir,
        event_listener,
        SubagentInjections::default(),
    )
    .await
}

/// Same as `run_agent_session`, but accepts optional sub-agent runtime
/// injections (registry, run_store, hooks, cancellation, checkpoint,
/// worktree) that the surface layer (CLI/Desktop) builds from flags.
pub async fn run_agent_session_with_injections(
    mut config: AgentConfig,
    task: &str,
    project_dir: &Path,
    event_listener: Arc<dyn EventListener>,
    injections: SubagentInjections,
) -> Result<AgentResult, RunSessionError> {
    if config.llm().api_key.is_none() {
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
    // Apply sub-agent injections from CLI flags (Phase 1-13 features).
    agent = injections.apply_to(agent);
    let result = agent.run(task, project_dir).await;

    Ok(result)
}
