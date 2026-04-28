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
fn t131tool_variables_id_and_category() {
    let t = DebugVariablesTool::new(empty_manager());
    assert_eq!(t.id(), "debug_variables");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t131tool_variables_schema_validates() {
    let t = DebugVariablesTool::new(empty_manager());
    t.schema().validate().unwrap();
}

#[tokio::test]
async fn t131tool_variables_missing_reference_returns_invalid_args() {
    let t = DebugVariablesTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({"session_id": "a"}), &ctx, &mut perms)
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t131tool_variables_zero_reference_returns_invalid_args_with_explanation() {
    // variablesReference == 0 means "not drillable". Calling
    // debug_variables on it is a logic error — explain WHY.
    let t = DebugVariablesTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "a", "variables_reference": 0}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => {
            assert!(msg.contains("must be > 0"));
            assert!(msg.contains("scalar value"));
        }
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t131tool_variables_invalid_filter_returns_invalid_args() {
    let t = DebugVariablesTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({
                "session_id": "a",
                "variables_reference": 7,
                "filter": "weird"
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => {
            assert!(msg.contains("`filter`"));
            assert!(msg.contains("indexed"));
            assert!(msg.contains("named"));
        }
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t131tool_variables_unknown_session_returns_actionable_error() {
    let t = DebugVariablesTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "ghost", "variables_reference": 7}),
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

#[test]
fn t131tool_format_variables_counts_drillable_children() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "variables".into(),
        success: true,
        message: None,
        body: Some(json!({
            "variables": [
                {"name": "x", "value": "42", "type": "i32", "variablesReference": 0},
                {"name": "v", "value": "[1, 2, 3]", "type": "Vec<i32>", "variablesReference": 11},
                {"name": "name", "value": "\"hi\"", "type": "&str", "variablesReference": 0},
                {"name": "map", "value": "HashMap{2}", "type": "HashMap<i32, String>", "variablesReference": 12},
            ]
        })),
    };
    let out = format_variables_output(&resp, "a", 7, 0, 100, None);
    assert_eq!(out.metadata["child_count"], 4);
    assert_eq!(out.metadata["drillable_count"], 2);
    assert_eq!(out.metadata["session_id"], "a");
    // Output text shows drill hints for the two structured ones.
    assert!(out.output.contains("[drill: 11]"));
    assert!(out.output.contains("[drill: 12]"));
    assert!(out.output.contains("x = 42"));
    // Scalar children must NOT have a drill hint.
    let x_line = out
        .output
        .lines()
        .find(|l| l.contains("x = 42"))
        .unwrap();
    assert!(!x_line.contains("[drill:"));
}

#[test]
fn t131tool_format_variables_handles_empty_response_gracefully() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "variables".into(),
        success: true,
        message: None,
        body: Some(json!({"variables": []})),
    };
    let out = format_variables_output(&resp, "a", 7, 0, 100, None);
    assert_eq!(out.metadata["child_count"], 0);
    assert_eq!(out.metadata["drillable_count"], 0);
}

#[test]
fn t131tool_format_variables_passes_filter_through_to_metadata() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "variables".into(),
        success: true,
        message: None,
        body: Some(json!({"variables": []})),
    };
    let out = format_variables_output(&resp, "a", 7, 0, 50, Some("indexed"));
    assert_eq!(out.metadata["filter"], "indexed");
    assert_eq!(out.metadata["count"], 50);
}

