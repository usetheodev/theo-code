//! Phase 52 (prompt-ab-testing-plan) — load `system_prompt` from a file
//! pointed to by `THEO_SYSTEM_PROMPT_FILE`, when set.
//!
//! Used by the A/B testing infrastructure to compare prompt variants without
//! rebuilding the binary. When the env var is unset, empty, or the file is
//! unreadable, the helpers return `None` and the caller keeps the existing
//! `default_system_prompt()` behavior — zero-impact for the normal path.

use std::path::Path;

/// Read a prompt file from `path`. Returns `None` when the path is empty or
/// the file cannot be read; warning messages are emitted to stderr only when
/// the path is non-empty (so the unset case stays silent).
pub(crate) fn read_prompt_file(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    match std::fs::read_to_string(Path::new(path)) {
        Ok(contents) => {
            eprintln!("[theo] using prompt from {}", path);
            Some(contents)
        }
        Err(err) => {
            eprintln!(
                "[theo] WARN: THEO_SYSTEM_PROMPT_FILE={} unreadable: {}; \
                 falling back to default",
                path, err
            );
            None
        }
    }
}

/// Resolve the override using the value of `THEO_SYSTEM_PROMPT_FILE` from the
/// process environment. Centralises the env-var lookup so callers stay tidy.
pub(crate) fn override_from_env() -> Option<String> {
    let path = std::env::var("THEO_SYSTEM_PROMPT_FILE").ok()?;
    read_prompt_file(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn prompt_file_env_no_op_when_unset() {
        // Arrange: ensure no path
        // Act
        let result = read_prompt_file("");
        // Assert
        assert!(result.is_none(), "empty path must be a no-op");
    }

    #[test]
    fn prompt_file_env_falls_back_when_empty() {
        // Empty string treated identical to unset — caller passes the env value
        let result = read_prompt_file("");
        assert!(result.is_none());
    }

    #[test]
    fn prompt_file_env_falls_back_when_unreadable() {
        // Arrange: path that does not exist
        let bogus = "/tmp/theo-prompt-override-does-not-exist-xyz-9z1.md";
        // Act
        let result = read_prompt_file(bogus);
        // Assert
        assert!(result.is_none(), "unreadable path must fall back");
    }

    #[test]
    fn prompt_file_env_overrides_default_when_set() {
        // Arrange: write a temp file with known content
        let mut f = tempfile::NamedTempFile::new().expect("temp file");
        write!(f, "Custom prompt for testing").unwrap();
        let path = f.path().to_string_lossy().to_string();
        // Act
        let result = read_prompt_file(&path);
        // Assert
        assert_eq!(result.as_deref(), Some("Custom prompt for testing"));
    }

    #[test]
    fn prompt_file_loaded_content_is_used_verbatim_no_processing() {
        // Arrange: content with multiple lines, special chars, trailing newline
        let payload = "line one\n\n## Heading\n- bullet `code`\n\u{00e1} \u{00e9}\n";
        let mut f = tempfile::NamedTempFile::new().expect("temp file");
        f.write_all(payload.as_bytes()).unwrap();
        let path = f.path().to_string_lossy().to_string();
        // Act
        let result = read_prompt_file(&path).expect("must load");
        // Assert: byte-for-byte
        assert_eq!(result, payload, "loader must not mutate file content");
    }
}
