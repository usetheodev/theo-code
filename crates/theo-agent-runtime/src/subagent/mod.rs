//! Sub-agent system — delegated autonomous execution.
//!
//! The main agent delegates work to specialized sub-agents via the
//! `delegate_task` tool. Each sub-agent is described by an `AgentSpec`
//! (theo-domain) which carries its system prompt, capability set, model
//! override, hooks, output format, isolation mode, and timeouts.
//!
//! Sub-agent = RunEngine with specialized config. Zero new subsystems.

pub mod approval;
pub mod builtins;
pub mod parser;
pub mod registry;
pub mod reloadable;
pub mod watcher;

pub use reloadable::ReloadableRegistry;

pub use approval::{ApprovalManifest, ApprovalMode, ApprovedEntry};
pub use parser::{parse_agent_spec, ParseError};
pub use registry::{LoadOutcome, RegistryWarning, SubAgentRegistry, WarningKind};

use std::path::PathBuf;
use std::sync::Arc;

use crate::agent_loop::{AgentLoop, AgentResult};
use crate::config::AgentConfig;
use crate::event_bus::EventBus;
use theo_domain::agent_spec::AgentSpec;
use theo_domain::event::{DomainEvent, EventType};
use theo_infra_llm::types::Message;

// ---------------------------------------------------------------------------
// SubAgentManager — orchestrates sub-agent spawning
// ---------------------------------------------------------------------------

/// Maximum sub-agent nesting depth. Sub-agents CANNOT spawn sub-agents.
const MAX_DEPTH: usize = 1;

pub struct SubAgentManager {
    config: AgentConfig,
    event_bus: Arc<EventBus>,
    project_dir: PathBuf,
    depth: usize,
    /// Optional registry for spec-based spawning (Phase 3). If `None`, the
    /// legacy role-based API (`spawn`) is used. The registry is opt-in so
    /// existing call sites don't need updating until Phase 4.
    registry: Option<Arc<SubAgentRegistry>>,
    /// Phase 10: optional persistence store. When Some, every spawn_with_spec
    /// creates a SubagentRun record (started → completed/failed/cancelled)
    /// and appends iteration events. None = no persistence (legacy).
    run_store: Option<Arc<crate::subagent_runs::FileSubagentRunStore>>,
    /// Phase 5: optional global hooks dispatched at SubagentStart/SubagentStop.
    hook_manager: Option<Arc<crate::lifecycle_hooks::HookManager>>,
    /// Phase 6: optional cancellation tree. When Some, spawn_with_spec creates
    /// a child token and bails out early if cancelled before the LLM call.
    cancellation: Option<Arc<crate::cancellation::CancellationTree>>,
    /// Phase 9: optional checkpoint manager. When Some, snapshot the workdir
    /// once at the start of every spawn_with_spec (pre-mutation safety).
    checkpoint_manager: Option<Arc<crate::checkpoint::CheckpointManager>>,
    /// Phase 11: optional worktree provider. When Some AND spec.isolation=="worktree",
    /// spawn_with_spec creates an isolated worktree, runs there, and cleans up.
    worktree_provider: Option<Arc<theo_isolation::WorktreeProvider>>,
    /// Phase 12: optional metrics collector. When Some, spawn_with_spec records
    /// per-agent metrics via MetricsCollector::record_subagent_run.
    metrics: Option<Arc<crate::observability::metrics::MetricsCollector>>,
    /// Phase 8: optional MCP registry. Filtered by spec.mcp_servers (allowlist)
    /// and the resulting hint is injected into the sub-agent's system prompt
    /// so the LLM is aware of MCP tools.
    mcp_registry: Option<Arc<theo_infra_mcp::McpRegistry>>,
}

impl SubAgentManager {
    /// Construct a manager bound to a registry. The registry resolves
    /// `agent_name` lookups (built-in / project / global / on-demand).
    pub fn with_registry(
        config: AgentConfig,
        event_bus: Arc<EventBus>,
        project_dir: PathBuf,
        registry: Arc<SubAgentRegistry>,
    ) -> Self {
        Self {
            config,
            event_bus,
            project_dir,
            depth: 0,
            registry: Some(registry),
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: None,
        }
    }

    /// Phase 3: convenience — builds a default registry (with the 4 builtins).
    /// Drop-in replacement for `new()` that unlocks the spec-based API.
    pub fn with_builtins(
        config: AgentConfig,
        event_bus: Arc<EventBus>,
        project_dir: PathBuf,
    ) -> Self {
        Self::with_registry(
            config,
            event_bus,
            project_dir,
            Arc::new(SubAgentRegistry::with_builtins()),
        )
    }

    /// Phase 10: attach a persistence store for sub-agent runs.
    /// When set, every `spawn_with_spec` persists a `SubagentRun` record.
    pub fn with_run_store(mut self, store: Arc<crate::subagent_runs::FileSubagentRunStore>) -> Self {
        self.run_store = Some(store);
        self
    }

    /// Phase 5: attach a global HookManager. Hooks fire at SubagentStart/Stop.
    pub fn with_hooks(mut self, hooks: Arc<crate::lifecycle_hooks::HookManager>) -> Self {
        self.hook_manager = Some(hooks);
        self
    }

    /// Phase 6: attach a cancellation tree. spawn_with_spec checks the token
    /// at start (after Started event) and aborts cleanly if cancelled.
    pub fn with_cancellation(
        mut self,
        tree: Arc<crate::cancellation::CancellationTree>,
    ) -> Self {
        self.cancellation = Some(tree);
        self
    }

    /// Phase 9: attach a checkpoint manager. spawn_with_spec auto-snapshots
    /// the workdir BEFORE the agent loop runs (pre-mutation safety).
    pub fn with_checkpoint(
        mut self,
        manager: Arc<crate::checkpoint::CheckpointManager>,
    ) -> Self {
        self.checkpoint_manager = Some(manager);
        self
    }

    /// Phase 11: attach a worktree provider. When spec.isolation == "worktree",
    /// spawn_with_spec creates an isolated git worktree, runs the sub-agent
    /// there, and removes the worktree on completion (per CleanupPolicy).
    pub fn with_worktree_provider(
        mut self,
        provider: Arc<theo_isolation::WorktreeProvider>,
    ) -> Self {
        self.worktree_provider = Some(provider);
        self
    }

    /// Phase 12: attach a metrics collector for per-agent breakdown (A4 gap).
    pub fn with_metrics(
        mut self,
        metrics: Arc<crate::observability::metrics::MetricsCollector>,
    ) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Phase 8: attach an MCP registry. When spec.mcp_servers is non-empty,
    /// the registry is filtered by the allowlist and a hint section is
    /// injected into the sub-agent's system prompt advertising the available
    /// `mcp:server:tool` namespace.
    pub fn with_mcp_registry(mut self, reg: Arc<theo_infra_mcp::McpRegistry>) -> Self {
        self.mcp_registry = Some(reg);
        self
    }

    /// Access the MCP registry, if any.
    pub fn mcp_registry(&self) -> Option<&theo_infra_mcp::McpRegistry> {
        self.mcp_registry.as_deref()
    }

    /// Access the registry, if any.
    pub fn registry(&self) -> Option<&SubAgentRegistry> {
        self.registry.as_deref()
    }

    /// Access the persistence store, if any.
    pub fn run_store(&self) -> Option<&crate::subagent_runs::FileSubagentRunStore> {
        self.run_store.as_deref()
    }

    /// Access the global hook manager, if any.
    pub fn hook_manager(&self) -> Option<&crate::lifecycle_hooks::HookManager> {
        self.hook_manager.as_deref()
    }

    /// Access the cancellation tree, if any.
    pub fn cancellation(&self) -> Option<&crate::cancellation::CancellationTree> {
        self.cancellation.as_deref()
    }

    /// Access the checkpoint manager, if any.
    pub fn checkpoint_manager(&self) -> Option<&crate::checkpoint::CheckpointManager> {
        self.checkpoint_manager.as_deref()
    }

    // ---------------------------------------------------------------------
    // Spec-based spawn API (the only API)
    // ---------------------------------------------------------------------

    /// Spawn a sub-agent from an `AgentSpec`.
    ///
    /// Differences vs. legacy `spawn`:
    /// - Uses `spec.system_prompt`, `spec.capability_set`, `spec.max_iterations`,
    ///   `spec.timeout_secs` directly (no hardcoded role match).
    /// - Emits `SubagentStarted` before spawn and `SubagentCompleted` after.
    /// - Populates `AgentResult.agent_name` and `AgentResult.context_used`.
    ///
    /// Backward-compat invariants preserved:
    /// - max_depth=1 enforcement
    /// - Sub-agent config: `is_subagent=true`, capability_set injected
    /// - EventBus forwarding via `PrefixedEventForwarder` (now tagged by `spec.name`)
    pub fn spawn_with_spec(
        &self,
        spec: &AgentSpec,
        objective: &str,
        context: Option<Vec<Message>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AgentResult> + Send + '_>> {
        let spec = spec.clone();
        let objective = objective.to_string();
        let context_text: Option<String> = context.as_ref().and_then(|msgs| {
            msgs.iter()
                .find_map(|m| m.content.as_ref().map(|c| c.to_string()))
        });

        Box::pin(async move {
            // Phase 11: optionally create an isolated worktree
            let worktree_handle = match (&self.worktree_provider, spec.isolation.as_deref()) {
                (Some(provider), Some("worktree")) => {
                    let base = spec
                        .isolation_base_branch
                        .clone()
                        .unwrap_or_else(|| "main".to_string());
                    let result = provider.create(&spec.name, &base).or_else(|_| {
                        // Try "master" fallback (legacy git default)
                        provider.create(&spec.name, "master")
                    });
                    // Phase 5: dispatch WorktreeCreate hook (informational)
                    if let (Ok(handle), Some(hooks)) = (&result, &self.hook_manager) {
                        use crate::lifecycle_hooks::{HookContext, HookEvent};
                        let _ = hooks.dispatch(
                            HookEvent::WorktreeCreate,
                            &HookContext {
                                tool_name: Some(handle.path.to_string_lossy().to_string()),
                                ..Default::default()
                            },
                        );
                    }
                    result.ok()
                }
                _ => None,
            };
            // The CWD the sub-agent will use: worktree path if isolated, else parent's project_dir
            let agent_cwd: PathBuf = worktree_handle
                .as_ref()
                .map(|h| h.path.clone())
                .unwrap_or_else(|| self.project_dir.clone());

            // Phase 9: auto-snapshot the workdir BEFORE the run (pre-mutation safety)
            let checkpoint_before: Option<String> = self
                .checkpoint_manager
                .as_ref()
                .and_then(|cm| {
                    cm.snapshot(&format!("pre-run:{}", spec.name)).ok()
                });

            // Phase 10: persist run start
            let run_id = format!(
                "subagent-{}-{}",
                spec.name,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_micros())
                    .unwrap_or(0)
            );
            if let Some(store) = &self.run_store {
                let run = crate::subagent_runs::SubagentRun::new_running(
                    &run_id,
                    None,
                    &spec,
                    &objective,
                    self.project_dir.to_string_lossy(),
                    checkpoint_before.clone(),
                );
                let _ = store.save(&run);
            }

            // Phase 5: build effective HookManager — per-agent overrides global
            let effective_hooks = build_effective_hooks(&spec, self.hook_manager.as_deref());

            // Phase 5: dispatch SubagentStart hook
            if let Some(hooks) = &effective_hooks {
                use crate::lifecycle_hooks::{HookContext, HookEvent, HookResponse};
                let resp = hooks.dispatch(HookEvent::SubagentStart, &HookContext::default());
                if let HookResponse::Block { reason } = resp {
                    let r = AgentResult {
                        success: false,
                        summary: format!("Sub-agent blocked by SubagentStart hook: {}", reason),
                        agent_name: spec.name.clone(),
                        context_used: context_text.clone(),
                        ..Default::default()
                    };
                    self.publish_completed(&spec, &r);
                    return r;
                }
            }

            // Emit SubagentStarted
            self.event_bus.publish(DomainEvent::new(
                EventType::SubagentStarted,
                format!("subagent:{}", spec.name).as_str(),
                serde_json::json!({
                    "agent_name": spec.name,
                    "agent_source": spec.source.as_str(),
                    "objective": objective,
                    "run_id": run_id,
                    "checkpoint_before": checkpoint_before,
                }),
            ));

            let start = std::time::Instant::now();

            // Phase 6: register child cancellation token (early-bail if root already cancelled)
            let cancellation_token = self
                .cancellation
                .as_ref()
                .map(|tree| tree.child(&run_id));
            if let Some(tok) = &cancellation_token
                && tok.is_cancelled() {
                    let r = AgentResult {
                        success: false,
                        summary: "Sub-agent cancelled before start (parent cancelled)".to_string(),
                        agent_name: spec.name.clone(),
                        context_used: context_text.clone(),
                        duration_ms: start.elapsed().as_millis() as u64,
                        cancelled: true,
                        worktree_path: worktree_handle.as_ref().map(|h| h.path.clone()),
                        ..Default::default()
                    };
                    if let Some(store) = &self.run_store
                        && let Ok(mut run) = store.load(&run_id) {
                            run.status = crate::subagent_runs::RunStatus::Cancelled;
                            run.summary = Some(r.summary.clone());
                            let _ = store.save(&run);
                        }
                    self.publish_completed(&spec, &r);
                    return r;
                }

            // Enforce max_depth
            if self.depth >= MAX_DEPTH {
                let r = AgentResult {
                    success: false,
                    summary: "Sub-agent depth limit reached. Sub-agents cannot spawn sub-agents."
                        .to_string(),
                    agent_name: spec.name.clone(),
                    context_used: context_text.clone(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    ..Default::default()
                };
                // Persist final state for early return path (Phase 10)
                if let Some(store) = &self.run_store
                    && let Ok(mut run) = store.load(&run_id) {
                        run.status = crate::subagent_runs::RunStatus::Failed;
                        run.finished_at = Some(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs() as i64)
                                .unwrap_or(0),
                        );
                        run.summary = Some(r.summary.clone());
                        let _ = store.save(&run);
                    }
                self.publish_completed(&spec, &r);
                return r;
            }

            // Build sub-agent config from spec
            let mut sub_config = self.config.clone();
            sub_config.system_prompt = spec.system_prompt.clone();
            sub_config.max_iterations = spec.max_iterations;
            sub_config.is_subagent = true;
            sub_config.capability_set = Some(spec.capability_set.clone());
            if let Some(m) = &spec.model_override {
                sub_config.model = m.clone();
            }

            // Create sub-agent EventBus with prefixed listener tagged by spec.name
            let sub_bus = Arc::new(crate::event_bus::EventBus::new());
            let prefixed = Arc::new(PrefixedEventForwarder {
                role_name: spec.name.clone(),
                parent_bus: self.event_bus.clone(),
            });
            sub_bus.subscribe(prefixed);

            // Prefix role name + project dir restriction (same format as legacy spawn)
            // If isolated, use the worktree path AND inject Pi-Mono safety rules.
            sub_config.system_prompt = if worktree_handle.is_some() {
                format!(
                    "[{}] {}\n\nIMPORTANT: You MUST only operate within the worktree directory: {}. \
                     Do NOT search, read, or access files outside this directory.\n\n{}",
                    spec.name,
                    sub_config.system_prompt,
                    agent_cwd.display(),
                    theo_isolation::safety_rules(),
                )
            } else {
                format!(
                    "[{}] {}\n\nIMPORTANT: You MUST only operate within the project directory: {}. \
                     Do NOT search, read, or access files outside this directory.",
                    spec.name,
                    sub_config.system_prompt,
                    agent_cwd.display(),
                )
            };

            // Phase 8: MCP integration — when the spec declares mcp_servers
            // and a global MCP registry is attached, filter to the allowlist
            // and inject a prompt hint advertising the mcp:<server>:<tool>
            // namespace.
            if !spec.mcp_servers.is_empty() {
                if let Some(global) = &self.mcp_registry {
                    let filtered = global.filtered(&spec.mcp_servers);
                    let hint = filtered.render_prompt_hint();
                    if !hint.is_empty() {
                        sub_config.system_prompt =
                            format!("{}\n\n{}", sub_config.system_prompt, hint);
                    }
                }
            }

            let registry = theo_tooling::registry::create_default_registry();
            let agent = AgentLoop::new(sub_config, registry);

            let history = context.unwrap_or_default();
            let timeout = std::time::Duration::from_secs(spec.timeout_secs);

            // Phase 6: race the agent against (timeout || cancellation)
            // Phase 11: agent uses worktree path when isolated
            let agent_run = agent.run_with_history(&objective, &agent_cwd, history, Some(sub_bus));
            let mut result = if let Some(tok) = cancellation_token {
                tokio::select! {
                    res = tokio::time::timeout(timeout, agent_run) => match res {
                        Ok(r) => r,
                        Err(_) => AgentResult {
                            success: false,
                            summary: format!(
                                "Sub-agent ({}) timed out after {}s. Objective: {}",
                                spec.name, spec.timeout_secs, objective
                            ),
                            ..Default::default()
                        },
                    },
                    _ = tok.cancelled() => AgentResult {
                        success: false,
                        summary: format!(
                            "Sub-agent ({}) cancelled mid-run by parent",
                            spec.name
                        ),
                        cancelled: true,
                        ..Default::default()
                    },
                }
            } else {
                match tokio::time::timeout(timeout, agent_run).await {
                    Ok(r) => r,
                    Err(_) => AgentResult {
                        success: false,
                        summary: format!(
                            "Sub-agent ({}) timed out after {}s. Objective: {}",
                            spec.name, spec.timeout_secs, objective
                        ),
                        ..Default::default()
                    },
                }
            };

            // Annotate result with spec metadata
            result.agent_name = spec.name.clone();
            result.context_used = context_text;
            result.duration_ms = start.elapsed().as_millis() as u64;
            result.worktree_path = worktree_handle.as_ref().map(|h| h.path.clone());

            // Phase 10: update persisted run with final status + metrics
            if let Some(store) = &self.run_store
                && let Ok(mut run) = store.load(&run_id) {
                    run.status = if result.cancelled {
                        crate::subagent_runs::RunStatus::Cancelled
                    } else if result.success {
                        crate::subagent_runs::RunStatus::Completed
                    } else {
                        crate::subagent_runs::RunStatus::Failed
                    };
                    run.finished_at = Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0),
                    );
                    run.iterations_used = result.iterations_used;
                    run.tokens_used = result.tokens_used;
                    run.summary = Some(result.summary.clone());
                    let _ = store.save(&run);
                }

            // Phase 7: try output format parsing
            if let Some(schema) = &spec.output_format {
                let strict = spec.output_format_strict.unwrap_or(false);
                match crate::output_format::try_parse_structured(&result.summary, schema) {
                    Ok(value) => {
                        result.structured = Some(value.clone());
                        // Phase 10: also persist structured_output if store attached
                        if let Some(store) = &self.run_store
                            && let Ok(mut run) = store.load(&run_id) {
                                run.structured_output = Some(value);
                                let _ = store.save(&run);
                            }
                    }
                    Err(err) => {
                        if strict {
                            // Strict mode: fail the run, append error to summary
                            result.success = false;
                            result.summary = format!(
                                "{}\n\n[output_format strict] {}",
                                result.summary, err
                            );
                        }
                        // best_effort (default): keep free-text, structured=None
                    }
                }
            }

            // Phase 5: dispatch SubagentStop hook (informational; can't cancel
            // — the run already finished). Block here is treated as marking
            // the result with a warning suffix.
            if let Some(hooks) = &effective_hooks {
                use crate::lifecycle_hooks::{HookContext, HookEvent, HookResponse};
                let resp = hooks.dispatch(HookEvent::SubagentStop, &HookContext::default());
                if let HookResponse::Block { reason } = resp {
                    result.summary = format!(
                        "{}\n\n[SubagentStop hook flagged] {}",
                        result.summary, reason
                    );
                }
            }

            // Phase 6: forget the cancellation token (cleanup tree)
            if let Some(tree) = &self.cancellation {
                tree.forget(&run_id);
            }

            // Phase 11: cleanup worktree on success (default policy: OnSuccess).
            // Failures preserve the worktree for inspection.
            if let (Some(handle), Some(provider)) = (&worktree_handle, &self.worktree_provider)
                && result.success {
                    let removed = provider.remove(handle, false).is_ok();
                    // Phase 5: WorktreeRemove hook (informational)
                    if removed
                        && let Some(hooks) = &self.hook_manager {
                            use crate::lifecycle_hooks::{HookContext, HookEvent};
                            let _ = hooks.dispatch(
                                HookEvent::WorktreeRemove,
                                &HookContext {
                                    tool_name: Some(handle.path.to_string_lossy().to_string()),
                                    ..Default::default()
                                },
                            );
                        }
                }

            self.publish_completed(&spec, &result);
            result
        })
    }

    /// Helper: builds user messages from a plain string and delegates to spawn_with_spec.
    pub fn spawn_with_spec_text(
        &self,
        spec: &AgentSpec,
        objective: &str,
        context: Option<&str>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AgentResult> + Send + '_>> {
        let messages = context.map(|c| vec![Message::user(c)]);
        self.spawn_with_spec(spec, objective, messages)
    }

    fn publish_completed(&self, spec: &AgentSpec, result: &AgentResult) {
        self.event_bus.publish(DomainEvent::new(
            EventType::SubagentCompleted,
            format!("subagent:{}", spec.name).as_str(),
            serde_json::json!({
                "agent_name": spec.name,
                "agent_source": spec.source.as_str(),
                "success": result.success,
                "summary": result.summary,
                "duration_ms": result.duration_ms,
                "tokens_used": result.tokens_used,
                "input_tokens": result.input_tokens,
                "output_tokens": result.output_tokens,
                "llm_calls": result.llm_calls,
                "iterations_used": result.iterations_used,
                "cancelled": result.cancelled,
                "worktree_path": result.worktree_path.as_ref().map(|p| p.to_string_lossy().to_string()),
            }),
        ));
        // Phase 12: per-agent metrics aggregation
        if let Some(metrics) = &self.metrics {
            metrics.record_subagent_run(
                &spec.name,
                result.success,
                crate::observability::otel::SubagentRunMetrics {
                    tokens_used: result.tokens_used,
                    input_tokens: result.input_tokens,
                    output_tokens: result.output_tokens,
                    llm_calls: result.llm_calls,
                    iterations_used: result.iterations_used,
                    duration_ms: result.duration_ms,
                },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// build_effective_hooks — Phase 5: per-agent hooks override globals
// ---------------------------------------------------------------------------

/// Merge per-agent hooks (from `spec.hooks`) with global `manager`.
/// Per-agent fires first (higher priority) thanks to `merge_with_priority`.
/// Returns `None` if neither source has hooks.
fn build_effective_hooks(
    spec: &AgentSpec,
    global: Option<&crate::lifecycle_hooks::HookManager>,
) -> Option<crate::lifecycle_hooks::HookManager> {
    let per_agent: Option<crate::lifecycle_hooks::HookManager> = spec
        .hooks
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    match (per_agent, global) {
        (None, None) => None,
        (Some(pa), None) => Some(pa),
        (None, Some(g)) => Some(g.clone()),
        (Some(pa), Some(g)) => {
            let mut merged = g.clone();
            merged.merge_with_priority(pa);
            Some(merged)
        }
    }
}

// ---------------------------------------------------------------------------
// PrefixedEventForwarder — tags sub-agent events with role name
// ---------------------------------------------------------------------------

struct PrefixedEventForwarder {
    role_name: String,
    parent_bus: Arc<EventBus>,
}

impl crate::event_bus::EventListener for PrefixedEventForwarder {
    fn on_event(&self, event: &DomainEvent) {
        // Clone event and add role prefix to entity_id
        let mut tagged = event.clone();
        tagged.entity_id = format!("[{}] {}", self.role_name, tagged.entity_id);
        self.parent_bus.publish(tagged);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use theo_domain::tool::ToolCategory;

    #[test]
    fn builtin_explorer_capability_is_read_only() {
        let spec = builtins::explorer();
        assert!(
            !spec.capability_set
                .can_use_tool("bash", ToolCategory::Execution)
        );
        assert!(
            !spec.capability_set
                .can_use_tool("edit", ToolCategory::FileOps)
        );
        assert!(
            !spec.capability_set
                .can_use_tool("write", ToolCategory::FileOps)
        );
    }

    #[test]
    fn builtin_implementer_capability_is_unrestricted() {
        let spec = builtins::implementer();
        assert!(spec.capability_set.denied_tools.is_empty());
        assert_eq!(
            spec.capability_set.allowed_tools,
            theo_domain::capability::AllowedTools::All
        );
    }

    #[test]
    fn builtin_verifier_cannot_edit_can_bash() {
        let spec = builtins::verifier();
        assert!(spec.capability_set.denied_tools.contains("edit"));
        assert!(spec.capability_set.denied_tools.contains("write"));
        assert!(!spec.capability_set.denied_tools.contains("bash"));
    }

    #[test]
    fn builtin_reviewer_is_read_only() {
        let spec = builtins::reviewer();
        assert!(spec.capability_set.denied_tools.contains("edit"));
        assert!(spec.capability_set.denied_tools.contains("write"));
    }

    #[test]
    fn registry_resolves_builtin_names() {
        let reg = SubAgentRegistry::with_builtins();
        assert!(reg.get("explorer").is_some());
        assert!(reg.get("implementer").is_some());
        assert!(reg.get("verifier").is_some());
        assert!(reg.get("reviewer").is_some());
        assert!(reg.get("unknown").is_none());
    }

    #[test]
    fn spec_based_subagent_config_is_marked() {
        // Verify that sub-agent configs are marked as sub-agents (is_subagent=true)
        // by the spawn_with_spec implementation. Indirect check via clone+set.
        let config = AgentConfig::default();
        assert!(!config.is_subagent, "parent config must not be sub-agent");
        let mut sub_config = config.clone();
        sub_config.is_subagent = true;
        assert!(sub_config.is_subagent, "sub-agent config must be marked");
    }

    #[test]
    fn max_depth_prevents_recursion() {
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1, // Already at max
            registry: Some(Arc::new(SubAgentRegistry::with_builtins())),
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: None,
        };

        let spec = theo_domain::agent_spec::AgentSpec::on_demand("test", "test obj");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { manager.spawn_with_spec(&spec, "test", None).await });
        assert!(!result.success);
        assert!(result.summary.contains("depth limit"));
    }

    // ── Phase 3: spec-based spawn + events ───────────────────────────────

    use crate::event_bus::EventListener;
    use std::sync::Mutex;
    use theo_domain::event::{DomainEvent, EventType};

    /// Test helper: captures events published to the bus.
    struct CaptureListener {
        events: Mutex<Vec<DomainEvent>>,
    }
    impl CaptureListener {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }
        fn events(&self) -> Vec<DomainEvent> {
            self.events.lock().unwrap().clone()
        }
    }
    impl EventListener for CaptureListener {
        fn on_event(&self, e: &DomainEvent) {
            self.events.lock().unwrap().push(e.clone());
        }
    }

    #[test]
    fn with_builtins_preserves_backward_compat_constructor_signature() {
        // Drop-in replacement for `new()`. Legacy call sites work unchanged.
        let bus = Arc::new(EventBus::new());
        let manager =
            SubAgentManager::with_builtins(AgentConfig::default(), bus, PathBuf::from("/tmp"));
        assert!(manager.registry().is_some());
        // Has 4 builtin specs
        assert_eq!(manager.registry().unwrap().len(), 4);
    }

    #[test]
    fn with_registry_uses_provided_registry() {
        let bus = Arc::new(EventBus::new());
        let mut custom = SubAgentRegistry::new();
        custom.register(theo_domain::agent_spec::AgentSpec::on_demand("x", "y"));
        let manager = SubAgentManager::with_registry(
            AgentConfig::default(),
            bus,
            PathBuf::from("/tmp"),
            Arc::new(custom),
        );
        assert_eq!(manager.registry().unwrap().len(), 1);
        assert!(manager.registry().unwrap().contains("x"));
    }

    #[test]
    fn spawn_with_spec_at_max_depth_emits_events_and_fails() {
        let bus = Arc::new(EventBus::new());
        let capture = Arc::new(CaptureListener::new());
        bus.subscribe(capture.clone() as Arc<dyn EventListener>);

        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1,
            registry: Some(Arc::new(SubAgentRegistry::with_builtins())),
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: None,
        };

        let spec = theo_domain::agent_spec::AgentSpec::on_demand("scout", "check x");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { manager.spawn_with_spec(&spec, "check x", None).await });

        // Result reflects the depth-limit failure
        assert!(!result.success);
        assert!(result.summary.contains("depth limit"));
        assert_eq!(result.agent_name, "scout");

        // Events published: SubagentStarted + SubagentCompleted
        let events = capture.events();
        assert!(
            events
                .iter()
                .any(|e| e.event_type == EventType::SubagentStarted),
            "SubagentStarted event missing"
        );
        let completed: Vec<&DomainEvent> = events
            .iter()
            .filter(|e| e.event_type == EventType::SubagentCompleted)
            .collect();
        assert_eq!(completed.len(), 1);
        assert_eq!(
            completed[0].payload.get("agent_name").and_then(|v| v.as_str()),
            Some("scout")
        );
        assert_eq!(
            completed[0].payload.get("agent_source").and_then(|v| v.as_str()),
            Some("on_demand")
        );
        assert_eq!(
            completed[0].payload.get("success").and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn spawn_with_spec_populates_agent_name_and_context() {
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1, // trigger depth-limit early return (no real LLM)
            registry: Some(Arc::new(SubAgentRegistry::with_builtins())),
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: None,
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("x", "y");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            manager
                .spawn_with_spec_text(&spec, "do y", Some("some context"))
                .await
        });
        assert_eq!(result.agent_name, "x");
        assert_eq!(result.context_used.as_deref(), Some("some context"));
    }

    #[test]
    fn spawn_with_spec_with_run_store_persists_run_record() {
        use crate::subagent_runs::FileSubagentRunStore;
        let tempdir = tempfile::TempDir::new().unwrap();
        let store = Arc::new(FileSubagentRunStore::new(tempdir.path()));
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1, // depth-limit early return (no real LLM)
            registry: None,
            run_store: Some(store.clone()),
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: None,
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("persisted", "test");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(async { manager.spawn_with_spec(&spec, "test", None).await });
        let runs = store.list().unwrap();
        assert_eq!(runs.len(), 1);
        let run = store.load(&runs[0]).unwrap();
        assert_eq!(run.agent_name, "persisted");
        // Final status set after early return
        assert!(matches!(
            run.status,
            crate::subagent_runs::RunStatus::Failed | crate::subagent_runs::RunStatus::Completed
        ));
    }

    #[test]
    fn spawn_with_spec_without_run_store_does_not_persist() {
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1,
            registry: None,
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: None,
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("x", "y");
        let rt = tokio::runtime::Runtime::new().unwrap();
        // Should not panic / not require store
        let _ = rt.block_on(async { manager.spawn_with_spec(&spec, "y", None).await });
    }

    #[test]
    fn with_hooks_builder_stores_reference() {
        use crate::lifecycle_hooks::HookManager;
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager::with_builtins(
            AgentConfig::default(),
            bus,
            PathBuf::from("/tmp"),
        )
        .with_hooks(Arc::new(HookManager::new()));
        assert!(manager.hook_manager().is_some());
    }

    #[test]
    fn with_worktree_provider_builder_stores_reference() {
        use std::path::PathBuf;
        let provider = Arc::new(theo_isolation::WorktreeProvider::new(
            PathBuf::from("/repo"),
            PathBuf::from("/wt"),
        ));
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager::with_builtins(
            AgentConfig::default(),
            bus,
            PathBuf::from("/tmp"),
        )
        .with_worktree_provider(provider);
        assert!(manager.worktree_provider.is_some());
    }

    #[test]
    fn with_cancellation_builder_stores_reference() {
        use crate::cancellation::CancellationTree;
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager::with_builtins(
            AgentConfig::default(),
            bus,
            PathBuf::from("/tmp"),
        )
        .with_cancellation(Arc::new(CancellationTree::new()));
        assert!(manager.cancellation().is_some());
    }

    #[test]
    fn spawn_with_spec_blocked_by_subagent_start_hook() {
        use crate::lifecycle_hooks::{HookEvent, HookManager, HookMatcher, HookResponse};
        let bus = Arc::new(EventBus::new());
        let mut hooks = HookManager::new();
        hooks.add(
            HookEvent::SubagentStart,
            HookMatcher {
                matcher: None,
                response: HookResponse::Block {
                    reason: "test block".into(),
                },
                timeout_secs: 60,
            },
        );
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 0,
            registry: None,
            run_store: None,
            hook_manager: Some(Arc::new(hooks)),
            cancellation: None,
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: None,
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("x", "y");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { manager.spawn_with_spec(&spec, "y", None).await });
        assert!(!result.success);
        assert!(result.summary.contains("test block"));
    }

    #[test]
    fn spawn_with_spec_early_cancelled_by_pre_run_cancel() {
        use crate::cancellation::CancellationTree;
        let bus = Arc::new(EventBus::new());
        let tree = Arc::new(CancellationTree::new());
        tree.cancel_all(); // root already cancelled

        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 0,
            registry: None,
            run_store: None,
            hook_manager: None,
            cancellation: Some(tree),
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: None,
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("x", "y");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { manager.spawn_with_spec(&spec, "y", None).await });
        assert!(!result.success);
        assert!(
            result.summary.contains("cancelled before start"),
            "got: {}",
            result.summary
        );
    }

    #[test]
    fn with_run_store_builder_stores_reference() {
        use crate::subagent_runs::FileSubagentRunStore;
        let tempdir = tempfile::TempDir::new().unwrap();
        let store = Arc::new(FileSubagentRunStore::new(tempdir.path()));
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager::with_builtins(
            AgentConfig::default(),
            bus,
            PathBuf::from("/tmp"),
        )
        .with_run_store(store);
        assert!(manager.run_store().is_some());
    }

    #[test]
    fn spawn_with_spec_text_none_context_leaves_context_used_none() {
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1,
            registry: None,
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: None,
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("y", "z");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result =
            rt.block_on(async { manager.spawn_with_spec_text(&spec, "do z", None).await });
        assert!(result.context_used.is_none());
    }
}
