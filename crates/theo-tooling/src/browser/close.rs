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

use crate::browser::session_manager::{BrowserSessionError, BrowserSessionManager};
use crate::browser::protocol::{BrowserAction, BrowserError, BrowserResult, ScreenshotFormat};

use crate::browser::tool_common::*;

// ---------------------------------------------------------------------------
// `browser_close`
// ---------------------------------------------------------------------------

pub struct BrowserCloseTool {
    manager: Arc<BrowserSessionManager>,
}

impl BrowserCloseTool {
    pub fn new(manager: Arc<BrowserSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserCloseTool {
    fn id(&self) -> &str {
        "browser_close"
    }

    fn description(&self) -> &str {
        "T2.1 — Close the current Playwright session and shut down the \
         sidecar. Idempotent: closing when no session is active is a no-op. \
         The next browser_open will respawn a fresh session. Always pair \
         with browser_open at the end of a workflow to free Chromium memory \
         (~150 MB per session). \
         Example: browser_close({})."
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
        let was_active = self.manager.terminate().await;
        Ok(ToolOutput::new(
            if was_active {
                "browser_close: session closed"
            } else {
                "browser_close: no session was active (no-op)"
            },
            if was_active {
                "Playwright sidecar terminated; Chromium freed."
            } else {
                "No browser session was active. terminate() is idempotent."
            },
        )
        .with_metadata(json!({
            "type": "browser_close",
            "was_active": was_active,
        })))
    }
}

// Tests for all 8 browser_* tools live in the sibling `tool_tests.rs`
// file, attached to the browser module via `#[path = "tool_tests.rs"]`
// in browser/mod.rs (T1.4 of god-files-2026-07-23-plan.md).
