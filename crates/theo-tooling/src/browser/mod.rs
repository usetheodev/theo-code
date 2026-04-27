//! T2.1 — Browser automation via a Node-hosted Playwright sidecar.
//!
//! The Rust side (`Capability::Browser`-gated) speaks JSON-RPC to a
//! Node subprocess running `scripts/playwright_sidecar.js`. The
//! sidecar exposes Playwright's chromium control as a stable
//! request/response API — Theo never embeds the browser bytes.
//!
//! Why a sidecar (per ADR D2): the Rust browser ecosystem
//! (chromiumoxide, headless_chrome) is not at parity with Playwright
//! for CDP completeness. A 150-line Node sidecar is the cheap path
//! to feature-parity with Cursor / Lovable / Bolt's browser
//! integration.
//!
//! This module ships:
//! - `protocol.rs` — `BrowserAction` + `BrowserResult` types and the
//!   JSON-RPC envelope used between the Rust client and the JS
//!   sidecar.
//! - `scripts/playwright_sidecar.js` (next to `crates/theo-tooling/`)
//!   — the Node implementation. Loads playwright on first action.
//!
//! Subprocess wiring (spawn + stdio routing) is the next iteration —
//! the protocol types are testable without a real Node runtime.

pub mod client;
pub mod protocol;
pub mod session_manager;
pub mod sidecar;
pub mod tool;

pub use client::{BrowserClient, BrowserClientError, NoopWriter, SidecarWriter};
pub use protocol::{
    BrowserAction, BrowserError, BrowserRequest, BrowserResponse, BrowserResult, ScreenshotFormat,
};
pub use session_manager::{BrowserSessionError, BrowserSessionManager};
pub use sidecar::{SidecarError, SidecarSession};
pub use tool::{
    BrowserClickTool, BrowserCloseTool, BrowserEvalTool, BrowserOpenTool,
    BrowserScreenshotTool, BrowserTypeTool, BrowserWaitForSelectorTool,
};
