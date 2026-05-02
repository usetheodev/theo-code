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
// `browser_click`
// ---------------------------------------------------------------------------

pub struct BrowserClickTool {
    manager: Arc<BrowserSessionManager>,
}

impl BrowserClickTool {
    pub fn new(manager: Arc<BrowserSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserClickTool {
    fn id(&self) -> &str {
        "browser_click"
    }

    fn description(&self) -> &str {
        "T2.1 — Click an element matching `selector` on the open page. \
         Selector syntax follows Playwright's flexible engine: CSS \
         (`#login`, `.btn-primary`), text (`text=Submit`), role \
         (`role=button[name=\"OK\"]`). Errors when no element matches \
         within the default timeout. Requires an open page (call \
         browser_open first). \
         Example: browser_click({selector: \"text=Sign in\"})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "selector".into(),
                param_type: "string".into(),
                description:
                    "Playwright selector — supports CSS, text=, role=, etc. See https://playwright.dev/docs/selectors."
                        .into(),
                required: true,
            }],
            input_examples: vec![
                json!({"selector": "#submit"}),
                json!({"selector": "text=Sign in"}),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let selector = args
            .get("selector")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing string `selector`".into()))?
            .trim()
            .to_string();
        if selector.is_empty() {
            return Err(ToolError::InvalidArgs("`selector` is empty".into()));
        }

        let result = self
            .manager
            .request(BrowserAction::Click {
                selector: selector.clone(),
            })
            .await
            .map_err(map_session_error)?;
        match result {
            BrowserResult::Empty => Ok(ToolOutput::new(
                format!("browser_click: clicked `{selector}`"),
                format!(
                    "Click dispatched on element matching `{selector}`. The page \
                     may have navigated; use browser_screenshot or browser_eval to \
                     observe the new state."
                ),
            )
            .with_metadata(json!({
                "type": "browser_click",
                "selector": selector,
            }))),
            other => Err(ToolError::Execution(format!(
                "unexpected sidecar result for `click`: {other:?}"
            ))),
        }
    }
}

