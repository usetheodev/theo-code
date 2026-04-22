//! Syntax highlighter for the REPL prompt.
//!
//! Tokens:
//! - `/command` → accent color
//! - `@file`    → warn color
//! - `--flag`   → dim color
//! - everything else → plain

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use crate::render::style::{StyleCaps, accent, dim, warn};

/// Highlight a line of user input and return a styled string.
///
/// Tokens are split by whitespace; whitespace is preserved in the
/// output. Idempotent for the same `caps`.
pub fn highlight(line: &str, caps: StyleCaps) -> String {
    if line.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut prev_end = 0;
    for (start, end) in token_spans(line) {
        // Keep whitespace literal
        out.push_str(&line[prev_end..start]);
        let tok = &line[start..end];
        let styled = if tok.starts_with("/") {
            accent(tok, caps).to_string()
        } else if tok.starts_with("@") {
            warn(tok, caps).to_string()
        } else if tok.starts_with("--") {
            dim(tok, caps).to_string()
        } else {
            tok.to_string()
        };
        out.push_str(&styled);
        prev_end = end;
    }
    out.push_str(&line[prev_end..]);
    out
}

/// Return the `(start, end)` byte indices of every non-whitespace
/// token in `line`.
fn token_spans(line: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    let bytes = line.as_bytes();
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if start < i {
            out.push((start, i));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> StyleCaps {
        StyleCaps::plain()
    }

    // ---- highlight plain mode ----

    #[test]
    fn test_empty_input() {
        assert_eq!(highlight("", plain()), "");
    }

    #[test]
    fn test_plain_text_unchanged() {
        assert_eq!(highlight("hello world", plain()), "hello world");
    }

    #[test]
    fn test_slash_command_highlighted_in_plain_is_identity() {
        // Plain caps → no ANSI → output equals input.
        assert_eq!(highlight("/help", plain()), "/help");
    }

    #[test]
    fn test_at_mention_highlighted_in_plain_is_identity() {
        assert_eq!(highlight("@file.rs", plain()), "@file.rs");
    }

    #[test]
    fn test_flag_highlighted_in_plain_is_identity() {
        assert_eq!(highlight("--verbose", plain()), "--verbose");
    }

    #[test]
    fn test_mixed_tokens_preserve_whitespace() {
        let input = "/model   gpt-4  --temp 0.7";
        let out = highlight(input, plain());
        assert_eq!(out, input);
    }

    // ---- TTY mode ----

    #[test]
    fn test_slash_command_has_ansi_in_tty() {
        let out = highlight("/help", StyleCaps::full());
        assert!(out.contains("\x1b["));
        assert!(out.contains("/help"));
    }

    #[test]
    fn test_at_mention_has_ansi_in_tty() {
        let out = highlight("@src/main.rs", StyleCaps::full());
        assert!(out.contains("\x1b["));
        assert!(out.contains("@src/main.rs"));
    }

    #[test]
    fn test_flag_has_ansi_in_tty() {
        let out = highlight("--verbose", StyleCaps::full());
        assert!(out.contains("\x1b["));
    }

    // ---- token_spans ----

    #[test]
    fn test_token_spans_single_token() {
        assert_eq!(token_spans("hello"), vec![(0, 5)]);
    }

    #[test]
    fn test_token_spans_multiple_tokens() {
        assert_eq!(token_spans("a bb ccc"), vec![(0, 1), (2, 4), (5, 8)]);
    }

    #[test]
    fn test_token_spans_leading_whitespace() {
        assert_eq!(token_spans("  hi"), vec![(2, 4)]);
    }

    #[test]
    fn test_token_spans_trailing_whitespace() {
        assert_eq!(token_spans("hi  "), vec![(0, 2)]);
    }

    #[test]
    fn test_token_spans_empty() {
        assert!(token_spans("").is_empty());
    }

    #[test]
    fn test_token_spans_only_whitespace() {
        assert!(token_spans("   ").is_empty());
    }

    // ---- idempotency ----

    #[test]
    fn test_highlight_is_deterministic() {
        let input = "/cmd @f --flag rest";
        assert_eq!(
            highlight(input, plain()),
            highlight(input, plain())
        );
    }
}
