//! `RuntimeContext` — bundle of run-loop helper handles +
//! cross-task atomics.
//!
//! T3.1 PR4 of the AgentRunEngine god-object split. Per
//! `docs/plans/T3.1-god-object-split-roadmap.md`.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::config::MessageQueues;
use crate::loop_state::ContextLoopState;
use crate::persistence::SnapshotStore;

/// Run-loop state + cross-task evolution counters.
pub struct RuntimeContext {
    pub snapshot_store: Option<Arc<dyn SnapshotStore>>,
    pub graph_context:
        Option<Arc<dyn theo_domain::graph_context::GraphContextProvider>>,
    pub context_loop_state: ContextLoopState,
    /// Steering and follow-up message queues for mid-run injection.
    /// Pi-mono ref: `packages/agent/src/agent-loop.ts:165-229`
    pub message_queues: MessageQueues,
    /// Accumulated token usage across LLM calls in this session.
    pub session_token_usage: theo_domain::budget::TokenUsage,
    /// PLAN_AUTO_EVOLUTION_SOTA: turns since the last memory
    /// reviewer spawn. `AtomicUsize` lets the counter survive fork
    /// boundaries (eliminates Hermes Issue #8506).
    pub memory_nudge_counter: Arc<crate::memory_lifecycle::MemoryNudgeCounter>,
    /// PLAN_AUTO_EVOLUTION_SOTA: tool iterations since the
    /// last skill reviewer spawn. Persists across task boundaries so
    /// short tasks don't reset accumulation mid-stream.
    pub skill_nudge_counter: Arc<crate::skill_reviewer::SkillNudgeCounter>,
    /// PLAN_AUTO_EVOLUTION_SOTA: flipped to `true` whenever
    /// `skill_manage.create` / `edit` / `patch` succeeds in the
    /// current task, suppressing the reviewer for that task.
    pub skill_created_this_task: AtomicBool,
    /// PLAN_AUTO_EVOLUTION_SOTA: flipped once autodream has
    /// been attempted for this session so we don't retry on every
    /// message in long-running sessions.
    pub autodream_attempted: AtomicBool,
    /// Optional resume context. When present, the dispatch loop
    /// consults `executed_tool_calls` before invoking each tool and
    /// replays cached results from `executed_tool_results` to avoid
    /// double side-effects.
    pub resume_context: Option<Arc<crate::subagent::resume::ResumeContext>>,
    /// T14.1 — Optional sender for partial-progress envelopes. When
    /// `Some`, every `ToolContext` constructed inside the run loop
    /// gets `stdout_tx` populated with a clone, so tools (e.g.
    /// `browser_open`, `browser_screenshot`) can emit progress
    /// updates the consumer side (TUI's `partial_progress::run_drainer`)
    /// can render.
    /// `None` for headless / non-streaming runs — tools see
    /// `stdout_tx: None` and silently no-op their `emit_progress`
    /// calls (per the helper's "no consumer = no cost" property).
    pub partial_progress_tx: Option<tokio::sync::mpsc::Sender<String>>,
}

impl RuntimeContext {
    pub fn new(context_loop_state: ContextLoopState) -> Self {
        Self {
            snapshot_store: None,
            graph_context: None,
            context_loop_state,
            message_queues: MessageQueues::default(),
            session_token_usage: theo_domain::budget::TokenUsage::default(),
            memory_nudge_counter: Arc::new(
                crate::memory_lifecycle::MemoryNudgeCounter::new(),
            ),
            skill_nudge_counter: Arc::new(
                crate::skill_reviewer::SkillNudgeCounter::new(),
            ),
            skill_created_this_task: AtomicBool::new(false),
            autodream_attempted: AtomicBool::new(false),
            resume_context: None,
            partial_progress_tx: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_state::ContextLoopState;

    #[test]
    fn t141_runtime_context_default_partial_tx_is_none() {
        // Headless / non-streaming default: tools see stdout_tx=None
        // and emit_progress silently no-ops. Pin this so a future
        // refactor that flips the default doesn't break headless
        // performance.
        let rt = RuntimeContext::new(ContextLoopState::default());
        assert!(rt.partial_progress_tx.is_none());
    }

    #[tokio::test]
    async fn t141_runtime_context_partial_tx_can_be_set_and_cloned() {
        // Builder side-effect smoke test: setting the tx field makes
        // it visible to clones the dispatch sites construct.
        let mut rt = RuntimeContext::new(ContextLoopState::default());
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(8);
        rt.partial_progress_tx = Some(tx);

        // Mimic what main_loop.rs does: clone into a "ToolContext".
        let cloned = rt
            .partial_progress_tx
            .as_ref()
            .expect("set above")
            .clone();
        cloned.send("test".to_string()).await.unwrap();
        let recv = rx.recv().await.unwrap();
        assert_eq!(recv, "test");
    }
}
