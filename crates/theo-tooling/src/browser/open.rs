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
// `browser_open`
// ---------------------------------------------------------------------------

pub struct BrowserOpenTool {
    manager: Arc<BrowserSessionManager>,
}

impl BrowserOpenTool {
    pub fn new(manager: Arc<BrowserSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserOpenTool {
    fn id(&self) -> &str {
        "browser_open"
    }

    fn description(&self) -> &str {
        "T2.1 — Navigate the headless Chromium session to `url` and wait for \
         load. Returns the final URL (after redirects) and the document title. \
         Lazily spawns the Playwright sidecar on first call. Use this BEFORE \
         browser_click / browser_screenshot / browser_eval — those need an \
         open page. Pair with browser_close at the end of the workflow. \
         Example: browser_open({url: \"https://example.com\"})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "url".into(),
                param_type: "string".into(),
                description:
                    "Absolute URL to navigate to. Sidecar uses Playwright's `page.goto(url, {waitUntil: 'load'})`."
                        .into(),
                required: true,
            }],
            input_examples: vec![json!({"url": "https://example.com"})],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let url = args
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing string `url`".into()))?
            .trim()
            .to_string();
        if url.is_empty() {
            return Err(ToolError::InvalidArgs("`url` is empty".into()));
        }

        // T14.1 — surface lifecycle progress to the streaming UI.
        // Page loads can take 2–10 s; a single static "tool started"
        // indicator gives a poor UX. Three checkpoints mirror what
        // a user sees in a real browser: spawn → navigate → ready.
        crate::partial::emit_progress_with_pct(
            ctx,
            "browser_open",
            format!("Spawning sidecar for {url}"),
            0.10,
        );

        let result = self
            .manager
            .request(BrowserAction::Open { url: url.clone() })
            .await
            .map_err(map_session_error)?;

        crate::partial::emit_progress_with_pct(ctx, "browser_open", "Navigated", 1.0);

        match result {
            BrowserResult::Navigated { final_url, title } => Ok(ToolOutput::new(
                format!("browser_open: {title}"),
                format!(
                    "Navigated to `{url}` (final: `{final_url}`).\nTitle: {title}.\n\
                     Use browser_screenshot / browser_eval / browser_click next."
                ),
            )
            .with_metadata(json!({
                "type": "browser_open",
                "requested_url": url,
                "final_url": final_url,
                "title": title,
            }))),
            other => Err(ToolError::Execution(format!(
                "unexpected sidecar result for `open`: {other:?}"
            ))),
        }
    }
}

