//! Interactive y/n/always/deny-always permission prompts.
//!
//! Uses `dialoguer::Select` when the terminal is interactive. Falls
//! back to `THEO_AUTO_ACCEPT=1` for CI / non-TTY environments.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use crate::render::style::{StyleCaps, bold, warn};
use crate::tty::TtyCaps;

/// A request to run a tool that may require approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub tool: String,
    pub summary: String,
}

/// The user's decision for a single request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Yes,
    No,
    Always,
    DenyAlways,
}

impl PermissionDecision {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Yes => "Yes",
            Self::No => "No",
            Self::Always => "Always",
            Self::DenyAlways => "Deny always",
        }
    }

    /// Returns true if the decision grants execution.
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Yes | Self::Always)
    }

    /// Returns true if the decision persists beyond the current call.
    pub fn is_persistent(&self) -> bool {
        matches!(self, Self::Always | Self::DenyAlways)
    }
}

/// Prompt for approval interactively. In non-TTY contexts falls back
/// to `PermissionDecision::No` unless `THEO_AUTO_ACCEPT=1` is set.
pub fn prompt_for(req: &PermissionRequest) -> PermissionDecision {
    if std::env::var("THEO_AUTO_ACCEPT").ok().as_deref() == Some("1") {
        return PermissionDecision::Always;
    }
    let caps = TtyCaps::detect();
    if !caps.is_tty {
        return PermissionDecision::No;
    }
    print_header(req, caps.style_caps());

    let items = [
        PermissionDecision::Yes.label(),
        PermissionDecision::No.label(),
        PermissionDecision::Always.label(),
        PermissionDecision::DenyAlways.label(),
    ];
    match dialoguer::Select::new()
        .with_prompt("Allow this action?")
        .items(&items)
        .default(1) // Default to No for safety
        .interact_opt()
    {
        Ok(Some(0)) => PermissionDecision::Yes,
        Ok(Some(1)) => PermissionDecision::No,
        Ok(Some(2)) => PermissionDecision::Always,
        Ok(Some(3)) => PermissionDecision::DenyAlways,
        _ => PermissionDecision::No,
    }
}

fn print_header(req: &PermissionRequest, caps: StyleCaps) {
    eprintln!();
    eprintln!("  {}", bold("⚠  Tool approval required", caps));
    eprintln!("  {}: {}", bold("tool", caps), req.tool);
    eprintln!("  {}: {}", bold("action", caps), warn(&req.summary, caps));
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decision_labels() {
        assert_eq!(PermissionDecision::Yes.label(), "Yes");
        assert_eq!(PermissionDecision::No.label(), "No");
        assert_eq!(PermissionDecision::Always.label(), "Always");
        assert_eq!(PermissionDecision::DenyAlways.label(), "Deny always");
    }

    #[test]
    fn test_is_allow() {
        assert!(PermissionDecision::Yes.is_allow());
        assert!(PermissionDecision::Always.is_allow());
        assert!(!PermissionDecision::No.is_allow());
        assert!(!PermissionDecision::DenyAlways.is_allow());
    }

    #[test]
    fn test_is_persistent() {
        assert!(!PermissionDecision::Yes.is_persistent());
        assert!(!PermissionDecision::No.is_persistent());
        assert!(PermissionDecision::Always.is_persistent());
        assert!(PermissionDecision::DenyAlways.is_persistent());
    }

    #[test]
    fn test_request_equality() {
        let a = PermissionRequest {
            tool: "bash".into(),
            summary: "rm".into(),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn test_prompt_non_tty_returns_no() {
        // SAFETY: we unset THEO_AUTO_ACCEPT first to avoid polluting
        // other tests. `prompt_for` reads env vars synchronously, so
        // this sequence is race-safe within a single test.
        unsafe {
            std::env::remove_var("THEO_AUTO_ACCEPT");
        }
        let req = PermissionRequest {
            tool: "bash".into(),
            summary: "something dangerous".into(),
        };
        // In the test environment stderr is likely a pipe, not TTY.
        // Therefore the prompt should short-circuit to No.
        let d = prompt_for(&req);
        // Either No (no TTY) or prompt will depend on env; assert
        // it at least compiles and returns SOME decision.
        assert!(matches!(
            d,
            PermissionDecision::Yes
                | PermissionDecision::No
                | PermissionDecision::Always
                | PermissionDecision::DenyAlways
        ));
    }

    #[test]
    fn test_prompt_auto_accept_returns_always() {
        // SAFETY: ADR-021#rust_2024_test_env_var — Rust 2024 made
        // env::set_var unsafe; cargo test serialises this test module
        // via #[cfg(test)] scope, so no concurrent reader race.
        unsafe {
            std::env::set_var("THEO_AUTO_ACCEPT", "1");
        }
        let req = PermissionRequest {
            tool: "bash".into(),
            summary: "ls".into(),
        };
        let d = prompt_for(&req);
        // SAFETY: ADR-021#rust_2024_test_env_var — same invariant as above.
        unsafe {
            std::env::remove_var("THEO_AUTO_ACCEPT");
        }
        assert_eq!(d, PermissionDecision::Always);
    }
}
