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
fn t131tool_stack_trace_id_and_category() {
    let t = DebugStackTraceTool::new(empty_manager());
    assert_eq!(t.id(), "debug_stack_trace");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t131tool_stack_trace_schema_validates_with_required_thread_id() {
    let t = DebugStackTraceTool::new(empty_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    let required: Vec<&str> = schema
        .params
        .iter()
        .filter(|p| p.required)
        .map(|p| p.name.as_str())
        .collect();
    assert!(required.contains(&"thread_id"));
    assert!(required.contains(&"session_id"));
}

#[tokio::test]
async fn t131tool_stack_trace_missing_thread_id_returns_invalid_args() {
    let t = DebugStackTraceTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({"session_id": "a"}), &ctx, &mut perms)
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t131tool_stack_trace_unknown_session_returns_actionable_error() {
    let t = DebugStackTraceTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "ghost", "thread_id": 1}),
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
fn t131tool_format_stack_trace_includes_frame_ids_and_source_locations() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "stackTrace".into(),
        success: true,
        message: None,
        body: Some(json!({
            "stackFrames": [
                {
                    "id": 1000,
                    "name": "main",
                    "source": {"path": "/abs/main.rs"},
                    "line": 42,
                    "column": 12,
                },
                {
                    "id": 1001,
                    "name": "<rust_main>",
                    "source": {"path": "/abs/lib.rs"},
                    "line": 5,
                    "column": 1,
                },
            ],
            "totalFrames": 2,
        })),
    };
    let out = format_stack_trace_output(&resp, "a", 1, 0, 20);
    assert_eq!(out.metadata["frame_count"], 2);
    assert_eq!(out.metadata["total_frames"], 2);
    assert_eq!(out.metadata["thread_id"], 1);
    // Frame id MUST be preserved — it's the input to debug_eval(frame_id).
    let frames = out.metadata["frames"].as_array().unwrap();
    assert_eq!(frames[0]["id"], 1000);
    assert_eq!(frames[0]["source_path"], "/abs/main.rs");
    assert_eq!(frames[0]["line"], 42);
    assert!(out.output.contains("main"));
    assert!(out.output.contains("/abs/main.rs:42:12"));
}

#[test]
fn t131tool_format_stack_trace_handles_empty_frames_with_explanation() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "stackTrace".into(),
        success: true,
        message: None,
        body: Some(json!({"stackFrames": [], "totalFrames": 0})),
    };
    let out = format_stack_trace_output(&resp, "a", 1, 0, 20);
    assert_eq!(out.metadata["frame_count"], 0);
    // Helps the agent diagnose: a stopped thread should HAVE frames.
    assert!(out.output.contains("no frames returned"));
    assert!(out.output.contains("stopped state"));
}

#[test]
fn t131tool_format_stack_trace_offsets_frame_numbers_by_start_frame() {
    let resp = DapResponse {
        seq: 1,
        message_type: "response".into(),
        request_seq: 1,
        command: "stackTrace".into(),
        success: true,
        message: None,
        body: Some(json!({
            "stackFrames": [
                {"id": 5000, "name": "fn_a", "source": {"path": "/x"}, "line": 1, "column": 1},
                {"id": 5001, "name": "fn_b", "source": {"path": "/x"}, "line": 2, "column": 1},
            ],
            "totalFrames": 100,
        })),
    };
    let out = format_stack_trace_output(&resp, "a", 1, 50, 2);
    // start_frame=50 means the rendered frames are #50, #51 — not #0, #1.
    assert!(out.output.contains("#50"));
    assert!(out.output.contains("#51"));
    assert!(!out.output.contains("#0  id=5000"));
}

