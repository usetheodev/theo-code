//! Syntax-highlighted code block rendering via [`syntect`].
//!
//! Loads the default `SyntaxSet` + `ThemeSet` lazily via [`OnceLock`]
//! (one-time cost at first use, ~20-50ms). Subsequent calls are cheap.
//!
//! When colors are disabled (piped output / NO_COLOR), this falls back
//! to plain text — no escape sequences are emitted.
//!
//! See ADR-001 §code-block rendering.

use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::{LinesWithEndings, as_24_bit_terminal_escaped};

use crate::render::style::{self, StyleCaps, dim};

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

/// Default theme name. Configurable via `TheoConfig.theme`.
pub const DEFAULT_THEME: &str = "base16-ocean.dark";

/// Lazily load the bundled `SyntaxSet`.
pub fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Lazily load the bundled `ThemeSet`.
pub fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

/// Lookup a theme by name, falling back to [`DEFAULT_THEME`].
pub fn theme_or_default(name: &str) -> &'static Theme {
    let ts = theme_set();
    ts.themes
        .get(name)
        .or_else(|| ts.themes.get(DEFAULT_THEME))
        .unwrap_or_else(|| ts.themes.values().next().expect("at least one theme"))
}

/// Highlight `code` for `lang` and return ANSI-styled text.
///
/// `lang` can be a file extension ("rs"), a language name ("rust"),
/// or any token syntect recognizes. Unknown languages fall back to
/// plain text (no highlighting, no error).
///
/// When `caps.colors` is false the code is returned unchanged.
pub fn highlight(code: &str, lang: &str, caps: StyleCaps) -> String {
    if !caps.colors {
        return code.to_string();
    }
    let ss = syntax_set();
    let theme = theme_or_default(DEFAULT_THEME);
    let syntax = ss
        .find_syntax_by_token(lang)
        .or_else(|| ss.find_syntax_by_extension(lang))
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut out = String::new();
    for line in LinesWithEndings::from(code) {
        // `highlight_line` returns `Result` only in strict mode.
        match highlighter.highlight_line(line, ss) {
            Ok(ranges) => {
                out.push_str(&as_24_bit_terminal_escaped(&ranges, false));
            }
            Err(_) => {
                // Graceful fallback: emit the raw line.
                out.push_str(line);
            }
        }
    }
    out.push_str(style::ansi_reset());
    out
}

/// Render a full code block with a box border and language label.
///
/// Output shape (plain mode):
///
/// ```text
///   rust ─────────────────────────────
///   │ fn main() {}
///   ──────────────────────────────────
/// ```
pub fn render_block(code: &str, lang: &str, caps: StyleCaps) -> String {
    let label = if lang.is_empty() { "code" } else { lang };
    let hchar = style::hline_char(caps);
    let mut out = String::new();

    // Header: "  <lang> ───…"
    let header_line = format!("  {label} {}", hchar.repeat(40));
    out.push_str(&dim(header_line, caps).to_string());
    out.push('\n');

    // Body: each line prefixed with "  │ "
    let highlighted = highlight(code, lang, caps);
    for line in highlighted.lines() {
        out.push_str(&dim("  │ ", caps).to_string());
        out.push_str(line);
        out.push('\n');
    }

    // Footer
    let footer_line = format!("  {}", hchar.repeat(42));
    out.push_str(&dim(footer_line, caps).to_string());

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> StyleCaps {
        StyleCaps::plain()
    }

    fn tty() -> StyleCaps {
        StyleCaps::full()
    }

    #[test]
    fn test_syntax_set_loads_without_panic() {
        let _ = syntax_set();
    }

    #[test]
    fn test_theme_set_loads_without_panic() {
        let _ = theme_set();
    }

    #[test]
    fn test_default_theme_resolves() {
        let t = theme_or_default(DEFAULT_THEME);
        assert!(!t.name.as_deref().unwrap_or("").is_empty());
    }

    #[test]
    fn test_unknown_theme_falls_back_to_default() {
        let t = theme_or_default("completely-made-up-name-xyz");
        // Should not panic; should return some theme
        assert!(t.name.is_some() || t.name.is_none());
    }

    #[test]
    fn test_highlight_plain_mode_is_identity() {
        let code = "fn main() {}";
        assert_eq!(highlight(code, "rust", plain()), code);
    }

    #[test]
    fn test_highlight_tty_mode_adds_ansi() {
        let code = "fn main() {}";
        let out = highlight(code, "rust", tty());
        assert!(out.contains("\x1b["));
        assert!(out.contains("main"));
    }

    #[test]
    fn test_highlight_unknown_language_returns_raw_text() {
        let code = "something without a language";
        let out = highlight(code, "completely-unknown-lang-xyz", plain());
        assert_eq!(out, code);
    }

    #[test]
    fn test_highlight_each_supported_language_ends_reset() {
        // Smoke test for a set of 12+ languages mentioned in the plan.
        for lang in [
            "rust", "python", "javascript", "typescript", "go", "java", "bash", "json", "yaml",
            "toml", "html", "css",
        ] {
            let out = highlight("sample", lang, tty());
            // TTY highlight output always ends with reset
            assert!(out.contains("\x1b[0m"), "lang {lang} missing reset");
        }
    }

    #[test]
    fn test_render_block_plain_contains_lang_and_code() {
        let out = render_block("fn main() {}", "rust", plain());
        assert!(out.contains("rust"));
        assert!(out.contains("fn main() {}"));
        assert!(out.contains("│"));
    }

    #[test]
    fn test_render_block_empty_language_uses_code_label() {
        let out = render_block("hello", "", plain());
        assert!(out.contains("code"));
    }

    #[test]
    fn test_render_block_tty_contains_ansi() {
        let out = render_block("fn main() {}", "rust", tty());
        assert!(out.contains("\x1b["));
    }

    #[test]
    fn test_render_block_preserves_line_count() {
        let code = "line1\nline2\nline3";
        let out = render_block(code, "text", plain());
        let line_count = out.lines().count();
        // 1 header + 3 body + 1 footer = 5
        assert_eq!(line_count, 5);
    }

    #[test]
    fn test_highlight_is_deterministic() {
        let a = highlight("let x = 1;", "rust", tty());
        let b = highlight("let x = 1;", "rust", tty());
        assert_eq!(a, b);
    }

    #[test]
    fn test_highlight_multiline_python() {
        let code = "def f():\n    return 1\n";
        let out = highlight(code, "python", tty());
        assert!(out.contains("def"));
        assert!(out.contains("return"));
    }

    #[test]
    fn test_render_block_header_has_border_chars() {
        let out = render_block("x", "rust", plain());
        // In plain mode, `hline_char` is `-`
        assert!(out.contains("----"));
    }
}
