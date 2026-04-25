//! `AgentResult` — outcome envelope of one agent loop execution.
//!
//! Split out of `agent_loop.rs` (REMEDIATION_PLAN T4.* — production-LOC
//! trim toward the per-file 500-line target). Re-exported from
//! `agent_loop/mod.rs` so the public path
//! `theo_agent_runtime::agent_loop::AgentResult` stays byte-identical.

/// Result of an agent loop execution.
#[derive(Debug, Clone, Default)]
pub struct AgentResult {
    pub success: bool,
    pub summary: String,
    pub files_edited: Vec<String>,
    pub iterations_used: usize,
    /// True when the summary was already displayed via ContentDelta streaming.
    /// The REPL should NOT re-print the summary in this case to avoid duplication.
    /// Only set for text-only responses (no tool calls) where content == summary.
    pub was_streamed: bool,
    /// Total tokens consumed during this run (LLM input + output).
    /// Collected by MetricsCollector, surfaced for display.
    pub tokens_used: u64,
    /// Input (prompt) tokens consumed during this run.
    pub input_tokens: u64,
    /// Output (completion) tokens consumed during this run.
    pub output_tokens: u64,
    /// Total tool calls dispatched during this run.
    pub tool_calls_total: u64,
    /// Tool calls that returned without error.
    pub tool_calls_success: u64,
    /// Total LLM API calls during this run.
    pub llm_calls: u64,
    /// Total LLM retries triggered during this run.
    pub retries: u64,
    /// Wall-clock duration of the run, milliseconds. Filled by the caller.
    pub duration_ms: u64,
    /// Name of the agent that produced this result. Empty string for
    /// top-level / pre-refactor paths — backward compat. Sub-agent
    /// callers (including tests) populate this via
    /// `SubAgentManager::spawn_with_spec`.
    pub agent_name: String,
    /// Raw context string passed to the sub-agent, when any. Set by the
    /// spawn path to the first non-empty `Message.content` of the
    /// supplied history vec. `None` on the top-level path.
    pub context_used: Option<String>,
    /// Structured output extracted from `summary` per the spec's
    /// `output_format`. `None` if no schema declared, parse failed in
    /// best_effort mode, or output is plain text.
    pub structured: Option<serde_json::Value>,
    /// True when the run terminated via cooperative cancellation (parent
    /// cancelled, root token, or per-agent token). Distinct from
    /// `success: false` which covers errors / timeouts.
    pub cancelled: bool,
    /// Path of the isolated worktree when `spec.isolation == "worktree"`.
    /// `None` when the sub-agent ran in the shared project CWD.
    pub worktree_path: Option<std::path::PathBuf>,
    /// Typed reason for
    /// the outcome. `None` only on legacy paths that haven't been
    /// migrated. Headless v3 schema emits this field; downstream
    /// statistical comparators use it to separate real agent failures
    /// from infra failures (rate-limit, auth, sandbox).
    ///
    /// Invariant (validated by tests): `success == true ⇔ class ==
    /// Some(ErrorClass::Solved)`.
    pub error_class: Option<theo_domain::error_class::ErrorClass>,
    /// Aggregate observability report (`RunReport`) for this run.
    /// `None` when observability is disabled or when the run completed
    /// before the report could be finalized. Populated by
    /// `AgentRunEngine::take_run_report()` from the engine's
    /// `last_run_report` slot.
    pub run_report: Option<crate::observability::report::RunReport>,
}

impl AgentResult {
    /// Build an `AgentResult` from an engine's current metrics snapshot.
    ///
    /// Replaces ~5 duplicated inline-struct literals that scattered the
    /// 12 metric fields across `run_engine.rs` return paths (REVIEW §2 /
    /// T3.1). Callers still set `success`, `summary`, `was_streamed`,
    /// `error_class`, and `iterations_used` — everything else comes from
    /// the engine state.
    pub fn from_engine_state(
        engine: &crate::run_engine::AgentRunEngine,
        success: bool,
        summary: String,
        was_streamed: bool,
        error_class: theo_domain::error_class::ErrorClass,
    ) -> Self {
        let m = engine.metrics();
        let (files_edited, iteration) = engine.run_result_context();
        Self {
            success,
            summary,
            was_streamed,
            files_edited,
            iterations_used: iteration,
            tokens_used: m.total_tokens_used,
            input_tokens: m.total_input_tokens,
            output_tokens: m.total_output_tokens,
            tool_calls_total: m.total_tool_calls,
            tool_calls_success: m.successful_tool_calls,
            llm_calls: m.total_llm_calls,
            retries: m.total_retries,
            duration_ms: 0,
            error_class: Some(error_class),
            ..Default::default()
        }
    }
}
