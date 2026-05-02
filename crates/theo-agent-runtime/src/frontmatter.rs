//! Markdown frontmatter splitter — shared between skills and agents.
//!
//! Splits a markdown file with `---`-delimited frontmatter into the
//! frontmatter block and the body. Pure parsing, no semantics.
//!
//! Used by:
//! - `subagent::parser` (YAML frontmatter → AgentSpec)
//! - `skill::parse_skill_file` (key-value frontmatter → SkillDefinition)
//!
//! Track A — frontmatter parser.

/// Split a markdown content string into `(frontmatter, body)` if it starts
/// with a `---`-delimited frontmatter block.
///
/// Returns `None` if:
/// - The content does not start with `---`
/// - The opening `---` is not followed by a closing `---`
///
/// The returned `frontmatter` excludes the delimiters and is NOT trimmed.
/// The returned `body` IS trimmed.
///
/// Examples:
/// ```
/// # use theo_agent_runtime::frontmatter::split_frontmatter;
/// let (fm, body) = split_frontmatter("---\nname: x\n---\nbody").unwrap();
/// assert!(fm.contains("name: x"));
/// assert_eq!(body, "body");
/// ```
pub fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }
    let after_first = &content[3..];
    // Skip leading newline after opening --- (typical YAML)
    let after_first = after_first.strip_prefix('\n').unwrap_or(after_first);
    // Find closing ---. Must be on its own line (preceded by newline) OR right after start.
    let end = find_closing_delimiter(after_first)?;
    let frontmatter = &after_first[..end];
    let rest = &after_first[end..];
    // Strip the closing --- and any trailing whitespace/newline before body
    let body_start = rest.strip_prefix("---").unwrap_or(rest);
    let body = body_start.trim();
    Some((frontmatter, body))
}

/// Find the position of a closing `---` delimiter that appears at the start
/// of a line (not inside the frontmatter content).
fn find_closing_delimiter(s: &str) -> Option<usize> {
    let mut pos = 0;
    while pos < s.len() {
        // Look for "---" preceded by start or newline
        if let Some(idx) = s[pos..].find("---") {
            let abs = pos + idx;
            let preceded_by_newline = abs == 0 || s.as_bytes().get(abs - 1) == Some(&b'\n');
            if preceded_by_newline {
                return Some(abs);
            }
            pos = abs + 3;
        } else {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_frontmatter_valid() {
        let content = "---\nname: test\ndescription: hi\n---\nThis is the body.";
        let (fm, body) = split_frontmatter(content).unwrap();
        assert!(fm.contains("name: test"));
        assert!(fm.contains("description: hi"));
        assert_eq!(body, "This is the body.");
    }

    #[test]
    fn split_frontmatter_no_delimiter_returns_none() {
        assert!(split_frontmatter("no frontmatter here").is_none());
        assert!(split_frontmatter("name: foo\nbody").is_none());
    }

    #[test]
    fn split_frontmatter_missing_closing_returns_none() {
        let content = "---\nname: test\nno closing delimiter";
        assert!(split_frontmatter(content).is_none());
    }

    #[test]
    fn split_frontmatter_empty_body_allowed() {
        let content = "---\nname: x\n---\n";
        let (_fm, body) = split_frontmatter(content).unwrap();
        assert_eq!(body, "");
    }

    #[test]
    fn split_frontmatter_handles_leading_whitespace() {
        let content = "\n\n---\nname: x\n---\nbody";
        let (_fm, body) = split_frontmatter(content).unwrap();
        assert_eq!(body, "body");
    }

    #[test]
    fn split_frontmatter_multiline_body() {
        let content = "---\nname: x\n---\nline1\nline2\nline3";
        let (_fm, body) = split_frontmatter(content).unwrap();
        assert_eq!(body, "line1\nline2\nline3");
    }

    #[test]
    fn split_frontmatter_yaml_with_arrays() {
        let content = "---\nname: x\ntools:\n  - read\n  - grep\n---\nbody";
        let (fm, _body) = split_frontmatter(content).unwrap();
        assert!(fm.contains("- read"));
        assert!(fm.contains("- grep"));
    }

    #[test]
    fn split_frontmatter_does_not_match_internal_dashes() {
        // --- inside a value (not at line start) should NOT be the delimiter
        let content = "---\nname: a-very---weird-name\ndescription: x\n---\nbody";
        let (fm, body) = split_frontmatter(content).unwrap();
        assert!(fm.contains("a-very---weird-name"));
        assert_eq!(body, "body");
    }
}
