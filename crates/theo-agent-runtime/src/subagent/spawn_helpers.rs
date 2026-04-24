//! Helpers for `SubAgentManager::spawn_with_spec_with_override`.
//!
//! Fase 4 (REMEDIATION_PLAN T4.5). Extracted from `subagent/mod.rs` to shrink
//! the 480-LOC monolithic future. Each helper is an `impl SubAgentManager`
//! method so it retains direct access to private fields without breaking
//! encapsulation.
//!
//! Scope of this first pass:
//!   - `finalize_persisted_run` — post-run persistence of final SubagentRun
//!   - `apply_output_format` — parse structured output per spec.output_format
//!   - `dispatch_stop_hook_annotate` — SubagentStop hook (informational)
//!   - `cleanup_worktree_if_success` — conditional worktree removal
//!
//! These are the most isolated blocks — they only read/write via
//! `&self` + `&mut AgentResult` + local run metadata, and do not touch the
//! LLM loop or cancellation lifecycle.

use std::path::Path;
use std::time::SystemTime;

use crate::agent_loop::AgentResult;
use crate::config::AgentConfig;
use crate::subagent::{SubAgentManager, WorktreeOverride};
use theo_domain::agent_spec::AgentSpec;
use theo_domain::event::{DomainEvent, EventType};

/// Race an agent run `future` against a timeout and an optional cancellation
/// token. Returns the agent's `AgentResult` on success, a synthesized
/// timeout result, or a synthesized cancellation result. Centralizes the
/// two legacy branches (with/without token) so the hot path stays readable.
pub(super) async fn run_agent_with_timeout<F>(
    future: F,
    timeout: std::time::Duration,
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
    spec_name: &str,
    timeout_secs: u64,
    objective: &str,
) -> AgentResult
where
    F: std::future::Future<Output = AgentResult>,
{
    let timeout_result = || AgentResult {
        success: false,
        summary: format!(
            "Sub-agent ({}) timed out after {}s. Objective: {}",
            spec_name, timeout_secs, objective
        ),
        ..Default::default()
    };
    let cancelled_result = || AgentResult {
        success: false,
        summary: format!("Sub-agent ({}) cancelled mid-run by parent", spec_name),
        cancelled: true,
        ..Default::default()
    };

    match cancellation_token {
        Some(tok) => {
            tokio::select! {
                res = tokio::time::timeout(timeout, future) => match res {
                    Ok(r) => r,
                    Err(_) => timeout_result(),
                },
                _ = tok.cancelled() => cancelled_result(),
            }
        }
        None => match tokio::time::timeout(timeout, future).await {
            Ok(r) => r,
            Err(_) => timeout_result(),
        },
    }
}

/// Build a deterministic-unique run_id for a sub-agent invocation using
/// `spec.name` + wall-clock micros. Collisions require sub-microsecond
/// spawns of the *same* spec within the same parent — unlikely in practice
/// and would only affect persistence de-dup.
pub(super) fn generate_run_id(spec: &AgentSpec) -> String {
    format!(
        "subagent-{}-{}",
        spec.name,
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros())
            .unwrap_or(0)
    )
}

impl SubAgentManager {
    /// Phase 9: auto-snapshot the workdir BEFORE the run (pre-mutation safety).
    /// No-op when no `checkpoint_manager` is attached. Failures are swallowed —
    /// the run proceeds even if snapshot fails (returns `None`).
    pub(super) fn snapshot_pre_run(&self, spec: &AgentSpec) -> Option<String> {
        self.checkpoint_manager
            .as_ref()
            .and_then(|cm| cm.snapshot(&format!("pre-run:{}", spec.name)).ok())
    }

    /// Build a child `EventBus` for the sub-agent that forwards every event
    /// back to the parent bus, tagged by `spec.name`. Returned bus is
    /// shared-owned (Arc) so it can be cloned into `AgentLoop`.
    pub(super) fn build_prefixed_sub_bus(
        &self,
        spec: &AgentSpec,
    ) -> std::sync::Arc<crate::event_bus::EventBus> {
        let sub_bus = std::sync::Arc::new(crate::event_bus::EventBus::new());
        let prefixed = std::sync::Arc::new(super::PrefixedEventForwarder {
            role_name: spec.name.clone(),
            parent_bus: self.event_bus.clone(),
        });
        sub_bus.subscribe(prefixed);
        sub_bus
    }

    /// Phase 6: register a child cancellation token (scoped to `run_id`) and
    /// bail out early when the parent is already cancelled. Returns the token
    /// on the happy path, or a `ready-to-publish` AgentResult when we must
    /// short-circuit (cancelled-before-start). `None` token return means no
    /// cancellation tree is attached — the agent runs without cancellation.
    #[allow(clippy::type_complexity)]
    pub(super) fn register_cancellation_or_bail(
        &self,
        run_id: &str,
        spec: &AgentSpec,
        context_text: &Option<String>,
        start_instant: std::time::Instant,
        worktree_handle: Option<&theo_isolation::WorktreeHandle>,
    ) -> Result<Option<tokio_util::sync::CancellationToken>, AgentResult> {
        let token = self.cancellation.as_ref().map(|tree| tree.child(run_id));
        if let Some(tok) = &token
            && tok.is_cancelled()
        {
            return Err(AgentResult {
                success: false,
                summary: "Sub-agent cancelled before start (parent cancelled)".to_string(),
                agent_name: spec.name.clone(),
                context_used: context_text.clone(),
                duration_ms: start_instant.elapsed().as_millis() as u64,
                cancelled: true,
                worktree_path: worktree_handle.map(|h| h.path.clone()),
                ..Default::default()
            });
        }
        Ok(token)
    }

    /// Enforce `MAX_DEPTH` on sub-agent nesting. Returns `Err(AgentResult)`
    /// when the limit is reached (caller persists + publishes); `Ok(())`
    /// otherwise.
    pub(super) fn enforce_max_depth(
        &self,
        spec: &AgentSpec,
        context_text: &Option<String>,
        start_instant: std::time::Instant,
    ) -> Result<(), AgentResult> {
        if self.depth >= super::MAX_DEPTH {
            return Err(AgentResult {
                success: false,
                summary: "Sub-agent depth limit reached. Sub-agents cannot spawn sub-agents."
                    .to_string(),
                agent_name: spec.name.clone(),
                context_used: context_text.clone(),
                duration_ms: start_instant.elapsed().as_millis() as u64,
                ..Default::default()
            });
        }
        Ok(())
    }

    /// Phase 30 (resume-runtime-wiring) — gap #3: consume (take) the pending
    /// resume context set by `Resumer` right before this spawn. When `Some`,
    /// the spawned `AgentLoop` runs in replay-mode: known call_ids return
    /// cached tool_results instead of re-executing the tool. Returns `None`
    /// when no resume is pending or the mutex is poisoned.
    pub(super) fn take_pending_resume_context(
        &self,
    ) -> Option<std::sync::Arc<crate::subagent::resume::ResumeContext>> {
        self.pending_resume_context
            .lock()
            .ok()
            .and_then(|mut g| g.take())
    }

    /// Phase 17 + Phase 20 (sota-gaps): register McpToolAdapter instances for
    /// every discovered MCP tool advertised in `spec.mcp_servers`. Triggers
    /// auto-discovery (fail-soft) when the cache doesn't already cover the
    /// requested servers, unless disabled via `THEO_MCP_AUTO_DISCOVERY=0`.
    /// No-op when `mcp_servers` is empty or either of the two MCP subsystems
    /// is absent.
    pub(super) async fn register_mcp_tool_adapters(
        &self,
        spec: &AgentSpec,
        registry: &mut theo_tooling::registry::ToolRegistry,
    ) {
        if spec.mcp_servers.is_empty() {
            return;
        }
        let (Some(cache), Some(global)) = (&self.mcp_discovery, &self.mcp_registry) else {
            return;
        };

        let auto_discovery_enabled =
            theo_domain::environment::bool_var("THEO_MCP_AUTO_DISCOVERY", true);
        if auto_discovery_enabled && super::needs_discovery(cache, &spec.mcp_servers) {
            let _report = cache
                .discover_filtered(
                    global.as_ref(),
                    &spec.mcp_servers,
                    theo_infra_mcp::DEFAULT_PER_SERVER_TIMEOUT,
                )
                .await;
        }

        let dispatcher =
            std::sync::Arc::new(theo_infra_mcp::McpDispatcher::new(global.clone()));
        let adapters =
            super::mcp_tools::build_adapters_for_spec(cache, &spec.mcp_servers, dispatcher);
        for adapter in adapters {
            if let Err(e) = registry.register(Box::new(adapter)) {
                eprintln!(
                    "[subagent {}] WARNING: failed to register MCP tool: {}",
                    spec.name, e
                );
            }
        }
    }

    /// Phase 10: persist the SubagentRun "running" record at spawn start.
    /// No-op when `run_store` is None. Errors are swallowed — failing to
    /// persist start must never block the actual run.
    pub(super) fn persist_run_start(
        &self,
        run_id: &str,
        spec: &AgentSpec,
        objective: &str,
        checkpoint_before: Option<String>,
    ) {
        let Some(store) = &self.run_store else { return };
        let run = crate::subagent_runs::SubagentRun::new_running(
            run_id,
            None,
            spec,
            objective,
            self.project_dir.to_string_lossy(),
            checkpoint_before,
        );
        let _ = store.save(&run);
    }

    /// Phase 12: build OTel-aligned start span attributes and publish the
    /// `SubagentStarted` event with the payload embedding them.
    pub(super) fn emit_subagent_started(
        &self,
        spec: &AgentSpec,
        run_id: &str,
        objective: &str,
        checkpoint_before: Option<&str>,
    ) {
        let mut start_span = crate::observability::otel::AgentRunSpan::from_spec(spec, run_id);
        start_span.set("gen_ai.operation.name", "subagent.spawn");
        start_span.set("theo.subagent.objective", objective.to_string());
        if let Some(cp) = checkpoint_before {
            start_span.set("theo.subagent.checkpoint_before", cp.to_string());
        }

        self.event_bus.publish(DomainEvent::new(
            EventType::SubagentStarted,
            format!("subagent:{}", spec.name).as_str(),
            serde_json::json!({
                "agent_name": spec.name,
                "agent_source": spec.source.as_str(),
                "objective": objective,
                "run_id": run_id,
                "checkpoint_before": checkpoint_before,
                "otel": start_span.to_json(),
            }),
        ));
    }

    /// Phase 5: dispatch `SubagentStart` hook. Returns `Some(blocked_result)`
    /// when the hook requested a `Block` (caller must short-circuit); returns
    /// `None` to let the run proceed.
    pub(super) fn dispatch_start_hook_or_block(
        &self,
        effective_hooks: Option<&crate::lifecycle_hooks::HookManager>,
        spec: &AgentSpec,
        context_text: &Option<String>,
    ) -> Option<AgentResult> {
        let hooks = effective_hooks?;
        use crate::lifecycle_hooks::{HookContext, HookEvent, HookResponse};
        let resp = hooks.dispatch(HookEvent::SubagentStart, &HookContext::default());
        match resp {
            HookResponse::Block { reason } => Some(AgentResult {
                success: false,
                summary: format!("Sub-agent blocked by SubagentStart hook: {}", reason),
                agent_name: spec.name.clone(),
                context_used: context_text.clone(),
                ..Default::default()
            }),
            _ => None,
        }
    }

    /// Phase 10: persist final state for an early-exit path (cancelled,
    /// max-depth reached, hook-blocked). Mirrors `finalize_persisted_run`
    /// but accepts an explicit status (the early paths know their outcome
    /// upfront, without waiting for `result.success`).
    pub(super) fn persist_early_exit(
        &self,
        run_id: &str,
        status: crate::subagent_runs::RunStatus,
        summary: &str,
    ) {
        let Some(store) = &self.run_store else { return };
        let Ok(mut run) = store.load(run_id) else { return };
        run.status = status;
        run.finished_at = Some(
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        );
        run.summary = Some(summary.to_string());
        let _ = store.save(&run);
    }

    /// Phase 11 + Phase 31: resolve a worktree handle honoring the override.
    /// Precedence:
    ///   - `Reuse(path)` → wrap the existing path with `WorktreeHandle::existing`
    ///     (synthetic branch `"(reused)"` flags it for cleanup-skip).
    ///   - `Recreate { base }` → call `provider.create(spec.name, base)`.
    ///   - `None` → legacy behavior: use `spec.isolation_base_branch` or `main`.
    /// A `master` fallback is attempted on provider failure to cover legacy git
    /// defaults. Returns `None` when no provider is attached OR
    /// `spec.isolation != "worktree"`.
    pub(super) fn resolve_worktree(
        &self,
        spec: &AgentSpec,
        worktree_override: &WorktreeOverride,
    ) -> Option<theo_isolation::WorktreeHandle> {
        match (
            &self.worktree_provider,
            spec.isolation.as_deref(),
            worktree_override,
        ) {
            (_, _, WorktreeOverride::Reuse(path)) => {
                Some(theo_isolation::WorktreeHandle::existing(path.clone()))
            }
            (Some(provider), Some("worktree"), WorktreeOverride::Recreate { base_branch }) => {
                let result = provider
                    .create(&spec.name, base_branch)
                    .or_else(|_| provider.create(&spec.name, "master"));
                self.dispatch_worktree_create_hook(&result);
                result.ok()
            }
            (Some(provider), Some("worktree"), WorktreeOverride::None) => {
                let base = spec
                    .isolation_base_branch
                    .clone()
                    .unwrap_or_else(|| "main".to_string());
                let result = provider
                    .create(&spec.name, &base)
                    .or_else(|_| provider.create(&spec.name, "master"));
                self.dispatch_worktree_create_hook(&result);
                result.ok()
            }
            _ => None,
        }
    }

    /// Phase 5: dispatch `WorktreeCreate` hook (informational). Only fires on
    /// `Ok(handle)` and only when a `hook_manager` is attached.
    fn dispatch_worktree_create_hook(
        &self,
        result: &Result<theo_isolation::WorktreeHandle, theo_isolation::IsolationError>,
    ) {
        let (Ok(handle), Some(hooks)) = (result.as_ref(), &self.hook_manager) else {
            return;
        };
        use crate::lifecycle_hooks::{HookContext, HookEvent};
        let _ = hooks.dispatch(
            HookEvent::WorktreeCreate,
            &HookContext {
                tool_name: Some(handle.path.to_string_lossy().to_string()),
                ..Default::default()
            },
        );
    }

    /// Phase 10: persist final run status + metrics after the sub-agent loop
    /// completes. No-op when `run_store` is `None` or the run record cannot
    /// be loaded (race / disk failure). Errors are swallowed by design —
    /// failing to persist must never crash the run.
    pub(super) fn finalize_persisted_run(&self, run_id: &str, result: &AgentResult) {
        let Some(store) = &self.run_store else { return };
        let Ok(mut run) = store.load(run_id) else { return };
        run.status = if result.cancelled {
            crate::subagent_runs::RunStatus::Cancelled
        } else if result.success {
            crate::subagent_runs::RunStatus::Completed
        } else {
            crate::subagent_runs::RunStatus::Failed
        };
        run.finished_at = Some(
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        );
        run.iterations_used = result.iterations_used;
        run.tokens_used = result.tokens_used;
        run.summary = Some(result.summary.clone());
        let _ = store.save(&run);
    }

    /// Phase 7: try to parse the summary against `spec.output_format`.
    /// Mutates `result.structured` on success; in strict mode a parse failure
    /// flips `result.success = false` and appends the error to the summary.
    /// In best-effort mode (default) parse failures are silent.
    pub(super) fn apply_output_format(
        &self,
        spec: &AgentSpec,
        run_id: &str,
        result: &mut AgentResult,
    ) {
        let Some(schema) = &spec.output_format else { return };
        let strict = spec.output_format_strict.unwrap_or(false);
        match crate::output_format::try_parse_structured(&result.summary, schema) {
            Ok(value) => {
                result.structured = Some(value.clone());
                if let Some(store) = &self.run_store
                    && let Ok(mut run) = store.load(run_id)
                {
                    run.structured_output = Some(value);
                    let _ = store.save(&run);
                }
            }
            Err(err) if strict => {
                result.success = false;
                result.summary =
                    format!("{}\n\n[output_format strict] {}", result.summary, err);
            }
            Err(_) => { /* best_effort: keep free-text, structured=None */ }
        }
    }

    /// Phase 5: dispatch `SubagentStop` hook (informational — the run already
    /// finished). A `Block` response is treated as a warning suffix appended
    /// to `result.summary` (it cannot cancel post-hoc).
    pub(super) fn dispatch_stop_hook_annotate(
        &self,
        effective_hooks: Option<&crate::lifecycle_hooks::HookManager>,
        result: &mut AgentResult,
    ) {
        let Some(hooks) = effective_hooks else { return };
        use crate::lifecycle_hooks::{HookContext, HookEvent, HookResponse};
        let resp = hooks.dispatch(HookEvent::SubagentStop, &HookContext::default());
        if let HookResponse::Block { reason } = resp {
            result.summary =
                format!("{}\n\n[SubagentStop hook flagged] {}", result.summary, reason);
        }
    }

    /// Build the sub-agent's `AgentConfig` from the parent config + `AgentSpec`.
    /// Injects:
    ///   - `system_prompt` with `[name]` prefix and cwd restriction banner
    ///   - `is_subagent=true` + `capability_set` from the spec
    ///   - `model_override` when provided by the spec
    ///   - Pi-Mono isolation safety rules when a worktree was allocated
    ///   - MCP prompt hint (from discovery cache or registry fallback)
    pub(super) fn build_sub_config(
        &self,
        spec: &AgentSpec,
        agent_cwd: &Path,
        has_worktree: bool,
    ) -> AgentConfig {
        let mut sub_config = self.config.clone();
        sub_config.system_prompt = spec.system_prompt.clone();
        sub_config.max_iterations = spec.max_iterations;
        sub_config.is_subagent = true;
        sub_config.capability_set = Some(spec.capability_set.clone());
        if let Some(m) = &spec.model_override {
            sub_config.model = m.clone();
        }

        sub_config.system_prompt = if has_worktree {
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

        // Phase 8 + Phase 17: MCP integration — inject prompt hint.
        // Preference: discovery cache (concrete tool names) → registry
        // (legacy namespace placeholder).
        if !spec.mcp_servers.is_empty() {
            let mut hint = String::new();
            if let Some(cache) = &self.mcp_discovery {
                hint = cache.render_prompt_hint(&spec.mcp_servers);
            }
            if hint.is_empty()
                && let Some(global) = &self.mcp_registry
            {
                let filtered = global.filtered(&spec.mcp_servers);
                hint = filtered.render_prompt_hint();
            }
            if !hint.is_empty() {
                sub_config.system_prompt =
                    format!("{}\n\n{}", sub_config.system_prompt, hint);
            }
        }

        sub_config
    }

    /// Phase 11: cleanup worktree on success (default policy: OnSuccess).
    /// Failures preserve the worktree for inspection. Phase 31 (resume
    /// wiring) — skip removal when the handle's synthetic branch is
    /// `"(reused)"`, since in that case this manager does NOT own the
    /// directory (it was reused from a crashed prior run).
    pub(super) fn cleanup_worktree_if_success(
        &self,
        worktree_handle: Option<&theo_isolation::WorktreeHandle>,
        result: &AgentResult,
    ) {
        let (Some(handle), Some(provider)) =
            (worktree_handle, &self.worktree_provider)
        else {
            return;
        };
        if !result.success || handle.branch == "(reused)" {
            return;
        }
        let removed = provider.remove(handle, false).is_ok();
        if removed
            && let Some(hooks) = &self.hook_manager
        {
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
}
