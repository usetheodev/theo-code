//! Wiring helpers invoked from `run_engine.rs` to tie memory-lifecycle,
//! autodream, bootstrap, and transcript-indexing subsystems to the agent
//! run. Each helper is fire-and-forget where possible, so the main loop
//! stays synchronous on the hot path.
//!
//! Fase 4 (REMEDIATION_PLAN T4.6). Moved from `memory_lifecycle.rs` to a
//! sibling file. Behavior is byte-identical.

use super::{
    recent_review_window, should_trigger_memory_review, MemoryNudgeCounter,
    MemoryReviewTrigger,
};
use crate::config::AgentConfig;

/// PLAN_AUTO_EVOLUTION_SOTA — autodream at session START.
/// Fire-and-forget. Respects the provided `attempted` flag so repeat
/// calls within the same `AgentRunEngine` are no-ops.
pub fn maybe_spawn_autodream(
    cfg: &AgentConfig,
    attempted: &std::sync::atomic::AtomicBool,
    project_dir: &std::path::Path,
    run_id: &str,
) {
    if !cfg.autodream_enabled {
        return;
    }
    // Relaxed: this is a single-thread idempotency flag (no causal
    // dependency on any other shared state). The swap returns the
    // previous value; if it was already `true`, another caller already
    // spawned autodream and we no-op (T5.4).
    if attempted.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Some(handle) = cfg.autodream.clone() else {
        return;
    };
    let memory_dir = project_dir.join(".theo").join("memory");
    let session_id = run_id.to_string();
    let timeout = std::time::Duration::from_secs(cfg.autodream_timeout_secs);
    tokio::spawn(async move {
        match tokio::time::timeout(
            timeout,
            crate::autodream::run_autodream(&memory_dir, &session_id, handle.as_executor()),
        )
        .await
        {
            Ok(Ok(Some(report))) => {
                eprintln!("[theo::autodream] ran: {report:?}");
            }
            Ok(Ok(None)) => {}
            Ok(Err(err)) => {
                eprintln!("[theo::autodream] failed: {err}");
            }
            Err(_) => {
                eprintln!("[theo::autodream] timed out after {}s", timeout.as_secs());
            }
        }
    });
}

/// Wiring helper: maybe prepend the bootstrap Q&A prompt.
pub fn maybe_prepend_bootstrap(
    cfg: &AgentConfig,
    project_dir: &std::path::Path,
    sp: String,
) -> String {
    if cfg.is_subagent {
        return sp;
    }
    let memory_dir = project_dir.join(".theo").join("memory");
    if crate::onboarding::needs_bootstrap(&memory_dir) {
        crate::onboarding::compose_bootstrap_system_prompt(&sp)
    } else {
        sp
    }
}

/// Wiring helper: transcript indexing.
///
/// Runs on the session shutdown path (`record_session_exit`), so we
/// `.await` the indexer inline instead of `tokio::spawn`ing. A detached
/// task would be killed the moment the headless binary exits its tokio
/// runtime, and no Tantivy files would hit disk.
pub async fn maybe_index_transcript(
    cfg: &AgentConfig,
    project_dir: &std::path::Path,
    run_id: &str,
    events: Vec<theo_domain::event::DomainEvent>,
) {
    if cfg.is_subagent || events.is_empty() {
        return;
    }
    let Some(handle) = cfg.memory().transcript_indexer.cloned() else {
        return;
    };
    let memory_dir = project_dir.join(".theo").join("memory");
    let session_id = run_id.to_string();
    if let Err(err) = handle
        .as_indexer()
        .record_session(&memory_dir, &session_id, &events)
        .await
    {
        eprintln!("[theo::transcript] indexing failed: {err}");
    }
}

/// Wiring helper: evaluate nudges at end-of-turn and spawn
/// reviewers when needed. Returns `true` if either reviewer was spawned
/// (callers use this for telemetry).
pub fn maybe_spawn_reviewers(
    cfg: &AgentConfig,
    memory_counter: &MemoryNudgeCounter,
    skill_counter: &crate::skill_reviewer::SkillNudgeCounter,
    messages: &[theo_infra_llm::types::Message],
    tool_calls_this_task: usize,
    skill_created_this_task: bool,
) -> bool {
    let mut spawned = false;
    if matches!(
        should_trigger_memory_review(cfg, memory_counter),
        MemoryReviewTrigger::ShouldSpawn
    ) && let Some(reviewer) = cfg.memory().reviewer.cloned()
    {
        let window = recent_review_window(messages, cfg.memory().review_nudge_interval);
        // Fire-and-forget: dropping the handle detaches intentionally.
        drop(spawn_memory_reviewer(reviewer, window));
        spawned = true;
    }

    if matches!(
        crate::skill_reviewer::should_trigger_skill_review(
            cfg.skill_review_nudge_interval,
            skill_counter,
            tool_calls_this_task,
            skill_created_this_task,
            cfg.skill_reviewer.is_some(),
        ),
        crate::skill_reviewer::SkillReviewTrigger::ShouldSpawn
    ) && let Some(reviewer) = cfg.skill_reviewer.clone()
    {
        let window = recent_review_window(messages, cfg.skill_review_nudge_interval.max(10));
        // Fire-and-forget: dropping the handle detaches intentionally.
        drop(crate::skill_reviewer::spawn_skill_reviewer(reviewer, window));
        spawned = true;
    }

    spawned
}

/// Spawn the background reviewer in a fire-and-forget task.
///
/// Failures are logged via stderr and never propagate back to the
/// caller — per AC-1.5. Returns the `JoinHandle` so tests can `await`
/// completion when they need deterministic assertions; in production
/// the handle is dropped immediately.
pub fn spawn_memory_reviewer(
    handle: crate::memory_reviewer::MemoryReviewerHandle,
    window: Vec<theo_infra_llm::types::Message>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // No tracing dep is wired into theo-agent-runtime yet — emit to
        // stderr only when the reviewer reports a failure. Success is
        // silent to avoid polluting stdout during interactive sessions.
        if let Err(err) = handle.as_reviewer().review(&window).await {
            eprintln!("[theo::memory_reviewer] background review failed: {err}");
        }
    })
}
