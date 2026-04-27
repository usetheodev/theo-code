// Sibling test body of `dap/tool.rs` re-attached via
// `#[cfg(test)] #[path = "tool_tests.rs"] mod tests;`. The inner
// attribute below is redundant for the compiler (the `mod` decl
// already cfg-gates this file) but signals to scripts/check-unwrap.sh
// and scripts/check-panic.sh that every line is test-only — so the
// production-only filter excludes the entire file from violation
// counts. Only test code lives here.
#![cfg(test)]

use super::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use theo_domain::session::{MessageId, SessionId};

fn make_ctx(project_dir: PathBuf) -> ToolContext {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolContext {
        session_id: SessionId::new("ses_test"),
        message_id: MessageId::new(""),
        call_id: "call_test".into(),
        agent: "build".into(),
        abort: rx,
        project_dir,
        graph_context: None,
        stdout_tx: None,
    }
}

fn empty_manager() -> Arc<DapSessionManager> {
    Arc::new(DapSessionManager::from_catalogue(HashMap::new()))
}

// ── debug_status ──────────────────────────────────────────────

#[test]
fn t131tool_status_id_and_category() {
    let t = DebugStatusTool::new(empty_manager());
    assert_eq!(t.id(), "debug_status");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t131tool_status_schema_validates_with_no_args() {
    let t = DebugStatusTool::new(empty_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    assert!(schema.params.is_empty());
}

#[tokio::test]
async fn t131tool_status_empty_catalogue_returns_zero_languages() {
    let t = DebugStatusTool::new(empty_manager());
    let mut perms = PermissionCollector::new();
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let out = t.execute(json!({}), &ctx, &mut perms).await.unwrap();
    assert_eq!(out.metadata["supported_language_count"], 0);
    assert_eq!(out.metadata["active_session_count"], 0);
    assert!(
        out.output.contains("No DAP adapters"),
        "empty catalogue must surface the install/print-debug fallback"
    );
    assert!(out.output.contains("print-debugging"));
    assert!(out.output.contains("No active debug sessions"));
}

#[tokio::test]
async fn t131tool_status_lists_languages_and_no_active_sessions() {
    use std::collections::HashMap;
    use std::path::PathBuf;
    let mut catalogue: HashMap<&'static str, crate::dap::DiscoveredAdapter> =
        HashMap::new();
    catalogue.insert(
        "rust",
        crate::dap::DiscoveredAdapter {
            name: "lldb-vscode",
            command: PathBuf::from("/usr/bin/lldb-vscode"),
            args: vec![],
            languages: &["rust"],
            file_extensions: &["rs"],
        },
    );
    catalogue.insert(
        "python",
        crate::dap::DiscoveredAdapter {
            name: "debugpy",
            command: PathBuf::from("/usr/bin/debugpy-adapter"),
            args: vec![],
            languages: &["python"],
            file_extensions: &["py"],
        },
    );
    let manager = Arc::new(DapSessionManager::from_catalogue(catalogue));
    let t = DebugStatusTool::new(manager);
    let mut perms = PermissionCollector::new();
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let out = t.execute(json!({}), &ctx, &mut perms).await.unwrap();

    assert_eq!(out.metadata["supported_language_count"], 2);
    let langs = out.metadata["supported_languages"].as_array().unwrap();
    // Alphabetical: python before rust.
    assert_eq!(langs[0], "python");
    assert_eq!(langs[1], "rust");
    // No sessions yet.
    assert_eq!(out.metadata["active_session_count"], 0);
    // Output mentions debug_launch as the next step.
    assert!(out.output.contains("debug_launch"));
}

// ── debug_launch ──────────────────────────────────────────────

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

// ── debug_set_breakpoint ──────────────────────────────────────

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

// ── debug_continue ────────────────────────────────────────────

#[test]
fn t131tool_continue_id_and_category() {
    let t = DebugContinueTool::new(empty_manager());
    assert_eq!(t.id(), "debug_continue");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t131tool_continue_schema_validates() {
    let t = DebugContinueTool::new(empty_manager());
    t.schema().validate().unwrap();
}

#[tokio::test]
async fn t131tool_continue_missing_session_id_returns_invalid_args() {
    let t = DebugContinueTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t.execute(json!({}), &ctx, &mut perms).await.unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t131tool_continue_unknown_session_returns_actionable_error() {
    let t = DebugContinueTool::new(empty_manager());
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

// ── debug_terminate ───────────────────────────────────────────

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

// ── shared helpers ────────────────────────────────────────────

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

// ── debug_step ────────────────────────────────────────────────

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

// ── debug_eval ────────────────────────────────────────────────

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

// ── debug_stack_trace ─────────────────────────────────────────

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

// ── debug_variables ───────────────────────────────────────────

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

// ── debug_scopes ──────────────────────────────────────────────

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

// ── debug_threads ─────────────────────────────────────────────

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

// Suppress unused-helper warning when no test needs make_ctx +
// PathBuf together (e.g. cfg-gated builds).
#[allow(dead_code)]
fn _force_paths_ref(_: &Path) {}
