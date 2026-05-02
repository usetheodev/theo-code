//! Diff viewer — renders unified diff with colored additions/removals.
//!
//! Uses the `similar` crate for computing diffs.
//! Renders as ratatui Lines with green (+), red (-), and gray (context) styling.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use ratatui::prelude::*;
use similar::{ChangeTag, TextDiff};

/// Compute a unified diff between old and new text, returning styled Lines.
pub fn diff_to_lines(old: &str, new: &str, filename: &str) -> Vec<Line<'static>> {
    let diff = TextDiff::from_lines(old, new);
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Header
    lines.push(Line::from(Span::styled(
        format!("  --- a/{filename}"),
        Style::default().fg(Color::Red),
    )));
    lines.push(Line::from(Span::styled(
        format!("  +++ b/{filename}"),
        Style::default().fg(Color::Green),
    )));

    for group in diff.grouped_ops(3) {
        // Hunk header — `grouped_ops` returns non-empty groups by
        // contract (similar crate); the `first()`/`last()` Option chain
        // here is a defensive guard rather than a real panic surface.
        let (Some(first), Some(last)) = (group.first(), group.last()) else {
            continue;
        };
        let old_start = first.old_range().start + 1;
        let old_len = last.old_range().end - first.old_range().start;
        let new_start = first.new_range().start + 1;
        let new_len = last.new_range().end - first.new_range().start;
        lines.push(Line::from(Span::styled(
            format!("  @@ -{old_start},{old_len} +{new_start},{new_len} @@"),
            Style::default().fg(Color::Cyan),
        )));

        for op in &group {
            for change in diff.iter_changes(op) {
                let (prefix, style) = match change.tag() {
                    ChangeTag::Delete => ("-", Style::default().fg(Color::Red)),
                    ChangeTag::Insert => ("+", Style::default().fg(Color::Green)),
                    ChangeTag::Equal => (" ", Style::default().fg(Color::DarkGray)),
                };
                let text = change.to_string_lossy();
                let display = text.trim_end_matches('\n');
                lines.push(Line::from(Span::styled(
                    format!("  {prefix}{display}"),
                    style,
                )));
            }
        }
    }

    if lines.len() <= 2 {
        // No changes
        lines.push(Line::from(Span::styled(
            "  (no changes)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_shows_additions() {
        let old = "line1\nline2\n";
        let new = "line1\nline2\nline3\n";
        let lines = diff_to_lines(old, new, "test.rs");
        let has_addition = lines.iter().any(|l| {
            l.spans.iter().any(|s| s.content.contains("+line3"))
        });
        assert!(has_addition, "should show added line");
    }

    #[test]
    fn diff_shows_removals() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nline3\n";
        let lines = diff_to_lines(old, new, "test.rs");
        let has_removal = lines.iter().any(|l| {
            l.spans.iter().any(|s| s.content.contains("-line2"))
        });
        assert!(has_removal, "should show removed line");
    }

    #[test]
    fn diff_no_changes() {
        let text = "same\n";
        let lines = diff_to_lines(text, text, "test.rs");
        let has_no_changes = lines.iter().any(|l| {
            l.spans.iter().any(|s| s.content.contains("no changes"))
        });
        assert!(has_no_changes, "should show no changes");
    }

    #[test]
    fn diff_has_header() {
        let lines = diff_to_lines("a\n", "b\n", "foo.rs");
        let has_header = lines.iter().any(|l| {
            l.spans.iter().any(|s| s.content.contains("--- a/foo.rs"))
        });
        assert!(has_header, "should have diff header");
    }
}
