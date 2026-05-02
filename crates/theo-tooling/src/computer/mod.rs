//! T4.1 — Computer Use protocol types.
//!
//! Maps to the Anthropic Computer Use API surface
//! (`tool: "computer_20250124"`). The Rust client converts an LLM-issued
//! action into an OS-specific subprocess call:
//!
//! - Linux: `xdotool` (X11) or `wlrctl`/`ydotool` (Wayland)
//! - macOS: `cliclick`
//! - Windows: `nircmd`
//!
//! Subprocess spawning is the next iteration; this module ships the
//! action / result types + serde wire format + error mapping. Pure
//! code — testable without any OS tool installed.
//!
//! Capability gate: `Capability::ComputerUse` is OFF by default
//! (per ADR D6 — risk is high). Caller MUST opt in explicitly.

pub mod driver;
pub mod protocol;
pub mod tool;

pub use driver::{execute_action, validate_xdotool_key};
pub use protocol::{
    ComputerAction, ComputerError, ComputerResult, MouseButton, ScrollDirection,
};
pub use tool::ComputerActionTool;
