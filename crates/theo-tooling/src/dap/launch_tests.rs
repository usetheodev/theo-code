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
fn t131tool_launch_id_and_category() {
    let t = DebugLaunchTool::new(empty_manager());
    assert_eq!(t.id(), "debug_launch");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t131tool_launch_schema_validates() {
    let t = DebugLaunchTool::new(empty_manager());
    t.schema().validate().unwrap();
}

#[test]
fn t131tool_launch_schema_lists_required_fields() {
    let t = DebugLaunchTool::new(empty_manager());
    let names: Vec<_> = t.schema().params.into_iter().collect();
    let required: Vec<&str> = names
        .iter()
        .filter(|p| p.required)
        .map(|p| p.name.as_str())
        .collect();
    for r in ["session_id", "language", "program"] {
        assert!(required.contains(&r), "{r} should be required");
    }
    let optional: Vec<&str> = names
        .iter()
        .filter(|p| !p.required)
        .map(|p| p.name.as_str())
        .collect();
    for o in ["args", "cwd", "env", "stop_on_entry"] {
        assert!(optional.contains(&o), "{o} should be optional");
    }
}

#[tokio::test]
async fn t131tool_launch_missing_session_id_returns_invalid_args() {
    let t = DebugLaunchTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"language": "rust", "program": "/bin/x"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t131tool_launch_empty_session_id_returns_invalid_args() {
    let t = DebugLaunchTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "  ", "language": "rust", "program": "/bin/x"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => assert!(msg.contains("`session_id` is empty")),
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t131tool_launch_missing_program_returns_invalid_args() {
    let t = DebugLaunchTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"session_id": "a", "language": "rust"}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t131tool_launch_unknown_language_returns_actionable_execution_error() {
    let t = DebugLaunchTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({
                "session_id": "a",
                "language": "haskell",
                "program": "/bin/myprog"
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::Execution(msg) => {
            assert!(msg.contains("no DAP adapter installed"));
            assert!(msg.contains("`haskell`"));
            // Mentions at least one alternative for context.
            assert!(
                msg.contains("lldb-vscode")
                    || msg.contains("debugpy")
                    || msg.contains("dlv")
                    || msg.contains("print debugging")
            );
        }
        other => panic!("expected Execution error, got {other:?}"),
    }
}

