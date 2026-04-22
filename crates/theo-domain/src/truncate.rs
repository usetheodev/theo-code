use std::path::PathBuf;

pub const MAX_LINES: usize = 2000;
pub const MAX_BYTES: usize = 50 * 1024; // 50KB

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TruncateDirection {
    Head,
    Tail,
}

#[derive(Debug, Clone)]
pub struct TruncateOptions {
    pub max_lines: usize,
    pub max_bytes: usize,
    pub direction: TruncateDirection,
}

impl Default for TruncateOptions {
    fn default() -> Self {
        Self {
            max_lines: MAX_LINES,
            max_bytes: MAX_BYTES,
            direction: TruncateDirection::Head,
        }
    }
}

#[derive(Debug)]
pub struct TruncateResult {
    pub content: String,
    pub truncated: bool,
    pub output_path: Option<PathBuf>,
}

/// Truncate content that exceeds line or byte limits.
///
/// When truncated, full content is written to a temp file and
/// a reference is included in the returned content.
pub fn truncate_output(content: &str, options: Option<TruncateOptions>) -> TruncateResult {
    let opts = options.unwrap_or_default();

    let byte_len = content.len();
    let lines: Vec<&str> = content.split('\n').collect();
    let line_count = if content.ends_with('\n') {
        lines.len().saturating_sub(1)
    } else {
        lines.len()
    };

    // Check if truncation is needed
    if byte_len <= opts.max_bytes && line_count <= opts.max_lines {
        return TruncateResult {
            content: content.to_string(),
            truncated: false,
            output_path: None,
        };
    }

    // Write full content to file
    let output_path = write_truncated_file(content);

    // Truncate by lines
    if line_count > opts.max_lines {
        let truncated_lines = opts.max_lines;
        let omitted = line_count - truncated_lines;

        let selected: Vec<&str> = match opts.direction {
            TruncateDirection::Head => lines[..truncated_lines].to_vec(),
            TruncateDirection::Tail => {
                let start = line_count.saturating_sub(truncated_lines);
                lines[start..line_count].to_vec()
            }
        };

        let truncated_content = selected.join("\n");

        let message = format!(
            "{truncated_content}\n\n...{omitted} lines truncated...\n\n\
            The tool call succeeded but the output was truncated. \
            You can use Grep to search within the full output or use the \
            Read tool to read specific portions.",
        );

        return TruncateResult {
            content: message,
            truncated: true,
            output_path,
        };
    }

    // Truncate by bytes
    let truncated_bytes = opts.max_bytes;
    let omitted_bytes = byte_len - truncated_bytes;
    let truncated_content = &content[..truncated_bytes];

    let message = format!(
        "{truncated_content}\n\n...{omitted_bytes} bytes truncated...\n\n\
        The tool call succeeded but the output was truncated. \
        You can use Grep to search within the full output or use the \
        Read tool to read specific portions.",
    );

    TruncateResult {
        content: message,
        truncated: true,
        output_path,
    }
}

/// Suggest Task tool hint when agent has task permission
pub fn suggest_task_hint(content: &str, has_task_permission: bool) -> String {
    if has_task_permission {
        format!("{content}\nYou can also use the Task tool to process the full output.")
    } else {
        content.to_string()
    }
}

fn write_truncated_file(content: &str) -> Option<PathBuf> {
    let dir = truncation_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let filename = format!("tool_{}", ulid());
    let filepath = dir.join(filename);
    if std::fs::write(&filepath, content).is_ok() {
        Some(filepath)
    } else {
        None
    }
}

fn truncation_dir() -> PathBuf {
    let home = dirs_home().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".opencode").join("truncation")
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

fn ulid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{ts:x}_{:x}", rand_u32())
}

fn rand_u32() -> u32 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    RandomState::new().build_hasher().finish() as u32
}

/// Cleanup truncated files older than 7 days
pub fn cleanup_truncated_files() -> std::io::Result<usize> {
    let dir = truncation_dir();
    if !dir.exists() {
        return Ok(0);
    }

    let seven_days = std::time::Duration::from_secs(7 * 24 * 60 * 60);
    let cutoff = std::time::SystemTime::now() - seven_days;
    let mut removed = 0;

    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if let Ok(metadata) = entry.metadata()
            && let Ok(modified) = metadata.modified()
                && modified < cutoff
                    && std::fs::remove_file(entry.path()).is_ok() {
                        removed += 1;
                    }
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_content_unchanged_when_under_limits() {
        let content = "line1\nline2\nline3";
        let result = truncate_output(content, None);
        assert!(!result.truncated);
        assert_eq!(result.content, content);
        assert!(result.output_path.is_none());
    }

    #[test]
    fn truncates_by_line_count() {
        let lines: String = (0..100)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate_output(
            &lines,
            Some(TruncateOptions {
                max_lines: 10,
                max_bytes: MAX_BYTES,
                direction: TruncateDirection::Head,
            }),
        );
        assert!(result.truncated);
        assert!(result.content.contains("...90 lines truncated..."));
    }

    #[test]
    fn truncates_by_byte_count() {
        let content = "a".repeat(1000);
        let result = truncate_output(
            &content,
            Some(TruncateOptions {
                max_lines: MAX_LINES,
                max_bytes: 100,
                direction: TruncateDirection::Head,
            }),
        );
        assert!(result.truncated);
        assert!(result.content.contains("truncated..."));
    }

    #[test]
    fn truncates_from_head_by_default() {
        let lines: String = (0..10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate_output(
            &lines,
            Some(TruncateOptions {
                max_lines: 3,
                max_bytes: MAX_BYTES,
                direction: TruncateDirection::Head,
            }),
        );
        assert!(result.truncated);
        assert!(result.content.contains("line0"));
        assert!(result.content.contains("line1"));
        assert!(result.content.contains("line2"));
        assert!(!result.content.contains("\nline9\n"));
    }

    #[test]
    fn truncates_from_tail_when_direction_is_tail() {
        let lines: String = (0..10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate_output(
            &lines,
            Some(TruncateOptions {
                max_lines: 3,
                max_bytes: MAX_BYTES,
                direction: TruncateDirection::Tail,
            }),
        );
        assert!(result.truncated);
        assert!(result.content.contains("line7"));
        assert!(result.content.contains("line8"));
        assert!(result.content.contains("line9"));
        assert!(!result.content.contains("\nline0\n"));
    }

    #[test]
    fn default_max_lines_is_2000() {
        assert_eq!(MAX_LINES, 2000);
    }

    #[test]
    fn default_max_bytes_is_50kb() {
        assert_eq!(MAX_BYTES, 50 * 1024);
    }

    #[test]
    fn writes_full_output_to_file_when_truncated() {
        let lines: String = (0..100)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate_output(
            &lines,
            Some(TruncateOptions {
                max_lines: 10,
                max_bytes: MAX_BYTES,
                direction: TruncateDirection::Head,
            }),
        );
        assert!(result.truncated);
        assert!(
            result
                .content
                .contains("The tool call succeeded but the output was truncated")
        );
        assert!(result.content.contains("Grep"));

        if let Some(path) = &result.output_path {
            let written = std::fs::read_to_string(path).unwrap();
            assert_eq!(written, lines);
            // Cleanup
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn does_not_write_file_when_not_truncated() {
        let content = "short content";
        let result = truncate_output(content, None);
        assert!(!result.truncated);
        assert!(result.output_path.is_none());
    }

    #[test]
    fn suggest_task_hint_includes_task_tool_when_permitted() {
        let content = "truncated output";
        let result = suggest_task_hint(content, true);
        assert!(result.contains("Task tool"));
    }

    #[test]
    fn suggest_task_hint_omits_task_tool_when_not_permitted() {
        let content = "truncated output";
        let result = suggest_task_hint(content, false);
        assert!(!result.contains("Task tool"));
    }
}
