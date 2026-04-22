//! Tab completion for slash commands and @file mentions.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
/// Complete a token that may be a slash command prefix.
///
/// Given a line and cursor position, return the list of candidate
/// completions that start with the current token. If the current
/// token does not begin with `/`, returns an empty vec.
pub fn complete_slash(line: &str, pos: usize, commands: &[String]) -> Vec<String> {
    let (start, prefix) = current_token(line, pos);
    if !prefix.starts_with('/') {
        return Vec::new();
    }
    let _ = start; // currently unused; keep signature for future caller
    commands
        .iter()
        .filter(|c| c.starts_with(prefix))
        .cloned()
        .collect()
}

/// Complete a token starting with `@` as a file path relative to `root`.
pub fn complete_mention(line: &str, pos: usize, root: &std::path::Path) -> Vec<String> {
    let (_, token) = current_token(line, pos);
    if !token.starts_with('@') {
        return Vec::new();
    }
    let partial = &token[1..];
    let (dir, name_prefix) = match partial.rfind('/') {
        Some(i) => (&partial[..i], &partial[i + 1..]),
        None => ("", partial),
    };
    let search_dir = if dir.is_empty() {
        root.to_path_buf()
    } else {
        root.join(dir)
    };
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&search_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && !name_prefix.starts_with('.') {
                continue;
            }
            if name.starts_with(name_prefix) {
                let full = if dir.is_empty() {
                    format!("@{name}")
                } else {
                    format!("@{dir}/{name}")
                };
                // Append a trailing / to directories so the user can
                // tab again and drill down.
                if entry.path().is_dir() {
                    out.push(format!("{full}/"));
                } else {
                    out.push(full);
                }
            }
        }
    }
    out.sort();
    out
}

/// Find the token that ends at `pos`. Returns `(start_index, &str)`.
fn current_token(line: &str, pos: usize) -> (usize, &str) {
    let safe_pos = pos.min(line.len());
    let before = &line[..safe_pos];
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, _)| i + 1)
        .unwrap_or(0);
    (start, &line[start..safe_pos])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn sample_commands() -> Vec<String> {
        vec![
            "/clear".to_string(),
            "/cost".to_string(),
            "/doctor".to_string(),
            "/help".to_string(),
            "/memory".to_string(),
            "/mode".to_string(),
            "/model".to_string(),
            "/status".to_string(),
            "/skills".to_string(),
        ]
    }

    // ---- complete_slash ----

    #[test]
    fn test_slash_complete_with_prefix_s_returns_status_and_skills() {
        let out = complete_slash("/s", 2, &sample_commands());
        assert!(out.contains(&"/status".to_string()));
        assert!(out.contains(&"/skills".to_string()));
    }

    #[test]
    fn test_slash_complete_with_full_match_returns_unique() {
        let out = complete_slash("/clear", 6, &sample_commands());
        assert_eq!(out, vec!["/clear".to_string()]);
    }

    #[test]
    fn test_slash_complete_no_match_returns_empty() {
        let out = complete_slash("/xyz", 4, &sample_commands());
        assert!(out.is_empty());
    }

    #[test]
    fn test_slash_complete_non_slash_returns_empty() {
        let out = complete_slash("hello", 5, &sample_commands());
        assert!(out.is_empty());
    }

    #[test]
    fn test_slash_complete_empty_slash_returns_all() {
        let out = complete_slash("/", 1, &sample_commands());
        assert_eq!(out.len(), sample_commands().len());
    }

    #[test]
    fn test_slash_complete_after_space_returns_empty() {
        // Cursor after a space — token is empty (not starting with /)
        let out = complete_slash("/help ", 6, &sample_commands());
        assert!(out.is_empty());
    }

    #[test]
    fn test_slash_complete_in_middle_of_word() {
        let out = complete_slash("/m", 2, &sample_commands());
        assert!(out.contains(&"/memory".to_string()));
        assert!(out.contains(&"/mode".to_string()));
        assert!(out.contains(&"/model".to_string()));
    }

    // ---- complete_mention ----

    #[test]
    fn test_mention_complete_lists_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("foo.rs"), "").unwrap();
        fs::write(dir.path().join("bar.rs"), "").unwrap();
        let out = complete_mention("@", 1, dir.path());
        assert!(out.contains(&"@bar.rs".to_string()));
        assert!(out.contains(&"@foo.rs".to_string()));
    }

    #[test]
    fn test_mention_complete_prefix_filters() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("foo.rs"), "").unwrap();
        fs::write(dir.path().join("bar.rs"), "").unwrap();
        let out = complete_mention("@f", 2, dir.path());
        assert_eq!(out, vec!["@foo.rs".to_string()]);
    }

    #[test]
    fn test_mention_complete_subdir_appends_slash() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        let out = complete_mention("@s", 2, dir.path());
        assert_eq!(out, vec!["@src/".to_string()]);
    }

    #[test]
    fn test_mention_complete_inside_subdir() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("main.rs"), "").unwrap();
        let out = complete_mention("@src/", 5, dir.path());
        assert_eq!(out, vec!["@src/main.rs".to_string()]);
    }

    #[test]
    fn test_mention_complete_skips_hidden_files_when_no_dot_prefix() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".hidden"), "").unwrap();
        fs::write(dir.path().join("visible"), "").unwrap();
        let out = complete_mention("@", 1, dir.path());
        assert!(!out.contains(&"@.hidden".to_string()));
        assert!(out.contains(&"@visible".to_string()));
    }

    #[test]
    fn test_mention_complete_includes_hidden_when_dot_prefix() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".env"), "").unwrap();
        let out = complete_mention("@.", 2, dir.path());
        assert_eq!(out, vec!["@.env".to_string()]);
    }

    #[test]
    fn test_mention_complete_non_at_returns_empty() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("foo"), "").unwrap();
        let out = complete_mention("foo", 3, dir.path());
        assert!(out.is_empty());
    }

    // ---- current_token ----

    #[test]
    fn test_current_token_at_start() {
        let (start, tok) = current_token("/help", 5);
        assert_eq!(start, 0);
        assert_eq!(tok, "/help");
    }

    #[test]
    fn test_current_token_after_space() {
        let (start, tok) = current_token("hello /hel", 10);
        assert_eq!(start, 6);
        assert_eq!(tok, "/hel");
    }

    #[test]
    fn test_current_token_pos_larger_than_line() {
        let (start, tok) = current_token("abc", 99);
        assert_eq!(start, 0);
        assert_eq!(tok, "abc");
    }
}
