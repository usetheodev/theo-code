//! REMEDIATION_PLAN T7.3 — Integration matrix for the `batch_execute`
//! meta-tool (the most self-contained dimension of the plan's matrix).
//!
//! The full plan lists 4 dimensions (`done`/`delegate_task`/`skill`/
//! `batch`) × ~5 variants each. `done`, `delegate_task`, and `skill`
//! need a live `AgentRunEngine` with registry/handoff-guardrail/plugin
//! fixtures. `batch_execute` is the one we can drive directly through
//! the public `tool_bridge::execute_tool_call` entry point, so it's
//! what lands here. The other dimensions stay tracked as follow-ups
//! with their existing in-crate coverage (`handle_done_call` tests in
//! `run_engine/dispatch/done.rs`, `handle_delegate_task` tests in
//! `run_engine/delegate_handler.rs`).

use std::path::PathBuf;

use theo_domain::tool::ToolContext;
use theo_infra_llm::types::{FunctionCall, ToolCall};
use theo_tooling::registry::create_default_registry;

use theo_agent_runtime::tool_bridge::execute_tool_call;

fn make_batch_call(calls: serde_json::Value) -> ToolCall {
    ToolCall {
        id: "b1".to_string(),
        call_type: "function".to_string(),
        function: FunctionCall {
            name: "batch_execute".to_string(),
            arguments: serde_json::json!({ "calls": calls }).to_string(),
        },
    }
}

fn test_ctx() -> ToolContext {
    ToolContext::test_context(PathBuf::from("/tmp"))
}

// ────────────────────────────────────────────────────────────────────
// Variant 1: 5 OK steps — all succeed, the combined result reports
// `ok: true` and contains all 5 per-step payloads.
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn batch_5_ok_steps_all_succeed() {
    let registry = create_default_registry();
    let ctx = test_ctx();

    // 5 trivial glob calls — each either matches nothing or something
    // under /tmp. Both outcomes are success at the tool level.
    let calls = (0..5)
        .map(|i| {
            serde_json::json!({
                "tool": "glob",
                "args": { "pattern": format!("/tmp/theo-t7-3-nonexistent-{i}") },
            })
        })
        .collect::<Vec<_>>();
    let call = make_batch_call(serde_json::Value::Array(calls));

    let (message, ok) = execute_tool_call(&registry, &call, &ctx).await;

    assert!(ok, "batch with 5 OK steps should report ok=true");
    let body: serde_json::Value =
        serde_json::from_str(&message.content.expect("body present")).expect("valid json");
    assert_eq!(body["ok"], true);
    let steps = body["steps"].as_array().expect("steps array");
    assert_eq!(steps.len(), 5, "all 5 steps should be present");
    for (i, step) in steps.iter().enumerate() {
        assert_eq!(step["step"], i);
        assert_eq!(step["tool"], "glob");
        assert_eq!(step["ok"], true, "step {i} must succeed");
    }
}

// ────────────────────────────────────────────────────────────────────
// Variant 2: 5 steps with a blocked meta-tool in position 3 — batch
// MUST early-exit with `ok: false` and the blocking step's error.
//
// "Blocked" here means: the step tries to call another meta-tool from
// inside batch_execute (batch, done, delegate_task, skill, tool_search,
// or batch_execute itself). These are on the `BATCH_EXECUTE_BLOCKED`
// list in `execute_meta.rs` to prevent recursion / accidental
// self-reference.
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn batch_blocked_meta_tool_early_exits_with_failure() {
    let registry = create_default_registry();
    let ctx = test_ctx();

    let calls = serde_json::json!([
        { "tool": "glob", "args": { "pattern": "/tmp/a" } },
        { "tool": "glob", "args": { "pattern": "/tmp/b" } },
        { "tool": "done", "args": { "summary": "should-not-run" } },
        { "tool": "glob", "args": { "pattern": "/tmp/d" } },
        { "tool": "glob", "args": { "pattern": "/tmp/e" } },
    ]);
    let call = make_batch_call(calls);

    let (message, ok) = execute_tool_call(&registry, &call, &ctx).await;

    assert!(!ok, "batch with blocked meta-tool must report ok=false");
    let body: serde_json::Value =
        serde_json::from_str(&message.content.expect("body present")).expect("valid json");
    assert_eq!(body["ok"], false);
    let steps = body["steps"].as_array().expect("steps array");

    // Expect steps 0..=2 (early-exit at the blocker). Steps 3 and 4
    // MUST NOT appear — batch_execute early-exits on failure.
    assert_eq!(
        steps.len(),
        3,
        "early-exit should stop at the blocker; steps 3/4 must not run"
    );
    assert_eq!(steps[0]["ok"], true);
    assert_eq!(steps[1]["ok"], true);
    assert_eq!(steps[2]["ok"], false);
    assert_eq!(steps[2]["tool"], "done");
    assert!(
        steps[2]["error"]
            .as_str()
            .unwrap_or("")
            .contains("blocked"),
        "error should mention 'blocked'; got {:?}",
        steps[2]
    );
}

// ────────────────────────────────────────────────────────────────────
// Variant 3: 25 steps — the documented practical limit. All MUST run.
//
// `batch_execute` itself has no hard cap (unlike the sibling `batch`
// meta-tool which documents a max of 25). Still, we assert the happy
// path at the documented practical ceiling for completeness.
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn batch_25_steps_all_run() {
    let registry = create_default_registry();
    let ctx = test_ctx();

    let calls = (0..25)
        .map(|i| {
            serde_json::json!({
                "tool": "glob",
                "args": { "pattern": format!("/tmp/theo-t7-3-max-{i}") },
            })
        })
        .collect::<Vec<_>>();
    let call = make_batch_call(serde_json::Value::Array(calls));

    let (message, ok) = execute_tool_call(&registry, &call, &ctx).await;

    assert!(ok, "batch of 25 OK steps should report ok=true");
    let body: serde_json::Value =
        serde_json::from_str(&message.content.expect("body present")).expect("valid json");
    let steps = body["steps"].as_array().expect("steps array");
    assert_eq!(steps.len(), 25);
    // Sequence invariant: steps come out in submission order.
    for (i, step) in steps.iter().enumerate() {
        assert_eq!(step["step"], i);
    }
}

// ────────────────────────────────────────────────────────────────────
// Variant 4: empty / malformed inputs — schema validation.
//
// The plan's "26 overflow" entry is specific to the `batch` tool, not
// `batch_execute`. For the executor we instead verify the two edge
// cases it explicitly errors on:
//   1. missing `calls` array
//   2. empty `calls` array
// Both MUST return ok=false with a helpful message, not crash.
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn batch_missing_calls_array_returns_actionable_error() {
    let registry = create_default_registry();
    let ctx = test_ctx();

    let call = ToolCall {
        id: "b-missing".to_string(),
        call_type: "function".to_string(),
        function: FunctionCall {
            name: "batch_execute".to_string(),
            arguments: "{}".to_string(),
        },
    };
    let (message, ok) = execute_tool_call(&registry, &call, &ctx).await;

    assert!(!ok, "missing `calls` must fail");
    let content = message.content.expect("body present");
    assert!(
        content.contains("calls"),
        "error should mention the missing field; got {content}"
    );
    assert!(
        content.contains("Example") || content.contains("example"),
        "error should show an example; got {content}"
    );
}

#[tokio::test]
async fn batch_empty_calls_array_returns_actionable_error() {
    let registry = create_default_registry();
    let ctx = test_ctx();

    let call = make_batch_call(serde_json::json!([]));
    let (message, ok) = execute_tool_call(&registry, &call, &ctx).await;

    assert!(!ok, "empty `calls` array must fail");
    let content = message.content.expect("body present");
    assert!(
        content.contains("empty"),
        "error should mention 'empty'; got {content}"
    );
}

// ────────────────────────────────────────────────────────────────────
// Variant 5: per-step arg failures propagate as step-level `ok: false`.
//
// The regular tool (not a meta-tool) failing must:
//   1. record the failure as a step result with ok=false and its
//      error message
//   2. trigger early-exit: subsequent steps do not run
//   3. surface the overall batch as ok=false
// ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn batch_step_failure_early_exits() {
    let registry = create_default_registry();
    let ctx = test_ctx();

    // Step 1: valid glob. Step 2: `read` of a non-existent file — will
    // fail. Step 3: MUST NOT run.
    let calls = serde_json::json!([
        { "tool": "glob", "args": { "pattern": "/tmp/first" } },
        { "tool": "read", "args": { "filePath": "/nonexistent/theo-t7-3" } },
        { "tool": "glob", "args": { "pattern": "/tmp/third-must-not-run" } },
    ]);
    let call = make_batch_call(calls);

    let (message, ok) = execute_tool_call(&registry, &call, &ctx).await;

    assert!(!ok);
    let body: serde_json::Value =
        serde_json::from_str(&message.content.expect("body")).expect("json");
    let steps = body["steps"].as_array().expect("steps");
    assert_eq!(
        steps.len(),
        2,
        "early-exit after the read failure: got {}",
        steps.len()
    );
    assert_eq!(steps[0]["ok"], true);
    assert_eq!(steps[1]["ok"], false);
}
