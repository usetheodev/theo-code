//! `@file` mention parsing.
//!
//! Extracts `@path/to/file` tokens from user input, reads them from
//! disk (bounded to 64KB by default per retrieval-engineer decision
//! in meeting 20260411-103954), and produces a contextual attachment
//! that the agent can consume alongside the user's prompt.

use std::path::{Path, PathBuf};

/// Max bytes per mention. Enforced at read time to avoid saturating
/// the LLM context window from a single `@file`.
pub const MAX_BYTES_PER_MENTION: usize = 64 * 1024;

/// Max number of mentions processed per turn. Anti-abuse.
pub const MAX_MENTIONS_PER_TURN: usize = 10;

/// A parsed mention with its raw token, resolved path, and byte count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    /// The token as it appeared in user input (with leading `@`).
    pub token: String,
    /// The path component (after stripping `@`).
    pub path: String,
}

/// Outcome of reading a mention from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MentionRead {
    Ok {
        path: PathBuf,
        content: String,
        bytes_read: usize,
        truncated: bool,
    },
    NotFound(PathBuf),
    Error(String),
}

/// Extract all `@path` mentions from a line of text.
///
/// Rules:
/// - A mention must be preceded by start-of-line or whitespace.
/// - The path ends at the first whitespace or closing bracket/quote.
/// - Mentions inside backticks are ignored (treated as literal).
/// - At most [`MAX_MENTIONS_PER_TURN`] mentions are returned.
pub fn extract(line: &str) -> Vec<Mention> {
    let mut out = Vec::new();
    let mut in_code = false;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'`' {
            in_code = !in_code;
            i += 1;
            continue;
        }
        if !in_code
            && c == b'@'
            && (i == 0 || bytes[i - 1].is_ascii_whitespace())
        {
            // Scan until whitespace or end
            let start = i;
            let mut end = i + 1;
            while end < bytes.len() {
                let b = bytes[end];
                if b.is_ascii_whitespace() || b == b')' || b == b'"' || b == b'\'' {
                    break;
                }
                end += 1;
            }
            let token = &line[start..end];
            if token.len() > 1 {
                out.push(Mention {
                    token: token.to_string(),
                    path: token[1..].to_string(),
                });
                if out.len() >= MAX_MENTIONS_PER_TURN {
                    break;
                }
            }
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// Read a mention from disk, enforcing size bounds.
pub fn read(mention: &Mention, root: &Path) -> MentionRead {
    let path = root.join(&mention.path);
    if !path.exists() {
        return MentionRead::NotFound(path);
    }
    match std::fs::read(&path) {
        Ok(bytes) => {
            let bytes_read = bytes.len();
            let truncated = bytes_read > MAX_BYTES_PER_MENTION;
            let end = bytes_read.min(MAX_BYTES_PER_MENTION);
            let slice = &bytes[..end];
            match std::str::from_utf8(slice) {
                Ok(s) => MentionRead::Ok {
                    path,
                    content: s.to_string(),
                    bytes_read,
                    truncated,
                },
                Err(_) => MentionRead::Error(format!(
                    "file {} is not valid UTF-8",
                    path.display()
                )),
            }
        }
        Err(e) => MentionRead::Error(format!("{e}: {}", path.display())),
    }
}

/// Render a mention's content as a context block to append to the
/// user's prompt. Used by the REPL when forwarding to the agent.
pub fn format_as_context(results: &[(Mention, MentionRead)]) -> String {
    let mut out = String::new();
    for (m, r) in results {
        match r {
            MentionRead::Ok {
                path,
                content,
                bytes_read,
                truncated,
            } => {
                out.push_str(&format!(
                    "\n--- attached: {} ({} bytes{}) ---\n",
                    path.display(),
                    bytes_read,
                    if *truncated { ", truncated" } else { "" }
                ));
                out.push_str(content);
                out.push('\n');
            }
            MentionRead::NotFound(p) => {
                out.push_str(&format!(
                    "\n--- {} not found for `{}` ---\n",
                    p.display(),
                    m.token
                ));
            }
            MentionRead::Error(e) => {
                out.push_str(&format!(
                    "\n--- error reading `{}`: {} ---\n",
                    m.token, e
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ---- extract ----

    #[test]
    fn test_extract_no_mentions_returns_empty() {
        assert!(extract("hello world").is_empty());
    }

    #[test]
    fn test_extract_single_mention() {
        let out = extract("please read @src/main.rs");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].token, "@src/main.rs");
        assert_eq!(out[0].path, "src/main.rs");
    }

    #[test]
    fn test_extract_multiple_mentions() {
        let out = extract("compare @a.rs with @b.rs please");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].path, "a.rs");
        assert_eq!(out[1].path, "b.rs");
    }

    #[test]
    fn test_extract_at_start_of_line() {
        let out = extract("@main.rs has bug");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, "main.rs");
    }

    #[test]
    fn test_extract_mid_word_at_is_not_mention() {
        // An @ in the middle of a word (e.g. email) is ignored.
        let out = extract("contact user@example.com");
        assert!(out.is_empty());
    }

    #[test]
    fn test_extract_inside_backticks_ignored() {
        let out = extract("use `@decorator` syntax");
        assert!(out.is_empty());
    }

    #[test]
    fn test_extract_max_mentions_cap() {
        let mut line = String::new();
        for i in 0..20 {
            line.push_str(&format!(" @file{i}.rs"));
        }
        let out = extract(&line);
        assert_eq!(out.len(), MAX_MENTIONS_PER_TURN);
    }

    #[test]
    fn test_extract_stops_at_closing_paren() {
        // Note: `(@foo.rs)` — the `(` before `@` is not whitespace, so
        // the current rule (must be preceded by whitespace or start)
        // does NOT recognize this as a mention. Document that behavior.
        let out = extract("See (@foo.rs) for details");
        assert!(out.is_empty());
    }

    #[test]
    fn test_extract_empty_at_is_ignored() {
        // Bare `@` with nothing after should not crash.
        let out = extract("just a @ sign");
        assert!(out.is_empty());
    }

    // ---- read ----

    #[test]
    fn test_read_existing_file_returns_ok() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("foo.txt"), "hello").unwrap();
        let m = Mention {
            token: "@foo.txt".to_string(),
            path: "foo.txt".to_string(),
        };
        match read(&m, dir.path()) {
            MentionRead::Ok {
                content,
                bytes_read,
                truncated,
                ..
            } => {
                assert_eq!(content, "hello");
                assert_eq!(bytes_read, 5);
                assert!(!truncated);
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn test_read_nonexistent_file_returns_not_found() {
        let dir = TempDir::new().unwrap();
        let m = Mention {
            token: "@missing".to_string(),
            path: "missing".to_string(),
        };
        assert!(matches!(read(&m, dir.path()), MentionRead::NotFound(_)));
    }

    #[test]
    fn test_read_truncates_large_file() {
        let dir = TempDir::new().unwrap();
        let big = "a".repeat(MAX_BYTES_PER_MENTION + 1000);
        fs::write(dir.path().join("big.txt"), &big).unwrap();
        let m = Mention {
            token: "@big.txt".to_string(),
            path: "big.txt".to_string(),
        };
        match read(&m, dir.path()) {
            MentionRead::Ok {
                content,
                bytes_read,
                truncated,
                ..
            } => {
                assert_eq!(bytes_read, MAX_BYTES_PER_MENTION + 1000);
                assert!(truncated);
                assert_eq!(content.len(), MAX_BYTES_PER_MENTION);
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn test_read_binary_file_returns_error() {
        let dir = TempDir::new().unwrap();
        let non_utf8 = vec![0xffu8, 0xfe, 0xfd];
        fs::write(dir.path().join("bin"), &non_utf8).unwrap();
        let m = Mention {
            token: "@bin".to_string(),
            path: "bin".to_string(),
        };
        assert!(matches!(read(&m, dir.path()), MentionRead::Error(_)));
    }

    // ---- format_as_context ----

    #[test]
    fn test_format_ok_has_content() {
        let m = Mention {
            token: "@x".to_string(),
            path: "x".to_string(),
        };
        let r = MentionRead::Ok {
            path: PathBuf::from("/tmp/x"),
            content: "body".to_string(),
            bytes_read: 4,
            truncated: false,
        };
        let out = format_as_context(&[(m, r)]);
        assert!(out.contains("attached"));
        assert!(out.contains("4 bytes"));
        assert!(out.contains("body"));
    }

    #[test]
    fn test_format_truncated_marker() {
        let m = Mention {
            token: "@x".to_string(),
            path: "x".to_string(),
        };
        let r = MentionRead::Ok {
            path: PathBuf::from("/tmp/x"),
            content: "body".to_string(),
            bytes_read: MAX_BYTES_PER_MENTION + 1,
            truncated: true,
        };
        let out = format_as_context(&[(m, r)]);
        assert!(out.contains("truncated"));
    }

    #[test]
    fn test_format_not_found_marker() {
        let m = Mention {
            token: "@missing".to_string(),
            path: "missing".to_string(),
        };
        let r = MentionRead::NotFound(PathBuf::from("/tmp/missing"));
        let out = format_as_context(&[(m, r)]);
        assert!(out.contains("not found"));
    }

    #[test]
    fn test_format_error_marker() {
        let m = Mention {
            token: "@broken".to_string(),
            path: "broken".to_string(),
        };
        let r = MentionRead::Error("oops".to_string());
        let out = format_as_context(&[(m, r)]);
        assert!(out.contains("error reading"));
        assert!(out.contains("oops"));
    }
}
