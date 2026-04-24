//! Handoff evaluation — runs the `GuardrailChain` + PreHandoff hook and
//! returns a `HandoffOutcome` (allow / block / redirect / rewrite). Emits
//! a `HandoffEvaluated` audit event for the dashboard.
//!
//! Fase 4 (REMEDIATION_PLAN T4.2). Extracted from `run_engine/mod.rs`.
//! Behavior is byte-identical.

use crate::run_engine::AgentRunEngine;
use crate::run_engine_helpers::truncate_handoff_objective;

/// Outcome of `AgentRunEngine::evaluate_handoff`. Phase 18 (sota-gaps).
#[derive(Debug, Clone)]
pub enum HandoffOutcome {
    /// Proceed with the spawn unchanged.
    Allow,
    /// Refuse the spawn and surface `refusal_message` to the LLM.
    Block { refusal_message: String },
    /// Spawn `new_agent_name` instead of the requested target. Objective
    /// is preserved. `guardrail_id` is logged for the audit event prefix.
    Redirect {
        guardrail_id: String,
        new_agent_name: String,
    },
    /// Spawn the requested target but with `new_objective` replacing the
    /// LLM-provided one.
    RewriteObjective {
        guardrail_id: String,
        new_objective: String,
    },
}

impl AgentRunEngine {
    /// Outcome of a handoff evaluation.
    ///
    /// `Block` short-circuits the spawn with a refusal message returned to
    /// the LLM. `Redirect`/`Rewrite` mutate the spawn arguments; the caller
    /// then continues with the new (target_agent, objective) pair.
    /// `Allow` is the default — proceed unchanged.
    pub fn evaluate_handoff(
        &self,
        chain: &crate::handoff_guardrail::GuardrailChain,
        source_agent: &str,
        target_agent: &str,
        target_spec: &theo_domain::agent_spec::AgentSpec,
        objective: &str,
    ) -> HandoffOutcome {
        use crate::handoff_guardrail::{GuardrailDecision, HandoffContext};
        let ctx = HandoffContext {
            source_agent,
            target_agent,
            target_spec,
            objective,
            source_capabilities: self.config.capability_set.as_ref(),
        };

        let decisions = chain.evaluate(&ctx);
        let blocked_by = decisions.iter().find_map(|(id, d)| match d {
            GuardrailDecision::Block { reason } => Some((id.clone(), reason.clone())),
            _ => None,
        });
        let warnings: Vec<String> = decisions
            .iter()
            .filter_map(|(id, d)| match d {
                GuardrailDecision::Warn { message } => Some(format!("[{}] {}", id, message)),
                _ => None,
            })
            .collect();
        let mutating = decisions.iter().find_map(|(id, d)| match d {
            GuardrailDecision::Redirect { new_agent_name } => {
                Some(("redirect", id.clone(), Some(new_agent_name.clone()), None))
            }
            GuardrailDecision::RewriteObjective { new_objective } => {
                Some(("rewrite", id.clone(), None, Some(new_objective.clone())))
            }
            _ => None,
        });

        // Phase 18 + 24: PreHandoff hook only fires when no chain block —
        // chain wins first. Hooks may also Block, becoming the final blocker.
        // Phase 24 (sota-gaps-followup): populates HookContext.target_agent
        // + target_objective so YAML matchers can regex-match against them.
        let hook_block = if blocked_by.is_none() {
            self.subagent_hooks.as_ref().and_then(|hooks| {
                use crate::lifecycle_hooks::{HookContext, HookEvent, HookResponse};
                let hook_ctx = HookContext {
                    tool_name: Some(format!("delegate_task:{}", target_agent)),
                    tool_args: Some(serde_json::json!({
                        "agent": target_agent,
                        "objective": objective,
                    })),
                    tool_result: None,
                    target_agent: Some(target_agent.to_string()),
                    target_objective: Some(objective.to_string()),
                };
                match hooks.dispatch(HookEvent::PreHandoff, &hook_ctx) {
                    HookResponse::Block { reason } => {
                        Some(("hook.pre_handoff".to_string(), reason))
                    }
                    _ => None,
                }
            })
        } else {
            None
        };

        let final_block = blocked_by.clone().or(hook_block.clone());
        let decision_label = if final_block.is_some() {
            "block"
        } else if let Some((label, _, _, _)) = &mutating {
            *label
        } else if !warnings.is_empty() {
            "warn"
        } else {
            "allow"
        };

        // Always publish an audit event.
        self.event_bus.publish(theo_domain::event::DomainEvent::new(
            theo_domain::event::EventType::HandoffEvaluated,
            self.run.run_id.as_str(),
            serde_json::json!({
                "source_agent": source_agent,
                "target_agent": target_agent,
                "target_source": target_spec.source.as_str(),
                "objective": truncate_handoff_objective(objective),
                "decision": decision_label,
                "reason": final_block.as_ref().map(|(_, r)| r.clone()),
                "blocked_by": final_block.as_ref().map(|(id, _)| id.clone()),
                "redirect_to": mutating.as_ref().and_then(|(_, _, n, _)| n.clone()),
                "rewrite_objective": mutating
                    .as_ref()
                    .and_then(|(_, _, _, o)| o.clone())
                    .map(|s| truncate_handoff_objective(&s)),
                "mutated_by": mutating.as_ref().map(|(_, id, _, _)| id.clone()),
                "guardrails_evaluated": chain.ids(),
                "warnings": warnings,
            }),
        ));

        if let Some((id, reason)) = final_block {
            return HandoffOutcome::Block {
                refusal_message: format!("[handoff refused by {}] {}", id, reason),
            };
        }
        match mutating {
            Some(("redirect", id, Some(new), _)) => HandoffOutcome::Redirect {
                guardrail_id: id,
                new_agent_name: new,
            },
            Some(("rewrite", id, _, Some(new))) => HandoffOutcome::RewriteObjective {
                guardrail_id: id,
                new_objective: new,
            },
            _ => HandoffOutcome::Allow,
        }
    }

    /// Backwards-compatible wrapper used by tests written before the
    /// outcome enum existed. Returns `Some(refusal)` on Block, `None`
    /// otherwise — note: redirects/rewrites now return None (caller is
    /// expected to handle them by inspecting the outcome enum directly).
    #[deprecated(note = "use evaluate_handoff instead")]
    pub fn evaluate_handoff_or_refuse(
        &self,
        chain: &crate::handoff_guardrail::GuardrailChain,
        source_agent: &str,
        target_agent: &str,
        target_spec: &theo_domain::agent_spec::AgentSpec,
        objective: &str,
    ) -> Option<String> {
        match self.evaluate_handoff(chain, source_agent, target_agent, target_spec, objective) {
            HandoffOutcome::Block { refusal_message } => Some(refusal_message),
            _ => None,
        }
    }
}
