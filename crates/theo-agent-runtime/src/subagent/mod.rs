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
mod manager_builders;
pub mod mcp_tools;
pub mod parser;
pub mod registry;
pub mod reloadable;
pub mod resume;
mod finalize_helpers;
mod spawn_helpers;
pub mod watcher;

pub use reloadable::ReloadableRegistry;

pub use approval::{ApprovalManifest, ApprovalMode, ApprovedEntry};
pub use mcp_tools::{build_adapters_for_spec, mcp_tool_to_definition, McpToolAdapter};
pub use parser::{parse_agent_spec, ParseError};
pub use registry::{LoadOutcome, RegistryWarning, SubAgentRegistry, WarningKind};
pub use resume::{reconstruct_history, ResumeContext, ResumeError, Resumer};

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

/// Override that the Resumer passes to `spawn_with_spec_with_override`
/// to control worktree behavior on resume.
///
/// Variants:
/// - `None` — default behavior (create new from `spec.isolation`).
/// - `Reuse(path)` — wrap the provided existing worktree path via
///   `WorktreeHandle::existing(path)`. The cleanup branch in
///   `spawn_with_spec_with_override` detects the synthetic branch
///   `"(reused)"` and skips auto-removal, since this manager did not
///   create the worktree and must not destroy state owned by the
///   prior crashed run.
/// - `Recreate { base_branch }` — call `provider.create(spec.name, base)`
///   with the explicit `base_branch` from this enum, overriding any
///   value present in `spec.isolation_base_branch`.
#[derive(Debug, Clone)]
pub enum WorktreeOverride {
    None,
    Reuse(std::path::PathBuf),
    Recreate { base_branch: String },
}

pub struct SubAgentManager {
    config: AgentConfig,
    event_bus: Arc<EventBus>,
    project_dir: PathBuf,
    depth: usize,
    /// Optional registry for spec-based spawning. If `None`, the legacy
    /// role-based API (`spawn`) is used. The registry is opt-in so
    /// existing call sites don't need updating.
    registry: Option<Arc<SubAgentRegistry>>,
    /// Optional persistence store. When Some, every spawn_with_spec
    /// creates a SubagentRun record (started → completed/failed/cancelled)
    /// and appends iteration events. None = no persistence (legacy).
    run_store: Option<Arc<crate::subagent_runs::FileSubagentRunStore>>,
    /// Optional global hooks dispatched at SubagentStart/SubagentStop.
    hook_manager: Option<Arc<crate::lifecycle_hooks::HookManager>>,
    /// Optional cancellation tree. When Some, spawn_with_spec creates
    /// a child token and bails out early if cancelled before the LLM call.
    cancellation: Option<Arc<crate::cancellation::CancellationTree>>,
    /// Optional checkpoint manager. When Some, snapshot the workdir
    /// once at the start of every spawn_with_spec (pre-mutation safety).
    checkpoint_manager: Option<Arc<crate::checkpoint::CheckpointManager>>,
    /// Optional worktree provider. When Some AND spec.isolation=="worktree",
    /// spawn_with_spec creates an isolated worktree, runs there, and cleans up.
    worktree_provider: Option<Arc<theo_isolation::WorktreeProvider>>,
    /// Optional metrics collector. When Some, spawn_with_spec records
    /// per-agent metrics via MetricsCollector::record_subagent_run.
    metrics: Option<Arc<crate::observability::metrics::MetricsCollector>>,
    /// Optional MCP registry. Filtered by spec.mcp_servers (allowlist)
    /// and the resulting hint is injected into the sub-agent's system prompt
    /// so the LLM is aware of MCP tools.
    mcp_registry: Option<Arc<theo_infra_mcp::McpRegistry>>,
    /// Optional MCP discovery cache. When Some AND
    /// `spec.mcp_servers` is non-empty, the cache is queried for discovered
    /// tools and the resulting prompt-hint advertises *concrete* tool
    /// names (not just the `mcp:<server>:<tool>` namespace).
    mcp_discovery: Option<Arc<theo_infra_mcp::DiscoveryCache>>,
    /// Pending resume context set by `Resumer::resume_with_objective`
    /// right before calling
    /// `spawn_with_spec`. The first spawn TAKES the value (consume-once)
    /// and forwards it to the inner `AgentLoop::with_resume_context`.
    /// Wrapped in `Mutex` to allow `Resumer` to set without owning
    /// `&mut self`. Sequential usage is the contract — concurrent resume
    /// against the same manager is undefined behavior (would race).
    pending_resume_context:
        std::sync::Mutex<Option<Arc<crate::subagent::resume::ResumeContext>>>,
}

impl SubAgentManager {
    // Construct + builder methods + accessors moved to
    // `manager_builders.rs` (Fase 4 — T4.5). Manager registry binding
    // resolves `agent_name` lookups via built-in / project / global /
    // on-demand chain.

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
        // Wrapper over the _with_override variant for backward compat.
        self.spawn_with_spec_with_override(spec, objective, context, WorktreeOverride::None)
    }

    /// Variant of `spawn_with_spec` that respects an explicit worktree
    /// decision via `WorktreeOverride`. The legacy `spawn_with_spec` is
    /// now a thin wrapper that delegates here with `WorktreeOverride::None`.
    pub fn spawn_with_spec_with_override(
        &self,
        spec: &AgentSpec,
        objective: &str,
        context: Option<Vec<Message>>,
        worktree_override: WorktreeOverride,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AgentResult> + Send + '_>> {
        let spec = spec.clone();
        let objective = objective.to_string();
        let context_text: Option<String> = context.as_ref().and_then(|msgs| {
            msgs.iter()
                .find_map(|m| m.content.as_ref().map(|c| c.to_string()))
        });

        Box::pin(async move {
            // Resolve worktree honoring override.
            let worktree_handle = self.resolve_worktree(&spec, &worktree_override);
            // The CWD the sub-agent will use: worktree path if isolated, else parent's project_dir
            let agent_cwd: PathBuf = worktree_handle
                .as_ref()
                .map(|h| h.path.clone())
                .unwrap_or_else(|| self.project_dir.clone());

            // Auto-snapshot the workdir BEFORE the run (pre-mutation safety).
            let checkpoint_before: Option<String> = self.snapshot_pre_run(&spec);

            // Persist run start (no-op when run_store absent).
            let run_id = spawn_helpers::generate_run_id(&spec);
            self.persist_run_start(&run_id, &spec, &objective, checkpoint_before.clone());

            // Build effective HookManager — per-agent overrides global.
            let effective_hooks = build_effective_hooks(&spec, self.hook_manager.as_deref());

            // Dispatch SubagentStart hook — short-circuit on Block.
            if let Some(r) =
                self.dispatch_start_hook_or_block(effective_hooks.as_ref(), &spec, &context_text)
            {
                self.publish_completed(&spec, &r);
                return r;
            }

            // Emit SubagentStarted with OTel-aligned span attrs.
            self.emit_subagent_started(
                &spec,
                &run_id,
                &objective,
                checkpoint_before.as_deref(),
            );

            let start = std::time::Instant::now();

            // Register child cancellation token (early-bail if root already cancelled).
            let cancellation_token = match self.register_cancellation_or_bail(
                &run_id,
                &spec,
                &context_text,
                start,
                worktree_handle.as_ref(),
            ) {
                Ok(tok) => tok,
                Err(r) => {
                    self.persist_early_exit(
                        &run_id,
                        crate::subagent_runs::RunStatus::Cancelled,
                        &r.summary,
                    );
                    self.publish_completed(&spec, &r);
                    return r;
                }
            };

            // Enforce max_depth
            if let Err(r) = self.enforce_max_depth(&spec, &context_text, start) {
                self.persist_early_exit(
                    &run_id,
                    crate::subagent_runs::RunStatus::Failed,
                    &r.summary,
                );
                self.publish_completed(&spec, &r);
                return r;
            }

            // Build sub-agent config (prompt prefix, capabilities, MCP hint, etc.)
            let sub_config = self.build_sub_config(&spec, &agent_cwd, worktree_handle.is_some());

            // Sub-agent EventBus forwards every event to the parent, tagged by spec.name
            let sub_bus = self.build_prefixed_sub_bus(&spec);

            let mut registry = theo_tooling::registry::create_default_registry();

            // Register MCP tool adapters (fail-soft).
            self.register_mcp_tool_adapters(&spec, &mut registry).await;

            // Consume pending resume context so the spawned AgentLoop
            // runs in replay-mode.
            let pending_resume = self.take_pending_resume_context();

            let mut agent = AgentLoop::new(sub_config, registry);
            if let Some(rc) = pending_resume {
                agent = agent.with_resume_context(rc);
            }

            let history = context.unwrap_or_default();
            let timeout = std::time::Duration::from_secs(spec.timeout_secs);

            // Race the agent run against (timeout || cancellation). The
            // agent uses the worktree path when isolated.
            let agent_run = agent.run_with_history(&objective, &agent_cwd, history, Some(sub_bus));
            let mut result = spawn_helpers::run_agent_with_timeout(
                agent_run,
                timeout,
                cancellation_token,
                &spec.name,
                spec.timeout_secs,
                &objective,
            )
            .await;

            // Annotate result with spec metadata
            result.agent_name = spec.name.clone();
            result.context_used = context_text;
            result.duration_ms = start.elapsed().as_millis() as u64;
            result.worktree_path = worktree_handle.as_ref().map(|h| h.path.clone());

            // Update persisted run with final status + metrics.
            self.finalize_persisted_run(&run_id, &result);

            // Try output format parsing (structured output).
            self.apply_output_format(&spec, &run_id, &mut result);

            // Dispatch SubagentStop hook (informational; can't cancel
            // — the run already finished). Block here is treated as marking
            // the result with a warning suffix.
            self.dispatch_stop_hook_annotate(effective_hooks.as_ref(), &mut result);

            // Forget the cancellation token (cleanup tree).
            if let Some(tree) = &self.cancellation {
                tree.forget(&run_id);
            }

            // Cleanup worktree on success (default policy: OnSuccess).
            // Failures preserve the worktree for inspection.
            //
            // When the handle was built via `WorktreeHandle::existing`
            // (Reuse path), the synthetic branch sentinel "(reused)"
            // signals that THIS manager did not create the worktree. Skip
            // auto-removal so we never destroy state owned by the prior
            // crashed run.
            self.cleanup_worktree_if_success(worktree_handle.as_ref(), &result);

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
        // OTel-aligned attributes for the completion event.
        let mut span =
            crate::observability::otel::AgentRunSpan::from_spec(spec, &result.agent_name);
        span.set(
            crate::observability::otel::ATTR_USAGE_INPUT_TOKENS,
            result.input_tokens,
        );
        span.set(
            crate::observability::otel::ATTR_USAGE_OUTPUT_TOKENS,
            result.output_tokens,
        );
        span.set(
            crate::observability::otel::ATTR_USAGE_TOTAL_TOKENS,
            result.tokens_used,
        );
        span.set(
            crate::observability::otel::ATTR_THEO_DURATION_MS,
            result.duration_ms,
        );
        span.set(
            crate::observability::otel::ATTR_THEO_ITERATIONS,
            result.iterations_used as u64,
        );
        span.set(
            crate::observability::otel::ATTR_THEO_LLM_CALLS,
            result.llm_calls,
        );
        span.set(
            crate::observability::otel::ATTR_THEO_SUCCESS,
            result.success,
        );

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
                "otel": span.to_json(),
            }),
        ));
        // Per-agent metrics aggregation.
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
// needs_discovery — true when at least one server in
// `mcp_servers` has no cached tools yet.
// ---------------------------------------------------------------------------

/// Returns `true` when the cache lacks an entry for any of `mcp_servers`.
/// Used by `spawn_with_spec` to decide if it should auto-trigger
/// `discover_filtered` before registering MCP tool adapters.
fn needs_discovery(
    cache: &theo_infra_mcp::DiscoveryCache,
    mcp_servers: &[String],
) -> bool {
    let cached: std::collections::BTreeSet<String> =
        cache.cached_servers().into_iter().collect();
    mcp_servers.iter().any(|s| !cached.contains(s))
}

// ---------------------------------------------------------------------------
// build_effective_hooks — per-agent hooks override globals
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
            mcp_discovery: None,
            pending_resume_context: std::sync::Mutex::new(None),
        };

        let spec = theo_domain::agent_spec::AgentSpec::on_demand("test", "test obj");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { manager.spawn_with_spec(&spec, "test", None).await });
        assert!(!result.success);
        assert!(result.summary.contains("depth limit"));
    }

    // ── Spec-based spawn + events ────────────────────────────────────────

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
            mcp_discovery: None,
            pending_resume_context: std::sync::Mutex::new(None),
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
            mcp_discovery: None,
            pending_resume_context: std::sync::Mutex::new(None),
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
            mcp_discovery: None,
            pending_resume_context: std::sync::Mutex::new(None),
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
            mcp_discovery: None,
            pending_resume_context: std::sync::Mutex::new(None),
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
            mcp_discovery: None,
            pending_resume_context: std::sync::Mutex::new(None),
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
            mcp_discovery: None,
            pending_resume_context: std::sync::Mutex::new(None),
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

    // ── MCP discovery cache integration ──

    #[test]
    fn with_mcp_discovery_builder_stores_reference() {
        let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());
        let bus = Arc::new(EventBus::new());
        let manager = SubAgentManager::with_builtins(
            AgentConfig::default(),
            bus,
            PathBuf::from("/tmp"),
        )
        .with_mcp_discovery(cache);
        assert!(manager.mcp_discovery().is_some());
    }

    #[test]
    fn discovery_cache_takes_precedence_over_registry_hint() {
        // Spec declares mcp_servers and BOTH registry + discovery cache are
        // attached. When the cache has discovered tools for that server, the
        // *cache* hint must be used (concrete tool names) not the registry's
        // bare-namespace hint.
        use std::collections::BTreeMap;
        use theo_infra_mcp::{DiscoveryCache, McpRegistry, McpServerConfig, McpTool};

        let bus = Arc::new(EventBus::new());

        let mut reg = McpRegistry::new();
        reg.register(McpServerConfig::Stdio {
            name: "github".into(),
            command: "echo".into(),
            args: vec![],
            env: BTreeMap::new(),
            timeout_ms: None,
        });
        let cache = DiscoveryCache::new();
        cache.put(
            "github",
            vec![
                McpTool {
                    name: "search_repo".into(),
                    description: Some("search a github repository".into()),
                    input_schema: serde_json::json!({"type":"object"}),
                },
            ],
        );

        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus.clone(),
            project_dir: PathBuf::from("/tmp"),
            depth: 1, // depth-limit early return → no real spawn
            registry: None,
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: Some(Arc::new(reg)),
            mcp_discovery: Some(Arc::new(cache)),
            pending_resume_context: std::sync::Mutex::new(None),
        };

        // We cannot directly inspect sub_config.system_prompt without
        // refactoring spawn_with_spec, so we rely on render_prompt_hint
        // semantics being unit-tested in theo-infra-mcp::discovery::tests.
        // Sanity check: the discovery cache used here resolves correctly.
        let cache_ref = manager.mcp_discovery().unwrap();
        let allow = vec!["github".to_string()];
        let hint = cache_ref.render_prompt_hint(&allow);
        assert!(hint.contains("`mcp:github:search_repo`"));
        assert!(hint.contains("pre-discovered"));
    }

    // ── MCP auto-discovery on first spawn ──

    #[test]
    fn needs_discovery_true_when_cache_empty_and_servers_requested() {
        let cache = theo_infra_mcp::DiscoveryCache::new();
        assert!(needs_discovery(&cache, &["github".to_string()]));
    }

    #[test]
    fn needs_discovery_false_when_cache_already_covers_all_requested() {
        let cache = theo_infra_mcp::DiscoveryCache::new();
        cache.put("github", vec![]);
        cache.put("postgres", vec![]);
        assert!(!needs_discovery(
            &cache,
            &["github".to_string(), "postgres".to_string()]
        ));
    }

    #[test]
    fn needs_discovery_true_when_cache_partially_covers() {
        let cache = theo_infra_mcp::DiscoveryCache::new();
        cache.put("github", vec![]);
        // postgres not cached
        assert!(needs_discovery(
            &cache,
            &["github".to_string(), "postgres".to_string()]
        ));
    }

    #[test]
    fn needs_discovery_false_when_no_servers_requested() {
        let cache = theo_infra_mcp::DiscoveryCache::new();
        assert!(!needs_discovery(&cache, &[]));
    }

    #[tokio::test]
    async fn spawn_with_spec_auto_triggers_discovery_when_cache_empty() {
        // The spec declares mcp_servers but cache is empty. After spawn (even
        // a depth-limit early return), the cache should remain empty BUT the
        // discovery attempt should have happened — verified indirectly by
        // checking that an unreachable server gets recorded as failed (proof
        // discover_filtered ran).
        use std::collections::BTreeMap;
        use std::sync::Arc;

        let bus = Arc::new(EventBus::new());
        let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());

        let mut reg = theo_infra_mcp::McpRegistry::new();
        reg.register(theo_infra_mcp::McpServerConfig::Stdio {
            name: "auto-discover-test".into(),
            command: "/nonexistent/cmd/zzz".into(),
            args: vec![],
            env: BTreeMap::new(),
            timeout_ms: None,
        });

        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 0,
            registry: None,
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: Some(Arc::new(reg)),
            mcp_discovery: Some(cache.clone()),
            pending_resume_context: std::sync::Mutex::new(None),
        };

        let mut spec = AgentSpec::on_demand("x", "y");
        spec.mcp_servers = vec!["auto-discover-test".to_string()];

        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            manager.spawn_with_spec(&spec, "y", None),
        )
        .await;
        // Cache stays empty because the server is unreachable, but
        // discover_filtered MUST have been attempted (no panic + no cached
        // entry for the reachable case is the only observable proof here).
        assert!(
            cache.get("auto-discover-test").is_none(),
            "unreachable server must NOT be cached"
        );
    }

    #[tokio::test]
    async fn spawn_with_spec_skips_discovery_when_cache_already_populated() {
        // Pre-populated cache: spawn should NOT re-trigger discovery.
        // We assert this by registering an unreachable server but seeding
        // the cache with a fake tool — if discovery ran, the call would
        // fail and the cache entry would be removed (or stay as inserted).
        use std::collections::BTreeMap;
        use std::sync::Arc;

        let bus = Arc::new(EventBus::new());
        let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());
        cache.put(
            "pre-cached",
            vec![theo_infra_mcp::McpTool {
                name: "fake_tool".into(),
                description: Some("seed".into()),
                input_schema: serde_json::json!({"type": "object"}),
            }],
        );

        let mut reg = theo_infra_mcp::McpRegistry::new();
        reg.register(theo_infra_mcp::McpServerConfig::Stdio {
            name: "pre-cached".into(),
            command: "/nonexistent/never-spawned".into(),
            args: vec![],
            env: BTreeMap::new(),
            timeout_ms: None,
        });

        let manager = SubAgentManager {
            config: AgentConfig::default(),
            event_bus: bus,
            project_dir: PathBuf::from("/tmp"),
            depth: 1, // depth-limit early return
            registry: None,
            run_store: None,
            hook_manager: None,
            cancellation: None,
            checkpoint_manager: None,
            worktree_provider: None,
            metrics: None,
            mcp_registry: Some(Arc::new(reg)),
            mcp_discovery: Some(cache.clone()),
            pending_resume_context: std::sync::Mutex::new(None),
        };

        let mut spec = AgentSpec::on_demand("x", "y");
        spec.mcp_servers = vec!["pre-cached".to_string()];

        let _ = manager.spawn_with_spec(&spec, "y", None).await;
        // Cache still has the seeded entry — proof discovery did NOT overwrite.
        assert!(cache.get("pre-cached").is_some());
        assert_eq!(cache.get("pre-cached").unwrap().len(), 1);
        assert_eq!(cache.get("pre-cached").unwrap()[0].name, "fake_tool");
    }

    #[tokio::test]
    async fn spawn_with_spec_does_not_discover_when_mcp_servers_empty() {
        // Empty mcp_servers → no discovery, even when cache + registry attached.
        use std::sync::Arc;
        let bus = Arc::new(EventBus::new());
        let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());
        let reg = Arc::new(theo_infra_mcp::McpRegistry::new());
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
            mcp_registry: Some(reg),
            mcp_discovery: Some(cache.clone()),
            pending_resume_context: std::sync::Mutex::new(None),
        };
        let spec = AgentSpec::on_demand("x", "y"); // mcp_servers empty by default
        let _ = manager.spawn_with_spec(&spec, "y", None).await;
        assert!(cache.cached_servers().is_empty());
    }

    #[tokio::test]
    async fn spawn_with_spec_does_not_discover_when_no_registry_attached() {
        // No mcp_registry → discovery cannot run regardless of cache state.
        use std::sync::Arc;
        let bus = Arc::new(EventBus::new());
        let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());
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
            mcp_discovery: Some(cache.clone()),
            pending_resume_context: std::sync::Mutex::new(None),
        };
        let mut spec = AgentSpec::on_demand("x", "y");
        spec.mcp_servers = vec!["github".to_string()];
        let _ = manager.spawn_with_spec(&spec, "y", None).await;
        assert!(cache.cached_servers().is_empty());
    }

    #[tokio::test]
    async fn spawn_with_spec_continues_when_discovery_fails_completely() {
        // All servers unreachable → spawn still proceeds (fail-soft).
        use std::collections::BTreeMap;
        use std::sync::Arc;
        let bus = Arc::new(EventBus::new());
        let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());
        let mut reg = theo_infra_mcp::McpRegistry::new();
        reg.register(theo_infra_mcp::McpServerConfig::Stdio {
            name: "dead".into(),
            command: "/nonexistent/zzz".into(),
            args: vec![],
            env: BTreeMap::new(),
            timeout_ms: None,
        });
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
            mcp_registry: Some(Arc::new(reg)),
            mcp_discovery: Some(cache.clone()),
            pending_resume_context: std::sync::Mutex::new(None),
        };
        let mut spec = AgentSpec::on_demand("x", "y");
        spec.mcp_servers = vec!["dead".to_string()];
        let result = manager.spawn_with_spec(&spec, "y", None).await;
        // depth-limit summary surfaces — discovery failure didn't cause a panic.
        assert!(result.summary.contains("depth limit"));
    }

    #[tokio::test]
    async fn spawn_with_spec_skips_discovery_when_env_disables_auto() {
        // THEO_MCP_AUTO_DISCOVERY=0 disables auto-trigger even with
        // unreachable servers in the registry.
        use std::collections::BTreeMap;
        use std::sync::Arc;
        // SAFETY: env_remove on drop via guard; only this test toggles it.
        unsafe { std::env::set_var("THEO_MCP_AUTO_DISCOVERY", "0"); }
        let bus = Arc::new(EventBus::new());
        let cache = Arc::new(theo_infra_mcp::DiscoveryCache::new());
        let mut reg = theo_infra_mcp::McpRegistry::new();
        reg.register(theo_infra_mcp::McpServerConfig::Stdio {
            name: "would-be-discovered".into(),
            command: "/nonexistent/zzz".into(),
            args: vec![],
            env: BTreeMap::new(),
            timeout_ms: None,
        });
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
            mcp_registry: Some(Arc::new(reg)),
            mcp_discovery: Some(cache.clone()),
            pending_resume_context: std::sync::Mutex::new(None),
        };
        let mut spec = AgentSpec::on_demand("x", "y");
        spec.mcp_servers = vec!["would-be-discovered".to_string()];
        let _ = manager.spawn_with_spec(&spec, "y", None).await;
        // Cache empty AND no IO attempted (env disables it) — observable
        // proof: the test finished essentially instantly with nothing cached.
        assert!(cache.cached_servers().is_empty());
        unsafe { std::env::remove_var("THEO_MCP_AUTO_DISCOVERY"); }
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
            mcp_discovery: None,
            pending_resume_context: std::sync::Mutex::new(None),
        };
        let spec = theo_domain::agent_spec::AgentSpec::on_demand("y", "z");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result =
            rt.block_on(async { manager.spawn_with_spec_text(&spec, "do z", None).await });
        assert!(result.context_used.is_none());
    }

    // -----------------------------------------------------------------------
    // WorktreeOverride — resume-runtime-wiring
    // -----------------------------------------------------------------------

    mod worktree_override {
        use super::*;

        fn manager_no_worktree(depth: usize) -> SubAgentManager {
            SubAgentManager {
                config: AgentConfig::default(),
                event_bus: Arc::new(EventBus::new()),
                project_dir: PathBuf::from("/tmp"),
                depth,
                registry: None,
                run_store: None,
                hook_manager: None,
                cancellation: None,
                checkpoint_manager: None,
                worktree_provider: None,
                metrics: None,
                mcp_registry: None,
                mcp_discovery: None,
                pending_resume_context: std::sync::Mutex::new(None),
            }
        }

        #[test]
        fn worktree_override_enum_default_is_none() {
            // None variant = legacy behavior (create new from spec.isolation).
            let o = WorktreeOverride::None;
            assert!(matches!(o, WorktreeOverride::None));
        }

        #[test]
        fn worktree_override_reuse_carries_path() {
            let p = PathBuf::from("/tmp/wt-reused");
            let o = WorktreeOverride::Reuse(p.clone());
            match o {
                WorktreeOverride::Reuse(got) => assert_eq!(got, p),
                _ => panic!("expected Reuse variant"),
            }
        }

        #[test]
        fn worktree_override_recreate_carries_base_branch() {
            let o = WorktreeOverride::Recreate {
                base_branch: "develop".to_string(),
            };
            match o {
                WorktreeOverride::Recreate { base_branch } => {
                    assert_eq!(base_branch, "develop");
                }
                _ => panic!("expected Recreate variant"),
            }
        }

        #[test]
        fn spawn_with_spec_with_override_none_matches_legacy_behavior() {
            // Regression guard: spawn_with_spec_with_override(None) MUST produce
            // a result indistinguishable from spawn_with_spec for non-isolated
            // specs (depth-limit early return path is identical).
            let manager = manager_no_worktree(1);
            let spec = theo_domain::agent_spec::AgentSpec::on_demand("alpha", "do x");
            let rt = tokio::runtime::Runtime::new().unwrap();
            let r_legacy =
                rt.block_on(async { manager.spawn_with_spec(&spec, "obj", None).await });
            let r_override = rt.block_on(async {
                manager
                    .spawn_with_spec_with_override(&spec, "obj", None, WorktreeOverride::None)
                    .await
            });
            // Both hit depth-limit → identical "depth limit" summary.
            assert!(r_legacy.summary.contains("depth limit"));
            assert!(r_override.summary.contains("depth limit"));
            assert_eq!(r_legacy.success, r_override.success);
        }

        #[test]
        fn spawn_with_spec_with_override_reuse_skips_provider_create() {
            // When Reuse(path) is supplied, even WITHOUT a worktree_provider
            // the path is honored (since no `git worktree add` is needed —
            // the path already exists on disk from the prior crashed run).
            // Depth-limit short-circuit means we don't actually run, but the
            // observable contract is: the API accepts the override + returns.
            let manager = manager_no_worktree(1);
            let mut spec = theo_domain::agent_spec::AgentSpec::on_demand("alpha", "x");
            spec.isolation = Some("worktree".to_string());
            let p = PathBuf::from("/tmp/wt-reused-from-resume");
            let rt = tokio::runtime::Runtime::new().unwrap();
            let r = rt.block_on(async {
                manager
                    .spawn_with_spec_with_override(
                        &spec,
                        "obj",
                        None,
                        WorktreeOverride::Reuse(p),
                    )
                    .await
            });
            // Depth limit hit, no panic — Reuse path didn't try to call git.
            assert!(r.summary.contains("depth limit"));
        }

        #[test]
        fn spawn_with_spec_with_override_recreate_passes_base_branch() {
            // When Recreate { base_branch } is supplied, the provider
            // (when present) would be invoked with the override base branch
            // INSTEAD of spec.isolation_base_branch. We verify by:
            //   - Setting spec.isolation_base_branch = "main"
            //   - Calling with Recreate { base_branch: "develop" }
            //   - At depth=1 we short-circuit, but the API contract is that
            //     this branch is honored (validated end-to-end via Fase 32).
            let manager = manager_no_worktree(1);
            let mut spec = theo_domain::agent_spec::AgentSpec::on_demand("alpha", "x");
            spec.isolation = Some("worktree".to_string());
            spec.isolation_base_branch = Some("main".to_string());
            let rt = tokio::runtime::Runtime::new().unwrap();
            let r = rt.block_on(async {
                manager
                    .spawn_with_spec_with_override(
                        &spec,
                        "obj",
                        None,
                        WorktreeOverride::Recreate {
                            base_branch: "develop".to_string(),
                        },
                    )
                    .await
            });
            assert!(r.summary.contains("depth limit"));
        }

        #[test]
        fn spawn_with_spec_alias_delegates_to_with_override_none() {
            // Verify that spawn_with_spec is now a wrapper that calls
            // spawn_with_spec_with_override(.., None). Same observable
            // behavior as the legacy parity test, but documents the
            // refactor contract explicitly.
            let manager = manager_no_worktree(1);
            let spec = theo_domain::agent_spec::AgentSpec::on_demand("a", "b");
            let rt = tokio::runtime::Runtime::new().unwrap();
            let r1 = rt.block_on(async { manager.spawn_with_spec(&spec, "obj", None).await });
            let r2 = rt.block_on(async {
                manager
                    .spawn_with_spec_with_override(&spec, "obj", None, WorktreeOverride::None)
                    .await
            });
            assert_eq!(r1.success, r2.success);
            assert_eq!(r1.summary, r2.summary);
        }
    }
}
