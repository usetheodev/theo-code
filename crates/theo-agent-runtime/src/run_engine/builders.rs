//! Builder methods (`with_*`) for `AgentRunEngine`.
//!
//! Extracted from `run_engine.rs` (Fase 4 — REMEDIATION_PLAN T4.2) into
//! a separate `impl AgentRunEngine` block. All methods here are pure
//! setters that return `Self` — they do not touch the hot execution
//! path. Keeping them here shrinks the main file by ~115 LOC and
//! makes the core state-transition code easier to review.

use std::sync::Arc;

use crate::config::MessageQueues;
use crate::persistence::SnapshotStore;

use super::AgentRunEngine;

impl AgentRunEngine {
    /// Inject the SubAgentRegistry. Used by delegate_task to look up
    /// named agents (built-in / project / global). When `None`, a default
    /// registry with builtins is constructed on each delegate_task call.
    pub fn with_subagent_registry(
        mut self,
        registry: Arc<crate::subagent::SubAgentRegistry>,
    ) -> Self {
        self.subagent.registry = Some(registry);
        self
    }

    /// Inject session persistence store. When set, sub-agent runs
    /// are persisted in `<base>/runs/{run_id}.json`.
    pub fn with_subagent_run_store(
        mut self,
        store: Arc<crate::subagent_runs::FileSubagentRunStore>,
    ) -> Self {
        self.subagent.run_store = Some(store);
        self
    }

    /// Inject global hooks (per-agent hooks merged via spec.hooks).
    pub fn with_subagent_hooks(
        mut self,
        hooks: Arc<crate::lifecycle_hooks::HookManager>,
    ) -> Self {
        self.subagent.hooks = Some(hooks);
        self
    }

    /// Inject cancellation tree. Sub-agents register children;
    /// root cancellation propagates.
    pub fn with_subagent_cancellation(
        mut self,
        tree: Arc<crate::cancellation::CancellationTree>,
    ) -> Self {
        self.subagent.cancellation = Some(tree);
        self
    }

    /// Inject checkpoint manager. Sub-agents auto-snapshot pre-run.
    pub fn with_subagent_checkpoint(
        mut self,
        manager: Arc<crate::checkpoint::CheckpointManager>,
    ) -> Self {
        self.subagent.checkpoint = Some(manager);
        self
    }

    /// Inject worktree provider. Sub-agents with isolation=worktree
    /// get an isolated git worktree.
    pub fn with_subagent_worktree(
        mut self,
        provider: Arc<theo_isolation::WorktreeProvider>,
    ) -> Self {
        self.subagent.worktree = Some(provider);
        self
    }

    /// Inject MCP registry. Sub-agents with non-empty
    /// `spec.mcp_servers` get a system-prompt hint listing the allowed
    /// `mcp:server:tool` namespace.
    pub fn with_subagent_mcp(mut self, mcp: Arc<theo_infra_mcp::McpRegistry>) -> Self {
        self.subagent.mcp = Some(mcp);
        self
    }

    /// Inject MCP discovery cache. When attached, sub-agents whose
    /// `mcp_servers` allowlist matches a cached server receive a richer
    /// system-prompt hint listing actual tool names instead of just the
    /// `mcp:<server>:<tool>` namespace placeholder.
    pub fn with_subagent_mcp_discovery(
        mut self,
        cache: Arc<theo_infra_mcp::DiscoveryCache>,
    ) -> Self {
        self.subagent.mcp_discovery = Some(cache);
        self
    }

    /// Inject the handoff guardrail chain. When `None`, a default
    /// chain (`GuardrailChain::with_default_builtins`) is constructed
    /// per `delegate_task` call.
    pub fn with_subagent_handoff_guardrails(
        mut self,
        chain: Arc<crate::handoff_guardrail::GuardrailChain>,
    ) -> Self {
        self.subagent.handoff_guardrails = Some(chain);
        self
    }

    /// Enable replay-mode dispatch. When set, each tool call is
    /// short-circuited if its `call_id` already appears in the
    /// context's `executed_tool_calls` set; the cached
    /// `Message::tool_result` from the event log is pushed instead of
    /// invoking the tool.
    pub fn with_resume_context(
        mut self,
        ctx: Arc<crate::subagent::resume::ResumeContext>,
    ) -> Self {
        self.resume_context = Some(ctx);
        self
    }

    /// Inject a ReloadableRegistry. Takes precedence over
    /// `with_subagent_registry`: delegate_task reads a fresh snapshot
    /// each call, so filesystem changes (via RegistryWatcher) take
    /// effect without needing to restart the agent.
    pub fn with_subagent_reloadable(
        mut self,
        reloadable: crate::subagent::ReloadableRegistry,
    ) -> Self {
        self.subagent.reloadable = Some(reloadable);
        self
    }

    /// Sets the message queues for steering and follow-up injection.
    pub fn with_message_queues(mut self, queues: MessageQueues) -> Self {
        self.message_queues = queues;
        self
    }

    /// Sets the graph context provider for code intelligence injection.
    pub fn with_graph_context(
        mut self,
        provider: Arc<dyn theo_domain::graph_context::GraphContextProvider>,
    ) -> Self {
        self.graph_context = Some(provider);
        self
    }

    /// Sets the snapshot store for persistence (Invariant 7).
    pub fn with_snapshot_store(mut self, store: Arc<dyn SnapshotStore>) -> Self {
        self.snapshot_store = Some(store);
        self
    }
}
