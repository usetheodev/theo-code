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
// `browser_screenshot`
// ---------------------------------------------------------------------------

pub struct BrowserScreenshotTool {
    manager: Arc<BrowserSessionManager>,
}

impl BrowserScreenshotTool {
    pub fn new(manager: Arc<BrowserSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for BrowserScreenshotTool {
    fn id(&self) -> &str {
        "browser_screenshot"
    }

    fn description(&self) -> &str {
        "T2.1 — Capture the current page as base64-encoded PNG (default) or \
         JPEG. `full_page: true` captures the entire scrollable area; default \
         `false` captures only the viewport. Returns the image as a vision \
         block — the next assistant turn sees the image and can reason about \
         the page visually. Requires an open page. \
         Example: browser_screenshot({full_page: true, format: \"png\"})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "full_page".into(),
                    param_type: "boolean".into(),
                    description:
                        "Capture entire scrollable area (true) or just the viewport (default false)."
                            .into(),
                    required: false,
                },
                ToolParam {
                    name: "format".into(),
                    param_type: "string".into(),
                    description: "`png` (default) or `jpeg`.".into(),
                    required: false,
                },
            ],
            input_examples: vec![
                json!({}),
                json!({"full_page": true}),
                json!({"format": "jpeg"}),
            ],
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
        let full_page = args.get("full_page").and_then(Value::as_bool).unwrap_or(false);
        let format = match args.get("format").and_then(Value::as_str) {
            None => ScreenshotFormat::Png,
            Some("png") => ScreenshotFormat::Png,
            Some("jpeg") | Some("jpg") => ScreenshotFormat::Jpeg,
            Some(other) => {
                return Err(ToolError::InvalidArgs(format!(
                    "`format` must be `png` or `jpeg` (got `{other}`)"
                )));
            }
        };

        // T14.1 — full-page captures of long pages can take seconds
        // (Playwright re-renders + the sidecar base64-encodes the
        // PNG). Surface a single in-progress checkpoint so the UI
        // doesn't appear frozen.
        crate::partial::emit_progress(
            ctx,
            "browser_screenshot",
            if full_page {
                "Capturing full page (may take a few seconds)…"
            } else {
                "Capturing viewport…"
            },
        );

        let result = self
            .manager
            .request(BrowserAction::Screenshot { full_page, format })
            .await
            .map_err(map_session_error)?;

        match result {
            BrowserResult::Screenshot { media_type, data } => {
                let bytes_b64 = data.len();
                Ok(ToolOutput::new(
                    format!(
                        "browser_screenshot: {} ({}{} base64 bytes)",
                        media_type,
                        if full_page { "full-page, " } else { "" },
                        bytes_b64
                    ),
                    "Screenshot captured. The image is attached as a vision \
                     block so the next assistant turn can read the page \
                     visually."
                        .to_string(),
                )
                .with_metadata(json!({
                    "type": "browser_screenshot",
                    "media_type": media_type,
                    "full_page": full_page,
                    "format": match format {
                        ScreenshotFormat::Png => "png",
                        ScreenshotFormat::Jpeg => "jpeg",
                    },
                    "base64_bytes": bytes_b64,
                    // The actual image content goes in metadata.data so
                    // the agent runtime's vision propagation can pick it
                    // up (see vision_propagation.rs).
                    "data": data,
                })))
            }
            other => Err(ToolError::Execution(format!(
                "unexpected sidecar result for `screenshot`: {other:?}"
            ))),
        }
    }
}

