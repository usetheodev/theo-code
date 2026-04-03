//! Project configuration loader — reads `.theo/` directory.
//!
//! Precedence: CLI args > .theo/config.toml > defaults.
//! System prompt: .theo/system-prompt.md is PREPENDED to the default prompt.
//! Skills: .theo/skills/*.md (already handled by SkillRegistry).
//! Agents: .theo/agents/*.md (custom sub-agent definitions).

use std::path::Path;

use serde::Deserialize;

use crate::config::AgentConfig;

// ---------------------------------------------------------------------------
// ProjectConfig — parsed from .theo/config.toml
// ---------------------------------------------------------------------------

/// Partial config from `.theo/config.toml`. All fields are optional.
/// Present fields override defaults; absent fields keep defaults.
#[derive(Debug, Deserialize, Default)]
pub struct ProjectConfig {
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_iterations: Option<usize>,
    pub max_tokens: Option<u32>,
    pub reasoning_effort: Option<String>,
    pub doom_loop_threshold: Option<usize>,
    pub context_loop_interval: Option<usize>,
}

impl ProjectConfig {
    /// Load project config from `.theo/config.toml` if it exists.
    /// Returns default (all None) if file doesn't exist or is invalid.
    pub fn load(project_dir: &Path) -> Self {
        let config_path = project_dir.join(".theo").join("config.toml");
        if !config_path.exists() {
            return Self::default();
        }

        match std::fs::read_to_string(&config_path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("[theo] Warning: failed to parse .theo/config.toml: {e}");
                    Self::default()
                }
            },
            Err(e) => {
                eprintln!("[theo] Warning: failed to read .theo/config.toml: {e}");
                Self::default()
            }
        }
    }

    /// Merge project config into an AgentConfig.
    /// Project values override defaults. CLI args should be applied AFTER this.
    pub fn apply_to(&self, config: &mut AgentConfig) {
        if let Some(ref model) = self.model {
            config.model = model.clone();
        }
        if let Some(temp) = self.temperature {
            config.temperature = temp;
        }
        if let Some(max_iter) = self.max_iterations {
            config.max_iterations = max_iter;
        }
        if let Some(max_tok) = self.max_tokens {
            config.max_tokens = max_tok;
        }
        if let Some(ref effort) = self.reasoning_effort {
            config.reasoning_effort = Some(effort.clone());
        }
        if let Some(threshold) = self.doom_loop_threshold {
            config.doom_loop_threshold = Some(threshold);
        }
        if let Some(interval) = self.context_loop_interval {
            config.context_loop_interval = interval;
        }
    }
}

// ---------------------------------------------------------------------------
// System Prompt — .theo/system-prompt.md (REPLACES default system prompt)
// ---------------------------------------------------------------------------

/// Load custom system prompt from `.theo/system-prompt.md`.
/// Returns None if file doesn't exist.
/// When present, this REPLACES the default system prompt entirely.
/// This is the agent's behavioral instructions (workflow, rules, personality).
pub fn load_system_prompt(project_dir: &Path) -> Option<String> {
    let prompt_path = project_dir.join(".theo").join("system-prompt.md");
    std::fs::read_to_string(prompt_path).ok()
}

// ---------------------------------------------------------------------------
// Project Context — .theo/theo.md (PREPENDED as context)
// ---------------------------------------------------------------------------

/// Load project context from `.theo/theo.md`.
/// Returns None if file doesn't exist.
/// This is PREPENDED as a system message with project architecture,
/// conventions, and structure — like CLAUDE.md for Claude Code.
pub fn load_project_context(project_dir: &Path) -> Option<String> {
    let prompt_path = project_dir.join(".theo").join("theo.md");
    std::fs::read_to_string(prompt_path).ok()
}

// ---------------------------------------------------------------------------
// Custom Agents — .theo/agents/*.md
// ---------------------------------------------------------------------------

/// A custom sub-agent definition loaded from `.theo/agents/*.md`.
#[derive(Debug, Clone)]
pub struct CustomAgentDef {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    /// Role for capability restriction. Defaults to "explorer" (read-only).
    pub role: String,
}

/// Load custom agent definitions from `.theo/agents/` directory.
pub fn load_custom_agents(project_dir: &Path) -> Vec<CustomAgentDef> {
    let agents_dir = project_dir.join(".theo").join("agents");
    let entries = match std::fs::read_dir(&agents_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut agents = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Some(agent) = parse_agent_file(&content) {
                agents.push(agent);
            }
        }
    }
    agents
}

/// Parse a custom agent markdown file with frontmatter.
///
/// Format:
/// ```markdown
/// ---
/// name: my-explorer
/// description: "Explores the project structure"
/// role: explorer
/// ---
///
/// You are a specialized explorer...
/// ```
fn parse_agent_file(content: &str) -> Option<CustomAgentDef> {
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }

    let after_first = &content[3..];
    let end = after_first.find("---")?;
    let frontmatter = &after_first[..end];
    let body = after_first[end + 3..].trim().to_string();

    let mut name = String::new();
    let mut description = String::new();
    let mut role = "explorer".to_string();

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key {
                "name" => name = value.to_string(),
                "description" => description = value.to_string(),
                "role" => role = value.to_string(),
                _ => {}
            }
        }
    }

    if name.is_empty() {
        return None;
    }

    Some(CustomAgentDef {
        name,
        description,
        system_prompt: body,
        role,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_config_missing_returns_defaults() {
        let config = ProjectConfig::load(Path::new("/nonexistent/path"));
        assert!(config.model.is_none());
        assert!(config.temperature.is_none());
        assert!(config.max_iterations.is_none());
    }

    #[test]
    fn project_config_loaded_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        let theo_dir = dir.path().join(".theo");
        std::fs::create_dir_all(&theo_dir).unwrap();
        std::fs::write(
            theo_dir.join("config.toml"),
            r#"
model = "gpt-4"
temperature = 0.5
max_iterations = 50
"#,
        )
        .unwrap();

        let config = ProjectConfig::load(dir.path());
        assert_eq!(config.model.as_deref(), Some("gpt-4"));
        assert_eq!(config.temperature, Some(0.5));
        assert_eq!(config.max_iterations, Some(50));
    }

    #[test]
    fn project_config_invalid_toml_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let theo_dir = dir.path().join(".theo");
        std::fs::create_dir_all(&theo_dir).unwrap();
        std::fs::write(theo_dir.join("config.toml"), "not valid toml {{{").unwrap();

        let config = ProjectConfig::load(dir.path());
        assert!(config.model.is_none());
    }

    #[test]
    fn apply_to_overrides_only_present_fields() {
        let project = ProjectConfig {
            model: Some("custom-model".into()),
            temperature: None,
            max_iterations: Some(42),
            ..Default::default()
        };

        let mut config = AgentConfig::default();
        let original_temp = config.temperature;
        project.apply_to(&mut config);

        assert_eq!(config.model, "custom-model");
        assert_eq!(config.max_iterations, 42);
        assert_eq!(config.temperature, original_temp); // unchanged
    }

    #[test]
    fn merge_with_empty_project_config_equals_base_config() {
        let project = ProjectConfig::default();
        let mut config = AgentConfig::default();
        let original_model = config.model.clone();
        let original_max = config.max_iterations;
        project.apply_to(&mut config);

        assert_eq!(config.model, original_model);
        assert_eq!(config.max_iterations, original_max);
    }

    #[test]
    fn load_system_prompt_returns_none_when_missing() {
        assert!(load_system_prompt(Path::new("/nonexistent")).is_none());
    }

    #[test]
    fn load_system_prompt_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let theo_dir = dir.path().join(".theo");
        std::fs::create_dir_all(&theo_dir).unwrap();
        std::fs::write(theo_dir.join("system-prompt.md"), "You are an agent...").unwrap();

        let prompt = load_system_prompt(dir.path());
        assert_eq!(prompt.as_deref(), Some("You are an agent..."));
    }

    #[test]
    fn load_project_context_returns_none_when_missing() {
        assert!(load_project_context(Path::new("/nonexistent")).is_none());
    }

    #[test]
    fn load_project_context_reads_theo_md() {
        let dir = tempfile::tempdir().unwrap();
        let theo_dir = dir.path().join(".theo");
        std::fs::create_dir_all(&theo_dir).unwrap();
        std::fs::write(theo_dir.join("theo.md"), "# My Project\nRust + Axum").unwrap();

        let context = load_project_context(dir.path());
        assert_eq!(context.as_deref(), Some("# My Project\nRust + Axum"));
    }

    #[test]
    fn parse_agent_file_valid() {
        let content = r#"---
name: my-agent
description: Does cool things
role: verifier
---

You are a custom verifier agent.
"#;
        let agent = parse_agent_file(content).unwrap();
        assert_eq!(agent.name, "my-agent");
        assert_eq!(agent.description, "Does cool things");
        assert_eq!(agent.role, "verifier");
        assert!(agent.system_prompt.contains("custom verifier"));
    }

    #[test]
    fn parse_agent_file_missing_name_returns_none() {
        let content = r#"---
description: No name
---
body
"#;
        assert!(parse_agent_file(content).is_none());
    }

    #[test]
    fn load_custom_agents_from_dir() {
        let dir = tempfile::tempdir().unwrap();
        let agents_dir = dir.path().join(".theo").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("scanner.md"),
            r#"---
name: scanner
description: Scans for issues
role: explorer
---

Scan the codebase for problems.
"#,
        )
        .unwrap();

        let agents = load_custom_agents(dir.path());
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "scanner");
    }

    #[test]
    fn load_custom_agents_empty_dir_returns_empty() {
        let agents = load_custom_agents(Path::new("/nonexistent"));
        assert!(agents.is_empty());
    }

    #[test]
    fn project_config_extra_fields_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let theo_dir = dir.path().join(".theo");
        std::fs::create_dir_all(&theo_dir).unwrap();
        std::fs::write(
            theo_dir.join("config.toml"),
            r#"
model = "gpt-4"
unknown_field = "should be ignored"
another_unknown = 42
"#,
        )
        .unwrap();

        let config = ProjectConfig::load(dir.path());
        assert_eq!(config.model.as_deref(), Some("gpt-4"));
    }
}
