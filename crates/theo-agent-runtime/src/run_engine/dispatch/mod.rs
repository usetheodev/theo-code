//! Meta-tool dispatch handlers — one module per meta-tool.
//!
//! Extracted from the monolithic `for call in tool_calls` loop in
//! `run_engine/mod.rs` (Fase 4 — REMEDIATION_PLAN T4.2). Each submodule
//! contributes an `impl AgentRunEngine` block whose private helpers are
//! named `handle_<tool>_call`.
//!
//! The outcome enum ([`DispatchOutcome`]) is shared across handlers so
//! the caller inside the main loop can pattern-match uniformly on one
//! of three behaviours:
//!   - `Converged(result)` — break the loop, return this AgentResult.
//!   - `Continue`          — continue the main loop (replanning / retry).
//!   - `Handled`           — fall through (tool result already pushed).

use crate::agent_loop::AgentResult;

pub(super) mod batch;
pub(super) mod delegate;
pub(super) mod done;
pub(in crate::run_engine) mod router;
pub(super) mod skill;

/// Shape returned by every meta-tool handler. The main loop consumes
/// this via `match` — no handler ever mutates `should_return` /
/// `continue` flags directly anymore.
pub(super) enum DispatchOutcome {
    /// Handler decided the run is done; break with this result.
    Converged(AgentResult),
    /// Handler pushed one or more messages and needs the main loop to
    /// re-plan / retry in the next iteration (equivalent to `continue`).
    Continue,
}
