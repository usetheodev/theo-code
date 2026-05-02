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
fn t131tool_breakpoint_id_and_category() {
    let t = DebugSetBreakpointTool::new(empty_manager());
    assert_eq!(t.id(), "debug_set_breakpoint");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t131tool_breakpoint_schema_validates() {
    let t = DebugSetBreakpointTool::new(empty_manager());
    t.schema().validate().unwrap();
}

#[tokio::test]
async fn t131tool_breakpoint_missing_lines_returns_invalid_args() {
    let t = DebugSetBreakpointTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "a", "file_path": "/x.rs"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t131tool_breakpoint_zero_line_returns_invalid_args() {
    // DAP line numbers are 1-based; 0 is a common bug.
    let t = DebugSetBreakpointTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "a", "file_path": "/x.rs", "lines": [10, 0, 25]}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => assert!(msg.contains("1-based")),
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t131tool_breakpoint_non_integer_line_returns_invalid_args() {
    let t = DebugSetBreakpointTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "a", "file_path": "/x.rs", "lines": [10, "twenty", 25]}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => assert!(msg.contains("positive integers")),
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t131tool_breakpoint_unknown_session_returns_actionable_error() {
    let t = DebugSetBreakpointTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "ghost", "file_path": "/x.rs", "lines": [10]}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::Execution(msg) => {
            assert!(msg.contains("no active debug session"));
            assert!(msg.contains("`ghost`"));
            assert!(msg.contains("debug_launch"));
        }
        other => panic!("expected Execution error, got {other:?}"),
    }
}

#[test]
fn t131tool_format_set_breakpoint_groups_verified_unverified() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "setBreakpoints".into(),
        success: true,
        message: None,
        body: Some(json!({
            "breakpoints": [
                {"verified": true, "line": 10},
                {"verified": false, "line": 25, "message": "no executable code at line"},
                {"verified": true, "line": 42},
            ]
        })),
    };
    let out = format_set_breakpoint_output(
        &resp,
        "a",
        "/abs/x.rs",
        &[10, 25, 42],
    );
    assert_eq!(out.metadata["verified_count"], 2);
    assert_eq!(out.metadata["unverified_count"], 1);
    assert_eq!(out.metadata["session_id"], "a");
    assert!(out.output.contains("/abs/x.rs"));
}

#[test]
fn t131tool_format_set_breakpoint_handles_empty_body() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "setBreakpoints".into(),
        success: true,
        message: None,
        body: None,
    };
    let out = format_set_breakpoint_output(&resp, "a", "/x.rs", &[]);
    assert_eq!(out.metadata["verified_count"], 0);
    assert_eq!(out.metadata["unverified_count"], 0);
}

#[test]
fn t131tool_check_response_passes_on_success() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "anything".into(),
        success: true,
        message: None,
        body: None,
    };
    check_response(&resp, "anything").unwrap();
}

#[test]
fn t131tool_check_response_returns_execution_error_on_failure() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "evaluate".into(),
        success: false,
        message: Some("expression not in scope".into()),
        body: None,
    };
    let err = check_response(&resp, "evaluate").unwrap_err();
    match err {
        ToolError::Execution(msg) => {
            assert!(msg.contains("evaluate"));
            assert!(msg.contains("expression not in scope"));
        }
        other => panic!("expected Execution error, got {other:?}"),
    }
}

