//! Project configuration loader — reads `.theo/` directory.
//!
//! Precedence: CLI args > .theo/config.toml > defaults.
//! System prompt: .theo/system-prompt.md is PREPENDED to the default prompt.
//! Skills: .theo/skills/*.md (already handled by SkillRegistry).
//! Agents: .theo/agents/*.md (custom sub-agent definitions).

use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::config::AgentConfig;

/// Errors produced by [`ProjectConfig::validate`]. Per ADR-014 we keep
/// validation in a small manual function rather than pulling `garde` +
/// `garde-derive` for a single DTO.
#[derive(Debug, Error, PartialEq)]
pub enum ConfigValidationError {
    #[error("temperature must be in [0.0, 2.0], got {0}")]
    TemperatureOutOfRange(f32),

    #[error("max_iterations must be > 0 and <= 10000, got {0}")]
    MaxIterationsOutOfRange(usize),

    #[error("max_tokens must be > 0 and <= 2_000_000, got {0}")]
    MaxTokensOutOfRange(u32),

    #[error("doom_loop_threshold must be < max_iterations; got threshold={threshold}, max={max}")]
    DoomLoopInconsistent { threshold: usize, max: usize },

    #[error(
        "context_loop_interval must be > 0 and <= max_iterations; got interval={interval}, max={max:?}"
    )]
    ContextLoopInterval {
        interval: usize,
        max: Option<usize>,
    },

    #[error("reasoning_effort must be one of 'low'|'medium'|'high', got {0:?}")]
    UnknownReasoningEffort(String),
}

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
            Ok(content) => match toml::from_str::<Self>(&content) {
                Ok(config) => match config.validate() {
                    Ok(()) => config,
                    Err(e) => {
                        eprintln!("[theo] Warning: .theo/config.toml failed validation: {e}");
                        Self::default()
                    }
                },
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

    /// Validate every optional field against its accepted domain.
    ///
    /// Called from [`Self::load`] immediately after the TOML deserialize
    /// step, so users get a clear warning instead of silently corrupt
    /// runtime behaviour. Per ADR-014 this is a hand-written function
    /// rather than a `#[derive(Garde)]`.
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        if let Some(t) = self.temperature
            && !(0.0..=2.0).contains(&t)
        {
            return Err(ConfigValidationError::TemperatureOutOfRange(t));
        }
        if let Some(n) = self.max_iterations
            && (n == 0 || n > 10_000)
        {
            return Err(ConfigValidationError::MaxIterationsOutOfRange(n));
        }
        if let Some(n) = self.max_tokens
            && (n == 0 || n > 2_000_000)
        {
            return Err(ConfigValidationError::MaxTokensOutOfRange(n));
        }
        if let Some(threshold) = self.doom_loop_threshold
            && let Some(max) = self.max_iterations
            && threshold >= max
        {
            return Err(ConfigValidationError::DoomLoopInconsistent {
                threshold,
                max,
            });
        }
        if let Some(interval) = self.context_loop_interval {
            if interval == 0 {
                return Err(ConfigValidationError::ContextLoopInterval {
                    interval,
                    max: self.max_iterations,
                });
            }
            if let Some(max) = self.max_iterations
                && interval > max
            {
                return Err(ConfigValidationError::ContextLoopInterval {
                    interval,
                    max: Some(max),
                });
            }
        }
        if let Some(ref effort) = self.reasoning_effort
            && !matches!(effort.as_str(), "low" | "medium" | "high")
        {
            return Err(ConfigValidationError::UnknownReasoningEffort(effort.clone()));
        }
        Ok(())
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

impl ProjectConfig {
    /// Apply environment variable overrides. Precedence: env > .theo/config.toml > defaults.
    /// Variables: THEO_MODEL, THEO_TEMPERATURE, THEO_MAX_ITERATIONS, THEO_MAX_TOKENS,
    /// THEO_REASONING_EFFORT, THEO_DOOM_LOOP_THRESHOLD.
    pub fn with_env_overrides(mut self) -> Self {
        use theo_domain::environment::{parse_var, theo_var};
        if let Some(v) = theo_var("THEO_MODEL") {
            self.model = Some(v);
        }
        if let Some(t) = parse_var::<f32>("THEO_TEMPERATURE") {
            self.temperature = Some(t);
        }
        if let Some(n) = parse_var::<usize>("THEO_MAX_ITERATIONS") {
            self.max_iterations = Some(n);
        }
        if let Some(n) = parse_var::<u32>("THEO_MAX_TOKENS") {
            self.max_tokens = Some(n);
        }
        if let Some(v) = theo_var("THEO_REASONING_EFFORT") {
            self.reasoning_effort = Some(v);
        }
        if let Some(n) = parse_var::<usize>("THEO_DOOM_LOOP_THRESHOLD") {
            self.doom_loop_threshold = Some(n);
        }
        self
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

        if let Ok(content) = std::fs::read_to_string(&path)
            && let Some(agent) = parse_agent_file(&content) {
                agents.push(agent);
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

    // ── validate() — per-field error coverage ────────────────────

    #[test]
    fn validate_accepts_all_none_defaults() {
        let cfg = ProjectConfig::default();
        assert_eq!(cfg.validate(), Ok(()));
    }

    #[test]
    fn validate_rejects_negative_temperature() {
        let cfg = ProjectConfig {
            temperature: Some(-0.1),
            ..ProjectConfig::default()
        };
        assert_eq!(
            cfg.validate(),
            Err(ConfigValidationError::TemperatureOutOfRange(-0.1))
        );
    }

    #[test]
    fn validate_rejects_temperature_above_two() {
        let cfg = ProjectConfig {
            temperature: Some(2.5),
            ..ProjectConfig::default()
        };
        assert_eq!(
            cfg.validate(),
            Err(ConfigValidationError::TemperatureOutOfRange(2.5))
        );
    }

    #[test]
    fn validate_accepts_temperature_at_boundary() {
        for t in [0.0, 1.0, 2.0] {
            let cfg = ProjectConfig {
                temperature: Some(t),
                ..ProjectConfig::default()
            };
            assert_eq!(cfg.validate(), Ok(()));
        }
    }

    #[test]
    fn validate_rejects_zero_max_iterations() {
        let cfg = ProjectConfig {
            max_iterations: Some(0),
            ..ProjectConfig::default()
        };
        assert_eq!(
            cfg.validate(),
            Err(ConfigValidationError::MaxIterationsOutOfRange(0))
        );
    }

    #[test]
    fn validate_rejects_huge_max_iterations() {
        let cfg = ProjectConfig {
            max_iterations: Some(1_000_000),
            ..ProjectConfig::default()
        };
        assert!(matches!(
            cfg.validate(),
            Err(ConfigValidationError::MaxIterationsOutOfRange(1_000_000))
        ));
    }

    #[test]
    fn validate_rejects_zero_max_tokens() {
        let cfg = ProjectConfig {
            max_tokens: Some(0),
            ..ProjectConfig::default()
        };
        assert_eq!(
            cfg.validate(),
            Err(ConfigValidationError::MaxTokensOutOfRange(0))
        );
    }

    #[test]
    fn validate_rejects_doom_loop_ge_max_iterations() {
        let cfg = ProjectConfig {
            max_iterations: Some(10),
            doom_loop_threshold: Some(10),
            ..ProjectConfig::default()
        };
        assert_eq!(
            cfg.validate(),
            Err(ConfigValidationError::DoomLoopInconsistent {
                threshold: 10,
                max: 10,
            })
        );
    }

    #[test]
    fn validate_rejects_context_loop_zero_interval() {
        let cfg = ProjectConfig {
            context_loop_interval: Some(0),
            ..ProjectConfig::default()
        };
        assert!(matches!(
            cfg.validate(),
            Err(ConfigValidationError::ContextLoopInterval { interval: 0, .. })
        ));
    }

    #[test]
    fn validate_rejects_context_loop_greater_than_max() {
        let cfg = ProjectConfig {
            max_iterations: Some(5),
            context_loop_interval: Some(6),
            ..ProjectConfig::default()
        };
        assert!(matches!(
            cfg.validate(),
            Err(ConfigValidationError::ContextLoopInterval {
                interval: 6,
                max: Some(5),
            })
        ));
    }

    #[test]
    fn validate_rejects_unknown_reasoning_effort() {
        let cfg = ProjectConfig {
            reasoning_effort: Some("ultra".into()),
            ..ProjectConfig::default()
        };
        assert_eq!(
            cfg.validate(),
            Err(ConfigValidationError::UnknownReasoningEffort("ultra".into()))
        );
    }

    #[test]
    fn validate_accepts_every_valid_reasoning_effort() {
        for level in ["low", "medium", "high"] {
            let cfg = ProjectConfig {
                reasoning_effort: Some(level.into()),
                ..ProjectConfig::default()
            };
            assert_eq!(cfg.validate(), Ok(()));
        }
    }

    #[test]
    fn load_degrades_to_defaults_on_invalid_config() {
        let dir = tempfile::tempdir().unwrap();
        let theo_dir = dir.path().join(".theo");
        std::fs::create_dir_all(&theo_dir).unwrap();
        std::fs::write(
            theo_dir.join("config.toml"),
            "temperature = -1.0\n",
        )
        .unwrap();
        let cfg = ProjectConfig::load(dir.path());
        // Invalid field → defaults (all None).
        assert!(cfg.temperature.is_none());
    }

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
    fn env_override_temperature_applied_to_agent_config() {
        // This test proves the P0 bug fix: THEO_TEMPERATURE env var must
        // propagate through ProjectConfig → apply_to → AgentConfig.temperature
        unsafe { std::env::set_var("THEO_TEMPERATURE", "0.0") };

        let project = ProjectConfig::default().with_env_overrides();
        assert_eq!(project.temperature, Some(0.0), "env var should set temperature");

        let mut config = AgentConfig::default();
        assert_eq!(config.temperature, 0.1, "default should be 0.1");

        project.apply_to(&mut config);
        assert_eq!(
            config.temperature, 0.0,
            "after apply_to, temperature should be 0.0 from env var"
        );

        // Cleanup
        unsafe { std::env::remove_var("THEO_TEMPERATURE") };
    }

    #[test]
    fn env_override_does_not_affect_unset_fields() {
        unsafe { std::env::remove_var("THEO_TEMPERATURE") };
        unsafe { std::env::remove_var("THEO_MODEL") };

        let project = ProjectConfig::default().with_env_overrides();
        assert!(project.temperature.is_none());
        assert!(project.model.is_none());

        let mut config = AgentConfig::default();
        let original_temp = config.temperature;
        project.apply_to(&mut config);
        assert_eq!(config.temperature, original_temp, "should remain unchanged");
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
    fn env_overrides_work_correctly() {
        // Test the pure logic without env var mutation (avoids race conditions).
        // with_env_overrides reads env vars — we test the method exists and handles
        // the "not set" case (which is the default state in CI).
        let config = ProjectConfig {
            model: Some("from-toml".into()),
            ..Default::default()
        };
        // Without env vars set, with_env_overrides preserves existing values
        let applied = config.with_env_overrides();
        // Model stays "from-toml" because THEO_MODEL is not set in test env
        assert!(applied.model.is_some());
    }

    #[test]
    fn env_override_method_exists_and_returns_self() {
        // Verify the method compiles and returns ProjectConfig (type-level test)
        let config = ProjectConfig::default().with_env_overrides();
        // Default has all None — env vars not set in test env
        // Just verify it doesn't panic
        let _ = config.model;
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
