//! `GuardrailChain` — aggregated set of handoff guardrails with ordered
//! evaluation and short-circuit queries.
//!
//! Fase 4 (REMEDIATION_PLAN T4.6). Extracted from `handoff_guardrail/mod.rs`.
//! Behavior is byte-identical; public re-exported from `mod.rs`.

use std::sync::Arc;

use super::builtins::{ObjectiveMustNotBeEmpty, ReadOnlyAgentMustNotMutate};
use super::{GuardrailDecision, HandoffContext, HandoffGuardrail};

/// Aggregated set of guardrails. `evaluate` runs every guardrail in order
/// and collects decisions; `first_block`/`first_decision` short-circuit
/// on the first non-Allow decision.
#[derive(Debug, Default, Clone)]
pub struct GuardrailChain {
    guardrails: Vec<Arc<dyn HandoffGuardrail>>,
}

impl GuardrailChain {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a chain seeded with built-in guardrails.
    pub fn with_default_builtins() -> Self {
        let mut c = Self::new();
        c.add(Arc::new(ReadOnlyAgentMustNotMutate));
        c.add(Arc::new(ObjectiveMustNotBeEmpty));
        c
    }

    pub fn add(&mut self, g: Arc<dyn HandoffGuardrail>) {
        self.guardrails.push(g);
    }

    pub fn len(&self) -> usize {
        self.guardrails.len()
    }

    pub fn is_empty(&self) -> bool {
        self.guardrails.is_empty()
    }

    /// Names of every registered guardrail.
    pub fn ids(&self) -> Vec<String> {
        self.guardrails.iter().map(|g| g.id().to_string()).collect()
    }

    /// Run every guardrail. Returns paired (id, decision) tuples in order.
    pub fn evaluate(&self, ctx: &HandoffContext<'_>) -> Vec<(String, GuardrailDecision)> {
        self.guardrails
            .iter()
            .map(|g| (g.id().to_string(), g.evaluate(ctx)))
            .collect()
    }

    /// Short-circuit query: returns `Some((id, reason))` of the first
    /// blocking guardrail, else `None`.
    pub fn first_block(&self, ctx: &HandoffContext<'_>) -> Option<(String, String)> {
        for g in &self.guardrails {
            if let GuardrailDecision::Block { reason } = g.evaluate(ctx) {
                return Some((g.id().to_string(), reason));
            }
        }
        None
    }

    /// Walk the chain returning the first non-`Allow` decision (Block,
    /// Redirect, RewriteObjective, or Warn) paired with its guardrail id.
    /// Returns `None` if every guardrail allowed.
    ///
    /// Phase 18: chains stop at the first opinionated decision so a custom
    /// guardrail registered after a built-in does not silently override
    /// the built-in's verdict.
    pub fn first_decision(
        &self,
        ctx: &HandoffContext<'_>,
    ) -> Option<(String, GuardrailDecision)> {
        for g in &self.guardrails {
            let decision = g.evaluate(ctx);
            if !decision.is_allow() {
                return Some((g.id().to_string(), decision));
            }
        }
        None
    }
}
