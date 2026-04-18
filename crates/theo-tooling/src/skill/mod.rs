use async_trait::async_trait;
use std::path::PathBuf;
use theo_domain::error::ToolError;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
    require_string,
};

#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub dir: PathBuf,
}

pub struct SkillTool {
    skills: Vec<SkillInfo>,
}

impl SkillTool {
    pub fn new(skills: Vec<SkillInfo>) -> Self {
        Self { skills }
    }

    pub fn description_text(&self) -> String {
        let mut sorted = self.skills.clone();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        sorted
            .iter()
            .map(|s| format!("**{}**: {}", s.name, s.description))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn id(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Load a specialized skill"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "name".to_string(),
                param_type: "string".to_string(),
                description: "Name of the skill to load".to_string(),
                required: true,
            }],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let name = require_string(&args, "name")?;

        let skill = self
            .skills
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| ToolError::NotFound(format!("Skill not found: {name}")))?;

        permissions.record(PermissionRequest {
            permission: PermissionType::Skill,
            patterns: vec![name.clone()],
            always: vec![name.clone()],
            metadata: serde_json::json!({}),
        });

        // Read skill files
        let mut files = Vec::new();
        if skill.dir.exists() {
            for entry in walkdir::WalkDir::new(&skill.dir)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file() && entry.file_name() != "SKILL.md" {
                    files.push(entry.path().display().to_string());
                }
            }
        }

        let file_list = files
            .iter()
            .map(|f| format!("<file>{f}</file>"))
            .collect::<Vec<_>>()
            .join("\n");

        let dir_url = format!("file://{}", skill.dir.display());
        let output = format!(
            "<skill_content name=\"{name}\">\nBase directory for this skill: {dir_url}\n{file_list}\n</skill_content>",
        );

        Ok(ToolOutput {
            title: name.clone(),
            output,
            metadata: serde_json::json!({
                "name": name,
                "dir": skill.dir.display().to_string(),
            }),
            attachments: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    #[test]
    fn description_sorts_skills_by_name_and_is_stable() {
        let skills = vec![
            SkillInfo {
                name: "zeta-skill".to_string(),
                description: "Zeta skill.".to_string(),
                dir: PathBuf::from("/tmp"),
            },
            SkillInfo {
                name: "alpha-skill".to_string(),
                description: "Alpha skill.".to_string(),
                dir: PathBuf::from("/tmp"),
            },
            SkillInfo {
                name: "middle-skill".to_string(),
                description: "Middle skill.".to_string(),
                dir: PathBuf::from("/tmp"),
            },
        ];

        let tool1 = SkillTool::new(skills.clone());
        let tool2 = SkillTool::new(skills);

        let desc1 = tool1.description_text();
        let desc2 = tool2.description_text();
        assert_eq!(desc1, desc2);

        let alpha_pos = desc1.find("**alpha-skill**: Alpha skill.").unwrap();
        let middle_pos = desc1.find("**middle-skill**: Middle skill.").unwrap();
        let zeta_pos = desc1.find("**zeta-skill**: Zeta skill.").unwrap();

        assert!(alpha_pos < middle_pos);
        assert!(middle_pos < zeta_pos);
    }

    #[tokio::test]
    async fn execute_returns_skill_content_with_files() {
        let tmp = TestDir::new();
        let skill_dir = tmp.path().join(".opencode/skill/tool-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: tool-skill\ndescription: Skill for tests.\n---\n\n# Tool Skill\n",
        )
        .unwrap();
        std::fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        std::fs::write(skill_dir.join("scripts/demo.txt"), "demo").unwrap();

        let skills = vec![SkillInfo {
            name: "tool-skill".to_string(),
            description: "Skill for tests.".to_string(),
            dir: skill_dir.clone(),
        }];

        let tool = SkillTool::new(skills);
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = tool
            .execute(serde_json::json!({"name": "tool-skill"}), &ctx, &mut perms)
            .await
            .unwrap();

        assert_eq!(perms.requests.len(), 1);
        assert_eq!(perms.requests[0].permission, PermissionType::Skill);
        assert!(
            perms.requests[0]
                .patterns
                .contains(&"tool-skill".to_string())
        );
        assert!(perms.requests[0].always.contains(&"tool-skill".to_string()));

        assert_eq!(
            result.metadata["dir"].as_str().unwrap(),
            skill_dir.display().to_string()
        );
        assert!(
            result
                .output
                .contains("<skill_content name=\"tool-skill\">")
        );
        let demo_path = skill_dir.join("scripts/demo.txt").display().to_string();
        assert!(result.output.contains(&format!("<file>{demo_path}</file>")));
    }
}
