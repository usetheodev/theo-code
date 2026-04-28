//! Shared helpers for the browser_* tool family (T1.4 of god-files-2026-07-23-plan.md).

#![allow(unused_imports, dead_code)]

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

pub fn map_session_error(err: BrowserSessionError) -> ToolError {
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

