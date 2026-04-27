//! T2.1 — Agent-callable browser tool family.
//!
//! Wraps `BrowserSessionManager` so the agent can drive Chromium
//! via the Playwright sidecar. Tools share one Arc'd manager so
//! navigation state (current page, cookies) persists across calls
//! within an agent run.
//!
//! Initial set: `browser_open`, `browser_click`, `browser_screenshot`,
//! `browser_close`. `browser_type` and `browser_eval` come next.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

use crate::browser::protocol::{BrowserAction, BrowserResult, ScreenshotFormat};
use crate::browser::session_manager::{BrowserSessionError, BrowserSessionManager};

fn map_session_error(err: BrowserSessionError) -> ToolError {
    match err {
        BrowserSessionError::NodeMissing { program } => ToolError::Execution(format!(
            "Node not found at `{program}`. Install Node.js (https://nodejs.org/) and run \
             `npx playwright install chromium` to enable browser tools, or fall back to \
             webfetch for static HTML."
        )),
        BrowserSessionError::ScriptMissing { path } => ToolError::Execution(format!(
            "Playwright sidecar script missing at `{path}`. The script ships under \
             crates/theo-tooling/scripts/playwright_sidecar.js — confirm the install bundle \
             includes it."
        )),
        BrowserSessionError::Client(e) => {
            ToolError::Execution(format!("browser client error: {e}"))
        }
    }
}

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
        _ctx: &ToolContext,
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

        let result = self
            .manager
            .request(BrowserAction::Open { url: url.clone() })
            .await
            .map_err(map_session_error)?;

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
        _ctx: &ToolContext,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use theo_domain::session::{MessageId, SessionId};

    fn make_ctx() -> ToolContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolContext {
            session_id: SessionId::new("ses_test"),
            message_id: MessageId::new(""),
            call_id: "call_test".into(),
            agent: "build".into(),
            abort: rx,
            project_dir: PathBuf::from("/tmp"),
            graph_context: None,
            stdout_tx: None,
        }
    }

    fn missing_script_manager() -> Arc<BrowserSessionManager> {
        // A path that is guaranteed not to exist — every ensure_client
        // call surfaces ScriptMissing, which the tools should map to
        // ToolError::Execution with an actionable message.
        Arc::new(BrowserSessionManager::new(
            "node",
            PathBuf::from("/nonexistent/playwright_sidecar.js"),
        ))
    }

    // ── browser_open ──────────────────────────────────────────────

    #[test]
    fn t21btool_open_id_and_category() {
        let t = BrowserOpenTool::new(missing_script_manager());
        assert_eq!(t.id(), "browser_open");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t21btool_open_schema_validates() {
        let t = BrowserOpenTool::new(missing_script_manager());
        t.schema().validate().unwrap();
    }

    #[tokio::test]
    async fn t21btool_open_missing_url_returns_invalid_args() {
        let t = BrowserOpenTool::new(missing_script_manager());
        let mut perms = PermissionCollector::new();
        let err = t.execute(json!({}), &make_ctx(), &mut perms).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t21btool_open_empty_url_returns_invalid_args() {
        let t = BrowserOpenTool::new(missing_script_manager());
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({"url": "  "}), &make_ctx(), &mut perms)
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => assert!(msg.contains("`url` is empty")),
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t21btool_open_missing_script_returns_actionable_error() {
        let t = BrowserOpenTool::new(missing_script_manager());
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"url": "https://example.com"}),
                &make_ctx(),
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => {
                assert!(msg.contains("playwright_sidecar.js"));
                assert!(msg.contains("crates/theo-tooling/scripts"));
            }
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    // ── browser_click ─────────────────────────────────────────────

    #[test]
    fn t21btool_click_id_and_category() {
        let t = BrowserClickTool::new(missing_script_manager());
        assert_eq!(t.id(), "browser_click");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t21btool_click_schema_validates() {
        let t = BrowserClickTool::new(missing_script_manager());
        t.schema().validate().unwrap();
    }

    #[tokio::test]
    async fn t21btool_click_missing_selector_returns_invalid_args() {
        let t = BrowserClickTool::new(missing_script_manager());
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({}), &make_ctx(), &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t21btool_click_empty_selector_returns_invalid_args() {
        let t = BrowserClickTool::new(missing_script_manager());
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"selector": "   "}),
                &make_ctx(),
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => assert!(msg.contains("`selector` is empty")),
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    // ── browser_screenshot ────────────────────────────────────────

    #[test]
    fn t21btool_screenshot_id_and_category() {
        let t = BrowserScreenshotTool::new(missing_script_manager());
        assert_eq!(t.id(), "browser_screenshot");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t21btool_screenshot_schema_validates_with_optional_args() {
        let t = BrowserScreenshotTool::new(missing_script_manager());
        let schema = t.schema();
        schema.validate().unwrap();
        // Both fields are optional — caller can omit both.
        for p in &schema.params {
            assert!(!p.required, "{} should be optional", p.name);
        }
    }

    #[tokio::test]
    async fn t21btool_screenshot_invalid_format_returns_invalid_args() {
        let t = BrowserScreenshotTool::new(missing_script_manager());
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"format": "webp"}),
                &make_ctx(),
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => {
                assert!(msg.contains("`format`"));
                assert!(msg.contains("png"));
                assert!(msg.contains("jpeg"));
            }
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t21btool_screenshot_jpg_alias_accepted() {
        // `jpg` is the common typo for `jpeg`. Tools accept both
        // for ergonomics; map to ScreenshotFormat::Jpeg internally.
        // We can't run the full request without a sidecar, so the
        // test only verifies args parsing — the actual sidecar
        // call surfaces ScriptMissing as expected.
        let t = BrowserScreenshotTool::new(missing_script_manager());
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({"format": "jpg"}), &make_ctx(), &mut perms)
            .await
            .unwrap_err();
        // The arg parser accepted `jpg` (no InvalidArgs); the failure
        // is the downstream missing-script error.
        match err {
            ToolError::Execution(msg) => assert!(msg.contains("playwright_sidecar.js")),
            other => panic!("expected downstream Execution error, got {other:?}"),
        }
    }

    // ── browser_close ─────────────────────────────────────────────

    #[test]
    fn t21btool_close_id_and_category() {
        let t = BrowserCloseTool::new(missing_script_manager());
        assert_eq!(t.id(), "browser_close");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t21btool_close_schema_validates_with_no_args() {
        let t = BrowserCloseTool::new(missing_script_manager());
        let schema = t.schema();
        schema.validate().unwrap();
        assert!(schema.params.is_empty());
    }

    #[tokio::test]
    async fn t21btool_close_no_active_session_returns_was_active_false() {
        // Idempotency invariant: closing without an open session is
        // a no-op success, NOT an error. Cleanup routines might call
        // this without knowing the state.
        let t = BrowserCloseTool::new(missing_script_manager());
        let mut perms = PermissionCollector::new();
        let out = t
            .execute(json!({}), &make_ctx(), &mut perms)
            .await
            .unwrap();
        assert_eq!(out.metadata["was_active"], false);
        assert!(out.title.contains("no session was active"));
    }
}
