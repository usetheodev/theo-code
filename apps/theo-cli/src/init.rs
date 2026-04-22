//! `theo init` — initialize a project for use with Theo.
//!
//! Creates `.theo/theo.md` with auto-detected project information and
//! `.theo/.gitignore` for generated files. Idempotent — never overwrites.

use std::path::Path;

// ---------------------------------------------------------------------------
// Project type detection
// ---------------------------------------------------------------------------

/// Detected project type based on manifest files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    Unknown,
}

impl ProjectType {
    pub fn label(&self) -> &'static str {
        match self {
            ProjectType::Rust => "Rust",
            ProjectType::Node => "Node.js / TypeScript",
            ProjectType::Python => "Python",
            ProjectType::Go => "Go",
            ProjectType::Unknown => "Unknown",
        }
    }

    pub fn build_cmd(&self) -> &'static str {
        match self {
            ProjectType::Rust => "cargo build",
            ProjectType::Node => "npm install && npm run build",
            ProjectType::Python => "pip install -e .",
            ProjectType::Go => "go build ./...",
            ProjectType::Unknown => "# add build command",
        }
    }

    pub fn test_cmd(&self) -> &'static str {
        match self {
            ProjectType::Rust => "cargo test",
            ProjectType::Node => "npm test",
            ProjectType::Python => "pytest",
            ProjectType::Go => "go test ./...",
            ProjectType::Unknown => "# add test command",
        }
    }
}

/// Detect project type from manifest files.
/// Priority: Cargo.toml > package.json > pyproject.toml > go.mod
pub fn detect_project_type(project_dir: &Path) -> ProjectType {
    if project_dir.join("Cargo.toml").exists() {
        ProjectType::Rust
    } else if project_dir.join("package.json").exists() {
        ProjectType::Node
    } else if project_dir.join("pyproject.toml").exists()
        || project_dir.join("requirements.txt").exists()
    {
        ProjectType::Python
    } else if project_dir.join("go.mod").exists() {
        ProjectType::Go
    } else {
        ProjectType::Unknown
    }
}

/// Try to read the project name from the manifest file.
fn detect_project_name(project_dir: &Path, project_type: ProjectType) -> String {
    match project_type {
        ProjectType::Rust => {
            // Extract name from Cargo.toml via simple line parsing.
            if let Ok(content) = std::fs::read_to_string(project_dir.join("Cargo.toml")) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("name") && trimmed.contains('=')
                        && let Some(val) = trimmed.split('=').nth(1) {
                            let name = val.trim().trim_matches('"').trim_matches('\'');
                            if !name.is_empty() {
                                return name.to_string();
                            }
                        }
                }
            }
        }
        ProjectType::Node => {
            if let Ok(content) = std::fs::read_to_string(project_dir.join("package.json")) {
                // Simple JSON extraction for "name" field.
                for line in content.lines() {
                    let trimmed = line.trim().trim_start_matches('{').trim();
                    if trimmed.starts_with("\"name\"")
                        && let Some(val) = trimmed.split(':').nth(1) {
                            let name = val
                                .trim()
                                .trim_end_matches('}')
                                .trim_matches(',')
                                .trim()
                                .trim_matches('"');
                            if !name.is_empty() {
                                return name.to_string();
                            }
                        }
                }
            }
        }
        ProjectType::Python => {
            if let Ok(content) = std::fs::read_to_string(project_dir.join("pyproject.toml")) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("name") && trimmed.contains('=')
                        && let Some(val) = trimmed.split('=').nth(1) {
                            let name = val.trim().trim_matches('"').trim_matches('\'');
                            if !name.is_empty() {
                                return name.to_string();
                            }
                        }
                }
            }
        }
        ProjectType::Go => {
            if let Ok(content) = std::fs::read_to_string(project_dir.join("go.mod"))
                && let Some(line) = content.lines().next()
                    && let Some(module) = line.strip_prefix("module ") {
                        return module.trim().to_string();
                    }
        }
        ProjectType::Unknown => {}
    }

    // Fallback: directory name
    project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my-project")
        .to_string()
}

// ---------------------------------------------------------------------------
// Template rendering
// ---------------------------------------------------------------------------

/// Render the theo.md template with project info and progressive disclosure pointers.
pub fn render_theo_md(
    project_name: &str,
    project_type: ProjectType,
    project_dir: &std::path::Path,
) -> String {
    let mut sections = format!(
        r#"# {project_name}

## Language
{lang}

## Build & Test

```bash
# Build
{build_cmd}

# Test
{test_cmd}
```

## Conventions

- Code language: English (variables, functions, types)
- Tests: Required for business logic
"#,
        lang = project_type.label(),
        build_cmd = project_type.build_cmd(),
        test_cmd = project_type.test_cmd(),
    );

    // Progressive disclosure: add pointers to existing docs
    let mut pointers = Vec::new();
    if project_dir.join("docs").is_dir() {
        pointers.push("- `docs/` — Project documentation");
    }
    if project_dir.join("docs/adr").is_dir() {
        pointers.push("- `docs/adr/` — Architecture Decision Records");
    }
    if project_dir.join("README.md").exists() {
        pointers.push("- `README.md` — Project overview");
    }
    if project_dir.join("ARCHITECTURE.md").exists() {
        pointers.push("- `ARCHITECTURE.md` — System architecture");
    }
    if project_dir.join("CONTRIBUTING.md").exists() {
        pointers.push("- `CONTRIBUTING.md` — Contribution guidelines");
    }
    if project_dir.join("CHANGELOG.md").exists() {
        pointers.push("- `CHANGELOG.md` — Change history");
    }

    if !pointers.is_empty() {
        sections.push_str("\n## Documentation Pointers\n\n");
        for p in &pointers {
            sections.push_str(p);
            sections.push('\n');
        }
    }

    sections.push_str("\n## Architecture\n\n<!-- Run `theo init` with API key to generate detailed architecture -->\n");

    sections
}

/// Content for .theo/.gitignore — exclude generated files, keep theo.md.
fn theo_gitignore() -> &'static str {
    "# Generated by Theo — do not commit these\ngraph.bin\ngraph.bin.tmp\nlearnings.json\nsnapshots/\nsessions/\n"
}

// ---------------------------------------------------------------------------
// Init command
// ---------------------------------------------------------------------------

/// Run the init command (static template, no LLM). Used as fallback and in tests.
#[allow(dead_code)]
pub fn run_init(project_dir: &Path) -> Result<bool, String> {
    if !project_dir.exists() {
        return Err(format!(
            "Directory does not exist: {}",
            project_dir.display()
        ));
    }

    let theo_dir = project_dir.join(".theo");
    let theo_md_path = theo_dir.join("theo.md");
    let gitignore_path = theo_dir.join(".gitignore");

    // Check if already initialized
    if theo_md_path.exists() {
        eprintln!("  .theo/theo.md already exists — skipping (edit manually if needed)");
        // Still create .gitignore if missing
        if !gitignore_path.exists() {
            std::fs::create_dir_all(&theo_dir)
                .map_err(|e| format!("Failed to create .theo/: {e}"))?;
            std::fs::write(&gitignore_path, theo_gitignore())
                .map_err(|e| format!("Failed to write .theo/.gitignore: {e}"))?;
            eprintln!("  Created .theo/.gitignore");
        }
        return Ok(false);
    }

    // Detect project
    let project_type = detect_project_type(project_dir);
    let project_name = detect_project_name(project_dir, project_type);

    eprintln!("  Project: {project_name}");
    eprintln!("  Language: {}", project_type.label());

    // Create .theo/ directory
    std::fs::create_dir_all(&theo_dir).map_err(|e| format!("Failed to create .theo/: {e}"))?;

    // Write theo.md
    let content = render_theo_md(&project_name, project_type, project_dir);
    std::fs::write(&theo_md_path, &content)
        .map_err(|e| format!("Failed to write .theo/theo.md: {e}"))?;
    eprintln!("  Created .theo/theo.md");

    // Write .gitignore
    std::fs::write(&gitignore_path, theo_gitignore())
        .map_err(|e| format!("Failed to write .theo/.gitignore: {e}"))?;
    eprintln!("  Created .theo/.gitignore");

    Ok(true)
}

// ---------------------------------------------------------------------------
// Agent-enriched init
// ---------------------------------------------------------------------------

/// Prompt for the agent to analyze the project and generate real content.
const ENRICH_PROMPT: &str = r#"You are initializing a project for the Theo coding agent. Analyze this project thoroughly and write TWO files:

1. `.theo/theo.md` — Project context file. Read the source code, config files, and structure. Write REAL content:
   - Project name and description (from README or manifest)
   - Language and framework
   - Build and test commands (real, not placeholder)
   - Architecture: describe the ACTUAL modules/layers/bounded contexts you find
   - Important directories: list REAL directories with what they contain
   - Conventions: detect actual patterns (naming, error handling, test style)

2. `.theo/changelog.md` — Extract REAL changelog from git history. Use `bash` to run `git log --oneline -20` and format as Keep a Changelog:
   ```markdown
   # Changelog

   ## [Unreleased]

   ### Added
   - (entries from recent git commits)
   ```

IMPORTANT:
- Write REAL content based on what you READ, not generic templates
- Use the `read`, `grep`, `glob`, `bash` tools to understand the project
- Use `codebase_context` if available for a structural overview
- Write both files using the `write` tool
- Call `done` when finished
"#;

/// Run init with agent enrichment. Creates template first, then enriches via LLM.
///
/// If LLM is not available or fails, the template files remain as-is (fallback).
pub async fn run_init_with_agent(
    project_dir: &std::path::Path,
    config: theo_agent_runtime::AgentConfig,
) -> Result<bool, String> {
    if !project_dir.exists() {
        return Err(format!(
            "Directory does not exist: {}",
            project_dir.display()
        ));
    }

    let theo_dir = project_dir.join(".theo");
    let theo_md_path = theo_dir.join("theo.md");

    // Idempotent: don't overwrite existing
    if theo_md_path.exists() {
        eprintln!("  .theo/theo.md already exists — skipping");
        return Ok(false);
    }

    // Create .theo/ dir and .gitignore
    std::fs::create_dir_all(&theo_dir).map_err(|e| format!("Failed to create .theo/: {e}"))?;
    let gitignore_path = theo_dir.join(".gitignore");
    if !gitignore_path.exists() {
        std::fs::write(&gitignore_path, theo_gitignore())
            .map_err(|e| format!("Failed to write .gitignore: {e}"))?;
    }

    // Check if LLM is available
    if config.api_key.is_none() {
        eprintln!("  No API key — using static template");
        let project_type = detect_project_type(project_dir);
        let project_name = detect_project_name(project_dir, project_type);
        let content = render_theo_md(&project_name, project_type, project_dir);
        std::fs::write(&theo_md_path, &content)
            .map_err(|e| format!("Failed to write theo.md: {e}"))?;
        eprintln!("  Created .theo/theo.md (template)");
        return Ok(true);
    }

    // Run agent to generate real content
    eprintln!("  Analyzing project with AI...");

    let event_bus = std::sync::Arc::new(theo_agent_runtime::event_bus::EventBus::new());
    let renderer = std::sync::Arc::new(crate::renderer::CliRenderer::new());
    event_bus.subscribe(renderer);

    let registry = theo_tooling::registry::create_default_registry();
    let mut agent_config = config;
    agent_config.max_iterations = 30; // Cap for init task
    agent_config.system_prompt = "You are a project analyzer. Read the codebase and generate configuration files. Be thorough but concise. Always use tools, never guess.".to_string();

    let agent = theo_agent_runtime::AgentLoop::new(agent_config, registry);

    let result = agent
        .run_with_history(ENRICH_PROMPT, project_dir, Vec::new(), Some(event_bus))
        .await;

    if result.success && theo_md_path.exists() {
        eprintln!("  Created .theo/theo.md (AI-generated)");
        if project_dir.join(".theo/changelog.md").exists() {
            eprintln!("  Created .theo/changelog.md (from git history)");
        }
        Ok(true)
    } else {
        // Fallback: if agent failed or didn't create the file, use template
        if !theo_md_path.exists() {
            eprintln!("  AI analysis incomplete — using static template as fallback");
            let project_type = detect_project_type(project_dir);
            let project_name = detect_project_name(project_dir, project_type);
            let content = render_theo_md(&project_name, project_type, project_dir);
            std::fs::write(&theo_md_path, &content)
                .map_err(|e| format!("Failed to write theo.md: {e}"))?;
            eprintln!("  Created .theo/theo.md (template fallback)");
        }
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_rust_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"test-rs\"\nversion = \"0.1.0\"\nedition = \"2021\"",
        )
        .unwrap();
        assert_eq!(detect_project_type(tmp.path()), ProjectType::Rust);
    }

    #[test]
    fn detect_node_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("package.json"), r#"{"name": "test-js"}"#).unwrap();
        assert_eq!(detect_project_type(tmp.path()), ProjectType::Node);
    }

    #[test]
    fn detect_python_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("pyproject.toml"),
            "[project]\nname = \"test-py\"",
        )
        .unwrap();
        assert_eq!(detect_project_type(tmp.path()), ProjectType::Python);
    }

    #[test]
    fn detect_go_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("go.mod"), "module github.com/test/go-proj").unwrap();
        assert_eq!(detect_project_type(tmp.path()), ProjectType::Go);
    }

    #[test]
    fn detect_unknown_project() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(detect_project_type(tmp.path()), ProjectType::Unknown);
    }

    #[test]
    fn detect_ambiguous_prefers_rust() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"mixed\"").unwrap();
        std::fs::write(tmp.path().join("package.json"), r#"{"name": "mixed"}"#).unwrap();
        assert_eq!(detect_project_type(tmp.path()), ProjectType::Rust);
    }

    #[test]
    fn detect_name_from_cargo_toml() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"",
        )
        .unwrap();
        let name = detect_project_name(tmp.path(), ProjectType::Rust);
        assert_eq!(name, "my-crate");
    }

    #[test]
    fn detect_name_from_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{"name": "@scope/my-pkg"}"#,
        )
        .unwrap();
        let name = detect_project_name(tmp.path(), ProjectType::Node);
        assert_eq!(name, "@scope/my-pkg");
    }

    #[test]
    fn detect_name_from_go_mod() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("go.mod"),
            "module github.com/user/repo\n\ngo 1.21",
        )
        .unwrap();
        let name = detect_project_name(tmp.path(), ProjectType::Go);
        assert_eq!(name, "github.com/user/repo");
    }

    #[test]
    fn detect_name_fallback_to_dirname() {
        let tmp = tempfile::tempdir().unwrap();
        let name = detect_project_name(tmp.path(), ProjectType::Unknown);
        // Should be the tempdir name, not empty
        assert!(!name.is_empty());
    }

    #[test]
    fn init_creates_theo_md() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"init-test\"\nversion = \"0.1.0\"",
        )
        .unwrap();

        let result = run_init(tmp.path());
        assert!(result.is_ok());
        assert!(result.unwrap()); // true = files created

        let theo_md = tmp.path().join(".theo/theo.md");
        assert!(theo_md.exists());
        let content = std::fs::read_to_string(&theo_md).unwrap();
        assert!(content.contains("init-test"));
        assert!(content.contains("Rust"));
        assert!(content.contains("cargo build"));
    }

    #[test]
    fn init_creates_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run_init(tmp.path());
        assert!(result.is_ok());

        let gitignore = tmp.path().join(".theo/.gitignore");
        assert!(gitignore.exists());
        let content = std::fs::read_to_string(&gitignore).unwrap();
        assert!(content.contains("graph.bin"));
        assert!(content.contains("learnings.json"));
    }

    #[test]
    fn init_idempotent_does_not_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".theo")).unwrap();
        std::fs::write(tmp.path().join(".theo/theo.md"), "custom content").unwrap();

        let result = run_init(tmp.path());
        assert!(result.is_ok());
        assert!(!result.unwrap()); // false = skipped

        // Original content preserved
        let content = std::fs::read_to_string(tmp.path().join(".theo/theo.md")).unwrap();
        assert_eq!(content, "custom content");
    }

    #[test]
    fn init_nonexistent_dir_returns_error() {
        let result = run_init(Path::new("/nonexistent/path/xyz"));
        assert!(result.is_err());
    }

    #[test]
    fn render_template_contains_project_info() {
        let tmp = tempfile::tempdir().unwrap();
        let content = render_theo_md("my-app", ProjectType::Node, tmp.path());
        assert!(content.contains("# my-app"));
        assert!(content.contains("Node.js"));
        assert!(content.contains("npm test"));
    }

    #[test]
    fn render_template_includes_doc_pointers_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("README.md"), "# My Project").unwrap();
        std::fs::create_dir_all(tmp.path().join("docs/adr")).unwrap();

        let content = render_theo_md("test-proj", ProjectType::Rust, tmp.path());
        assert!(content.contains("## Documentation Pointers"));
        assert!(content.contains("README.md"));
        assert!(content.contains("docs/adr/"));
    }

    #[test]
    fn render_template_omits_pointers_when_no_docs() {
        let tmp = tempfile::tempdir().unwrap();
        let content = render_theo_md("bare", ProjectType::Unknown, tmp.path());
        assert!(!content.contains("## Documentation Pointers"));
    }
}
