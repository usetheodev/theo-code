//! Post-run finalization helpers for `SubAgentManager::spawn_with_spec_with_override`.
//!
//! Split out of `subagent/spawn_helpers.rs` (REMEDIATION_PLAN T4.* —
//! production-LOC trim toward the per-file 500-line target). These four
//! methods were already documented in the parent module as the most
//! isolated post-run blocks: they only read/write via `&self` +
//! `&mut AgentResult` + run metadata, never the LLM loop or
//! cancellation tree.
//!
//! Methods kept on `pub(super) impl SubAgentManager` so callers in the
//! sibling module need no change.
//!
//!   - `finalize_persisted_run` — post-run persistence of final SubagentRun
//!   - `apply_output_format` — parse structured output per spec.output_format
//!   - `dispatch_stop_hook_annotate` — SubagentStop hook (informational)
//!   - `cleanup_worktree_if_success` — conditional worktree removal

use std::time::SystemTime;

use crate::agent_loop::AgentResult;
use crate::subagent::SubAgentManager;
use theo_domain::agent_spec::AgentSpec;

impl SubAgentManager {
    /// Persist final run status + metrics after the sub-agent loop
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

    /// Try to parse the summary against `spec.output_format`.
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

    /// Dispatch `SubagentStop` hook (informational — the run already
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

    /// Cleanup worktree on success (default policy: OnSuccess).
    /// Failures preserve the worktree for inspection. Resume-runtime
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
            use crate::lifecycle_hooks::{HookContext, HookEvent, HookResponse};
            // T4.10e / find_p2_011 — log non-Allow responses (idem
            // WorktreeCreate side in spawn_helpers).
            let resp = hooks.dispatch(
                HookEvent::WorktreeRemove,
                &HookContext {
                    tool_name: Some(handle.path.to_string_lossy().to_string()),
                    ..Default::default()
                },
            );
            if !matches!(resp, HookResponse::Allow) {
                tracing::debug!(
                    event = "WorktreeRemove",
                    response = ?resp,
                    worktree = %handle.path.display(),
                    "non-Allow hook response (informational)"
                );
            }
        }
    }
}
