//! Skills system — auto-invocable packaged capabilities.
//!
//! Skills are reusable workflows that the agent invokes automatically
//! when the task matches. Data-driven via markdown files with frontmatter.

pub mod bundled;

use std::path::Path;

use crate::subagent::SubAgentRole;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum SkillMode {
    /// Instructions injected into the main agent's context.
    InContext,
    /// Spawns a sub-agent with the skill instructions as system prompt.
    SubAgent { role: SubAgentRole },
}

#[derive(Debug, Clone)]
pub struct SkillDefinition {
    pub name: String,
    pub trigger: String,
    pub mode: SkillMode,
    pub instructions: String,
}

// ---------------------------------------------------------------------------
// SkillRegistry
// ---------------------------------------------------------------------------

pub struct SkillRegistry {
    skills: Vec<SkillDefinition>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self { skills: Vec::new() }
    }

    /// Load bundled skills (compiled into the binary).
    pub fn load_bundled(&mut self) {
        self.skills.extend(bundled::bundled_skills());
    }

    /// Load skills from a directory of .md files.
    /// Skills with the same name as existing ones override them.
    pub fn load_from_dir(&mut self, dir: &Path) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }

            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Some(skill) = parse_skill_file(&content) {
                    // Override existing skill with same name
                    self.skills.retain(|s| s.name != skill.name);
                    self.skills.push(skill);
                }
            }
        }
    }

    pub fn list(&self) -> &[SkillDefinition] {
        &self.skills
    }

    pub fn get(&self, name: &str) -> Option<&SkillDefinition> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// Generate a summary of all skills for the system prompt.
    pub fn triggers_summary(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        self.skills
            .iter()
            .map(|s| {
                let mode_str = match &s.mode {
                    SkillMode::InContext => "in-context",
                    SkillMode::SubAgent { role } => &format!("sub-agent:{}", role.display_name()),
                };
                format!("- **{}** ({}): {}", s.name, mode_str, s.trigger)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Frontmatter parser
// ---------------------------------------------------------------------------

/// Parse a skill markdown file with frontmatter.
///
/// Format:
/// ```markdown
/// ---
/// name: commit
/// trigger: "when the user asks to commit..."
/// mode: in_context
/// ---
///
/// ## Instructions
/// ...
/// ```
fn parse_skill_file(content: &str) -> Option<SkillDefinition> {
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }

    let after_first = &content[3..];
    let end = after_first.find("---")?;
    let frontmatter = &after_first[..end];
    let body = after_first[end + 3..].trim().to_string();

    let mut name = String::new();
    let mut trigger = String::new();
    let mut mode_str = String::new();
    let mut subagent_role_str = String::new();

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key {
                "name" => name = value.to_string(),
                "trigger" => trigger = value.to_string(),
                "mode" => mode_str = value.to_string(),
                "subagent_role" => subagent_role_str = value.to_string(),
                _ => {}
            }
        }
    }

    if name.is_empty() || trigger.is_empty() {
        return None;
    }

    let mode = match mode_str.as_str() {
        "subagent" | "sub_agent" => {
            let role = SubAgentRole::from_str(&subagent_role_str).unwrap_or(SubAgentRole::Explorer);
            SkillMode::SubAgent { role }
        }
        _ => SkillMode::InContext,
    };

    Some(SkillDefinition {
        name,
        trigger,
        mode,
        instructions: body,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skill_file_in_context() {
        let content = r#"---
name: commit
trigger: when the user asks to commit
mode: in_context
---

## Instructions
Run git status first.
"#;
        let skill = parse_skill_file(content).unwrap();
        assert_eq!(skill.name, "commit");
        assert_eq!(skill.trigger, "when the user asks to commit");
        assert!(matches!(skill.mode, SkillMode::InContext));
        assert!(skill.instructions.contains("git status"));
    }

    #[test]
    fn parse_skill_file_subagent() {
        let content = r#"---
name: test
trigger: run tests
mode: subagent
subagent_role: verifier
---

Run cargo test.
"#;
        let skill = parse_skill_file(content).unwrap();
        assert_eq!(skill.name, "test");
        assert!(matches!(skill.mode, SkillMode::SubAgent { .. }));
    }

    #[test]
    fn parse_skill_file_missing_name_returns_none() {
        let content = r#"---
trigger: something
---
body
"#;
        assert!(parse_skill_file(content).is_none());
    }

    #[test]
    fn parse_skill_file_no_frontmatter_returns_none() {
        assert!(parse_skill_file("just plain text").is_none());
    }

    #[test]
    fn registry_load_bundled() {
        let mut registry = SkillRegistry::new();
        registry.load_bundled();
        assert!(registry.list().len() >= 5, "Should have 5+ bundled skills");
    }

    #[test]
    fn registry_get_by_name() {
        let mut registry = SkillRegistry::new();
        registry.load_bundled();
        assert!(registry.get("commit").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn registry_override_by_name() {
        let mut registry = SkillRegistry::new();
        registry.load_bundled();
        let original_trigger = registry.get("commit").unwrap().trigger.clone();

        // Add custom skill with same name
        registry.skills.retain(|s| s.name != "commit");
        registry.skills.push(SkillDefinition {
            name: "commit".into(),
            trigger: "custom trigger".into(),
            mode: SkillMode::InContext,
            instructions: "custom".into(),
        });

        assert_ne!(registry.get("commit").unwrap().trigger, original_trigger);
        assert_eq!(registry.get("commit").unwrap().trigger, "custom trigger");
    }

    #[test]
    fn triggers_summary_formatted() {
        let mut registry = SkillRegistry::new();
        registry.load_bundled();
        let summary = registry.triggers_summary();
        assert!(summary.contains("**commit**"));
        assert!(summary.contains("**test**"));
    }

    #[test]
    fn load_from_dir_with_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("deploy.md"),
            r#"---
name: deploy
trigger: deploy the application
mode: in_context
---
Run deploy script.
"#,
        )
        .unwrap();

        let mut registry = SkillRegistry::new();
        registry.load_from_dir(dir.path());
        assert_eq!(registry.list().len(), 1);
        assert_eq!(
            registry.get("deploy").unwrap().trigger,
            "deploy the application"
        );
    }
}
