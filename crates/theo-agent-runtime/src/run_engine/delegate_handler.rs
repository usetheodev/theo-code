//! `delegate_task` meta-tool handler — builds a `SubAgentManager`, applies
//! Handoff guardrails, and spawns a single sub-agent or a `parallel` fan-out.
//!
//! Fase 4 (REMEDIATION_PLAN T4.2). Extracted from `run_engine/mod.rs`.
//! Behavior is byte-identical; dispatched from
//! `run_engine/dispatch/delegate.rs` via `AgentRunEngine::handle_delegate_task`.

use std::sync::Arc;

use crate::run_engine::{AgentRunEngine, HandoffOutcome};

/// Routing decision for `delegate_task` based on the JSON args. Pure
/// function — extracted for the T7.3 dispatch matrix so the four
/// branches (single / parallel / both / neither) can be unit-tested
/// without instantiating an `AgentRunEngine`.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DelegateRoute {
    /// `agent`+`objective` provided → single delegation.
    Single,
    /// `parallel` array provided → parallel fan-out.
    Parallel,
    /// Both `agent` and `parallel` present → caller error.
    ErrorBoth,
    /// Neither `agent` nor `parallel` present → caller error.
    ErrorNeither,
}

/// Classify the `delegate_task` args into a `DelegateRoute`. The
/// runtime accepts EITHER the `agent`+`objective` form OR the
/// `parallel` form — not both, not neither. This helper is the
/// single source of truth for that contract.
pub(super) fn classify_delegate_args(args: &serde_json::Value) -> DelegateRoute {
    let has_agent = args.get("agent").is_some();
    let has_parallel = args.get("parallel").is_some();
    match (has_agent, has_parallel) {
        (true, true) => DelegateRoute::ErrorBoth,
        (false, false) => DelegateRoute::ErrorNeither,
        (true, false) => DelegateRoute::Single,
        (false, true) => DelegateRoute::Parallel,
    }
}

impl AgentRunEngine {
    /// Dispatch a `delegate_task` call. Args accept either
    /// `agent`+`objective` (single) OR `parallel: [...]` (multi). Both
    /// or neither is an error.
    ///
    /// Routing:
    /// - Known agent name → `spawn_with_spec` with the registered spec.
    /// - Unknown name → `AgentSpec::on_demand` (read-only by S1).
    pub(super) async fn handle_delegate_task(&mut self, args: serde_json::Value) -> String {
        match classify_delegate_args(&args) {
            DelegateRoute::Single => {
                let manager = self.build_subagent_manager();
                let guardrails = self.resolve_handoff_guardrails();
                self.delegate_single(args, manager, guardrails).await
            }
            DelegateRoute::Parallel => {
                let manager = self.build_subagent_manager();
                let guardrails = self.resolve_handoff_guardrails();
                self.delegate_parallel(args, manager, guardrails).await
            }
            DelegateRoute::ErrorBoth => {
                "Error: delegate_task accepts EITHER `agent`+`objective` OR `parallel`, not both."
                    .to_string()
            }
            DelegateRoute::ErrorNeither => {
                "Error: delegate_task requires either `agent`+`objective` or `parallel`."
                    .to_string()
            }
        }
    }

    /// Build a `SubAgentManager` with every optional integration (run store,
    /// hooks, cancellation, checkpoint, worktree, MCP registry + discovery,
    /// metrics) chained in. Registry is either the reloadable
    /// snapshot, the static one, or a fresh builtins+load_all.
    fn build_subagent_manager(&self) -> crate::subagent::SubAgentManager {
        let registry: Arc<crate::subagent::SubAgentRegistry> = if let Some(rel) =
            &self.subagent.reloadable
        {
            Arc::new(rel.snapshot())
        } else if let Some(r) = &self.subagent.registry {
            r.clone()
        } else {
            let mut reg = crate::subagent::SubAgentRegistry::with_builtins();
            // T4.10r / find_p2_003 — surface warnings (e.g. malformed
            // `.theo/agents/*.toml`) via tracing instead of dropping
            // the entire `LoadOutcome`. Custom-agent loading was
            // architecturally invisible before this fix — users had
            // no way to detect that their `.theo/agents/` directory
            // failed to parse.
            let outcome = reg.load_all(
                Some(&self.project_dir),
                None,
                crate::subagent::ApprovalMode::TrustAll,
            );
            for w in &outcome.warnings {
                tracing::warn!(
                    kind = ?w.kind,
                    path = ?w.path,
                    "subagent registry load warning: {}",
                    w.message
                );
            }
            Arc::new(reg)
        };

        let mut manager = crate::subagent::SubAgentManager::with_registry(
            self.config.clone(),
            self.event_bus.clone(),
            self.project_dir.clone(),
            registry,
        )
        .with_metrics(self.obs.metrics.clone());

        if let Some(store) = &self.subagent.run_store {
            manager = manager.with_run_store(store.clone());
        }
        if let Some(hooks) = &self.subagent.hooks {
            manager = manager.with_hooks(hooks.clone());
        }
        if let Some(tree) = &self.subagent.cancellation {
            manager = manager.with_cancellation(tree.clone());
        }
        if let Some(cm) = &self.subagent.checkpoint {
            manager = manager.with_checkpoint(cm.clone());
        }
        if let Some(wp) = &self.subagent.worktree {
            manager = manager.with_worktree_provider(wp.clone());
        }
        if let Some(mcp) = &self.subagent.mcp {
            manager = manager.with_mcp_registry(mcp.clone());
        }
        if let Some(cache) = &self.subagent.mcp_discovery {
            manager = manager.with_mcp_discovery(cache.clone());
        }
        manager
    }

    /// Resolve handoff guardrail chain — injected or default.
    fn resolve_handoff_guardrails(&self) -> Arc<crate::handoff_guardrail::GuardrailChain> {
        self.subagent.handoff_guardrails
            .clone()
            .unwrap_or_else(|| {
                Arc::new(crate::handoff_guardrail::GuardrailChain::with_default_builtins())
            })
    }

    /// Single-delegation path: read `agent`+`objective`+`context`, run
    /// guardrails, spawn, aggregate tokens, return the LLM-facing formatted
    /// message (with optional redirect-note prefix).
    async fn delegate_single(
        &mut self,
        args: serde_json::Value,
        mut manager: crate::subagent::SubAgentManager,
        guardrails: Arc<crate::handoff_guardrail::GuardrailChain>,
    ) -> String {
        let agent_name = args
            .get("agent")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let objective = args
            .get("objective")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let context = args
            .get("context")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if agent_name.is_empty() {
            return "Error: `agent` must be a non-empty string.".to_string();
        }
        if objective.is_empty() {
            return "Error: `objective` is required when delegating to a single agent."
                .to_string();
        }

        let initial_spec = manager
            .registry()
            .and_then(|r| r.get(&agent_name).cloned())
            .unwrap_or_else(|| {
                theo_domain::agent_spec::AgentSpec::on_demand(&agent_name, &objective)
            });

        let (spec, objective, redirect_note) = match self.apply_handoff_guardrails(
            &guardrails,
            &agent_name,
            initial_spec,
            objective,
            &manager,
        ) {
            GuardrailResolution::Block(refusal) => return refusal,
            GuardrailResolution::Resolved {
                spec,
                objective,
                note,
            } => (spec, objective, note),
        };
        // Re-borrow mutably now that we own spec/objective/note.
        let _ = &mut manager;

        let result = manager
            .spawn_with_spec_text(&spec, &objective, context.as_deref())
            .await;

        self.llm.budget_enforcer.record_tokens(result.tokens_used);
        self.obs.metrics.record_delegated_tokens(result.tokens_used);

        let prefix = redirect_note
            .map(|n| format!("{} ", n))
            .unwrap_or_default();
        if result.success {
            format!(
                "{}[{} sub-agent completed] {}",
                prefix, spec.name, result.summary
            )
        } else {
            format!(
                "{}[{} sub-agent failed] {}",
                prefix, spec.name, result.summary
            )
        }
    }

    /// Parallel fan-out: iterate the `parallel` array, run per-entry
    /// guardrails, spawn each, aggregate a combined human-readable log
    /// that the LLM will see in the tool-result message.
    async fn delegate_parallel(
        &mut self,
        args: serde_json::Value,
        mut manager: crate::subagent::SubAgentManager,
        guardrails: Arc<crate::handoff_guardrail::GuardrailChain>,
    ) -> String {
        let arr = args
            .get("parallel")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if arr.is_empty() {
            return "Error: `parallel` must be a non-empty array.".to_string();
        }

        let mut combined = String::new();
        for (i, entry) in arr.iter().enumerate() {
            let agent_name = entry
                .get("agent")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let objective = entry
                .get("objective")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let context = entry
                .get("context")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if agent_name.is_empty() || objective.is_empty() {
                combined.push_str(&format!(
                    "[Sub-agent {}] ERROR: missing agent/objective\n",
                    i + 1
                ));
                continue;
            }
            let initial_spec = manager
                .registry()
                .and_then(|r| r.get(&agent_name).cloned())
                .unwrap_or_else(|| {
                    theo_domain::agent_spec::AgentSpec::on_demand(&agent_name, &objective)
                });

            let initial_name = initial_spec.name.clone();
            let (spec, objective, redirect_note) = match self.apply_handoff_guardrails(
                &guardrails,
                &agent_name,
                initial_spec,
                objective,
                &manager,
            ) {
                GuardrailResolution::Block(refusal) => {
                    combined.push_str(&format!(
                        "[Sub-agent {}] ❌ {} (handoff refused): {}\n",
                        i + 1,
                        initial_name,
                        refusal,
                    ));
                    continue;
                }
                GuardrailResolution::Resolved {
                    spec,
                    objective,
                    note,
                } => (spec, objective, note),
            };
            let _ = &mut manager;

            let result = manager
                .spawn_with_spec_text(&spec, &objective, context.as_deref())
                .await;

            self.llm.budget_enforcer.record_tokens(result.tokens_used);
            self.obs.metrics.record_delegated_tokens(result.tokens_used);

            let mark = if result.success { "✅" } else { "❌" };
            let prefix = redirect_note
                .map(|n| format!("{} ", n))
                .unwrap_or_default();
            combined.push_str(&format!(
                "[Sub-agent {}] {} {}{} ({}): {}\n",
                i + 1,
                mark,
                prefix,
                spec.name,
                spec.source.as_str(),
                result.summary,
            ));
        }
        combined
    }

    /// Run the handoff guardrail chain against a candidate spec
    /// and translate the outcome back into mutated spawn args (or a
    /// short-circuit refusal). Shared between single + parallel paths.
    fn apply_handoff_guardrails(
        &self,
        guardrails: &crate::handoff_guardrail::GuardrailChain,
        agent_name: &str,
        initial_spec: theo_domain::agent_spec::AgentSpec,
        objective: String,
        manager: &crate::subagent::SubAgentManager,
    ) -> GuardrailResolution {
        match self.evaluate_handoff(guardrails, "main", agent_name, &initial_spec, &objective) {
            HandoffOutcome::Block { refusal_message } => GuardrailResolution::Block(refusal_message),
            HandoffOutcome::Allow => GuardrailResolution::Resolved {
                spec: initial_spec,
                objective,
                note: None,
            },
            HandoffOutcome::Redirect {
                guardrail_id,
                new_agent_name,
            } => {
                let new_spec = manager
                    .registry()
                    .and_then(|r| r.get(&new_agent_name).cloned())
                    .unwrap_or_else(|| {
                        theo_domain::agent_spec::AgentSpec::on_demand(&new_agent_name, &objective)
                    });
                let note = format!(
                    "[handoff redirected by {} → {}]",
                    guardrail_id, new_agent_name
                );
                GuardrailResolution::Resolved {
                    spec: new_spec,
                    objective,
                    note: Some(note),
                }
            }
            HandoffOutcome::RewriteObjective {
                guardrail_id,
                new_objective,
            } => {
                let note = format!("[handoff objective rewritten by {}]", guardrail_id);
                GuardrailResolution::Resolved {
                    spec: initial_spec,
                    objective: new_objective,
                    note: Some(note),
                }
            }
        }
    }
}

/// Internal outcome of `apply_handoff_guardrails`: either a short-circuit
/// refusal or the resolved (spec, objective, optional note) tuple.
///
/// `Resolved` carries an owned `AgentSpec` (heavy) while `Block`
/// only carries a refusal string. Boxing the spec would mean an
/// extra allocation on every successful handoff resolution — the
/// common path. The size asymmetry is intentional given the call
/// pattern (Block is rare, Resolved is the default).
#[allow(clippy::large_enum_variant)]
enum GuardrailResolution {
    Block(String),
    Resolved {
        spec: theo_domain::agent_spec::AgentSpec,
        objective: String,
        note: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    //! T7.3 dispatch matrix for `delegate_task × {single / parallel /
    //! both / neither}`. The classify function is pure on its JSON
    //! args, so we can pin every branch without an engine fixture.
    //! The {single / parallel / worktree} dimension from the plan
    //! splits "single" into worktree-strategy variants, which the
    //! runtime decides INSIDE the spawn (`subagent::spawn_helpers::
    //! resolve_worktree`) rather than at the dispatch entry — they
    //! are exercised via the existing `subagent_characterization`
    //! suite.
    use super::{DelegateRoute, classify_delegate_args};

    #[test]
    fn delegate_single_when_only_agent_present() {
        let args = serde_json::json!({
            "agent": "scout",
            "objective": "investigate"
        });
        assert_eq!(classify_delegate_args(&args), DelegateRoute::Single);
    }

    #[test]
    fn delegate_parallel_when_only_parallel_present() {
        let args = serde_json::json!({
            "parallel": [
                {"agent": "scout", "objective": "task-1"},
                {"agent": "scout", "objective": "task-2"}
            ]
        });
        assert_eq!(classify_delegate_args(&args), DelegateRoute::Parallel);
    }

    #[test]
    fn delegate_error_both_when_agent_and_parallel_present() {
        let args = serde_json::json!({
            "agent": "scout",
            "objective": "investigate",
            "parallel": []
        });
        assert_eq!(classify_delegate_args(&args), DelegateRoute::ErrorBoth);
    }

    #[test]
    fn delegate_error_neither_when_neither_present() {
        let args = serde_json::json!({});
        assert_eq!(classify_delegate_args(&args), DelegateRoute::ErrorNeither);
    }

    /// Edge: presence is keyed off `args.get(field).is_some()`, so
    /// even a `null`-valued field counts as present. The dispatcher
    /// downstream is expected to surface a clear error from the
    /// extractor when the field is the wrong type.
    #[test]
    fn delegate_classify_treats_null_field_as_present() {
        let args = serde_json::json!({ "agent": null });
        assert_eq!(classify_delegate_args(&args), DelegateRoute::Single);
    }

    /// Edge: an unrelated field alone is the `ErrorNeither` branch —
    /// no auto-routing to single just because *some* field is set.
    #[test]
    fn delegate_classify_unrelated_field_is_error_neither() {
        let args = serde_json::json!({ "context": "anything" });
        assert_eq!(classify_delegate_args(&args), DelegateRoute::ErrorNeither);
    }
}
