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
// `browser_eval`
// ---------------------------------------------------------------------------

pub struct BrowserEvalTool {
    manager: Arc<BrowserSessionManager>,
}

impl BrowserEvalTool {
    pub fn new(manager: Arc<BrowserSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserEvalTool {
    fn id(&self) -> &str {
        "browser_eval"
    }

    fn description(&self) -> &str {
        "T2.1 — Run a JS expression / IIFE in the page context and return its \
         JSON-serialized value. Useful for extracting structured data the page \
         exposes only at runtime (auth tokens in localStorage, hydrated React \
         state, etc.). The result must be JSON-serialisable — DOM nodes and \
         functions return as null. Requires an open page. \
         Example: browser_eval({js: \"document.title\"}) or \
         browser_eval({js: \"JSON.stringify(window.__INITIAL_STATE__)\"})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "js".into(),
                param_type: "string".into(),
                description:
                    "JS expression or arrow body. Result must be JSON-serialisable. \
                     Use `JSON.stringify(x)` for complex objects."
                        .into(),
                required: true,
            }],
            input_examples: vec![
                json!({"js": "document.title"}),
                json!({"js": "Array.from(document.querySelectorAll('a')).map(a => a.href)"}),
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
        let js = args
            .get("js")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing string `js`".into()))?
            .trim()
            .to_string();
        if js.is_empty() {
            return Err(ToolError::InvalidArgs("`js` is empty".into()));
        }

        let result = self
            .manager
            .request(BrowserAction::Eval { js: js.clone() })
            .await
            .map_err(map_session_error)?;
        match result {
            BrowserResult::EvalResult { value } => {
                let preview = match &value {
                    Value::String(s) => {
                        let trimmed: String = s.chars().take(80).collect();
                        if s.chars().count() > 80 {
                            format!("\"{trimmed}…\"")
                        } else {
                            format!("\"{trimmed}\"")
                        }
                    }
                    other => other.to_string().chars().take(80).collect(),
                };
                Ok(ToolOutput::new(
                    format!("browser_eval: {preview}"),
                    format!("expression: {js}\nresult: {value}"),
                )
                .with_metadata(json!({
                    "type": "browser_eval",
                    "expression": js,
                    "result": value,
                })))
            }
            other => Err(ToolError::Execution(format!(
                "unexpected sidecar result for `eval`: {other:?}"
            ))),
        }
    }
}

