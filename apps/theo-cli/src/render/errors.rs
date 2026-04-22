//! Structured error/warning messages with optional hint + docs link.
//!
//! All user-facing error messages should be constructed through this
//! module so they consistently use styled icons, colors, and layout.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use crate::render::style::{StyleCaps, bold, dim, error, warn};

/// A structured error message.
#[derive(Debug, Clone)]
pub struct CliError {
    pub title: String,
    pub detail: Option<String>,
    pub hint: Option<String>,
    pub docs: Option<String>,
}

impl CliError {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            detail: None,
            hint: None,
            docs: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_docs(mut self, url: impl Into<String>) -> Self {
        self.docs = Some(url.into());
        self
    }
}

/// Render an error as a styled multi-line string.
pub fn format_error(err: &CliError, caps: StyleCaps) -> String {
    let mut out = String::new();
    out.push_str(&error(format!("✗ {}", err.title), caps).to_string());
    if let Some(d) = &err.detail {
        out.push('\n');
        out.push_str(&format!("  {}", dim(d, caps)));
    }
    if let Some(h) = &err.hint {
        out.push('\n');
        out.push_str(&format!(
            "  {} {h}",
            bold("hint:", caps)
        ));
    }
    if let Some(u) = &err.docs {
        out.push('\n');
        out.push_str(&format!(
            "  {} {u}",
            dim("docs:", caps)
        ));
    }
    out
}

/// A simpler warning variant (no docs link, yellow tone).
#[derive(Debug, Clone)]
pub struct CliWarning {
    pub title: String,
    pub hint: Option<String>,
}

impl CliWarning {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            hint: None,
        }
    }
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

pub fn format_warning(w: &CliWarning, caps: StyleCaps) -> String {
    let mut out = warn(format!("⚠ {}", w.title), caps).to_string();
    if let Some(h) = &w.hint {
        out.push('\n');
        out.push_str(&format!("  {} {h}", bold("hint:", caps)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> StyleCaps {
        StyleCaps::plain()
    }

    #[test]
    fn test_new_error_has_only_title() {
        let e = CliError::new("failed");
        assert_eq!(e.title, "failed");
        assert!(e.detail.is_none());
        assert!(e.hint.is_none());
        assert!(e.docs.is_none());
    }

    #[test]
    fn test_builder_sets_all_fields() {
        let e = CliError::new("failed")
            .with_detail("stack trace")
            .with_hint("try again")
            .with_docs("https://example.com");
        assert_eq!(e.detail.as_deref(), Some("stack trace"));
        assert_eq!(e.hint.as_deref(), Some("try again"));
        assert_eq!(e.docs.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn test_format_error_plain_has_title() {
        let e = CliError::new("boom");
        let out = format_error(&e, plain());
        assert!(out.starts_with("✗ boom"));
    }

    #[test]
    fn test_format_error_includes_hint() {
        let e = CliError::new("oops").with_hint("check config");
        let out = format_error(&e, plain());
        assert!(out.contains("hint:"));
        assert!(out.contains("check config"));
    }

    #[test]
    fn test_format_error_includes_docs() {
        let e = CliError::new("oops").with_docs("https://x");
        let out = format_error(&e, plain());
        assert!(out.contains("docs:"));
        assert!(out.contains("https://x"));
    }

    #[test]
    fn test_format_error_tty_has_ansi() {
        let e = CliError::new("oops");
        let out = format_error(&e, StyleCaps::full());
        assert!(out.contains("\x1b["));
    }

    #[test]
    fn test_warning_formats_with_icon() {
        let w = CliWarning::new("beware");
        let out = format_warning(&w, plain());
        assert!(out.contains("⚠ beware"));
    }

    #[test]
    fn test_warning_with_hint() {
        let w = CliWarning::new("disk").with_hint("free up space");
        let out = format_warning(&w, plain());
        assert!(out.contains("hint:"));
        assert!(out.contains("free up space"));
    }

    #[test]
    fn test_error_format_is_multi_line_when_detail_present() {
        let e = CliError::new("x").with_detail("y");
        let out = format_error(&e, plain());
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn test_error_format_is_deterministic() {
        let e = CliError::new("x").with_hint("y").with_docs("z");
        let a = format_error(&e, plain());
        let b = format_error(&e, plain());
        assert_eq!(a, b);
    }
}
