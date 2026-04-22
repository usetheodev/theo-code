//! Edit/patch diff rendering with optional syntax highlighting.
//!
//! Given a list of [`DiffLine`]s (parsed from Edit or apply_patch
//! events), produce terminal-formatted output: `+` in green, `-` in
//! red, context dim.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use crate::render::code_block;
use crate::render::style::{StyleCaps, bold, dim, error, success};

/// Classification of a line inside a diff hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    /// `+` new/added line
    Added(String),
    /// `-` removed line
    Removed(String),
    /// Unchanged context line
    Context(String),
    /// `@@ -a,b +c,d @@` hunk header
    Hunk(String),
}

/// Render a single diff line.
pub fn render_line(line: &DiffLine, caps: StyleCaps) -> String {
    match line {
        DiffLine::Added(s) => {
            let body = format!("+ {s}");
            success(body, caps).to_string()
        }
        DiffLine::Removed(s) => {
            let body = format!("- {s}");
            error(body, caps).to_string()
        }
        DiffLine::Context(s) => {
            let body = format!("  {s}");
            dim(body, caps).to_string()
        }
        DiffLine::Hunk(s) => bold(s.clone(), caps).to_string(),
    }
}

/// Render a sequence of diff lines as a block, joining with newlines.
pub fn render_lines(lines: &[DiffLine], caps: StyleCaps) -> String {
    lines
        .iter()
        .map(|l| render_line(l, caps))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse a unified-diff patch string into hunks of [`DiffLine`].
///
/// Only classifies lines — it does not validate the patch structure.
pub fn parse_unified(patch: &str) -> Vec<DiffLine> {
    patch
        .lines()
        .filter_map(|line| {
            if line.starts_with("@@") {
                Some(DiffLine::Hunk(line.to_string()))
            } else if let Some(rest) = line.strip_prefix('+') {
                // Filter out `+++ b/path` headers (two+ leading `+`).
                if rest.starts_with("++") {
                    None
                } else {
                    Some(DiffLine::Added(rest.to_string()))
                }
            } else if let Some(rest) = line.strip_prefix('-') {
                if rest.starts_with("--") {
                    None
                } else {
                    Some(DiffLine::Removed(rest.to_string()))
                }
            } else {
                line.strip_prefix(' ')
                    .map(|rest| DiffLine::Context(rest.to_string()))
                // Other metadata (diff --git, index, etc.) → None.
            }
        })
        .collect()
}

/// Render an Edit operation as a two-line diff (old → new).
///
/// Used when the tool event provides `oldString` / `newString` rather
/// than a unified patch.
pub fn render_edit_pair(old: &str, new: &str, caps: StyleCaps) -> String {
    let old_lines: Vec<DiffLine> = old
        .lines()
        .map(|l| DiffLine::Removed(l.to_string()))
        .collect();
    let new_lines: Vec<DiffLine> = new
        .lines()
        .map(|l| DiffLine::Added(l.to_string()))
        .collect();
    let mut all = old_lines;
    all.extend(new_lines);
    render_lines(&all, caps)
}

/// Render a diff with syntax highlighting applied to + and - line
/// bodies. `lang` is the file extension or language token.
///
/// In plain caps this collapses to [`render_lines`].
pub fn render_lines_with_syntax(lines: &[DiffLine], lang: &str, caps: StyleCaps) -> String {
    if !caps.colors {
        return render_lines(lines, caps);
    }
    lines
        .iter()
        .map(|l| match l {
            DiffLine::Added(s) => {
                let highlighted = code_block::highlight(s, lang, caps);
                success(format!("+ {highlighted}"), caps).to_string()
            }
            DiffLine::Removed(s) => {
                let highlighted = code_block::highlight(s, lang, caps);
                error(format!("- {highlighted}"), caps).to_string()
            }
            other => render_line(other, caps),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> StyleCaps {
        StyleCaps::plain()
    }

    // ---- render_line ----

    #[test]
    fn test_render_added_plain() {
        let out = render_line(&DiffLine::Added("new".to_string()), plain());
        assert_eq!(out, "+ new");
    }

    #[test]
    fn test_render_removed_plain() {
        let out = render_line(&DiffLine::Removed("old".to_string()), plain());
        assert_eq!(out, "- old");
    }

    #[test]
    fn test_render_context_plain() {
        let out = render_line(&DiffLine::Context("unchanged".to_string()), plain());
        assert_eq!(out, "  unchanged");
    }

    #[test]
    fn test_render_hunk_plain() {
        let out = render_line(&DiffLine::Hunk("@@ -1,3 +1,4 @@".to_string()), plain());
        assert_eq!(out, "@@ -1,3 +1,4 @@");
    }

    #[test]
    fn test_render_added_tty_has_ansi() {
        let out = render_line(&DiffLine::Added("x".to_string()), StyleCaps::full());
        assert!(out.contains("\x1b["));
        assert!(out.contains("+"));
    }

    // ---- render_lines ----

    #[test]
    fn test_render_lines_joins_with_newline() {
        let lines = vec![
            DiffLine::Added("new".to_string()),
            DiffLine::Removed("old".to_string()),
        ];
        let out = render_lines(&lines, plain());
        assert_eq!(out, "+ new\n- old");
    }

    #[test]
    fn test_render_lines_empty_returns_empty() {
        let out = render_lines(&[], plain());
        assert_eq!(out, "");
    }

    // ---- parse_unified ----

    #[test]
    fn test_parse_unified_extracts_hunks_and_changes() {
        let patch = "\
--- a/foo.txt
+++ b/foo.txt
@@ -1,3 +1,3 @@
 context
-old line
+new line
 more context";
        let parsed = parse_unified(patch);
        assert_eq!(parsed.len(), 5);
        assert!(matches!(parsed[0], DiffLine::Hunk(_)));
        assert!(matches!(parsed[1], DiffLine::Context(_)));
        assert!(matches!(parsed[2], DiffLine::Removed(_)));
        assert!(matches!(parsed[3], DiffLine::Added(_)));
        assert!(matches!(parsed[4], DiffLine::Context(_)));
    }

    #[test]
    fn test_parse_unified_skips_file_headers() {
        let patch = "--- a/x\n+++ b/x\n+added\n-removed";
        let parsed = parse_unified(patch);
        // +++ and --- are header lines, not changes
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], DiffLine::Added("added".to_string()));
        assert_eq!(parsed[1], DiffLine::Removed("removed".to_string()));
    }

    #[test]
    fn test_parse_unified_empty_returns_empty() {
        assert!(parse_unified("").is_empty());
    }

    #[test]
    fn test_parse_unified_only_context() {
        let patch = " line one\n line two";
        let parsed = parse_unified(patch);
        assert_eq!(parsed.len(), 2);
        for p in parsed {
            assert!(matches!(p, DiffLine::Context(_)));
        }
    }

    // ---- render_edit_pair ----

    #[test]
    fn test_render_edit_pair_single_line() {
        let out = render_edit_pair("foo", "bar", plain());
        assert_eq!(out, "- foo\n+ bar");
    }

    #[test]
    fn test_render_edit_pair_multi_line() {
        let out = render_edit_pair("a\nb", "c\nd", plain());
        assert_eq!(out, "- a\n- b\n+ c\n+ d");
    }

    #[test]
    fn test_render_edit_pair_empty_old() {
        let out = render_edit_pair("", "added", plain());
        assert_eq!(out, "+ added");
    }

    #[test]
    fn test_render_edit_pair_empty_new() {
        let out = render_edit_pair("removed", "", plain());
        assert_eq!(out, "- removed");
    }

    // ---- render_lines_with_syntax ----

    #[test]
    fn test_render_lines_with_syntax_plain_falls_back() {
        let lines = vec![DiffLine::Added("fn main() {}".to_string())];
        let out = render_lines_with_syntax(&lines, "rust", plain());
        assert_eq!(out, "+ fn main() {}");
    }

    #[test]
    fn test_render_lines_with_syntax_tty_contains_ansi() {
        let lines = vec![DiffLine::Added("fn main() {}".to_string())];
        let out = render_lines_with_syntax(&lines, "rust", StyleCaps::full());
        assert!(out.contains("\x1b["));
        assert!(out.contains("main"));
    }

    #[test]
    fn test_render_lines_with_syntax_preserves_context_lines() {
        let lines = vec![
            DiffLine::Context("unchanged".to_string()),
            DiffLine::Added("new".to_string()),
        ];
        let out = render_lines_with_syntax(&lines, "rust", plain());
        assert!(out.contains("unchanged"));
        assert!(out.contains("new"));
    }
}
