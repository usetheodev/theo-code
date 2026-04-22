//! TTY capability detection.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use crate::render::style::StyleCaps;

/// Full terminal capabilities detected at startup.
///
/// Combines TTY detection, NO_COLOR respect, and terminal width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtyCaps {
    /// Output target is a real terminal (not piped).
    pub is_tty: bool,
    /// ANSI color is enabled (respects NO_COLOR env var).
    pub colors: bool,
    /// Unicode box-drawing characters are safe to emit.
    pub unicode: bool,
    /// Current terminal width in columns.
    pub width: u16,
}

impl TtyCaps {
    /// Construct from explicit parameters (test helper / override).
    pub const fn new(is_tty: bool, colors: bool, unicode: bool, width: u16) -> Self {
        Self {
            is_tty,
            colors,
            unicode,
            width,
        }
    }

    /// Detect capabilities from the environment and stderr.
    ///
    /// Rules:
    /// - `colors = is_tty && !NO_COLOR`
    /// - `unicode = is_tty` (assume modern terminals handle it)
    /// - `width = crossterm::terminal::size()` or 80 fallback
    pub fn detect() -> Self {
        let is_tty = console::Term::stderr().features().is_attended();
        let no_color = std::env::var_os("NO_COLOR").is_some();
        let colors = is_tty && !no_color;
        let width = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80);
        Self {
            is_tty,
            colors,
            unicode: is_tty,
            width,
        }
    }

    /// Detect using explicit env var lookups (testable version).
    ///
    /// `get_env` is a closure that returns `Some(value)` for a present
    /// env var. This lets tests simulate NO_COLOR without polluting the
    /// process environment.
    pub fn detect_with<F>(is_tty: bool, width: u16, get_env: F) -> Self
    where
        F: Fn(&str) -> Option<String>,
    {
        let no_color = get_env("NO_COLOR").is_some();
        let colors = is_tty && !no_color;
        Self {
            is_tty,
            colors,
            unicode: is_tty,
            width,
        }
    }

    /// Plain caps (no tty, no colors, 80 cols).
    pub const fn plain() -> Self {
        Self::new(false, false, false, 80)
    }

    /// Downgrade to [`StyleCaps`] for the `render::style` module.
    pub fn style_caps(&self) -> StyleCaps {
        StyleCaps {
            colors: self.colors,
            unicode: self.unicode,
        }
    }
}

impl Default for TtyCaps {
    fn default() -> Self {
        Self::plain()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_has_no_features() {
        let c = TtyCaps::plain();
        assert!(!c.is_tty);
        assert!(!c.colors);
        assert!(!c.unicode);
        assert_eq!(c.width, 80);
    }

    #[test]
    fn test_detect_with_tty_and_no_env_enables_colors() {
        let c = TtyCaps::detect_with(true, 120, |_| None);
        assert!(c.is_tty);
        assert!(c.colors);
        assert!(c.unicode);
        assert_eq!(c.width, 120);
    }

    #[test]
    fn test_detect_with_no_color_env_disables_colors() {
        let c = TtyCaps::detect_with(true, 120, |k| {
            if k == "NO_COLOR" {
                Some("1".into())
            } else {
                None
            }
        });
        assert!(c.is_tty);
        assert!(!c.colors, "NO_COLOR must disable colors even in TTY");
        // Unicode still follows TTY (not gated by NO_COLOR).
        assert!(c.unicode);
    }

    #[test]
    fn test_detect_with_no_tty_disables_everything() {
        let c = TtyCaps::detect_with(false, 80, |_| None);
        assert!(!c.is_tty);
        assert!(!c.colors);
        assert!(!c.unicode);
    }

    #[test]
    fn test_detect_with_no_tty_ignores_no_color() {
        // Even without NO_COLOR, no TTY means no colors.
        let c = TtyCaps::detect_with(false, 80, |_| None);
        assert!(!c.colors);
    }

    #[test]
    fn test_style_caps_matches_tty_caps() {
        let t = TtyCaps::new(true, true, true, 100);
        let s = t.style_caps();
        assert!(s.colors);
        assert!(s.unicode);
    }

    #[test]
    fn test_style_caps_plain_is_plain() {
        let t = TtyCaps::plain();
        let s = t.style_caps();
        assert!(!s.colors);
        assert!(!s.unicode);
    }

    #[test]
    fn test_default_is_plain() {
        assert_eq!(TtyCaps::default(), TtyCaps::plain());
    }

    #[test]
    fn test_detect_does_not_panic() {
        // Live detection shouldn't panic regardless of environment.
        let _ = TtyCaps::detect();
    }

    #[test]
    fn test_new_preserves_fields() {
        let c = TtyCaps::new(true, false, true, 200);
        assert!(c.is_tty);
        assert!(!c.colors);
        assert!(c.unicode);
        assert_eq!(c.width, 200);
    }
}
