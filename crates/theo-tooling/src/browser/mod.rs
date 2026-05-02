//! T2.1 — Browser automation via a Node-hosted Playwright sidecar +
//! agent-callable tool family.
//!
//! The Rust side (`Capability::Browser`-gated) speaks JSON-RPC to a
//! Node subprocess running `scripts/playwright_sidecar.js`. 8 agent
//! tools live one-per-file. Pre-2026-04-28 the family lived in a
//! single 868-LOC `tool.rs`; the per-file split was T1.4 of
//! `docs/plans/god-files-2026-07-23-plan.md` (ADR D2).

pub mod client;
pub mod protocol;
pub mod session_manager;
pub mod sidecar;

pub(crate) mod tool_common;

mod click;
mod close;
mod eval;
mod open;
mod screenshot;
mod status;
mod type_text;
mod wait_for_selector;

pub use click::BrowserClickTool;
pub use close::BrowserCloseTool;
pub use eval::BrowserEvalTool;
pub use open::BrowserOpenTool;
pub use screenshot::BrowserScreenshotTool;
pub use status::BrowserStatusTool;
pub use type_text::BrowserTypeTool;
pub use wait_for_selector::BrowserWaitForSelectorTool;

pub use client::{BrowserClient, BrowserClientError, NoopWriter, SidecarWriter};
pub use protocol::{
    BrowserAction, BrowserError, BrowserRequest, BrowserResponse, BrowserResult, ScreenshotFormat,
};
pub use session_manager::{BrowserSessionError, BrowserSessionManager, BrowserStatus};
pub use sidecar::{SidecarError, SidecarSession};

#[cfg(test)]
#[path = "tool_tests.rs"]
mod tests;
