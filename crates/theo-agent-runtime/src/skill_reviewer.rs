//! Skill reviewer trait — 
//!
//! Background skill reviewer spawned after `skill_review_nudge_interval`
//! tool iterations in a task without any skill being created. Mirror of
//! `memory_reviewer` for procedural-knowledge capture.
//!
//! Reference pattern: `referencias/hermes-agent/run_agent.py:2756-2778`
//! (skill review prompt) and `run_agent.py:11846-11875` (post-loop
//! nudge check + spawn decision).
//!
//! Our Rust adaptation uses `AtomicUsize` on the `RunEngine` so
//! iteration counters survive sub-agent forks without the gateway
//! reset bug (Hermes Issue #8506).

use std::sync::Arc;

use async_trait::async_trait;
use theo_infra_llm::types::Message;
use thiserror::Error;

/// Prompt lifted verbatim from Hermes — kept here so downstream
/// implementations produce the same behaviour.
/// Source: `referencias/hermes-agent/run_agent.py:2756-2764`.
pub const DEFAULT_SKILL_REVIEW_PROMPT: &str = concat!(
    "Review the conversation above and consider saving or updating a skill if appropriate.\n\n",
    "Focus on: was a non-trivial approach used to complete a task that required trial ",
    "and error, or changing course due to experiential findings along the way, or did ",
    "the user expect or desire a different method or outcome?\n\n",
    "If a relevant skill already exists, update it with what you learned. ",
    "Otherwise, create a new skill if the approach is reusable.\n",
    "If nothing is worth saving, just say 'Nothing to save.' and stop."
);

#[derive(Debug, Error)]
pub enum SkillReviewError {
    #[error("skill reviewer backend failure: {0}")]
    Backend(String),
    #[error("skill reviewer timed out after {0:?}")]
    Timeout(std::time::Duration),
    #[error("skill reviewer invoked with empty message window")]
    EmptyWindow,
    #[error("skill reviewer produced a body that failed security scan: {0}")]
    Rejected(String),
}

/// Action the reviewer wants performed on the skill catalog. Mirrors
/// the five operations exposed by Hermes's `skill_manage` tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillAction {
    /// Create a brand-new skill. `category` is optional; defaults to
    /// `general`.
    Create {
        name: String,
        category: Option<String>,
        body: String,
    },
    /// Full rewrite of `SKILL.md` for an existing skill.
    Edit { name: String, body: String },
    /// Targeted find-and-replace.
    Patch {
        name: String,
        old_string: String,
        new_string: String,
        file_path: Option<String>,
    },
    /// Remove the skill (agent-owned only — callers must check).
    Delete { name: String },
    /// Nothing worth saving.
    NoOp,
}

#[async_trait]
pub trait SkillReviewer: Send + Sync {
    /// Examine the conversation window and decide whether to
    /// create/patch/edit a skill. Must be safe to drop mid-flight.
    async fn review(
        &self,
        conversation: &[Message],
    ) -> Result<SkillAction, SkillReviewError>;

    /// Short name for logs/tests.
    fn name(&self) -> &'static str;
}

/// No-op reviewer. Always returns `SkillAction::NoOp`.
#[derive(Debug, Clone, Default)]
pub struct NullSkillReviewer;

#[async_trait]
impl SkillReviewer for NullSkillReviewer {
    async fn review(&self, _: &[Message]) -> Result<SkillAction, SkillReviewError> {
        Ok(SkillAction::NoOp)
    }
    fn name(&self) -> &'static str {
        "null"
    }
}

#[derive(Clone)]
pub struct SkillReviewerHandle(pub Arc<dyn SkillReviewer>);

impl SkillReviewerHandle {
    pub fn new(r: Arc<dyn SkillReviewer>) -> Self {
        Self(r)
    }
    pub fn as_reviewer(&self) -> &dyn SkillReviewer {
        self.0.as_ref()
    }
}

impl std::fmt::Debug for SkillReviewerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SkillReviewerHandle")
            .field(&self.0.name())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Nudge counter — mirror of memory_reviewer::MemoryNudgeCounter.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct SkillNudgeCounter {
    inner: std::sync::atomic::AtomicUsize,
}

impl SkillNudgeCounter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self) -> usize {
        self.inner.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.inner.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_by(&self, n: usize) -> usize {
        self.inner
            .fetch_add(n, std::sync::atomic::Ordering::Relaxed)
            .saturating_add(n)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillReviewTrigger {
    /// Counter below threshold — keep going.
    NotReady,
    /// Counter hit threshold; reviewer should be spawned, counter reset.
    ShouldSpawn,
    /// Feature explicitly disabled.
    Disabled,
}

/// Decide whether the skill reviewer should fire.
///
/// Unlike memory nudges (turn-based), skill nudges are
/// tool-iteration-based: the caller passes the number of tool calls
/// made in the completed task. The reviewer only fires when no skill
/// was created during the task (tracked by `skill_created_in_task`).
/// Matches Hermes `run_agent.py:11848-11852`.
pub fn should_trigger_skill_review(
    interval: usize,
    counter: &SkillNudgeCounter,
    tool_calls_in_task: usize,
    skill_created_in_task: bool,
    has_reviewer: bool,
) -> SkillReviewTrigger {
    if interval == 0 || !has_reviewer || skill_created_in_task {
        return SkillReviewTrigger::Disabled;
    }
    let total = counter.increment_by(tool_calls_in_task);
    if total >= interval {
        counter.reset();
        SkillReviewTrigger::ShouldSpawn
    } else {
        SkillReviewTrigger::NotReady
    }
}

/// Spawn the background skill reviewer. Fire-and-forget. Errors are
/// logged to stderr and swallowed (AC-1.5 equivalent for skills).
pub fn spawn_skill_reviewer(
    handle: SkillReviewerHandle,
    window: Vec<Message>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        match handle.as_reviewer().review(&window).await {
            Ok(SkillAction::NoOp) => {}
            Ok(action) => {
                tracing::info!(
                    action = ?action,
                    "skill_reviewer action ready (executor wiring happens at application layer)"
                );
            }
            Err(err) => {
                tracing::warn!(error = %err, "skill_reviewer background review failed");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_null_reviewer_returns_noop() {
        let r = NullSkillReviewer;
        assert_eq!(r.review(&[]).await.unwrap(), SkillAction::NoOp);
    }

    #[test]
    fn test_counter_increments_by_tool_call_count() {
        let c = SkillNudgeCounter::new();
        assert_eq!(c.increment_by(3), 3);
        assert_eq!(c.increment_by(2), 5);
        assert_eq!(c.get(), 5);
    }

    #[test]
    fn test_trigger_disabled_when_zero_interval() {
        let c = SkillNudgeCounter::new();
        let trig = should_trigger_skill_review(0, &c, 10, false, true);
        assert_eq!(trig, SkillReviewTrigger::Disabled);
        assert_eq!(c.get(), 0);
    }

    #[test]
    fn test_trigger_disabled_when_no_reviewer() {
        let c = SkillNudgeCounter::new();
        let trig = should_trigger_skill_review(5, &c, 10, false, false);
        assert_eq!(trig, SkillReviewTrigger::Disabled);
    }

    #[test]
    fn test_trigger_disabled_when_skill_already_created_in_task() {
        let c = SkillNudgeCounter::new();
        let trig = should_trigger_skill_review(5, &c, 10, true, true);
        assert_eq!(trig, SkillReviewTrigger::Disabled);
    }

    #[test]
    fn test_trigger_not_ready_when_under_threshold() {
        let c = SkillNudgeCounter::new();
        let trig = should_trigger_skill_review(10, &c, 3, false, true);
        assert_eq!(trig, SkillReviewTrigger::NotReady);
        assert_eq!(c.get(), 3);
    }

    #[test]
    fn test_trigger_spawn_at_threshold_and_resets() {
        let c = SkillNudgeCounter::new();
        let trig = should_trigger_skill_review(5, &c, 5, false, true);
        assert_eq!(trig, SkillReviewTrigger::ShouldSpawn);
        assert_eq!(c.get(), 0);
    }

    #[test]
    fn test_trigger_accumulates_across_tasks() {
        // Two tasks with 3 tool calls each → 6 total → fires at 5.
        let c = SkillNudgeCounter::new();
        let t1 = should_trigger_skill_review(5, &c, 3, false, true);
        assert_eq!(t1, SkillReviewTrigger::NotReady);
        let t2 = should_trigger_skill_review(5, &c, 3, false, true);
        assert_eq!(t2, SkillReviewTrigger::ShouldSpawn);
    }

    #[tokio::test]
    async fn test_spawn_with_failing_reviewer_does_not_panic() {
        struct Fail;
        #[async_trait]
        impl SkillReviewer for Fail {
            async fn review(&self, _: &[Message]) -> Result<SkillAction, SkillReviewError> {
                Err(SkillReviewError::Backend("boom".into()))
            }
            fn name(&self) -> &'static str {
                "fail"
            }
        }
        let handle = SkillReviewerHandle::new(Arc::new(Fail));
        spawn_skill_reviewer(handle, vec![Message::user("hi")])
            .await
            .expect("task should complete");
    }

    #[test]
    fn test_default_prompt_mentions_skill_save_semantics() {
        assert!(DEFAULT_SKILL_REVIEW_PROMPT.contains("saving or updating a skill"));
        assert!(DEFAULT_SKILL_REVIEW_PROMPT.contains("Nothing to save"));
    }
}
