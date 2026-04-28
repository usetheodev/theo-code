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
    /// `parking_lot::Mutex` (T4.10j / find_p4_001) — std::sync::Mutex
    /// can poison on a panicked holder, after which `.lock().ok()`
    /// silently degraded resume mode. parking_lot does not poison and
    /// is consistent with the rest of the runtime's locking primitives.
    pending_resume_context:
        parking_lot::Mutex<Option<Arc<crate::subagent::resume::ResumeContext>>>,
    /// Optional concurrency cap on `spawn_with_spec`. When `Some(n)`,
    /// at most `n` spawns can run in parallel — additional requests
    /// await a permit. `None` means unbounded (legacy behaviour).
    ///
    /// T4.4 / find_p6_011 — without this cap a malicious or buggy
    /// parent agent could fan out an unbounded number of sub-agents
    /// (DoS via runaway spawn).
    spawn_semaphore: Option<Arc<tokio::sync::Semaphore>>,
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
            // T4.4 / find_p6_011 — Acquire a spawn permit BEFORE doing
            // any work so a concurrency cap (when configured via
            // `with_max_concurrent_spawns`) actually backpressures
            // runaway spawns. Permit lives for the entire spawn and is
            // released automatically when `_permit` is dropped.
            let _permit = match self.spawn_semaphore.as_ref() {
                Some(sem) => Some(
                    sem.clone()
                        .acquire_owned()
                        .await
                        .expect("spawn_semaphore is never closed during runtime"),
                ),
                None => None,
            };

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
#[path = "mod_tests.rs"]
mod tests;
