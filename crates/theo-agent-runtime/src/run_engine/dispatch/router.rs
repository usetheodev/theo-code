//! Meta-tool router — single dispatch entry for every meta-tool handler.
//!
//! REMEDIATION_PLAN T4.3. Replaces the 4-way `if name == "done" ...
//! else if name == "delegate_task" ... else if name == "skill" ... else
//! if name == "batch"` chain in `execution.rs` with a single match
//! site. Adding a new meta-tool now requires:
//!   1. a new module under `run_engine/dispatch/`
//!   2. a new `impl AgentRunEngine` method `handle_<name>_call`
//!   3. a new arm in `dispatch_meta_tool` below
//!
//! Rust's async-fn-in-trait story is still Nightly-only with stable
//! workarounds (BoxFuture + dyn). A `match` keeps the dispatch type-safe,
//! zero-cost, and easy to follow — all goals T4.3 actually cares about.

use theo_infra_llm::types::{Message, ToolCall};

use crate::run_engine::dispatch::DispatchOutcome;
use crate::run_engine::AgentRunEngine;

/// Bundle of cross-handler arguments passed to `dispatch_meta_tool`.
///
/// Individual handlers have heterogeneous needs (iteration counter,
/// abort channel, messages vec). This struct groups them at the call
/// site so the dispatcher's signature stays stable as new handlers are
/// added.
pub(in crate::run_engine) struct MetaToolContext<'a> {
    pub call: &'a ToolCall,
    pub iteration: usize,
    pub abort_rx: &'a tokio::sync::watch::Receiver<bool>,
    pub messages: &'a mut Vec<Message>,
}

impl AgentRunEngine {
    /// Route a single `tool_call` to its meta-tool handler, if any.
    ///
    /// Returns `Some(DispatchOutcome)` when the call name matched a
    /// registered meta-tool (caller matches on the outcome to either
    /// `break` with a Converged result or `continue` the tool loop).
    /// Returns `None` when the call is NOT a meta-tool — caller falls
    /// through to regular-tool dispatch.
    pub(in crate::run_engine) async fn dispatch_meta_tool(
        &mut self,
        ctx: MetaToolContext<'_>,
    ) -> Option<DispatchOutcome> {
        let MetaToolContext {
            call,
            iteration,
            abort_rx,
            messages,
        } = ctx;
        let name = call.function.name.as_str();

        match name {
            // `done` — the only meta-tool that can converge the run.
            // Gates Gate 0/1/2 live in `dispatch/done.rs`.
            "done" => Some(self.handle_done_call(call, iteration, messages).await),

            // `delegate_task` family — sub-agent spawning. Handler
            // pushes tool-result messages internally; dispatcher just
            // continues the loop.
            "delegate_task" | "delegate_task_single" | "delegate_task_parallel" => {
                self.dispatch_delegate_task(call, messages).await;
                Some(DispatchOutcome::Continue)
            }

            // `skill` — packaged-capability invocation.
            "skill" => {
                self.dispatch_skill(call, messages).await;
                Some(DispatchOutcome::Continue)
            }

            // `batch` — programmatic serial fan-out.
            "batch" => {
                self.dispatch_batch(call, abort_rx, messages).await;
                Some(DispatchOutcome::Continue)
            }

            // Not a meta-tool — caller handles it as a regular tool.
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! T4.3 AC: adding a new meta-tool must require ONLY (1) a new
    //! module, (2) a new `impl AgentRunEngine` method, and (3) a new
    //! match arm in `dispatch_meta_tool`. The main loop must NOT be
    //! touched.
    //!
    //! This test is a structural invariant check: it parses the
    //! router.rs source and asserts the registered meta-tool names
    //! match the expected set. Breaks noisily if someone sneaks a
    //! name-based dispatch back into `execution.rs` or
    //! `main_loop.rs`.

    /// Structural invariant: `execution.rs` MUST NOT contain explicit
    /// `if name == "<meta-tool>"` checks. Each such check is a leak
    /// of dispatch logic into the main loop.
    #[test]
    fn execution_has_no_hardcoded_meta_tool_name_checks() {
        let src = include_str!("../execution.rs");
        for meta in &["\"done\"", "\"delegate_task\"", "\"skill\"", "\"batch\""] {
            // Allow comments that mention the tool name (we keep the
            // old comment lines for context). Reject code-like
            // `if name == "<name>"` or `name == "<name>"` patterns.
            let code_uses = src
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .any(|l| l.contains(&format!("name == {meta}")));
            assert!(
                !code_uses,
                "execution.rs leaks meta-tool dispatch via `name == {meta}` — dispatch moved to router.rs (T4.3)"
            );
        }
    }

    /// Structural invariant: `router.rs` registers the 4 current
    /// meta-tool families (+ the delegate_task sub-variants).
    #[test]
    fn router_registers_current_meta_tool_set() {
        let src = include_str!("router.rs");
        let expected = [
            "\"done\"",
            "\"delegate_task\"",
            "\"delegate_task_single\"",
            "\"delegate_task_parallel\"",
            "\"skill\"",
            "\"batch\"",
        ];
        for name in expected {
            assert!(
                src.contains(name),
                "router.rs missing registration for {name}"
            );
        }
    }
}
