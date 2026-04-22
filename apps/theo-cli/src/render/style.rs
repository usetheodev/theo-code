//! Style primitives for terminal rendering.
//!
//! This module is the ONLY place in `apps/theo-cli` allowed to emit ANSI
//! escape sequences. CI enforces this via a grep rule.
//!
//! All style functions return a [`Styled`] value that respects the
//! current [`StyleCaps`]. When colors are disabled (NO_COLOR, piped output),
//! the wrappers become no-ops and the raw text is emitted unchanged.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use std::fmt;

use crossterm::style::Stylize;

/// Terminal style capability flags.
///
/// Determines whether color/attribute escapes should be emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleCaps {
    /// Whether the output target supports ANSI color.
    pub colors: bool,
    /// Whether the output supports Unicode box-drawing characters.
    pub unicode: bool,
}

impl StyleCaps {
    /// All features enabled (default for TTY).
    pub const fn full() -> Self {
        Self {
            colors: true,
            unicode: true,
        }
    }

    /// No colors, no unicode (safe for piped output, CI logs).
    pub const fn plain() -> Self {
        Self {
            colors: false,
            unicode: false,
        }
    }

    /// Colors only (TTY without unicode, rare).
    pub const fn colors_only() -> Self {
        Self {
            colors: true,
            unicode: false,
        }
    }
}

impl Default for StyleCaps {
    fn default() -> Self {
        Self::full()
    }
}

/// A piece of text with a style applied.
///
/// Use the constructors (`success`, `error`, `warn`, `dim`, `accent`,
/// `tool_name`, `code_bg`, `bold`) to build styled strings, then `Display`
/// to render them with the appropriate escapes for the given `StyleCaps`.
#[derive(Debug, Clone)]
pub struct Styled {
    text: String,
    kind: StyleKind,
    caps: StyleCaps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleKind {
    Success,
    Error,
    Warn,
    Dim,
    Accent,
    ToolName,
    CodeBg,
    Bold,
}

impl Styled {
    fn new(text: impl Into<String>, kind: StyleKind, caps: StyleCaps) -> Self {
        Self {
            text: text.into(),
            kind,
            caps,
        }
    }

    /// Override caps after construction (for tests or late detection).
    pub fn with_caps(mut self, caps: StyleCaps) -> Self {
        self.caps = caps;
        self
    }

    /// The raw text with no escapes applied (test helper).
    pub fn raw(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for Styled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.caps.colors {
            return f.write_str(&self.text);
        }
        let rendered = match self.kind {
            StyleKind::Success => self.text.clone().green().to_string(),
            StyleKind::Error => self.text.clone().red().to_string(),
            StyleKind::Warn => self.text.clone().yellow().to_string(),
            StyleKind::Dim => self.text.clone().dark_grey().to_string(),
            StyleKind::Accent => self.text.clone().cyan().to_string(),
            StyleKind::ToolName => self.text.clone().magenta().bold().to_string(),
            StyleKind::CodeBg => self.text.clone().on_dark_grey().to_string(),
            StyleKind::Bold => self.text.clone().bold().to_string(),
        };
        f.write_str(&rendered)
    }
}

// ---- Public constructors ----

/// Green text for successful operations.
pub fn success(text: impl Into<String>, caps: StyleCaps) -> Styled {
    Styled::new(text, StyleKind::Success, caps)
}

/// Red text for errors.
pub fn error(text: impl Into<String>, caps: StyleCaps) -> Styled {
    Styled::new(text, StyleKind::Error, caps)
}

/// Yellow text for warnings.
pub fn warn(text: impl Into<String>, caps: StyleCaps) -> Styled {
    Styled::new(text, StyleKind::Warn, caps)
}

/// Dark grey text for secondary/muted content.
pub fn dim(text: impl Into<String>, caps: StyleCaps) -> Styled {
    Styled::new(text, StyleKind::Dim, caps)
}

/// Cyan text for accents (prompts, headings, key-value labels).
pub fn accent(text: impl Into<String>, caps: StyleCaps) -> Styled {
    Styled::new(text, StyleKind::Accent, caps)
}

/// Magenta + bold for tool names in result rendering.
pub fn tool_name(text: impl Into<String>, caps: StyleCaps) -> Styled {
    Styled::new(text, StyleKind::ToolName, caps)
}

/// Grey background for inline code snippets.
pub fn code_bg(text: impl Into<String>, caps: StyleCaps) -> Styled {
    Styled::new(text, StyleKind::CodeBg, caps)
}

/// Bold text (for emphasis, headers).
pub fn bold(text: impl Into<String>, caps: StyleCaps) -> Styled {
    Styled::new(text, StyleKind::Bold, caps)
}

// ---- Unicode-aware symbols ----

/// Return a check mark appropriate to the caps.
pub fn check_symbol(caps: StyleCaps) -> &'static str {
    if caps.unicode { "✓" } else { "OK" }
}

/// Return a cross mark appropriate to the caps.
pub fn cross_symbol(caps: StyleCaps) -> &'static str {
    if caps.unicode { "✗" } else { "X" }
}

/// Return a bullet point appropriate to the caps.
pub fn bullet(caps: StyleCaps) -> &'static str {
    if caps.unicode { "•" } else { "*" }
}

/// Horizontal line character for separators.
pub fn hline_char(caps: StyleCaps) -> &'static str {
    if caps.unicode { "─" } else { "-" }
}

/// Raw ANSI reset sequence.
///
/// This is the **only** place in the crate where a raw `\x1b[` escape
/// may appear. External callers (e.g. `render::code_block` when
/// piping syntect output) must use this helper so the T0.3 DoD
/// (`grep -v style.rs` returns empty) holds.
pub fn ansi_reset() -> &'static str {
    "\x1b[0m"
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    // ---- StyleCaps ----

    #[test]
    fn test_caps_full_has_colors_and_unicode() {
        let c = StyleCaps::full();
        assert!(c.colors);
        assert!(c.unicode);
    }

    #[test]
    fn test_caps_plain_disables_everything() {
        let c = StyleCaps::plain();
        assert!(!c.colors);
        assert!(!c.unicode);
    }

    #[test]
    fn test_caps_default_is_full() {
        assert_eq!(StyleCaps::default(), StyleCaps::full());
    }

    // ---- Color stripping ----

    #[test]
    fn test_success_with_colors_contains_ansi() {
        let s = success("ok", StyleCaps::full()).to_string();
        assert!(s.contains("ok"));
        assert!(s.contains("\x1b["), "expected ANSI in TTY mode");
    }

    #[test]
    fn test_success_without_colors_is_plain() {
        let s = success("ok", StyleCaps::plain()).to_string();
        assert_eq!(s, "ok");
    }

    #[test]
    fn test_error_without_colors_is_plain() {
        let s = error("boom", StyleCaps::plain()).to_string();
        assert_eq!(s, "boom");
    }

    #[test]
    fn test_warn_without_colors_is_plain() {
        let s = warn("careful", StyleCaps::plain()).to_string();
        assert_eq!(s, "careful");
    }

    #[test]
    fn test_dim_without_colors_is_plain() {
        let s = dim("muted", StyleCaps::plain()).to_string();
        assert_eq!(s, "muted");
    }

    #[test]
    fn test_accent_without_colors_is_plain() {
        let s = accent("theo>", StyleCaps::plain()).to_string();
        assert_eq!(s, "theo>");
    }

    #[test]
    fn test_tool_name_without_colors_is_plain() {
        let s = tool_name("Read", StyleCaps::plain()).to_string();
        assert_eq!(s, "Read");
    }

    #[test]
    fn test_code_bg_without_colors_is_plain() {
        let s = code_bg("code", StyleCaps::plain()).to_string();
        assert_eq!(s, "code");
    }

    #[test]
    fn test_bold_without_colors_is_plain() {
        let s = bold("hi", StyleCaps::plain()).to_string();
        assert_eq!(s, "hi");
    }

    #[test]
    fn test_all_styles_in_tty_contain_text() {
        let caps = StyleCaps::full();
        let checks = [
            success("a", caps).to_string(),
            error("b", caps).to_string(),
            warn("c", caps).to_string(),
            dim("d", caps).to_string(),
            accent("e", caps).to_string(),
            tool_name("f", caps).to_string(),
            code_bg("g", caps).to_string(),
            bold("h", caps).to_string(),
        ];
        for (i, s) in checks.iter().enumerate() {
            let expected = (b'a' + i as u8) as char;
            assert!(
                s.contains(expected),
                "style #{i} should contain {expected}, got {s:?}"
            );
        }
    }

    #[test]
    fn test_with_caps_overrides_colors() {
        let s = success("ok", StyleCaps::full())
            .with_caps(StyleCaps::plain())
            .to_string();
        assert_eq!(s, "ok");
    }

    #[test]
    fn test_raw_returns_unstyled_text() {
        let s = success("ok", StyleCaps::full());
        assert_eq!(s.raw(), "ok");
    }

    // ---- Symbols ----

    #[test]
    fn test_check_symbol_unicode() {
        assert_eq!(check_symbol(StyleCaps::full()), "✓");
    }

    #[test]
    fn test_check_symbol_ascii() {
        assert_eq!(check_symbol(StyleCaps::plain()), "OK");
    }

    #[test]
    fn test_cross_symbol_unicode() {
        assert_eq!(cross_symbol(StyleCaps::full()), "✗");
    }

    #[test]
    fn test_cross_symbol_ascii() {
        assert_eq!(cross_symbol(StyleCaps::plain()), "X");
    }

    #[test]
    fn test_bullet_unicode_vs_ascii() {
        assert_eq!(bullet(StyleCaps::full()), "•");
        assert_eq!(bullet(StyleCaps::plain()), "*");
    }

    #[test]
    fn test_hline_char_unicode_vs_ascii() {
        assert_eq!(hline_char(StyleCaps::full()), "─");
        assert_eq!(hline_char(StyleCaps::plain()), "-");
    }

    #[test]
    fn test_ansi_reset_is_reset_sequence() {
        assert_eq!(ansi_reset(), "\x1b[0m");
    }

    // ---- Idempotency with caps ----

    #[test]
    fn test_piped_output_is_idempotent() {
        let caps = StyleCaps::plain();
        let first = format!("{} {}", success("a", caps), error("b", caps));
        let second = format!("{} {}", success("a", caps), error("b", caps));
        assert_eq!(first, second);
        assert_eq!(first, "a b");
    }
}
