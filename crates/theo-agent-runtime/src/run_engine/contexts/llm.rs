//! `LlmContext` — bundle of LLM-execution-side handles.
//!
//! T3.1 PR5 (final) of the AgentRunEngine god-object split. Per
//! `docs/plans/T3.1-god-object-split-roadmap.md`.

use theo_infra_llm::LlmClient;
use theo_tooling::registry::ToolRegistry;
use std::sync::Arc;

use crate::budget_enforcer::BudgetEnforcer;
use crate::convergence::ConvergenceEvaluator;

/// LLM-side handles: the provider client, the tool registry it
/// dispatches against, the convergence evaluator, and the budget
/// enforcer that gates each call.
pub struct LlmContext {
    pub client: LlmClient,
    pub registry: Arc<ToolRegistry>,
    pub convergence: ConvergenceEvaluator,
    pub budget_enforcer: BudgetEnforcer,
}

impl LlmContext {
    pub fn new(
        client: LlmClient,
        registry: Arc<ToolRegistry>,
        convergence: ConvergenceEvaluator,
        budget_enforcer: BudgetEnforcer,
    ) -> Self {
        Self {
            client,
            registry,
            convergence,
            budget_enforcer,
        }
    }
}
