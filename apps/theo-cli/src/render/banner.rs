//! Startup banner rendering.
//!
//! Replaces the inline `print_banner` in `repl.rs` with a testable
//! pure function that returns a fully styled string.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use crate::render::style::{StyleCaps, accent, bold, dim};

/// Input to the banner renderer.
#[derive(Debug, Clone)]
pub struct BannerInfo<'a> {
    pub version: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub mode: &'a str,
    pub project: &'a str,
}

/// Render the startup banner as a multi-line string.
pub fn render_banner(info: &BannerInfo<'_>, caps: StyleCaps) -> String {
    let mut out = String::new();
    out.push_str(&bold(format!("theo v{}", info.version), caps).to_string());
    out.push(' ');
    out.push_str(&dim("— type /help for commands", caps).to_string());
    out.push('\n');
    out.push_str(&format!(
        "Provider: {} · Model: {} · Mode: {}",
        accent(info.provider, caps),
        accent(info.model, caps),
        accent(info.mode, caps),
    ));
    out.push('\n');
    out.push_str(&dim(format!("Project: {}", info.project), caps).to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info<'a>() -> BannerInfo<'a> {
        BannerInfo {
            version: "0.1.0",
            provider: "openai",
            model: "gpt-4",
            mode: "agent",
            project: "/home/paulo/theo-code",
        }
    }

    #[test]
    fn test_banner_plain_contains_fields() {
        let out = render_banner(&info(), StyleCaps::plain());
        assert!(out.contains("theo v0.1.0"));
        assert!(out.contains("openai"));
        assert!(out.contains("gpt-4"));
        assert!(out.contains("agent"));
        assert!(out.contains("/home/paulo/theo-code"));
    }

    #[test]
    fn test_banner_has_three_lines() {
        let out = render_banner(&info(), StyleCaps::plain());
        assert_eq!(out.lines().count(), 3);
    }

    #[test]
    fn test_banner_tty_contains_ansi() {
        let out = render_banner(&info(), StyleCaps::full());
        assert!(out.contains("\x1b["));
    }

    #[test]
    fn test_banner_is_deterministic() {
        let i = info();
        assert_eq!(
            render_banner(&i, StyleCaps::plain()),
            render_banner(&i, StyleCaps::plain())
        );
    }

    #[test]
    fn test_banner_mentions_help_hint() {
        let out = render_banner(&info(), StyleCaps::plain());
        assert!(out.contains("/help"));
    }
}
