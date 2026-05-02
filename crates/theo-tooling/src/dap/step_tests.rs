// Per-tool sibling test file extracted from dap/tool_tests.rs (T3.1).
//
// Test-only file; gates use the inner cfg(test) attribute below to
// classify every line as test code.
#![cfg(test)]

#[allow(unused_imports)]
use crate::dap::{
    DapResponse, DapSessionManager, DebugContinueTool, DebugEvalTool, DebugLaunchTool,
    DebugScopesTool, DebugSetBreakpointTool, DebugStackTraceTool, DebugStatusTool, DebugStepTool,
    DebugTerminateTool, DebugThreadsTool, DebugVariablesTool,
};
#[allow(unused_imports)]
use crate::dap::breakpoint::format_set_breakpoint_output;
#[allow(unused_imports)]
use crate::dap::eval::format_eval_output;
#[allow(unused_imports)]
use crate::dap::scopes::format_scopes_output;
#[allow(unused_imports)]
use crate::dap::stack_trace::format_stack_trace_output;
#[allow(unused_imports)]
use crate::dap::threads::format_threads_output;
#[allow(unused_imports)]
use crate::dap::variables::format_variables_output;
#[allow(unused_imports)]
use std::collections::HashMap;
#[allow(unused_imports)]
use std::path::{Path, PathBuf};
#[allow(unused_imports)]
use std::sync::Arc;

#[allow(unused_imports)]
use serde_json::json;

#[allow(unused_imports)]
use theo_domain::error::ToolError;
#[allow(unused_imports)]
use theo_domain::session::{MessageId, SessionId};
#[allow(unused_imports)]
use theo_domain::tool::{PermissionCollector, Tool, ToolCategory, ToolContext};

#[allow(unused_imports)]
use crate::dap::tool_common::{
    check_response, map_session_error, parse_session_id, require_session,
};

#[allow(unused_imports)]
use crate::dap::test_helpers::{empty_manager, make_ctx};

#[test]
fn t131tool_step_id_and_category() {
    let t = DebugStepTool::new(empty_manager());
    assert_eq!(t.id(), "debug_step");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t131tool_step_schema_validates_and_requires_kind_thread() {
    let t = DebugStepTool::new(empty_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    let required: Vec<&str> = schema
        .params
        .iter()
        .filter(|p| p.required)
        .map(|p| p.name.as_str())
        .collect();
    for r in ["session_id", "kind", "thread_id"] {
        assert!(required.contains(&r), "{r} must be required");
    }
}

#[tokio::test]
async fn t131tool_step_missing_kind_returns_invalid_args() {
    let t = DebugStepTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "a", "thread_id": 1}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t131tool_step_invalid_kind_returns_invalid_args_with_options() {
    let t = DebugStepTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "a", "kind": "sideways", "thread_id": 1}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => {
            assert!(msg.contains("over"));
            assert!(msg.contains("in"));
            assert!(msg.contains("out"));
            assert!(msg.contains("sideways"));
        }
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t131tool_step_missing_thread_id_returns_invalid_args_with_hint() {
    // Common bug: copying continue() args (where thread_id is
    // optional) into step(). Error message points at the
    // `stopped` event source.
    let t = DebugStepTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "a", "kind": "over"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => {
            assert!(msg.contains("thread_id"));
            assert!(msg.contains("stopped event"));
        }
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t131tool_step_unknown_session_returns_actionable_error() {
    let t = DebugStepTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "ghost", "kind": "over", "thread_id": 1}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::Execution(msg) => assert!(msg.contains("no active debug session")),
        other => panic!("expected Execution error, got {other:?}"),
    }
}

