//! Memory lifecycle helper (plan phase RM0).
//!
//! Central dispatch for the four MemoryProvider hooks called from the
//! agent loop. Every entry point short-circuits to a no-op when the
//! feature flag `AgentConfig.memory.enabled` is false or when no provider
//! is configured — runtime behaviour is identical to pre-RM0 in that
//! case. Keeps the hot path in `run_engine.rs` free of explicit
//! `if memory_enabled && provider.is_some() { ... }` noise.
//!
//! Reference: `referencias/hermes-agent/agent/memory_manager.py:97-206`
//! (fan-out + error isolation) and plan `outputs/agent-memory-plan.md` §RM0.

use theo_domain::memory::build_memory_context_block;

use crate::config::AgentConfig;

/// Entry point for the four hooks. Methods borrow from `AgentConfig`
/// rather than owning state so the helper stays zero-size.
pub struct MemoryLifecycle;

impl MemoryLifecycle {
    /// Pre-LLM hook. Returns a fenced memory block for injection into the
    /// next LLM prompt, or an empty string when memory is disabled or
    /// the provider has nothing relevant.
    pub async fn prefetch(cfg: &AgentConfig, query: &str) -> String {
        let Some(handle) = Self::active_handle(cfg) else {
            return String::new();
        };
        let raw = handle.as_provider().prefetch(query).await;
        if raw.is_empty() {
            String::new()
        } else {
            build_memory_context_block(&raw)
        }
    }

    /// Post-LLM hook. Persists the just-completed exchange. Silent on
    /// disabled/no-provider (pre-RM0 behaviour).
    pub async fn sync_turn(cfg: &AgentConfig, user: &str, assistant: &str) {
        if let Some(handle) = Self::active_handle(cfg) {
            handle.as_provider().sync_turn(user, assistant).await;
        }
    }

    /// Invoked just before compaction destroys message detail. Returns
    /// any fact-extraction payload the provider generated (empty string
    /// when disabled).
    pub async fn on_pre_compress(cfg: &AgentConfig, messages_as_text: &str) -> String {
        let Some(handle) = Self::active_handle(cfg) else {
            return String::new();
        };
        handle.as_provider().on_pre_compress(messages_as_text).await
    }

    /// Session lifecycle hook — called at convergence/abort.
    pub async fn on_session_end(cfg: &AgentConfig) {
        if let Some(handle) = Self::active_handle(cfg) {
            handle.as_provider().on_session_end().await;
        }
    }

    fn active_handle(cfg: &AgentConfig) -> Option<&crate::config::MemoryHandle> {
        let mem = cfg.memory();
        if mem.enabled { mem.provider.as_ref() } else { None }
    }
}

// ---------------------------------------------------------------------------
// PLAN_AUTO_EVOLUTION_SOTA: nudge counter + background reviewer.
// ---------------------------------------------------------------------------

/// Decision emitted by [`should_trigger_memory_review`].
///
/// Splitting the counter bookkeeping from the actual `tokio::spawn` call
/// lets us cover the counter logic with synchronous unit tests (no async
/// runtime / no real LLM) while the spawn wrapper stays as a tiny
/// wiring-only helper with integration tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryReviewTrigger {
    /// Counter below threshold — keep going.
    NotReady,
    /// Counter hit threshold; reviewer should be spawned, counter reset.
    ShouldSpawn,
    /// Feature explicitly disabled (`interval == 0` or no reviewer wired).
    Disabled,
}

/// Atomic nudge counter for memory reviewer spawning.
///
/// Separate type so `RunEngine` can own one and the logic stays testable
/// without mocking the whole engine. Matches Hermes
/// `run_agent.py:1420 (_turns_since_memory = 0)` but lifted to
/// `AtomicUsize` so it survives across `run_conversation` calls without
/// hitting Issue #8506 (gateway reset).
#[derive(Debug, Default)]
pub struct MemoryNudgeCounter {
    inner: std::sync::atomic::AtomicUsize,
}

impl MemoryNudgeCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the current counter value without modifying it.
    pub fn get(&self) -> usize {
        self.inner.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Resets the counter to zero (used after spawning a reviewer or
    /// when a sub-agent fork needs anti-recursion wiring).
    pub fn reset(&self) {
        self.inner.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Increments the counter and returns the new value.
    pub fn increment(&self) -> usize {
        self.inner
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .saturating_add(1)
    }
}

/// Decide whether the background memory reviewer should be spawned now.
///
/// Increments the counter on `Active` memory configs; returns
/// `Disabled` without touching the counter when the feature is off. The
/// caller receives the decision and spawns a task when `ShouldSpawn` is
/// returned.
///
/// Reference: `referencias/hermes-agent/run_agent.py:8747-8753`.
pub fn should_trigger_memory_review(
    cfg: &AgentConfig,
    counter: &MemoryNudgeCounter,
) -> MemoryReviewTrigger {
    // Disabled when: interval == 0, memory off, no provider, or no
    // reviewer wired. The provider check matches Hermes's
    // `"memory" in self.valid_tool_names` guard.
    let mem = cfg.memory();
    if mem.review_nudge_interval == 0
        || !mem.enabled
        || mem.provider.is_none()
        || mem.reviewer.is_none()
    {
        return MemoryReviewTrigger::Disabled;
    }

    let current = counter.increment();
    if current >= mem.review_nudge_interval {
        counter.reset();
        MemoryReviewTrigger::ShouldSpawn
    } else {
        MemoryReviewTrigger::NotReady
    }
}

/// Select the most recent window of messages to hand to the reviewer.
///
/// Caps at `min(interval, 20)` to keep the reviewer's prompt small
/// enough to fit a lightweight review model even in long sessions.
/// Matches the AC-1.6 bound.
pub fn recent_review_window(
    messages: &[theo_infra_llm::types::Message],
    interval: usize,
) -> Vec<theo_infra_llm::types::Message> {
    let take = interval.clamp(1, 20);
    let start = messages.len().saturating_sub(take);
    messages[start..].to_vec()
}

// ─── Wiring helpers (invoked from run_engine.rs) ───
// Moved to `wiring.rs`; re-exported here so the public path
// `crate::memory_lifecycle::maybe_*` stays byte-identical for all callers.
mod wiring;
pub use wiring::{
    maybe_index_transcript, maybe_prepend_bootstrap, maybe_spawn_autodream,
    maybe_spawn_reviewers, spawn_memory_reviewer,
};

#[cfg(test)]
mod phase1_tests {
    use super::*;
    use crate::memory_reviewer::{MemoryReviewError, MemoryReviewerHandle, NullMemoryReviewer};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering as AOrdering};
    use theo_domain::memory::MemoryProvider;
    use theo_infra_llm::types::Message;

    /// Minimal provider stub so we can flip `memory_provider.is_some()`
    /// without pulling in the full BuiltinMemoryProvider.
    struct StubProvider;

    #[async_trait::async_trait]
    impl MemoryProvider for StubProvider {
        fn name(&self) -> &str {
            "stub"
        }
        async fn prefetch(&self, _query: &str) -> String {
            String::new()
        }
        async fn sync_turn(&self, _user: &str, _assistant: &str) {}
    }

    fn cfg_with_reviewer(interval: usize, reviewer: Option<MemoryReviewerHandle>) -> AgentConfig {
        AgentConfig {
            memory: crate::config::MemoryConfig {
                enabled: true,
                provider: Some(crate::config::MemoryHandle::new(Arc::new(StubProvider))),
                review_nudge_interval: interval,
                reviewer,
                ..crate::config::MemoryConfig::default()
            },
            ..AgentConfig::default()
        }
    }

    // ── AC-1.1 ─────────────────────────────────────────────────────
    #[test]
    fn test_ac_1_1_counter_increments_on_each_call() {
        let cfg = cfg_with_reviewer(
            10,
            Some(MemoryReviewerHandle::new(Arc::new(NullMemoryReviewer))),
        );
        let counter = MemoryNudgeCounter::new();

        assert_eq!(counter.get(), 0);
        for expected in 1..=9 {
            let trig = should_trigger_memory_review(&cfg, &counter);
            assert_eq!(trig, MemoryReviewTrigger::NotReady);
            assert_eq!(counter.get(), expected);
        }
    }

    // ── AC-1.2 + AC-1.3 ────────────────────────────────────────────
    #[test]
    fn test_ac_1_2_spawn_triggers_at_threshold_and_counter_resets() {
        let cfg = cfg_with_reviewer(
            3,
            Some(MemoryReviewerHandle::new(Arc::new(NullMemoryReviewer))),
        );
        let counter = MemoryNudgeCounter::new();

        assert_eq!(should_trigger_memory_review(&cfg, &counter), MemoryReviewTrigger::NotReady);
        assert_eq!(should_trigger_memory_review(&cfg, &counter), MemoryReviewTrigger::NotReady);
        assert_eq!(
            should_trigger_memory_review(&cfg, &counter),
            MemoryReviewTrigger::ShouldSpawn
        );
        assert_eq!(counter.get(), 0, "counter must reset after spawn");
    }

    // ── AC-1.4 ─────────────────────────────────────────────────────
    #[test]
    fn test_ac_1_4_zero_interval_disables_feature() {
        let cfg = cfg_with_reviewer(
            0,
            Some(MemoryReviewerHandle::new(Arc::new(NullMemoryReviewer))),
        );
        let counter = MemoryNudgeCounter::new();
        assert_eq!(
            should_trigger_memory_review(&cfg, &counter),
            MemoryReviewTrigger::Disabled
        );
        assert_eq!(counter.get(), 0, "counter untouched when disabled");
    }

    #[test]
    fn test_disabled_when_no_reviewer_even_if_interval_set() {
        let cfg = cfg_with_reviewer(3, None);
        let counter = MemoryNudgeCounter::new();
        assert_eq!(
            should_trigger_memory_review(&cfg, &counter),
            MemoryReviewTrigger::Disabled
        );
    }

    #[test]
    fn test_disabled_when_memory_off() {
        let mut cfg = cfg_with_reviewer(
            3,
            Some(MemoryReviewerHandle::new(Arc::new(NullMemoryReviewer))),
        );
        cfg.memory.enabled = false;
        let counter = MemoryNudgeCounter::new();
        assert_eq!(
            should_trigger_memory_review(&cfg, &counter),
            MemoryReviewTrigger::Disabled
        );
    }

    // ── AC-1.6 ─────────────────────────────────────────────────────
    #[test]
    fn test_ac_1_6_window_respects_interval_and_20_msg_cap() {
        let msgs: Vec<Message> = (0..30).map(|i| Message::user(format!("m{i}"))).collect();

        // interval < 20 → take `interval` most recent
        let w = recent_review_window(&msgs, 5);
        assert_eq!(w.len(), 5);
        assert_eq!(
            w.last().and_then(|m| m.content.clone()),
            Some("m29".to_string())
        );

        // interval > 20 → capped at 20
        let w = recent_review_window(&msgs, 100);
        assert_eq!(w.len(), 20);

        // messages shorter than interval → return all
        let short: Vec<Message> = (0..3).map(|i| Message::user(format!("s{i}"))).collect();
        let w = recent_review_window(&short, 10);
        assert_eq!(w.len(), 3);
    }

    // ── AC-1.5 ─────────────────────────────────────────────────────
    struct FailingReviewer {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl crate::memory_reviewer::MemoryReviewer for FailingReviewer {
        async fn review(&self, _: &[Message]) -> Result<usize, MemoryReviewError> {
            self.calls.fetch_add(1, AOrdering::Relaxed);
            Err(MemoryReviewError::Backend("boom".into()))
        }
        fn name(&self) -> &'static str {
            "failing"
        }
    }

    #[tokio::test]
    async fn test_ac_1_5_reviewer_failure_does_not_crash_spawn() {
        let calls = Arc::new(AtomicUsize::new(0));
        let reviewer = Arc::new(FailingReviewer { calls: calls.clone() });
        let handle = MemoryReviewerHandle::new(reviewer);

        // The spawn helper must complete cleanly even when the reviewer
        // returns an error. Awaiting the JoinHandle must not yield an
        // error panic.
        let jh = spawn_memory_reviewer(
            handle,
            vec![Message::user("hello")],
        );
        jh.await.expect("spawn_memory_reviewer must never panic");
        assert_eq!(calls.load(AOrdering::Relaxed), 1);
    }

    // ── AC-1.7 — reviewer clone must zero interval to prevent recursion.
    // Because the reviewer receives just a `Vec<Message>` (not an
    // `AgentConfig`), the anti-recursion invariant is satisfied by
    // construction: the trait can't re-enter `should_trigger_memory_review`
    // without a config, and any LLM-backed reviewer is responsible for
    // zeroing the interval on its own AgentConfig clone. We document
    // the design here and add a contract test that crosschecks the
    // Default config shape.
    #[test]
    fn test_ac_1_7_default_config_disables_reviewer_until_explicitly_wired() {
        let cfg = AgentConfig::default();
        assert!(cfg.memory.reviewer.is_none());
        let counter = MemoryNudgeCounter::new();
        assert_eq!(
            should_trigger_memory_review(&cfg, &counter),
            MemoryReviewTrigger::Disabled
        );
    }
}

/// Run-engine helpers. Moved to a sibling file to keep
/// `memory_lifecycle/mod.rs` below the structural-hygiene cap. Re-exported
/// here as `pub mod` so the public path
/// `crate::memory_lifecycle::run_engine_hooks::*` stays byte-identical for
/// all callers.
pub mod run_engine_hooks;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MemoryHandle;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use theo_domain::memory::{MEMORY_FENCE_OPEN, MemoryProvider, NullMemoryProvider};

    /// Records every hook invocation in order so ACs can assert on the
    /// full sequence.
    #[derive(Default)]
    struct RecordingProvider {
        log: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingProvider {
        fn new() -> (Arc<Self>, Arc<Mutex<Vec<String>>>) {
            let log = Arc::new(Mutex::new(Vec::new()));
            (
                Arc::new(Self { log: log.clone() }),
                log,
            )
        }
    }

    #[async_trait]
    impl MemoryProvider for RecordingProvider {
        fn name(&self) -> &str {
            "recording"
        }
        async fn prefetch(&self, query: &str) -> String {
            self.log
                .lock()
                .expect("t")
                .push(format!("prefetch:{query}"));
            format!("past fact about {query}")
        }
        async fn sync_turn(&self, user: &str, assistant: &str) {
            self.log
                .lock()
                .expect("t")
                .push(format!("sync:{user}>>{assistant}"));
        }
        async fn on_pre_compress(&self, txt: &str) -> String {
            self.log
                .lock()
                .expect("t")
                .push(format!("pre_compress:{}", txt.len()));
            "extracted".to_string()
        }
        async fn on_session_end(&self) {
            self.log.lock().expect("t").push("end".into());
        }
    }

    fn cfg_with(provider: Arc<dyn MemoryProvider>, enabled: bool) -> AgentConfig {
        AgentConfig {
            memory: crate::config::MemoryConfig {
                enabled,
                provider: Some(MemoryHandle::new(provider)),
                ..crate::config::MemoryConfig::default()
            },
            ..AgentConfig::default()
        }
    }

    // ── RM0-AC-1 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_1_prefetch_invokes_provider_when_enabled() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);

        let block = MemoryLifecycle::prefetch(&cfg, "routing").await;

        assert!(
            block.contains(MEMORY_FENCE_OPEN),
            "block must be fenced: {block}"
        );
        assert!(block.contains("past fact about routing"));
        assert_eq!(log.lock().expect("t").first().expect("t"), "prefetch:routing");
    }

    // ── RM0-AC-2 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_2_sync_turn_persists_user_and_assistant() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);
        MemoryLifecycle::sync_turn(&cfg, "hello", "world").await;
        assert_eq!(log.lock().expect("t").last().expect("t"), "sync:hello>>world");
    }

    // ── RM0-AC-3 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_3_on_pre_compress_receives_messages_text() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);
        let out = MemoryLifecycle::on_pre_compress(&cfg, "abc").await;
        assert_eq!(out, "extracted");
        assert_eq!(log.lock().expect("t").last().expect("t"), "pre_compress:3");
    }

    // ── RM0-AC-4 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_4_on_session_end_triggers_provider_close() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);
        MemoryLifecycle::on_session_end(&cfg).await;
        assert_eq!(log.lock().expect("t").last().expect("t"), "end");
    }

    // ── RM0-AC-5 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_5_memory_disabled_short_circuits_all_hooks() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, false);

        let block = MemoryLifecycle::prefetch(&cfg, "q").await;
        MemoryLifecycle::sync_turn(&cfg, "u", "a").await;
        let fx = MemoryLifecycle::on_pre_compress(&cfg, "any").await;
        MemoryLifecycle::on_session_end(&cfg).await;

        assert_eq!(block, "");
        assert_eq!(fx, "");
        assert!(
            log.lock().expect("t").is_empty(),
            "disabled memory must not call provider; got {:?}",
            log.lock().expect("t")
        );
    }

    // ── RM0-AC-6 ─────────────────────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_6_null_provider_preserves_behavior() {
        // With NullMemoryProvider + enabled, hooks complete without side effects.
        let null: Arc<dyn MemoryProvider> = Arc::new(NullMemoryProvider);
        let cfg = cfg_with(null, true);

        let block = MemoryLifecycle::prefetch(&cfg, "anything").await;
        MemoryLifecycle::sync_turn(&cfg, "u", "a").await;
        let fx = MemoryLifecycle::on_pre_compress(&cfg, "m").await;
        MemoryLifecycle::on_session_end(&cfg).await;

        assert_eq!(block, "", "null provider returns empty (no fence)");
        assert_eq!(fx, "");
    }

    // ── RM0-AC-7 (integration) ───────────────────────────────────
    #[tokio::test]
    async fn test_rm0_ac_7_hooks_invoked_in_canonical_order() {
        let (provider, log) = RecordingProvider::new();
        let cfg = cfg_with(provider, true);

        // Canonical sequence for a single-turn session:
        // prefetch → [LLM call happens here] → sync_turn → on_pre_compress
        // (maybe) → on_session_end.
        MemoryLifecycle::prefetch(&cfg, "q").await;
        MemoryLifecycle::sync_turn(&cfg, "u", "a").await;
        MemoryLifecycle::on_pre_compress(&cfg, "mid-session text").await;
        MemoryLifecycle::on_session_end(&cfg).await;

        let entries = log.lock().expect("t").clone();
        assert_eq!(entries.len(), 4);
        assert!(entries[0].starts_with("prefetch:"));
        assert!(entries[1].starts_with("sync:"));
        assert!(entries[2].starts_with("pre_compress:"));
        assert_eq!(entries[3], "end");
    }

    // ── Bonus: no provider + enabled also short-circuits ─────────
    #[tokio::test]
    async fn test_rm0_bonus_enabled_without_provider_is_noop() {
        let cfg = AgentConfig {
            memory: crate::config::MemoryConfig {
                enabled: true,
                provider: None,
                ..crate::config::MemoryConfig::default()
            },
            ..AgentConfig::default()
        };

        assert_eq!(MemoryLifecycle::prefetch(&cfg, "q").await, "");
        MemoryLifecycle::sync_turn(&cfg, "u", "a").await; // no panic
        assert_eq!(MemoryLifecycle::on_pre_compress(&cfg, "x").await, "");
        MemoryLifecycle::on_session_end(&cfg).await;
    }

    // ── inject_episode_history tests ──────────────
    mod t0_3 {
        use super::super::run_engine_hooks::inject_episode_history;
        use theo_infra_llm::types::Message;

        fn write_episode(
            dir: &std::path::Path,
            id: &str,
            lifecycle: &str,
            ttl: serde_json::Value,
            constraints: &[&str],
            failed: &[&str],
            created_at: u64,
        ) {
            let episodes_dir = dir.join(".theo/memory/episodes");
            std::fs::create_dir_all(&episodes_dir).expect("t");
            let payload = serde_json::json!({
                "summary_id": id,
                "run_id": id,
                "task_id": null,
                "window_start_event_id": "",
                "window_end_event_id": "",
                "machine_summary": {
                    "objective": format!("goal-{id}"),
                    "key_actions": [],
                    "outcome": "Success",
                    "successful_steps": [],
                    "failed_attempts": failed,
                    "learned_constraints": constraints,
                    "files_touched": []
                },
                "human_summary": null,
                "evidence_event_ids": [],
                "affected_files": ["src/main.rs"],
                "open_questions": [],
                "unresolved_hypotheses": [],
                "referenced_community_ids": [],
                "supersedes_summary_id": null,
                "schema_version": 1,
                "created_at": created_at,
                "ttl_policy": ttl,
                "lifecycle": lifecycle
            });
            std::fs::write(
                episodes_dir.join(format!("{id}.json")),
                serde_json::to_string(&payload).expect("t"),
            )
            .expect("t");
        }

        #[test]
        fn test_t0_3_ac_1_loads_recent_episodes() {
            let dir = tempfile::tempdir().expect("t");
            write_episode(
                dir.path(),
                "ep-a",
                "Active",
                serde_json::json!("RunScoped"),
                &["no unwrap"],
                &[],
                1,
            );
            let mut messages: Vec<Message> = Vec::new();
            inject_episode_history(dir.path(), 100_000, &mut messages);
            assert_eq!(messages.len(), 1);
            assert!(messages[0].content.as_ref().expect("t").contains("goal-ep-a"));
            assert!(messages[0].content.as_ref().expect("t").contains("no unwrap"));
        }

        #[test]
        fn test_t0_3_ac_2_archived_excluded() {
            let dir = tempfile::tempdir().expect("t");
            write_episode(
                dir.path(),
                "ep-old",
                "Archived",
                serde_json::json!("Permanent"),
                &[],
                &[],
                1,
            );
            let mut messages: Vec<Message> = Vec::new();
            inject_episode_history(dir.path(), 100_000, &mut messages);
            assert!(
                messages.is_empty(),
                "archived episodes must not be injected"
            );
        }

        #[test]
        fn test_t0_3_ac_3_expired_ttl_excluded() {
            let dir = tempfile::tempdir().expect("t");
            // created_at = 1 ms ago, seconds = 0 → expired
            write_episode(
                dir.path(),
                "ep-expired",
                "Active",
                serde_json::json!({"TimeScoped": {"seconds": 0}}),
                &[],
                &[],
                1,
            );
            let mut messages: Vec<Message> = Vec::new();
            inject_episode_history(dir.path(), 100_000, &mut messages);
            assert!(messages.is_empty());
        }

        #[test]
        fn test_t0_3_ac_5_failed_attempts_visible() {
            let dir = tempfile::tempdir().expect("t");
            write_episode(
                dir.path(),
                "ep-fail",
                "Active",
                serde_json::json!("RunScoped"),
                &[],
                &["bash: permission denied"],
                1,
            );
            let mut messages: Vec<Message> = Vec::new();
            inject_episode_history(dir.path(), 100_000, &mut messages);
            assert_eq!(messages.len(), 1);
            assert!(
                messages[0]
                    .content
                    .as_ref()
                    .expect("t")
                    .contains("permission denied")
            );
        }

        #[test]
        fn test_t0_3_ac_6_respects_5pct_token_budget() {
            let dir = tempfile::tempdir().expect("t");
            // Write a huge constraint string to force truncation.
            let huge: String = "x".repeat(100_000);
            write_episode(
                dir.path(),
                "ep-big",
                "Active",
                serde_json::json!("RunScoped"),
                &[huge.as_str()],
                &[],
                1,
            );
            // 1000 tokens * 4 chars / 20 = 200 chars budget.
            let mut messages: Vec<Message> = Vec::new();
            inject_episode_history(dir.path(), 1000, &mut messages);
            assert_eq!(messages.len(), 1);
            let body = messages[0].content.as_ref().expect("t");
            assert!(
                body.len() <= 260,
                "must respect 5% budget, got {} chars",
                body.len()
            );
            assert!(body.contains("truncated"), "must mark truncation");
        }

        #[test]
        fn test_t0_3_ac_7_no_episodes_is_noop() {
            let dir = tempfile::tempdir().expect("t");
            let mut messages: Vec<Message> = Vec::new();
            inject_episode_history(dir.path(), 100_000, &mut messages);
            assert!(messages.is_empty(), "no episodes → no system message");
        }
    }
}
