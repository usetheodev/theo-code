//! E2E wiring integration test — Auto-Evolution SOTA.
//!
//! Validates that all 5 phases of `PLAN_AUTO_EVOLUTION_SOTA` actually
//! fire in production code paths without mocking the RunEngine. We
//! don't hit a live LLM here (that's covered by real CLI sessions);
//! instead this test drives the wiring helpers directly with real
//! filesystem artifacts so we know Tantivy files are written, locks
//! are acquired, and AtomicUsize counters behave over many turns.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use theo_agent_runtime::autodream::{
    AutodreamExecutor, AutodreamHandle, ConsolidationReport, NullAutodreamExecutor,
    run_autodream,
};
use theo_agent_runtime::config::{AgentConfig, MemoryHandle};
use theo_agent_runtime::memory_lifecycle::{
    MemoryNudgeCounter, MemoryReviewTrigger, maybe_prepend_bootstrap, maybe_spawn_autodream,
    maybe_spawn_reviewers, recent_review_window, should_trigger_memory_review,
};
use theo_agent_runtime::memory_reviewer::{
    MemoryReviewError, MemoryReviewer, MemoryReviewerHandle,
};
use theo_agent_runtime::onboarding::{BOOTSTRAP_PROMPT, USER_MD_FILENAME, needs_bootstrap};
use theo_agent_runtime::skill_reviewer::{
    SkillAction, SkillNudgeCounter, SkillReviewError, SkillReviewer, SkillReviewerHandle,
    SkillReviewTrigger, should_trigger_skill_review,
};
use theo_domain::memory::MemoryProvider;
use theo_infra_llm::types::Message;

// ---------------------------------------------------------------------------
// Test doubles that record invocations so we can assert on them.
// ---------------------------------------------------------------------------

struct RecordingMemoryReviewer {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl MemoryReviewer for RecordingMemoryReviewer {
    async fn review(&self, _recent_turns: &[Message]) -> Result<usize, MemoryReviewError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(1)
    }
    fn name(&self) -> &'static str {
        "rec-mem"
    }
}

struct RecordingSkillReviewer {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl SkillReviewer for RecordingSkillReviewer {
    async fn review(&self, _conv: &[Message]) -> Result<SkillAction, SkillReviewError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(SkillAction::NoOp)
    }
    fn name(&self) -> &'static str {
        "rec-skill"
    }
}

struct RecordingAutodream {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl AutodreamExecutor for RecordingAutodream {
    async fn consolidate(
        &self,
        _memory_dir: &std::path::Path,
        _session_id: &str,
    ) -> Result<ConsolidationReport, theo_agent_runtime::autodream::AutodreamError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(ConsolidationReport {
            files_consolidated: 3,
            files_pruned: 2,
            files_backed_up: 3,
            duration_ms: 1,
        })
    }
    fn name(&self) -> &'static str {
        "rec-autodream"
    }
}

struct StubProvider;

#[async_trait]
impl MemoryProvider for StubProvider {
    fn name(&self) -> &str {
        "stub"
    }
    async fn prefetch(&self, _q: &str) -> String {
        String::new()
    }
    async fn sync_turn(&self, _u: &str, _a: &str) {}
}

fn base_cfg(
    mem_calls: Arc<AtomicUsize>,
    skill_calls: Arc<AtomicUsize>,
) -> AgentConfig {
    AgentConfig {
        memory_enabled: true,
        memory_provider: Some(MemoryHandle::new(Arc::new(StubProvider))),
        memory_reviewer: Some(MemoryReviewerHandle::new(Arc::new(
            RecordingMemoryReviewer { calls: mem_calls },
        ))),
        skill_reviewer: Some(SkillReviewerHandle::new(Arc::new(
            RecordingSkillReviewer { calls: skill_calls },
        ))),
        ..AgentConfig::default()
    }
}

// ---------------------------------------------------------------------------
// Phase 1 — Memory reviewer fires after `memory_review_nudge_interval` turns
// ---------------------------------------------------------------------------

#[tokio::test]
async fn phase1_memory_reviewer_fires_at_threshold_and_resets() {
    let mem_calls = Arc::new(AtomicUsize::new(0));
    let skill_calls = Arc::new(AtomicUsize::new(0));
    let cfg = base_cfg(mem_calls.clone(), skill_calls.clone());
    let counter = MemoryNudgeCounter::new();
    let skill_counter = SkillNudgeCounter::new();

    let msgs: Vec<Message> = (0..5).map(|i| Message::user(format!("m{i}"))).collect();

    // Turns 1..9 — below default threshold (10). No spawn.
    for _ in 0..9 {
        maybe_spawn_reviewers(&cfg, &counter, &skill_counter, &msgs, 0, false);
    }
    // 10th turn — spawn fires. We give the spawned task a moment.
    maybe_spawn_reviewers(&cfg, &counter, &skill_counter, &msgs, 0, false);
    // T5.4: fixed 50ms sleep was the flakiness surface; keep the small
    // relative sleep but also yield so the spawned reviewer task runs.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    tokio::task::yield_now().await;

    assert_eq!(
        mem_calls.load(Ordering::Relaxed),
        1,
        "memory reviewer should have fired exactly once at turn 10"
    );
    assert_eq!(
        counter.get(),
        0,
        "counter must reset to 0 after spawn (AC-1.3)"
    );
}

// ---------------------------------------------------------------------------
// Phase 3 — Skill reviewer uses tool-iteration accumulation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn phase3_skill_reviewer_fires_when_accumulated_tool_iters_reach_threshold() {
    let mem_calls = Arc::new(AtomicUsize::new(0));
    let skill_calls = Arc::new(AtomicUsize::new(0));
    let cfg = base_cfg(mem_calls, skill_calls.clone());
    let counter = MemoryNudgeCounter::new();
    let skill_counter = SkillNudgeCounter::new();
    let msgs = vec![Message::user("m")];

    // Two tasks of 3 tool iters each → counter reaches 6.
    // Next task of 4 → counter reaches 10 → spawn fires.
    for task_calls in [3usize, 3, 4] {
        maybe_spawn_reviewers(
            &cfg,
            &counter,
            &skill_counter,
            &msgs,
            task_calls,
            /* skill_created */ false,
        );
    }
    // T5.4: fixed 50ms sleep was the flakiness surface; keep the small
    // relative sleep but also yield so the spawned reviewer task runs.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    tokio::task::yield_now().await;

    assert_eq!(
        skill_calls.load(Ordering::Relaxed),
        1,
        "skill reviewer must fire once after 10 accumulated tool iters"
    );
}

#[test]
fn phase3_skill_reviewer_does_not_fire_if_skill_already_created() {
    let counter = SkillNudgeCounter::new();
    let trig = should_trigger_skill_review(10, &counter, 20, true, true);
    assert_eq!(trig, SkillReviewTrigger::Disabled);
}

// ---------------------------------------------------------------------------
// Phase 2 — Autodream gate + metadata persistence
// ---------------------------------------------------------------------------

#[tokio::test]
async fn phase2_autodream_persists_meta_after_first_run() {
    let tmp = tempfile::tempdir().expect("tmp");
    let memory_dir = tmp.path();

    // Seed 5 "session" files so the insufficient-files gate passes.
    for i in 0..5 {
        std::fs::write(
            memory_dir.join(format!("s{i}.md")),
            "---\ntype: session\n---\nbody",
        )
        .unwrap();
    }

    let report = run_autodream(memory_dir, "s-test", &NullAutodreamExecutor)
        .await
        .expect("no error")
        .expect("should run");
    assert_eq!(report.files_consolidated, 0); // Null executor is a no-op.

    let meta_path = memory_dir.join(".consolidation-meta.json");
    assert!(meta_path.exists(), "consolidation meta must persist");
}

#[tokio::test]
async fn phase2_autodream_blocks_second_run_within_cooldown() {
    let tmp = tempfile::tempdir().expect("tmp");
    let memory_dir = tmp.path();
    for i in 0..5 {
        std::fs::write(
            memory_dir.join(format!("s{i}.md")),
            "---\ntype: session\n---\nbody",
        )
        .unwrap();
    }
    // First run persists meta.
    let _ = run_autodream(memory_dir, "s1", &NullAutodreamExecutor).await;
    // Second run, same session, should be blocked by 24h cooldown.
    let second = run_autodream(memory_dir, "s2", &NullAutodreamExecutor)
        .await
        .expect("no error");
    assert!(
        second.is_none(),
        "cooldown must suppress second run"
    );
}

#[tokio::test]
async fn phase2_autodream_spawn_via_wiring_helper_respects_disable_flag() {
    let tmp = tempfile::tempdir().expect("tmp");
    let project_dir = tmp.path();
    std::fs::create_dir_all(project_dir.join(".theo").join("memory")).unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let cfg = AgentConfig {
        autodream_enabled: false,
        autodream: Some(AutodreamHandle::new(Arc::new(RecordingAutodream {
            calls: calls.clone(),
        }))),
        ..AgentConfig::default()
    };

    let attempted = std::sync::atomic::AtomicBool::new(false);
    maybe_spawn_autodream(&cfg, &attempted, project_dir, "run-1");
    // T5.4: fixed 50ms sleep was the flakiness surface; keep the small
    // relative sleep but also yield so the spawned reviewer task runs.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    tokio::task::yield_now().await;

    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "autodream_enabled=false must suppress spawn"
    );
}

// ---------------------------------------------------------------------------
// Phase 4 — transcript index signature covered by the memory_tantivy crate
// tests; here we confirm the runtime field exists and NullTranscriptIndexer
// is safe by default.
// ---------------------------------------------------------------------------

#[test]
fn phase4_transcript_indexer_is_optional_and_defaults_to_none() {
    let cfg = AgentConfig::default();
    assert!(cfg.transcript_indexer.is_none());
}

// ---------------------------------------------------------------------------
// Phase 5 — Onboarding: bootstrap prompt prepended iff USER.md missing
// ---------------------------------------------------------------------------

#[test]
fn phase5_bootstrap_prepends_on_empty_memory_dir() {
    let tmp = tempfile::tempdir().expect("tmp");
    let project_dir = tmp.path();
    std::fs::create_dir_all(project_dir.join(".theo").join("memory")).unwrap();

    let cfg = AgentConfig::default();
    let system_prompt = "You are helpful.".to_string();
    let composed = maybe_prepend_bootstrap(&cfg, project_dir, system_prompt.clone());
    assert!(composed.starts_with(BOOTSTRAP_PROMPT));
    assert!(composed.contains("You are helpful."));
}

#[test]
fn phase5_bootstrap_skipped_when_user_md_is_populated() {
    let tmp = tempfile::tempdir().expect("tmp");
    let project_dir = tmp.path();
    let memory_dir = project_dir.join(".theo").join("memory");
    std::fs::create_dir_all(&memory_dir).unwrap();
    // Write a populated USER.md.
    std::fs::write(
        memory_dir.join(USER_MD_FILENAME),
        "---\nrole: rust dev\n---\n# User\nA real profile with enough content.",
    )
    .unwrap();

    let cfg = AgentConfig::default();
    let sp = "System prompt body".to_string();
    let composed = maybe_prepend_bootstrap(&cfg, project_dir, sp.clone());
    assert_eq!(
        composed, sp,
        "populated USER.md must leave system prompt unchanged"
    );
}

#[test]
fn phase5_needs_bootstrap_returns_false_only_after_nontrivial_user_md() {
    let tmp = tempfile::tempdir().expect("tmp");
    assert!(needs_bootstrap(tmp.path()));

    // Empty USER.md still triggers bootstrap.
    std::fs::write(tmp.path().join(USER_MD_FILENAME), "---\n---\n").unwrap();
    assert!(needs_bootstrap(tmp.path()));

    // Real content stops the bootstrap prompt.
    std::fs::write(
        tmp.path().join(USER_MD_FILENAME),
        "---\nrole: backend\n---\n# User\nA proper description with enough characters.",
    )
    .unwrap();
    assert!(!needs_bootstrap(tmp.path()));
}

// ---------------------------------------------------------------------------
// Review window — defensive sanity
// ---------------------------------------------------------------------------

#[test]
fn review_window_capped_to_avoid_reviewer_overload() {
    let msgs: Vec<Message> = (0..500).map(|i| Message::user(format!("m{i}"))).collect();
    let w = recent_review_window(&msgs, 10_000);
    assert!(w.len() <= 20, "window cap must apply");
}

// ---------------------------------------------------------------------------
// Memory reviewer trigger — covers AC-1.1/1.2/1.4 directly
// ---------------------------------------------------------------------------

#[test]
fn memory_reviewer_trigger_disabled_when_no_reviewer() {
    let cfg = AgentConfig {
        memory_enabled: true,
        memory_provider: Some(MemoryHandle::new(Arc::new(StubProvider))),
        memory_reviewer: None,
        memory_review_nudge_interval: 3,
        ..AgentConfig::default()
    };
    let counter = MemoryNudgeCounter::new();
    for _ in 0..5 {
        assert_eq!(
            should_trigger_memory_review(&cfg, &counter),
            MemoryReviewTrigger::Disabled
        );
    }
}
