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
fn t131tool_scopes_id_and_category() {
    let t = DebugScopesTool::new(empty_manager());
    assert_eq!(t.id(), "debug_scopes");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t131tool_scopes_schema_validates() {
    let t = DebugScopesTool::new(empty_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    for p in &schema.params {
        assert!(p.required, "{} should be required", p.name);
    }
}

#[tokio::test]
async fn t131tool_scopes_missing_frame_id_returns_invalid_args() {
    let t = DebugScopesTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({"session_id": "a"}), &ctx, &mut perms)
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t131tool_scopes_unknown_session_returns_actionable_error() {
    let t = DebugScopesTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "ghost", "frame_id": 1}),
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
fn t131tool_format_scopes_marks_drillable_children() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "scopes".into(),
        success: true,
        message: None,
        body: Some(json!({
            "scopes": [
                {"name": "Locals", "variablesReference": 11, "expensive": false},
                {"name": "Globals", "variablesReference": 12, "expensive": true},
                {"name": "Empty", "variablesReference": 0, "expensive": false},
            ]
        })),
    };
    let out = format_scopes_output(&resp, "a", 1000);
    assert_eq!(out.metadata["scope_count"], 3);
    assert!(out.output.contains("Locals"));
    assert!(out.output.contains("[drill: 11]"));
    assert!(out.output.contains("[drill: 12]"));
    // Expensive scopes get a clear marker so the agent knows
    // fetching them is non-trivial.
    assert!(out.output.contains("EXPENSIVE"));
    // The empty scope (variablesReference == 0) gets no drill hint.
    assert!(out.output.contains("Empty"));
    let empty_line = out.output.lines().find(|l| l.contains("Empty")).unwrap();
    assert!(!empty_line.contains("[drill:"));
}

#[test]
fn t131tool_format_scopes_empty_response_includes_diagnostic_hint() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "scopes".into(),
        success: true,
        message: None,
        body: Some(json!({"scopes": []})),
    };
    let out = format_scopes_output(&resp, "a", 1000);
    assert_eq!(out.metadata["scope_count"], 0);
    assert!(out.output.contains("no scopes returned"));
    assert!(out.output.contains("frame may be invalid"));
}

