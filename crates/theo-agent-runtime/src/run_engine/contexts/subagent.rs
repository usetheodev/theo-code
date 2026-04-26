//! `SubagentContext` — owned bundle of the 10 sub-agent integration
//! handles previously held as flat `subagent_*` fields on
//! [`crate::run_engine::AgentRunEngine`].
//!
//! T3.1 PR1 / find_p3_001 — the AgentRunEngine god-object split.
//! This is the first of 5 contexts; the other four
//! (`Observability`, `Tracking`, `Runtime`, `Llm`) follow in
//! subsequent PRs per `docs/plans/T3.1-god-object-split-roadmap.md`.
//!
//! Every field is owned by this struct (not borrowed) so the engine
//! holds a single `subagent: SubagentContext` value rather than 10
//! separate Optionals. Builders / accessors live alongside the rest
//! of the engine API in `run_engine::builders` and `run_engine::mod`.

use std::sync::Arc;

use crate::cancellation::CancellationTree;
use crate::checkpoint::CheckpointManager;
use crate::handoff_guardrail::GuardrailChain;
use crate::lifecycle_hooks::HookManager;
use crate::subagent::{ReloadableRegistry, SubAgentRegistry};
use crate::subagent_runs::FileSubagentRunStore;

/// Sub-agent integration plumbing held by [`AgentRunEngine`]. All
/// fields are `Option`/lazy: a fully-defaulted `SubagentContext`
/// represents an engine that does not delegate to sub-agents.
pub struct SubagentContext {
    pub registry: Option<Arc<SubAgentRegistry>>,
    pub run_store: Option<Arc<FileSubagentRunStore>>,
    pub hooks: Option<Arc<HookManager>>,
    pub cancellation: Option<Arc<CancellationTree>>,
    pub checkpoint: Option<Arc<CheckpointManager>>,
    pub worktree: Option<Arc<theo_isolation::WorktreeProvider>>,
    pub mcp: Option<Arc<theo_infra_mcp::McpRegistry>>,
    /// Optional MCP discovery cache propagated to spawn_with_spec.
    pub mcp_discovery: Option<Arc<theo_infra_mcp::DiscoveryCache>>,
    /// Optional handoff guardrail chain. When `None`, a default chain
    /// (built-ins) is used per delegate_task call. Programmatic callers
    /// can register custom guardrails by injecting a chain.
    pub handoff_guardrails: Option<Arc<GuardrailChain>>,
    /// Lazy-built dispatcher for `mcp:server:tool` calls. Built from
    /// `mcp` on first use.
    pub mcp_dispatcher: std::sync::OnceLock<Arc<theo_infra_mcp::McpDispatcher>>,
    /// Optional ReloadableRegistry. When Some, takes precedence over
    /// `registry`: each delegate_task call reads `reloadable.snapshot()`
    /// so watcher changes take effect immediately without restart.
    pub reloadable: Option<ReloadableRegistry>,
}

impl Default for SubagentContext {
    fn default() -> Self {
        Self {
            registry: None,
            run_store: None,
            hooks: None,
            cancellation: None,
            checkpoint: None,
            worktree: None,
            mcp: None,
            mcp_discovery: None,
            handoff_guardrails: None,
            mcp_dispatcher: std::sync::OnceLock::new(),
            reloadable: None,
        }
    }
}

impl SubagentContext {
    /// Lazy: build the McpDispatcher from `mcp` registry on first call.
    pub fn mcp_dispatcher(&self) -> Option<Arc<theo_infra_mcp::McpDispatcher>> {
        let reg = self.mcp.as_ref()?;
        Some(
            self.mcp_dispatcher
                .get_or_init(|| Arc::new(theo_infra_mcp::McpDispatcher::new(reg.clone())))
                .clone(),
        )
    }
}
