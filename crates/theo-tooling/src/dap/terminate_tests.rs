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
fn t131tool_terminate_id_and_category() {
    let t = DebugTerminateTool::new(empty_manager());
    assert_eq!(t.id(), "debug_terminate");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t131tool_terminate_schema_validates() {
    let t = DebugTerminateTool::new(empty_manager());
    t.schema().validate().unwrap();
}

#[tokio::test]
async fn t131tool_terminate_unknown_session_returns_was_active_false() {
    // Idempotency invariant: terminating an unknown session is a
    // no-op success, NOT an error. The agent might call this in
    // a cleanup routine without knowing the state.
    let t = DebugTerminateTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let out = t
        .execute(json!({"session_id": "ghost"}), &ctx, &mut perms)
        .await
        .unwrap();
    assert_eq!(out.metadata["was_active"], false);
    assert_eq!(out.metadata["session_id"], "ghost");
    assert!(out.title.contains("no active session"));
}

#[tokio::test]
async fn t131tool_terminate_missing_session_id_returns_invalid_args() {
    let t = DebugTerminateTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t.execute(json!({}), &ctx, &mut perms).await.unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t131tool_terminate_empty_session_id_returns_invalid_args() {
    let t = DebugTerminateTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({"session_id": ""}), &ctx, &mut perms)
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

