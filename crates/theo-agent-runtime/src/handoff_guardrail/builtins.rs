//! Built-in handoff guardrails — shipped out of the box and composed into
//! the default `GuardrailChain` via `with_default_builtins`.
//!
//! Fase 4 (REMEDIATION_PLAN T4.6). Extracted from `handoff_guardrail/mod.rs`.
//! Behavior is byte-identical; public re-exported from `mod.rs`.

use theo_domain::capability::CapabilitySet;
use theo_domain::tool::ToolCategory;

use super::{GuardrailDecision, HandoffContext, HandoffGuardrail};

/// Block when the target sub-agent has no write/edit capability but the
/// objective contains explicit mutation intent. Heuristic — false positives
/// are acceptable because the user can always re-issue with an
/// implementation agent.
#[derive(Debug)]
pub struct ReadOnlyAgentMustNotMutate;

impl ReadOnlyAgentMustNotMutate {
    /// Detects mutation keywords. Word-boundary matching on lowercased
    /// objective. Conservative: only flags clearly imperative verbs.
    pub fn objective_implies_mutation(objective: &str) -> bool {
        let lower = objective.to_lowercase();
        const VERBS: &[&str] = &[
            "implement ",
            "edit ",
            "write ",
            "modify ",
            "create ",
            "patch ",
            "refactor ",
            "fix bug",
            "delete ",
            "add new ",
            "rewrite ",
            "remove ",
        ];
        VERBS.iter().any(|w| lower.contains(w))
    }

    /// True when a `CapabilitySet` permits no file mutation tools.
    pub fn is_capability_set_read_only(caps: &CapabilitySet) -> bool {
        // Read-only ⇔ neither edit nor write nor bash usable.
        let can_edit = caps.can_use_tool("edit", ToolCategory::FileOps);
        let can_write = caps.can_use_tool("write", ToolCategory::FileOps);
        let can_bash = caps.can_use_tool("bash", ToolCategory::Execution);
        !can_edit && !can_write && !can_bash
    }
}

impl HandoffGuardrail for ReadOnlyAgentMustNotMutate {
    fn id(&self) -> &str {
        "builtin.read_only_agent_must_not_mutate"
    }
    fn evaluate(&self, ctx: &HandoffContext<'_>) -> GuardrailDecision {
        if !Self::objective_implies_mutation(ctx.objective) {
            return GuardrailDecision::Allow;
        }
        if Self::is_capability_set_read_only(&ctx.target_spec.capability_set) {
            // Plan §18 default: redirect to `implementer` rather than block —
            // the LLM rarely benefits from a refusal here; transparently
            // upgrading the target preserves intent. The handle_delegate_task
            // path emits a `HandoffEvaluated` audit event so the operator can
            // see exactly which redirection happened.
            return GuardrailDecision::Redirect {
                new_agent_name: "implementer".to_string(),
            };
        }
        GuardrailDecision::Allow
    }
}

/// Reject empty objectives. Cheap sanity check that catches LLM hallucination
/// of a `delegate_task` call without the required argument string.
#[derive(Debug)]
pub struct ObjectiveMustNotBeEmpty;

impl HandoffGuardrail for ObjectiveMustNotBeEmpty {
    fn id(&self) -> &str {
        "builtin.objective_must_not_be_empty"
    }
    fn evaluate(&self, ctx: &HandoffContext<'_>) -> GuardrailDecision {
        if ctx.objective.trim().is_empty() {
            GuardrailDecision::Block {
                reason: format!(
                    "Empty objective for handoff to '{}'. Provide a concrete instruction.",
                    ctx.target_agent
                ),
            }
        } else {
            GuardrailDecision::Allow
        }
    }
}
