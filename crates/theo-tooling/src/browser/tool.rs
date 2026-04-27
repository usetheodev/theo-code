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
use crate::browser::session_manager::{BrowserSessionError, BrowserSessionManager, BrowserStatus};

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// Test body lives in the sibling file `tool_tests.rs` so this file
// stays small and consistent with the other sidecar-backed tool
// families (`dap/tool.rs`, `lsp/tool.rs`). Same module path
// (`crate::browser::tool::tests::*`), same visibility (private items
// reachable via `use super::*` from inside `tool_tests.rs`).
#[cfg(test)]
#[path = "tool_tests.rs"]
mod tests;
