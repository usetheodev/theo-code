//! Interactive permission prompts for tool execution.
//!
//! Wraps `dialoguer::Select` with y/n/always/deny-always choices.
//! Session-scoped ACL decisions are stored in a thread-safe
//! `PermissionSession` that the REPL can inject into the tool runner.
//!
//! See ADR-004: CLI may depend on governance indirectly via this
//! presentation-layer gate.

pub mod prompt;
pub mod session;

pub use prompt::{PermissionDecision, PermissionRequest, prompt_for};
pub use session::{PermissionSession, SessionOutcome};
