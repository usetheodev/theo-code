use std::path::Path;
use std::sync::Arc;

use theo_agent_runtime::event_bus::EventListener;
use theo_agent_runtime::{AgentConfig, AgentLoop, AgentResult};
use theo_domain::graph_context::GraphContextProvider;
use theo_tooling::registry::create_default_registry_with_project;

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

    // T15.1 — populate docs_search index from project's well-known
    // Markdown locations (docs/, .theo/wiki/, ~/.cache/theo/docs/)
    // so the agent has searchable docs out of the box.
    let registry = create_default_registry_with_project(project_dir);
    let mut agent = AgentLoop::new(config, registry).with_event_listener(event_listener);
    if let Some(gc) = graph_context {
        agent = agent.with_graph_context(gc);
    }

    // T14.1 — when THEO_PROGRESS_STDERR=1 is set (headless ops,
    // benchmark visibility, CI smoke tests), wire the partial-
    // progress drainer to stderr. The TUI integration in
    // apps/theo-cli/src/tui/* will eventually replace stderr with
    // an in-place status line update; this stderr fallback lets the
    // wire be exercised end-to-end RIGHT NOW without touching the
    // TUI render loop.
    let progress_drainer = if env_progress_stderr_enabled() {
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(64);
        agent = agent.with_partial_progress_tx(tx);
        let handle = tokio::spawn(stderr_progress_drainer(rx));
        Some(handle)
    } else {
        None
    };

    // Apply sub-agent injections from CLI flags (Phase 1-13 features).
    agent = injections.apply_to(agent);
    let result = agent.run(task, project_dir).await;

    // Wait for the drainer to finish flushing remaining frames.
    // The agent loop has already exited, so the sender has dropped
    // (when its last clone goes), and the drainer returns naturally.
    if let Some(handle) = progress_drainer {
        let _ = handle.await;
    }

    Ok(result)
}

/// Returns true when the operator opted into stderr progress dumps.
/// Truthy values: `1`, `true` (case-insensitive); empty / unset =
/// false; any other non-empty value also truthy (matches the
/// permissive convention used by `THEO_NO_GRAPHCTX`).
fn env_progress_stderr_enabled() -> bool {
    match std::env::var("THEO_PROGRESS_STDERR") {
        Ok(v) => !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"),
        Err(_) => false,
    }
}

/// Default drainer used when `THEO_PROGRESS_STDERR=1`. Writes one
/// `[partial] tool: content [42%]` line to stderr per debounced
/// frame. Exits when the sender drops (agent loop exit).
async fn stderr_progress_drainer(rx: tokio::sync::mpsc::Receiver<String>) {
    use std::time::Duration;

    use tokio::time::Instant;

    // Inline copy of apps/theo-cli's run_drainer logic to avoid the
    // arch-contract violation (theo-application can't depend on
    // theo-cli). Same 50ms debounce + latest-wins-per-tool.
    const DEBOUNCE: Duration = Duration::from_millis(50);
    let mut rx = rx;
    loop {
        let first = match rx.recv().await {
            Some(line) => line,
            None => return,
        };
        let mut latest: std::collections::HashMap<String, (String, Option<f64>)> =
            std::collections::HashMap::new();
        absorb(&first, &mut latest);
        let deadline = Instant::now() + DEBOUNCE;
        loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            if timeout.is_zero() {
                break;
            }
            match tokio::time::timeout(timeout, rx.recv()).await {
                Ok(Some(line)) => absorb(&line, &mut latest),
                Ok(None) => {
                    flush(&latest);
                    return;
                }
                Err(_elapsed) => break,
            }
        }
        flush(&latest);
    }
}

fn absorb(
    line: &str,
    latest: &mut std::collections::HashMap<String, (String, Option<f64>)>,
) {
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return, // malformed — silently skip
    };
    if v.get("type").and_then(|t| t.as_str()) != Some("partial") {
        return;
    }
    let tool = v
        .get("tool")
        .and_then(|t| t.as_str())
        .unwrap_or("?")
        .to_string();
    let content = v
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let progress = v.get("progress").and_then(|p| p.as_f64());
    latest.insert(tool, (content, progress));
}

fn flush(latest: &std::collections::HashMap<String, (String, Option<f64>)>) {
    let mut keys: Vec<&String> = latest.keys().collect();
    keys.sort();
    for k in keys {
        let (content, progress) = &latest[k];
        match progress {
            Some(p) => {
                let pct = (p * 100.0).round().clamp(0.0, 100.0) as u32;
                eprintln!("[partial] {k}: {content} [{pct}%]");
            }
            None => {
                eprintln!("[partial] {k}: {content}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::Duration;

    #[test]
    fn t141_env_progress_stderr_disabled_by_default() {
        // Save / restore so we don't poison sibling tests.
        let prev = std::env::var_os("THEO_PROGRESS_STDERR");
        // SAFETY: test mutates a uniquely-named env var; the test runs single-threaded under cargo test's per-test isolation, no concurrent reader exists.
        unsafe {
            std::env::remove_var("THEO_PROGRESS_STDERR");
        }
        assert!(!env_progress_stderr_enabled());
        if let Some(v) = prev {
            // SAFETY: test mutates a uniquely-named env var; the test runs single-threaded under cargo test's per-test isolation, no concurrent reader exists.
            unsafe {
                std::env::set_var("THEO_PROGRESS_STDERR", v);
            }
        }
    }

    #[test]
    fn t141_env_progress_stderr_recognises_truthy_values() {
        let prev = std::env::var_os("THEO_PROGRESS_STDERR");
        // SAFETY: test mutates a uniquely-named env var; the test runs single-threaded under cargo test's per-test isolation, no concurrent reader exists.
        unsafe {
            for v in ["1", "true", "TRUE", "yes", "on"] {
                std::env::set_var("THEO_PROGRESS_STDERR", v);
                assert!(
                    env_progress_stderr_enabled(),
                    "{v} should be recognised as truthy"
                );
            }
            for v in ["0", "false", "FALSE", ""] {
                std::env::set_var("THEO_PROGRESS_STDERR", v);
                assert!(
                    !env_progress_stderr_enabled(),
                    "{v} should be recognised as falsy"
                );
            }
            std::env::remove_var("THEO_PROGRESS_STDERR");
            if let Some(v) = prev {
                std::env::set_var("THEO_PROGRESS_STDERR", v);
            }
        }
    }

    // ── absorb / flush — pure logic tests ─────────────────────────

    #[test]
    fn t141_absorb_parses_full_envelope() {
        let mut latest: HashMap<String, (String, Option<f64>)> = HashMap::new();
        let line = r#"{"type":"partial","tool":"a","content":"loading","progress":0.5}"#;
        absorb(line, &mut latest);
        assert_eq!(latest.len(), 1);
        let (content, progress) = &latest["a"];
        assert_eq!(content, "loading");
        assert_eq!(*progress, Some(0.5));
    }

    #[test]
    fn t141_absorb_silently_skips_malformed_json() {
        let mut latest: HashMap<String, (String, Option<f64>)> = HashMap::new();
        absorb("not json", &mut latest);
        absorb(r#"{"random": "shape"}"#, &mut latest);
        absorb(r#"{"type":"final","tool":"x","content":"y"}"#, &mut latest);
        assert!(
            latest.is_empty(),
            "absorb must skip malformed lines silently"
        );
    }

    #[test]
    fn t141_absorb_latest_wins_per_tool() {
        let mut latest: HashMap<String, (String, Option<f64>)> = HashMap::new();
        absorb(
            r#"{"type":"partial","tool":"a","content":"first"}"#,
            &mut latest,
        );
        absorb(
            r#"{"type":"partial","tool":"a","content":"second","progress":0.9}"#,
            &mut latest,
        );
        let (content, progress) = &latest["a"];
        assert_eq!(content, "second", "latest content wins");
        assert_eq!(*progress, Some(0.9));
    }

    // ── stderr_progress_drainer end-to-end ────────────────────────

    #[tokio::test]
    async fn t141_stderr_drainer_returns_when_sender_drops() {
        // The drainer must NOT hang the run_agent_session shutdown.
        // Send 0 events, drop the sender, await the drainer with a
        // short timeout — it should resolve quickly.
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(8);
        drop(tx); // close immediately
        let result = tokio::time::timeout(
            Duration::from_millis(200),
            stderr_progress_drainer(rx),
        )
        .await;
        assert!(result.is_ok(), "drainer must return promptly on close");
    }

    #[tokio::test]
    async fn t141_stderr_drainer_processes_events_then_exits_on_close() {
        // Smoke: feed some envelopes, drop the sender, drainer
        // processes the in-flight burst and exits within the
        // debounce window + a small slack.
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(8);
        tx.send(r#"{"type":"partial","tool":"a","content":"x"}"#.to_string())
            .await
            .unwrap();
        tx.send(r#"{"type":"partial","tool":"b","content":"y"}"#.to_string())
            .await
            .unwrap();
        drop(tx);
        let result = tokio::time::timeout(
            Duration::from_millis(300),
            stderr_progress_drainer(rx),
        )
        .await;
        assert!(result.is_ok());
    }
}
