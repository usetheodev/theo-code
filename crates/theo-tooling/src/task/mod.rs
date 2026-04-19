use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
    require_string,
};

#[derive(Debug, Clone)]
pub struct SubagentInfo {
    pub name: String,
    pub description: String,
}

pub struct TaskTool {
    subagents: Vec<SubagentInfo>,
}

impl TaskTool {
    pub fn new(subagents: Vec<SubagentInfo>) -> Self {
        Self { subagents }
    }

    pub fn description_text(&self) -> String {
        let mut sorted = self.subagents.clone();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        let agent_list: Vec<String> = sorted
            .iter()
            .map(|a| format!("- {}: {}", a.name, a.description))
            .collect();
        format!(
            "Spawn a subagent to handle a task.\n\nAvailable agents:\n{}",
            agent_list.join("\n")
        )
    }
}

#[async_trait]
impl Tool for TaskTool {
    fn id(&self) -> &str {
        "task"
    }

    fn description(&self) -> &str {
        "Spawn a subagent for a specialized task"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "description".to_string(),
                    param_type: "string".to_string(),
                    description: "Brief description of the task".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "prompt".to_string(),
                    param_type: "string".to_string(),
                    description: "Detailed prompt for the subagent".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "subagent_type".to_string(),
                    param_type: "string".to_string(),
                    description: "Type of subagent to spawn".to_string(),
                    required: true,
                },
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let description = require_string(&args, "description")?;
        let prompt = require_string(&args, "prompt")?;
        let _subagent_type = require_string(&args, "subagent_type")?;

        // TODO: Implement actual subagent spawning
        Ok(ToolOutput {
            title: description,
            output: format!("Task spawned with prompt: {prompt}"),
            metadata: serde_json::json!({}),
            attachments: None,
            llm_suffix: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_sorts_subagents_by_name_and_is_stable() {
        let subagents = vec![
            SubagentInfo {
                name: "zebra".to_string(),
                description: "Zebra agent".to_string(),
            },
            SubagentInfo {
                name: "alpha".to_string(),
                description: "Alpha agent".to_string(),
            },
            SubagentInfo {
                name: "explore".to_string(),
                description: "Explore agent".to_string(),
            },
            SubagentInfo {
                name: "general".to_string(),
                description: "General agent".to_string(),
            },
        ];

        let tool1 = TaskTool::new(subagents.clone());
        let tool2 = TaskTool::new(subagents);

        let desc1 = tool1.description_text();
        let desc2 = tool2.description_text();
        assert_eq!(desc1, desc2);

        let alpha = desc1.find("- alpha: Alpha agent").unwrap();
        let explore = desc1.find("- explore:").unwrap();
        let general = desc1.find("- general:").unwrap();
        let zebra = desc1.find("- zebra: Zebra agent").unwrap();

        assert!(alpha < explore);
        assert!(explore < general);
        assert!(general < zebra);
    }
}
