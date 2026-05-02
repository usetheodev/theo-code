//! Single-tool slice extracted from `browser/tool.rs` (T1.4 of god-files-2026-07-23-plan.md, ADR D2).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

use crate::browser::session_manager::{BrowserSessionError, BrowserSessionManager, BrowserStatus};
use crate::browser::protocol::{BrowserAction, BrowserError, BrowserResult, ScreenshotFormat};

use crate::browser::tool_common::*;

// ---------------------------------------------------------------------------
// `browser_status`
// ---------------------------------------------------------------------------

/// `browser_status` — report whether the Playwright sidecar script is
/// reachable AND whether a browser session is currently active. Lets
/// the agent decide between `browser_open` (and the rest of the
/// browser_* family) and a `webfetch` fallback BEFORE issuing a
/// doomed call against an environment with no Node / no script.
pub struct BrowserStatusTool {
    manager: Arc<BrowserSessionManager>,
}

impl BrowserStatusTool {
    pub fn new(manager: Arc<BrowserSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserStatusTool {
    fn id(&self) -> &str {
        "browser_status"
    }

    fn description(&self) -> &str {
        "T2.1 — Report whether the Playwright sidecar (Node.js + bundled \
         script) is reachable AND whether a browser session is currently \
         active. Use BEFORE `browser_open` / `browser_click` / \
         `browser_screenshot` / `browser_eval` to know whether the \
         environment can drive a browser at all; when the script is \
         missing, fall back to `webfetch` for static HTML. \
         Example: browser_status({})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![],
            input_examples: vec![json!({})],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        _args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let BrowserStatus {
            node_program,
            script_path,
            script_present,
            session_active,
        } = self.manager.status().await;
        let script_path_str = script_path.display().to_string();
        let metadata = json!({
            "type": "browser_status",
            "node_program": node_program,
            "script_path": script_path_str,
            "script_present": script_present,
            "session_active": session_active,
        });
        let mut output = String::new();
        if !script_present {
            output.push_str(&format!(
                "Playwright sidecar script not reachable at `{script_path_str}`. \
                 Browser tools (browser_open / browser_click / browser_screenshot / \
                 browser_eval / browser_wait_for_selector / browser_close) cannot \
                 run; fall back to `webfetch` for static HTML or set \
                 $THEO_BROWSER_SIDECAR to a valid script path."
            ));
        } else {
            output.push_str(&format!(
                "Playwright sidecar script present at `{script_path_str}` (node \
                 binary `{node_program}`)."
            ));
        }
        if session_active {
            output.push_str(
                "\nA browser session is currently active. Use browser_click / \
                 browser_type / browser_screenshot / browser_eval against the \
                 open page; close it with browser_close when finished.",
            );
        } else if script_present {
            output.push_str(
                "\nNo active session yet. Open one with browser_open({url}).",
            );
        }
        let title = format!(
            "browser_status: script_present={script_present}, session_active={session_active}"
        );
        Ok(ToolOutput::new(title, output).with_metadata(metadata))
    }
}

