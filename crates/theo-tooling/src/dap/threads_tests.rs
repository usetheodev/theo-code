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
fn t131tool_threads_id_and_category() {
    let t = DebugThreadsTool::new(empty_manager());
    assert_eq!(t.id(), "debug_threads");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t131tool_threads_schema_validates_with_only_session_id() {
    let t = DebugThreadsTool::new(empty_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    assert_eq!(schema.params.len(), 1);
    assert_eq!(schema.params[0].name, "session_id");
    assert!(schema.params[0].required);
}

#[tokio::test]
async fn t131tool_threads_unknown_session_returns_actionable_error() {
    let t = DebugThreadsTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({"session_id": "ghost"}), &ctx, &mut perms)
        .await
        .unwrap_err();
    match err {
        ToolError::Execution(msg) => assert!(msg.contains("no active debug session")),
        other => panic!("expected Execution error, got {other:?}"),
    }
}

#[test]
fn t131tool_format_threads_lists_each_thread() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "threads".into(),
        success: true,
        message: None,
        body: Some(json!({
            "threads": [
                {"id": 1, "name": "main"},
                {"id": 2, "name": "worker-0"},
                {"id": 3, "name": "worker-1"},
            ]
        })),
    };
    let out = format_threads_output(&resp, "a");
    assert_eq!(out.metadata["thread_count"], 3);
    assert!(out.output.contains("main"));
    assert!(out.output.contains("worker-0"));
    assert!(out.output.contains("worker-1"));
}

#[test]
fn t131tool_format_threads_empty_response_includes_relaunch_hint() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "threads".into(),
        success: true,
        message: None,
        body: Some(json!({"threads": []})),
    };
    let out = format_threads_output(&resp, "a");
    assert_eq!(out.metadata["thread_count"], 0);
    // Empty → adapter probably terminated. Tell the agent to
    // call debug_terminate + re-launch instead of looping.
    assert!(out.output.contains("debug_terminate"));
    assert!(out.output.contains("re-launch"));
}

