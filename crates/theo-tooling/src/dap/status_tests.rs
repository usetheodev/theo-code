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

