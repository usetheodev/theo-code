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
// `browser_type`
// ---------------------------------------------------------------------------

pub struct BrowserTypeTool {
    manager: Arc<BrowserSessionManager>,
}

impl BrowserTypeTool {
    pub fn new(manager: Arc<BrowserSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserTypeTool {
    fn id(&self) -> &str {
        "browser_type"
    }

    fn description(&self) -> &str {
        "T2.1 — Fill the input matching `selector` with `text`. Uses \
         Playwright's `page.fill` (faster than `page.keyboard.type` for \
         forms — single atomic value set, no per-key delay). The previous \
         value is REPLACED, not appended. Requires an open page. \
         Example: browser_type({selector: \"input[name=q]\", text: \"hello\"})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "selector".into(),
                    param_type: "string".into(),
                    description:
                        "Playwright selector for the input — same syntax as browser_click."
                            .into(),
                    required: true,
                },
                ToolParam {
                    name: "text".into(),
                    param_type: "string".into(),
                    description:
                        "Value to set. Empty string clears the field."
                            .into(),
                    required: true,
                },
            ],
            input_examples: vec![json!({"selector": "input[name=q]", "text": "hello"})],
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
        let text = args
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing string `text`".into()))?
            .to_string();

        let result = self
            .manager
            .request(BrowserAction::Type {
                selector: selector.clone(),
                text: text.clone(),
            })
            .await
            .map_err(map_session_error)?;
        match result {
            BrowserResult::Empty => Ok(ToolOutput::new(
                format!("browser_type: filled `{selector}` ({} chars)", text.chars().count()),
                format!(
                    "Set value of `{selector}` to {} chars. The page may have \
                     fired input/change events; use browser_screenshot to verify.",
                    text.chars().count()
                ),
            )
            .with_metadata(json!({
                "type": "browser_type",
                "selector": selector,
                "text_length": text.chars().count(),
            }))),
            other => Err(ToolError::Execution(format!(
                "unexpected sidecar result for `type`: {other:?}"
            ))),
        }
    }
}

