//! Shared helpers for the `debug_*` tool family.
//!
//! Extracted from `dap/tool.rs` during the god-files-2026-07-23 plan
//! Phase 1 split (D2 strategy). Each individual tool lives in its own
//! file under `dap/`; they all import these helpers via
//! `use crate::dap::tool_common::*;`.

use std::sync::Arc;

use serde_json::Value;

use theo_domain::error::ToolError;

use crate::dap::client::DapClient;
use crate::dap::protocol::DapResponse;
use crate::dap::session_manager::{DapSessionError, DapSessionManager};

pub(crate) fn parse_session_id(args: &Value) -> Result<String, ToolError> {
    let id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArgs("missing string `session_id`".into()))?
        .trim()
        .to_string();
    if id.is_empty() {
        return Err(ToolError::InvalidArgs("`session_id` is empty".into()));
    }
    Ok(id)
}

pub(crate) fn map_session_error(err: DapSessionError) -> ToolError {
    match err {
        DapSessionError::NoAdapterForLanguage { language } => ToolError::Execution(format!(
            "no DAP adapter installed for language `{language}`. Install one              (e.g. lldb-vscode for rust/c/cpp, debugpy for python, dlv for go,              js-debug-adapter for javascript/typescript) or fall back to print              debugging."
        )),
        DapSessionError::SessionAlreadyExists { id } => ToolError::InvalidArgs(format!(
            "debug session id `{id}` is already active. Pick a different              session_id, or call `debug_terminate({{session_id: \"{id}\"}})`              first."
        )),
        DapSessionError::InitializeFailed(msg) => ToolError::Execution(format!(
            "DAP `initialize` failed: {msg}"
        )),
        DapSessionError::LaunchFailed(msg) => ToolError::Execution(format!(
            "DAP `launch` failed: {msg}"
        )),
        DapSessionError::AttachFailed(msg) => ToolError::Execution(format!(
            "DAP `attach` failed: {msg}"
        )),
        DapSessionError::Client(e) => ToolError::Execution(format!("DAP client error: {e}")),
    }
}

pub(crate) fn require_session(
    manager: &DapSessionManager,
    session_id: &str,
) -> impl std::future::Future<
    Output = Result<Arc<DapClient<tokio::process::ChildStdin>>, ToolError>,
> + Send {
    let id = session_id.to_string();
    let manager = manager.clone();
    async move {
        manager.session(&id).await.ok_or_else(|| {
            ToolError::Execution(format!(
                "no active debug session with id `{id}`. Call `debug_launch`                  first to open one."
            ))
        })
    }
}

pub(crate) fn check_response(resp: &DapResponse, command: &str) -> Result<(), ToolError> {
    if !resp.success {
        let msg = resp
            .message
            .as_deref()
            .unwrap_or("(no message)");
        return Err(ToolError::Execution(format!(
            "DAP `{command}` failed: {msg}"
        )));
    }
    Ok(())
}
