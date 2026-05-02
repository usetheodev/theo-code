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
// `browser_wait_for_selector`
// ---------------------------------------------------------------------------

pub struct BrowserWaitForSelectorTool {
    manager: Arc<BrowserSessionManager>,
}

impl BrowserWaitForSelectorTool {
    pub fn new(manager: Arc<BrowserSessionManager>) -> Self {
        Self { manager }
    }
}

const DEFAULT_WAIT_MS: u64 = 5_000;
const MAX_WAIT_MS: u64 = 60_000;

#[async_trait]
impl Tool for BrowserWaitForSelectorTool {
    fn id(&self) -> &str {
        "browser_wait_for_selector"
    }

    fn description(&self) -> &str {
        "T2.1 — Wait until `selector` appears in the page (DAP `waitForSelector`). \
         Use BEFORE browser_click on dynamically-rendered content (SPA navigation, \
         infinite scroll, AJAX-injected DOM). Default timeout 5000ms; max 60000ms \
         (1 minute). Returns success when found, or surfaces a typed \
         SelectorTimeout error including the timeout duration. \
         Example: browser_wait_for_selector({selector: \".loaded\", timeout_ms: 10000})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "selector".into(),
                    param_type: "string".into(),
                    description: "Playwright selector to wait for.".into(),
                    required: true,
                },
                ToolParam {
                    name: "timeout_ms".into(),
                    param_type: "integer".into(),
                    description: format!(
                        "Max wait in milliseconds (default {DEFAULT_WAIT_MS}, capped at {MAX_WAIT_MS})."
                    ),
                    required: false,
                },
            ],
            input_examples: vec![
                json!({"selector": ".loaded"}),
                json!({"selector": "#dynamic-content", "timeout_ms": 10000}),
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
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_WAIT_MS)
            .min(MAX_WAIT_MS);

        let result = self
            .manager
            .request(BrowserAction::WaitForSelector {
                selector: selector.clone(),
                timeout_ms,
            })
            .await
            .map_err(map_session_error)?;
        match result {
            BrowserResult::SelectorFound => Ok(ToolOutput::new(
                format!("browser_wait_for_selector: `{selector}` appeared"),
                format!(
                    "Element matching `{selector}` is now in the DOM (waited up to {timeout_ms}ms). \
                     Safe to call browser_click / browser_eval against it."
                ),
            )
            .with_metadata(json!({
                "type": "browser_wait_for_selector",
                "selector": selector,
                "timeout_ms": timeout_ms,
            }))),
            other => Err(ToolError::Execution(format!(
                "unexpected sidecar result for `wait_for_selector`: {other:?}"
            ))),
        }
    }
}

