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
fn t131tool_eval_id_and_category() {
    let t = DebugEvalTool::new(empty_manager());
    assert_eq!(t.id(), "debug_eval");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t131tool_eval_schema_validates() {
    let t = DebugEvalTool::new(empty_manager());
    t.schema().validate().unwrap();
}

#[test]
fn t131tool_eval_schema_marks_frame_id_and_context_optional() {
    let t = DebugEvalTool::new(empty_manager());
    let schema = t.schema();
    let optional: Vec<&str> = schema
        .params
        .iter()
        .filter(|p| !p.required)
        .map(|p| p.name.as_str())
        .collect();
    assert!(optional.contains(&"frame_id"));
    assert!(optional.contains(&"context"));
}

#[tokio::test]
async fn t131tool_eval_missing_expression_returns_invalid_args() {
    let t = DebugEvalTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({"session_id": "a"}), &ctx, &mut perms)
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t131tool_eval_empty_expression_returns_invalid_args() {
    let t = DebugEvalTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "a", "expression": "   "}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => assert!(msg.contains("`expression` is empty")),
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t131tool_eval_invalid_context_returns_invalid_args() {
    let t = DebugEvalTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({
                "session_id": "a",
                "expression": "x",
                "context": "side_effects_pls"
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => {
            assert!(msg.contains("`context`"));
            assert!(msg.contains("watch"));
            assert!(msg.contains("repl"));
            assert!(msg.contains("hover"));
        }
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t131tool_eval_unknown_session_returns_actionable_error() {
    let t = DebugEvalTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "ghost", "expression": "x"}),
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
fn t131tool_format_eval_includes_result_type_and_variables_reference() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "evaluate".into(),
        success: true,
        message: None,
        body: Some(json!({
            "result": "Some(42)",
            "type": "Option<i32>",
            "variablesReference": 7,
        })),
    };
    let out = format_eval_output(&resp, "a", "my_var", "watch", Some(3));
    assert_eq!(out.metadata["result"], "Some(42)");
    assert_eq!(out.metadata["value_type"], "Option<i32>");
    assert_eq!(out.metadata["variables_reference"], 7);
    assert_eq!(out.metadata["frame_id"], 3);
    assert_eq!(out.metadata["context"], "watch");
    assert!(out.output.contains("expression: my_var"));
    assert!(out.output.contains("result: Some(42)"));
    assert!(out.output.contains("type: Option<i32>"));
    assert!(out.output.contains("variablesReference: 7"));
}

#[test]
fn t131tool_format_eval_handles_zero_variables_reference_silently() {
    // Primitive values have variablesReference == 0; no drill-down hint.
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "evaluate".into(),
        success: true,
        message: None,
        body: Some(json!({
            "result": "42",
            "type": "i32",
            "variablesReference": 0,
        })),
    };
    let out = format_eval_output(&resp, "a", "x", "watch", None);
    assert_eq!(out.metadata["variables_reference"], 0);
    assert!(!out.output.contains("variablesReference:"));
}

#[test]
fn t131tool_format_eval_handles_missing_body_gracefully() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "evaluate".into(),
        success: true,
        message: None,
        body: None,
    };
    let out = format_eval_output(&resp, "a", "x", "watch", None);
    assert_eq!(out.metadata["result"], "(no result)");
    assert!(out.metadata["value_type"].is_null());
    assert_eq!(out.metadata["variables_reference"], 0);
}

