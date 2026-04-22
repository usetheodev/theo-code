//! Memory reviewer trait — Phase 1 of PLAN_AUTO_EVOLUTION_SOTA.
//!
//! Background memory reviewer spawned after `memory_review_nudge_interval`
//! turns. Runs fire-and-forget so it never competes with the user's task
//! for model attention.
//!
//! Reference pattern: `referencias/hermes-agent/run_agent.py:2745-2879`
//! (memory review prompt + `_spawn_background_review` thread spawn).
//!
//! Rust adaptation:
//! - `AtomicUsize` counter on `RunEngine` replaces Hermes's per-instance
//!   `self._turns_since_memory`. Eliminates Hermes Issue #8506 (gateway
//!   mode resets counter to 0 on each message because a fresh AIAgent is
//!   instantiated per request).
//! - `tokio::spawn` with owned clones replaces `threading.Thread(daemon=True)`.
//! - Trait + implementations so Null/Llm variants can be swapped without
//!   changing `memory_lifecycle.rs` callers.
//!
//! Anti-recursion: forked reviewer agents MUST set
//! `memory_review_nudge_interval = 0` to prevent reviewers spawning
//! reviewers. Matches Hermes `run_agent.py:2820` pattern.
//!
//! Errors are logged via `tracing::warn!` and never propagated to the
//! main loop — background reviewers are best-effort.

use std::sync::Arc;

use async_trait::async_trait;
use theo_infra_llm::types::Message;
use thiserror::Error;

/// Hermes-compatible default prompt for memory review.
/// Source: `referencias/hermes-agent/run_agent.py:2745-2754`.
pub const DEFAULT_MEMORY_REVIEW_PROMPT: &str = concat!(
    "Review the conversation above and consider saving to memory if appropriate.\n\n",
    "Focus on:\n",
    "1. Has the user revealed things about themselves - their persona, desires, ",
    "preferences, or personal details worth remembering?\n",
    "2. Has the user expressed expectations about how you should behave, their work ",
    "style, or ways they want you to operate?\n\n",
    "If something stands out, save it using the memory tool. ",
    "If nothing is worth saving, just say 'Nothing to save.' and stop."
);

/// Errors that can occur during a memory review pass.
#[derive(Debug, Error)]
pub enum MemoryReviewError {
    /// LLM or downstream I/O failure. Carries the underlying message
    /// rather than a boxed error so the trait stays object-safe without
    /// cloning unboxed errors around.
    #[error("memory reviewer backend failure: {0}")]
    Backend(String),
    /// Reviewer timed out before completing.
    #[error("memory reviewer timed out after {0:?}")]
    Timeout(std::time::Duration),
    /// Reviewer was invoked with no messages to examine.
    #[error("memory reviewer invoked with empty message window")]
    EmptyWindow,
}

/// Behaviour contract for anything that can review a conversation window
/// and persist extracted facts to the memory subsystem.
#[async_trait]
pub trait MemoryReviewer: Send + Sync {
    /// Review recent turns and return the number of memory entries the
    /// reviewer decided to persist. Implementations must be safe to drop
    /// mid-flight — the caller will `tokio::spawn` the future and never
    /// `await` its completion for correctness, only for telemetry.
    async fn review(&self, recent_turns: &[Message]) -> Result<usize, MemoryReviewError>;

    /// Short human-readable name (e.g. `"llm"`, `"null"`). Used by
    /// tracing spans and unit tests so we don't need `Debug` on impls.
    fn name(&self) -> &'static str;
}

/// No-op reviewer. Used when memory review is disabled or as a default
/// before the production executor is wired. Returns 0 entries on every
/// invocation without touching any backing store.
#[derive(Debug, Clone, Default)]
pub struct NullMemoryReviewer;

#[async_trait]
impl MemoryReviewer for NullMemoryReviewer {
    async fn review(&self, _recent_turns: &[Message]) -> Result<usize, MemoryReviewError> {
        Ok(0)
    }

    fn name(&self) -> &'static str {
        "null"
    }
}

/// Debug-friendly wrapper around `Arc<dyn MemoryReviewer>` so
/// `AgentConfig` keeps its `#[derive(Debug, Clone)]` without forcing a
/// `Debug` bound into the trait. Matches the pattern used by
/// `MemoryHandle` and `RouterHandle`.
#[derive(Clone)]
pub struct MemoryReviewerHandle(pub Arc<dyn MemoryReviewer>);

impl MemoryReviewerHandle {
    pub fn new(reviewer: Arc<dyn MemoryReviewer>) -> Self {
        Self(reviewer)
    }

    pub fn as_reviewer(&self) -> &dyn MemoryReviewer {
        self.0.as_ref()
    }
}

impl std::fmt::Debug for MemoryReviewerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MemoryReviewerHandle")
            .field(&self.0.name())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_infra_llm::types::Message;

    #[tokio::test]
    async fn test_null_reviewer_returns_zero_entries() {
        // Arrange.
        let reviewer = NullMemoryReviewer;
        let msgs = vec![Message::user("hello")];

        // Act.
        let out = reviewer.review(&msgs).await.expect("null reviewer never errors");

        // Assert.
        assert_eq!(out, 0);
    }

    #[tokio::test]
    async fn test_null_reviewer_accepts_empty_window() {
        // Null reviewer intentionally does NOT enforce the empty-window
        // rule so tests that stub it out don't need to fabricate turns.
        let reviewer = NullMemoryReviewer;
        let out = reviewer.review(&[]).await.expect("null reviewer never errors");
        assert_eq!(out, 0);
    }

    #[test]
    fn test_default_prompt_mentions_memory_tool() {
        // Ensure we don't accidentally regress the Hermes-equivalent
        // prompt into something that doesn't instruct saving.
        assert!(DEFAULT_MEMORY_REVIEW_PROMPT.contains("memory tool"));
        assert!(DEFAULT_MEMORY_REVIEW_PROMPT.contains("Nothing to save"));
    }

    #[test]
    fn test_handle_debug_shows_name() {
        let handle = MemoryReviewerHandle::new(Arc::new(NullMemoryReviewer));
        let dbg = format!("{handle:?}");
        assert!(dbg.contains("null"), "expected name in debug: {dbg}");
    }
}
