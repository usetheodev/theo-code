//! Roadmap parser — reads `.theo/plans/*.md` and extracts executable microtasks.
//!
//! The roadmap file is the persistent source of truth for Plan mode.
//! Tasks are parsed from `### Task N: title` sections with structured fields.
//! After each task is executed, the header is marked with ✅.

use std::path::Path;

// ---------------------------------------------------------------------------
// RoadmapTask
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RoadmapTask {
    pub number: usize,
    pub title: String,
    pub files: String,
    pub description: String,
    pub acceptance_criteria: String,
    pub dod: String,
    pub completed: bool,
}

impl RoadmapTask {
    /// Build a prompt for the agent to execute this task.
    pub fn to_agent_prompt(&self) -> String {
        let mut prompt = format!("## Task {}: {}\n", self.number, self.title);
        if !self.files.is_empty() {
            prompt.push_str(&format!("Files: {}\n", self.files));
        }
        if !self.description.is_empty() {
            prompt.push_str(&format!("\n{}\n", self.description));
        }
        if !self.dod.is_empty() {
            prompt.push_str(&format!(
                "\n**Definition of Done**: {}\n\
                 Verify this DoD is met before calling done().\n",
                self.dod
            ));
        }
        if !self.acceptance_criteria.is_empty() {
            prompt.push_str(&format!(
                "\n**Verification**: {}\n",
                self.acceptance_criteria
            ));
        }
        prompt
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse a roadmap markdown file into a list of tasks.
/// Lenient: missing fields become empty strings, malformed tasks are skipped.
pub fn parse_roadmap(path: &Path) -> Result<Vec<RoadmapTask>, RoadmapError> {
    let content = std::fs::read_to_string(path).map_err(|e| RoadmapError::Io(e.to_string()))?;
    Ok(parse_roadmap_content(&content))
}

/// Parse roadmap from string content (testable without filesystem).
pub fn parse_roadmap_content(content: &str) -> Vec<RoadmapTask> {
    let mut tasks = Vec::new();
    let mut current: Option<TaskBuilder> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Detect task header: ### Task N: title or ### Task N: ✅ title
        if trimmed.starts_with("### Task ") {
            // Flush previous task
            if let Some(builder) = current.take() {
                if let Some(task) = builder.build() {
                    tasks.push(task);
                }
            }
            current = Some(parse_task_header(trimmed));
            continue;
        }

        // Parse field lines within a task
        if let Some(ref mut builder) = current {
            if let Some((key, value)) = parse_field_line(trimmed) {
                match key.to_lowercase().as_str() {
                    k if k.contains("arquivo") || k.contains("file") => {
                        builder.files = value.to_string();
                    }
                    k if k.contains("que fazer")
                        || k.contains("description")
                        || k.contains("what") =>
                    {
                        builder.description = value.to_string();
                    }
                    k if k.contains("aceite")
                        || k.contains("acceptance")
                        || k.contains("verification") =>
                    {
                        builder.acceptance_criteria = value.to_string();
                    }
                    k if k.contains("dod") || k.contains("definition of done") => {
                        builder.dod = value.to_string();
                    }
                    _ => {}
                }
            }
        }
    }

    // Flush last task
    if let Some(builder) = current {
        if let Some(task) = builder.build() {
            tasks.push(task);
        }
    }

    tasks
}

/// Find the latest roadmap file in `.theo/plans/`.
pub fn find_latest_roadmap(project_dir: &Path) -> Option<std::path::PathBuf> {
    let plans_dir = project_dir.join(".theo").join("plans");
    let entries = std::fs::read_dir(&plans_dir).ok()?;

    let mut files: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();

    files.sort();
    files.last().cloned()
}

/// Mark a task as completed in the roadmap file.
/// Changes `### Task N: title` to `### Task N: ✅ title`.
/// Idempotent: if already marked, no change.
pub fn mark_task_completed(path: &Path, task_number: usize) -> Result<(), RoadmapError> {
    let content = std::fs::read_to_string(path).map_err(|e| RoadmapError::Io(e.to_string()))?;

    let marker = format!("### Task {}:", task_number);
    let mut found = false;
    let mut new_lines: Vec<String> = Vec::new();

    for line in content.lines() {
        if line.trim().starts_with(&marker) {
            found = true;
            if line.contains("✅") {
                // Already marked — idempotent
                new_lines.push(line.to_string());
            } else {
                // Insert ✅ after "### Task N: "
                let after_marker = &line[line.find(&marker).unwrap() + marker.len()..];
                let title = after_marker.trim();
                new_lines.push(format!("### Task {}: ✅ {}", task_number, title));
            }
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !found {
        return Err(RoadmapError::TaskNotFound(task_number));
    }

    let new_content = new_lines.join("\n");
    // Atomic write: write to temp then rename
    let temp_path = path.with_extension("md.tmp");
    std::fs::write(&temp_path, &new_content).map_err(|e| RoadmapError::Io(e.to_string()))?;
    std::fs::rename(&temp_path, path).map_err(|e| RoadmapError::Io(e.to_string()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

struct TaskBuilder {
    number: Option<usize>,
    title: String,
    files: String,
    description: String,
    acceptance_criteria: String,
    dod: String,
    completed: bool,
}

impl TaskBuilder {
    fn build(self) -> Option<RoadmapTask> {
        let number = self.number?;
        Some(RoadmapTask {
            number,
            title: self.title,
            files: self.files,
            description: self.description,
            acceptance_criteria: self.acceptance_criteria,
            dod: self.dod,
            completed: self.completed,
        })
    }
}

fn parse_task_header(line: &str) -> TaskBuilder {
    // Format: ### Task N: title  or  ### Task N: ✅ title
    let after_task = line
        .trim_start_matches('#')
        .trim()
        .strip_prefix("Task ")
        .unwrap_or("");
    let (number_str, rest) = after_task.split_once(':').unwrap_or(("", ""));
    let number = number_str.trim().parse::<usize>().ok();
    let rest = rest.trim();
    let completed = rest.starts_with("✅");
    let title = rest.trim_start_matches("✅").trim().to_string();

    TaskBuilder {
        number,
        title,
        files: String::new(),
        description: String::new(),
        acceptance_criteria: String::new(),
        dod: String::new(),
        completed,
    }
}

fn parse_field_line(line: &str) -> Option<(&str, &str)> {
    // Format: - **Key**: value
    let line = line.strip_prefix('-')?.trim();
    if !line.starts_with("**") {
        return None;
    }
    let line = line.strip_prefix("**")?;
    let end = line.find("**")?;
    let key = &line[..end];
    let rest = &line[end + 2..];
    let value = rest.strip_prefix(':').unwrap_or(rest).trim();
    Some((key, value))
}

// ---------------------------------------------------------------------------
// Checkbox Progress Parser (shared between fix_plan and roadmap)
// ---------------------------------------------------------------------------

/// Parse markdown checkbox progress from content string.
/// Counts `- [x]`/`- [X]` as completed and `- [ ]` as pending.
/// Returns (completed, total). Used by both fix_plan.md and roadmap tracking.
pub fn parse_checkbox_progress(content: &str) -> (usize, usize) {
    let mut completed = 0;
    let mut total = 0;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
            completed += 1;
            total += 1;
        } else if trimmed.starts_with("- [ ]") {
            total += 1;
        }
    }
    (completed, total)
}

/// Parse checkbox progress from a file path. Returns (0, 0) if file doesn't exist.
pub fn parse_checkbox_progress_from_file(path: &std::path::Path) -> (usize, usize) {
    match std::fs::read_to_string(path) {
        Ok(content) => parse_checkbox_progress(&content),
        Err(_) => (0, 0),
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum RoadmapError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("Task not found: {0}")]
    TaskNotFound(usize),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_ROADMAP: &str = r#"# Roadmap: Auth System

## Entendimento
Something here

## Microtasks

### Task 1: Create auth module
- **Arquivo(s)**: src/auth.rs
- **O que fazer**: Implement JWT validation
- **Critério de aceite**: cargo test passes
- **DoD**: File exists with validate_token function

### Task 2: Add middleware
- **Arquivo(s)**: src/main.rs, src/middleware.rs
- **O que fazer**: Wire auth middleware into router
- **Critério de aceite**: Protected routes return 401 without token
- **DoD**: Middleware applied to /api/* routes

### Task 3: Write tests
- **Arquivo(s)**: src/auth.rs
- **O que fazer**: Unit tests for token validation
- **Critério de aceite**: cargo test -- auth passes
- **DoD**: 5+ test cases covering happy path and errors

## Riscos
| # | Risco | Severidade |
|---|-------|-----------|
| 1 | Token expiry | low |
"#;

    #[test]
    fn parse_roadmap_extracts_all_tasks() {
        let tasks = parse_roadmap_content(SAMPLE_ROADMAP);
        assert_eq!(tasks.len(), 3);
    }

    #[test]
    fn parse_roadmap_fields_mapped_correctly() {
        let tasks = parse_roadmap_content(SAMPLE_ROADMAP);
        let t1 = &tasks[0];
        assert_eq!(t1.number, 1);
        assert_eq!(t1.title, "Create auth module");
        assert_eq!(t1.files, "src/auth.rs");
        assert!(t1.description.contains("JWT validation"));
        assert!(t1.acceptance_criteria.contains("cargo test"));
        assert!(t1.dod.contains("validate_token"));
        assert!(!t1.completed);
    }

    #[test]
    fn parse_roadmap_task_number_extracted_from_header() {
        let tasks = parse_roadmap_content(SAMPLE_ROADMAP);
        assert_eq!(tasks[0].number, 1);
        assert_eq!(tasks[1].number, 2);
        assert_eq!(tasks[2].number, 3);
    }

    #[test]
    fn parse_roadmap_empty_content_returns_empty() {
        let tasks = parse_roadmap_content("");
        assert!(tasks.is_empty());
    }

    #[test]
    fn parse_roadmap_no_tasks_returns_empty() {
        let tasks = parse_roadmap_content("# Just a title\nSome text\n");
        assert!(tasks.is_empty());
    }

    #[test]
    fn parse_roadmap_malformed_task_header_skipped() {
        let content = "### Task abc: bad number\n- **DoD**: something\n";
        let tasks = parse_roadmap_content(content);
        assert!(tasks.is_empty());
    }

    #[test]
    fn parse_roadmap_missing_fields_still_parses() {
        let content = "### Task 1: Minimal\n";
        let tasks = parse_roadmap_content(content);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Minimal");
        assert!(tasks[0].files.is_empty());
        assert!(tasks[0].dod.is_empty());
    }

    #[test]
    fn parse_roadmap_non_sequential_numbers() {
        let content = "### Task 1: First\n### Task 5: Fifth\n### Task 10: Tenth\n";
        let tasks = parse_roadmap_content(content);
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].number, 1);
        assert_eq!(tasks[1].number, 5);
        assert_eq!(tasks[2].number, 10);
    }

    #[test]
    fn parse_roadmap_completed_task_detected() {
        let content = "### Task 1: ✅ Already done\n- **DoD**: was done\n";
        let tasks = parse_roadmap_content(content);
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].completed);
        assert_eq!(tasks[0].title, "Already done");
    }

    #[test]
    fn parse_roadmap_colon_in_file_path() {
        let content = "### Task 1: Test\n- **Arquivo(s)**: src/main.rs:42\n";
        let tasks = parse_roadmap_content(content);
        assert_eq!(tasks[0].files, "src/main.rs:42");
    }

    #[test]
    fn to_agent_prompt_contains_task_info() {
        let task = RoadmapTask {
            number: 1,
            title: "Create model".into(),
            files: "src/model.rs".into(),
            description: "Add struct".into(),
            acceptance_criteria: "cargo check".into(),
            dod: "File exists".into(),
            completed: false,
        };
        let prompt = task.to_agent_prompt();
        assert!(prompt.contains("Task 1: Create model"));
        assert!(prompt.contains("src/model.rs"));
        assert!(prompt.contains("File exists"));
        assert!(prompt.contains("cargo check"));
    }

    // -- mark_task_completed tests --

    #[test]
    fn mark_task_completed_adds_checkmark() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roadmap.md");
        std::fs::write(&path, SAMPLE_ROADMAP).unwrap();

        mark_task_completed(&path, 1).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("### Task 1: ✅ Create auth module"));
    }

    #[test]
    fn mark_task_completed_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roadmap.md");
        std::fs::write(&path, SAMPLE_ROADMAP).unwrap();

        mark_task_completed(&path, 1).unwrap();
        mark_task_completed(&path, 1).unwrap(); // second call

        let content = std::fs::read_to_string(&path).unwrap();
        // Should have exactly one ✅, not two
        let count = content.matches("### Task 1: ✅").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn mark_task_completed_does_not_corrupt_other_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roadmap.md");
        std::fs::write(&path, SAMPLE_ROADMAP).unwrap();

        mark_task_completed(&path, 2).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("### Task 2: ✅ Add middleware"));
        assert!(content.contains("### Task 1: Create auth module")); // untouched
        assert!(content.contains("### Task 3: Write tests")); // untouched
        assert!(!content.contains("### Task 1: ✅")); // NOT marked
    }

    #[test]
    fn mark_task_completed_preserves_other_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roadmap.md");
        std::fs::write(&path, SAMPLE_ROADMAP).unwrap();

        mark_task_completed(&path, 1).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("## Riscos"));
        assert!(content.contains("## Entendimento"));
    }

    #[test]
    fn mark_task_completed_task_not_found_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roadmap.md");
        std::fs::write(&path, SAMPLE_ROADMAP).unwrap();

        let err = mark_task_completed(&path, 99).unwrap_err();
        assert!(matches!(err, RoadmapError::TaskNotFound(99)));
    }

    #[test]
    fn mark_task_completed_file_not_found_returns_error() {
        let err = mark_task_completed(Path::new("/nonexistent/roadmap.md"), 1).unwrap_err();
        assert!(matches!(err, RoadmapError::Io(_)));
    }

    // -- Checkbox progress parser tests --

    #[test]
    fn checkbox_progress_counts_correctly() {
        let content = "# Tasks\n- [x] Done\n- [ ] Pending\n- [X] Also done\n";
        let (completed, total) = parse_checkbox_progress(content);
        assert_eq!(completed, 2);
        assert_eq!(total, 3);
    }

    #[test]
    fn checkbox_progress_empty_returns_zero() {
        assert_eq!(parse_checkbox_progress(""), (0, 0));
        assert_eq!(parse_checkbox_progress("no checkboxes here"), (0, 0));
    }

    #[test]
    fn checkbox_progress_from_file_missing() {
        assert_eq!(
            parse_checkbox_progress_from_file(Path::new("/nonexistent")),
            (0, 0)
        );
    }

    #[test]
    fn parse_roadmap_file_not_found_returns_error() {
        let err = parse_roadmap(Path::new("/nonexistent/roadmap.md")).unwrap_err();
        assert!(matches!(err, RoadmapError::Io(_)));
    }
}
