//! Builder methods + accessors for [`SubAgentManager`].
//!
//! Fase 4 (REMEDIATION_PLAN T4.5). Extracted from the 1896-LOC
//! `subagent/mod.rs` god-file. Same pattern as `run_engine/builders.rs`:
//! pure `with_*` setters and `&self` accessors in a separate `impl`
//! block that shares access to private fields via parent-scope
//! visibility.

use std::path::PathBuf;
use std::sync::Arc;

use super::{SubAgentManager, SubAgentRegistry};
use crate::config::AgentConfig;
use crate::event_bus::EventBus;

impl SubAgentManager {
    /// Construct with a custom registry.
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
            mcp_discovery: None,
            pending_resume_context: std::sync::Mutex::new(None),
        }
    }

    /// Convenience — builds a default registry (with the 4 builtins).
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

    /// Attach a persistence store for sub-agent runs.
    /// When set, every `spawn_with_spec` persists a `SubagentRun` record.
    pub fn with_run_store(
        mut self,
        store: Arc<crate::subagent_runs::FileSubagentRunStore>,
    ) -> Self {
        self.run_store = Some(store);
        self
    }

    /// Attach a global HookManager. Hooks fire at SubagentStart/Stop.
    pub fn with_hooks(mut self, hooks: Arc<crate::lifecycle_hooks::HookManager>) -> Self {
        self.hook_manager = Some(hooks);
        self
    }

    /// Attach a cancellation tree. spawn_with_spec checks the token
    /// at start (after Started event) and aborts cleanly if cancelled.
    pub fn with_cancellation(
        mut self,
        tree: Arc<crate::cancellation::CancellationTree>,
    ) -> Self {
        self.cancellation = Some(tree);
        self
    }

    /// Attach a checkpoint manager. spawn_with_spec auto-snapshots
    /// the workdir BEFORE the agent loop runs (pre-mutation safety).
    pub fn with_checkpoint(
        mut self,
        manager: Arc<crate::checkpoint::CheckpointManager>,
    ) -> Self {
        self.checkpoint_manager = Some(manager);
        self
    }

    /// Attach a worktree provider. When spec.isolation == "worktree",
    /// spawn_with_spec creates an isolated git worktree, runs the
    /// sub-agent there, and removes the worktree on completion.
    pub fn with_worktree_provider(
        mut self,
        provider: Arc<theo_isolation::WorktreeProvider>,
    ) -> Self {
        self.worktree_provider = Some(provider);
        self
    }

    /// Attach a metrics collector for per-agent breakdown.
    pub fn with_metrics(
        mut self,
        metrics: Arc<crate::observability::metrics::MetricsCollector>,
    ) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Attach an MCP registry. When spec.mcp_servers is non-empty,
    /// the registry is filtered by the allowlist and a hint section is
    /// injected into the sub-agent's system prompt advertising the
    /// available `mcp:server:tool` namespace.
    pub fn with_mcp_registry(mut self, reg: Arc<theo_infra_mcp::McpRegistry>) -> Self {
        self.mcp_registry = Some(reg);
        self
    }

    /// Attach a pre-discovery cache. When attached together with
    /// the registry, sub-agents whose `mcp_servers` allowlist matches
    /// a cached server receive a richer system-prompt hint listing
    /// actual tool names instead of the bare namespace placeholder.
    pub fn with_mcp_discovery(
        mut self,
        cache: Arc<theo_infra_mcp::DiscoveryCache>,
    ) -> Self {
        self.mcp_discovery = Some(cache);
        self
    }

    // -----------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------

    /// Access the MCP registry, if any.
    pub fn mcp_registry(&self) -> Option<&theo_infra_mcp::McpRegistry> {
        self.mcp_registry.as_deref()
    }

    /// Access the MCP discovery cache, if any.
    pub fn mcp_discovery(&self) -> Option<&theo_infra_mcp::DiscoveryCache> {
        self.mcp_discovery.as_deref()
    }

    /// Stage a `ResumeContext` to be consumed by the next
    /// `spawn_with_spec` call. The context is taken (consumed) on
    /// entry to spawn — subsequent spawns get None. Used by
    /// `Resumer::resume_with_objective` to enable replay-mode dispatch
    /// in the spawned `AgentLoop`.
    pub fn set_pending_resume_context(
        &self,
        ctx: Arc<crate::subagent::resume::ResumeContext>,
    ) {
        if let Ok(mut g) = self.pending_resume_context.lock() {
            *g = Some(ctx);
        }
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
    pub fn checkpoint_manager(
        &self,
    ) -> Option<&crate::checkpoint::CheckpointManager> {
        self.checkpoint_manager.as_deref()
    }
}
