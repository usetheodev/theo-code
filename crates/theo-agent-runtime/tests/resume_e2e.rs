//! Phase 32 (resume-runtime-wiring) — gap #3 + #10 combined E2E test.
//!
//! Validates that the resume pipeline reconstructs both the tool-replay
//! state (gap #3) AND the worktree strategy (gap #10) from the persisted
//! event log of a previously crashed sub-agent run.
//!
//! Strategy: instead of driving a real LLM through a crash (which would
//! require an integration with mocked transport), we simulate the
//! "pre-crash" persisted state directly:
//!   1. Build a SubagentRun + event log entries that match what would
//!      have been written by a partially-completed run.
//!   2. Construct a Resumer over that store.
//!   3. Verify the ResumeContext correctly identifies:
//!      - Which call_ids should NOT re-execute (from `tool_result` events)
//!      - Whether the worktree should be Reused or Recreated
//!   4. Verify the WorktreeOverride translation honors that strategy.
//!
//! The actual short-circuit during dispatch is unit-tested in
//! run_engine::tests::dispatch_replays + agent_loop::tests::with_resume_context.
//! The translation Resumer→Override is unit-tested in
//! subagent::resume::tests::worktree.
//! This file glues both layers in the same scenario.

use serde_json::json;
use tempfile::TempDir;

use theo_agent_runtime::config::AgentConfig;
use theo_agent_runtime::event_bus::EventBus;
use theo_agent_runtime::subagent::resume::{Resumer, WorktreeStrategy};
use theo_agent_runtime::subagent::SubAgentManager;
use theo_agent_runtime::subagent::WorktreeOverride;
use theo_agent_runtime::subagent_runs::{
    FileSubagentRunStore, RunStatus, SubagentEvent, SubagentRun,
};
use theo_domain::agent_spec::AgentSpec;

fn ts(offset_ms: i64) -> i64 {
    1_700_000_000_000 + offset_ms
}

fn isolated_spec(name: &str) -> AgentSpec {
    let mut spec = AgentSpec::on_demand(name, "Resume E2E test agent");
    spec.isolation = Some("worktree".to_string());
    spec.isolation_base_branch = Some("main".to_string());
    spec
}

fn manager(project_dir: &std::path::Path) -> std::sync::Arc<SubAgentManager> {
    std::sync::Arc::new(SubAgentManager::with_builtins(
        AgentConfig::default(),
        std::sync::Arc::new(EventBus::new()),
        project_dir.to_path_buf(),
    ))
}

#[test]
fn resume_e2e_replays_completed_tools_and_reuses_worktree() {
    // ── Arrange ──────────────────────────────────────────────────────────
    let store_dir = TempDir::new().unwrap();
    let store = FileSubagentRunStore::new(store_dir.path());

    // The "worktree path" the original run used. We create it on disk so
    // WorktreeStrategy::from_spec_and_cwd resolves to Reuse(path).
    let worktree_dir = TempDir::new().unwrap();

    let spec = isolated_spec("crashed-explorer");
    let mut run = SubagentRun::new_running(
        "run-crash-1",
        None,
        &spec,
        "find OWASP issues in src/auth.rs",
        worktree_dir.path().to_string_lossy().as_ref(),
        Some("checkpoint-pre-crash".to_string()),
    );
    // Mark Running so Resumer accepts it as resumable.
    run.status = RunStatus::Running;
    store.save(&run).unwrap();

    // Simulate the event log of a partially-completed run:
    //   - tool_dispatched(c1) → tool_result(c1)  (a side-effecting tool that COMPLETED)
    //   - tool_dispatched(c2) → NO tool_result   (tool was cancelled mid-execution)
    let events = vec![
        SubagentEvent {
            timestamp: ts(0),
            event_type: "tool_dispatched".into(),
            payload: json!({
                "call_id": "c1",
                "name": "write_file",
                "args": {"path": "/tmp/x.txt", "content": "ok"},
            }),
        },
        SubagentEvent {
            timestamp: ts(10),
            event_type: "tool_result".into(),
            payload: json!({
                "call_id": "c1",
                "name": "write_file",
                "content": "{\"ok\":true,\"bytes_written\":2}",
            }),
        },
        SubagentEvent {
            timestamp: ts(20),
            event_type: "tool_dispatched".into(),
            payload: json!({
                "call_id": "c2",
                "name": "bash",
                "args": {"command": "long-running-script.sh"},
            }),
        },
        // No tool_result for c2 — cancelled / crashed mid-execution.
    ];
    for e in &events {
        store.append_event("run-crash-1", e).unwrap();
    }

    let project_dir = TempDir::new().unwrap();
    let mgr = manager(project_dir.path());
    let resumer = Resumer::new(&store, &mgr);

    // ── Act ──────────────────────────────────────────────────────────────
    let ctx = resumer
        .build_context("run-crash-1")
        .expect("build_context should succeed for a Running run");

    // ── Assert: gap #3 — tool replay state ───────────────────────────────
    // c1 has a result event → must short-circuit on resume.
    assert!(
        ctx.executed_tool_calls.contains("c1"),
        "executed_tool_calls must contain c1 (it had a tool_result event)"
    );
    // c2 was dispatched but never completed → must dispatch normally on resume.
    assert!(
        !ctx.executed_tool_calls.contains("c2"),
        "executed_tool_calls must NOT contain c2 (no tool_result event)"
    );
    // c1 must be replayable from the cache.
    let cached = ctx
        .cached_tool_result("c1")
        .expect("c1 must be cached for replay");
    assert_eq!(cached.tool_call_id.as_deref(), Some("c1"));
    assert!(cached
        .content
        .as_deref()
        .unwrap()
        .contains("bytes_written"));
    // c2 must NOT be cached.
    assert!(
        ctx.cached_tool_result("c2").is_none(),
        "c2 must NOT be cached"
    );
    // The short-circuit predicate matches the dispatch contract.
    assert!(ctx.should_skip_tool_call("c1"));
    assert!(!ctx.should_skip_tool_call("c2"));
    assert!(!ctx.should_skip_tool_call("c-brand-new"));

    // ── Assert: gap #10 — worktree strategy ──────────────────────────────
    // The original cwd path STILL EXISTS on disk → strategy is Reuse.
    match &ctx.worktree_strategy {
        WorktreeStrategy::Reuse(p) => {
            assert_eq!(p, worktree_dir.path());
        }
        other => panic!("expected WorktreeStrategy::Reuse, got {:?}", other),
    }

    // ── Assert: Resumer→Override translation ─────────────────────────────
    // Mirror the translation logic used inside resume_with_objective so
    // we lock the contract that Reuse strategy → Reuse override (no
    // provider.create call, no fresh worktree path).
    let translated = match &ctx.worktree_strategy {
        WorktreeStrategy::None => WorktreeOverride::None,
        WorktreeStrategy::Reuse(p) => WorktreeOverride::Reuse(p.clone()),
        WorktreeStrategy::Recreate { base_branch } => WorktreeOverride::Recreate {
            base_branch: base_branch.clone(),
        },
    };
    match translated {
        WorktreeOverride::Reuse(p) => assert_eq!(p, worktree_dir.path()),
        _ => panic!("expected WorktreeOverride::Reuse"),
    }
}

#[test]
fn resume_e2e_recreates_worktree_when_original_was_cleaned() {
    // ── Arrange ──────────────────────────────────────────────────────────
    let store_dir = TempDir::new().unwrap();
    let store = FileSubagentRunStore::new(store_dir.path());

    // The original cwd path was already cleaned up (CleanupPolicy::OnSuccess
    // happened, OR the user deleted it). Use a path we know does not exist.
    let cleaned_path =
        std::path::PathBuf::from("/tmp/theo-resume-e2e-cleaned-xyz-does-not-exist");
    assert!(!cleaned_path.exists(), "test pre-condition: path must NOT exist");

    let mut spec = isolated_spec("crashed-then-cleaned");
    spec.isolation_base_branch = Some("develop".to_string()); // explicit base

    let mut run = SubagentRun::new_running(
        "run-cleaned-1",
        None,
        &spec,
        "objective",
        cleaned_path.to_string_lossy().as_ref(),
        None,
    );
    run.status = RunStatus::Running;
    store.save(&run).unwrap();

    // Empty event log — no tools dispatched yet.
    let project_dir = TempDir::new().unwrap();
    let mgr = manager(project_dir.path());
    let resumer = Resumer::new(&store, &mgr);

    // ── Act ──────────────────────────────────────────────────────────────
    let ctx = resumer
        .build_context("run-cleaned-1")
        .expect("build_context should succeed");

    // ── Assert: gap #10 — Recreate strategy chosen ───────────────────────
    match &ctx.worktree_strategy {
        WorktreeStrategy::Recreate { base_branch } => {
            assert_eq!(base_branch, "develop");
        }
        other => panic!("expected WorktreeStrategy::Recreate, got {:?}", other),
    }

    // ── Assert: empty replay state (no completed tools to short-circuit) ─
    assert!(ctx.executed_tool_calls.is_empty());
    assert!(ctx.executed_tool_results.is_empty());

    // ── Assert: Resumer→Override translation produces Recreate ───────────
    let translated = match &ctx.worktree_strategy {
        WorktreeStrategy::Recreate { base_branch } => WorktreeOverride::Recreate {
            base_branch: base_branch.clone(),
        },
        _ => panic!("expected Recreate strategy"),
    };
    match translated {
        WorktreeOverride::Recreate { base_branch } => {
            assert_eq!(base_branch, "develop");
        }
        _ => panic!("expected Recreate override"),
    }
}

#[test]
fn resume_e2e_skips_when_terminal_status() {
    // Pre-condition: build_context must REJECT runs in terminal state
    // (Completed / Failed / Cancelled / Abandoned). Resume only makes
    // sense for Running runs that were interrupted.
    let store_dir = TempDir::new().unwrap();
    let store = FileSubagentRunStore::new(store_dir.path());

    let spec = isolated_spec("already-completed");
    let mut run = SubagentRun::new_running(
        "run-done-1",
        None,
        &spec,
        "objective",
        "/tmp/whatever",
        None,
    );
    run.status = RunStatus::Completed;
    store.save(&run).unwrap();

    let project_dir = TempDir::new().unwrap();
    let mgr = manager(project_dir.path());
    let resumer = Resumer::new(&store, &mgr);

    let result = resumer.build_context("run-done-1");
    assert!(
        result.is_err(),
        "build_context must reject terminal runs; got Ok"
    );
}
