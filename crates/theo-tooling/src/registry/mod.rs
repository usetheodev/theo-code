use theo_domain::error::ToolError;
use theo_domain::tool::{Tool, ToolCategory, ToolDefinition};
use std::collections::HashMap;
use std::path::Path;

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool, validating its schema on insertion.
    pub fn register(&mut self, tool: Box<dyn Tool>) -> Result<(), ToolError> {
        let schema = tool.schema();
        if let Err(e) = schema.validate() {
            return Err(ToolError::Validation(format!(
                "Tool '{}' has invalid schema: {e}",
                tool.id()
            )));
        }
        self.tools.insert(tool.id().to_string(), tool);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&dyn Tool> {
        self.tools.get(id).map(|t| t.as_ref())
    }

    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.tools.keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Return sorted tool IDs filtered by category.
    pub fn ids_by_category(&self, category: ToolCategory) -> Vec<String> {
        let mut ids: Vec<String> = self
            .tools
            .values()
            .filter(|t| t.category() == category)
            .map(|t| t.id().to_string())
            .collect();
        ids.sort();
        ids
    }

    /// Generate ToolDefinitions for all registered tools (sorted by id).
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        let mut defs: Vec<ToolDefinition> = self
            .tools
            .values()
            .map(|t| t.definition())
            .collect();
        defs.sort_by(|a, b| a.id.cmp(&b.id));
        defs
    }

    /// Generate ToolDefinitions filtered by category.
    pub fn definitions_by_category(&self, category: ToolCategory) -> Vec<ToolDefinition> {
        let mut defs: Vec<ToolDefinition> = self
            .tools
            .values()
            .filter(|t| t.category() == category)
            .map(|t| t.definition())
            .collect();
        defs.sort_by(|a, b| a.id.cmp(&b.id));
        defs
    }

    /// Load custom tools from a directory (e.g., .opencode/tool/ or .opencode/tools/)
    pub fn load_custom_tools_from_dir(&mut self, _dir: &Path) -> Result<Vec<String>, ToolError> {
        // TODO: Implement dynamic tool loading from directory
        // For now, return empty list
        Ok(vec![])
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a registry with all built-in tools.
///
/// Panics if any built-in tool has an invalid schema (programming error).
pub fn create_default_registry() -> ToolRegistry {
    use crate::bash::BashTool;
    use crate::read::ReadTool;
    use crate::write::WriteTool;
    use crate::edit::EditTool;
    use crate::grep::GrepTool;
    use crate::glob::GlobTool;
    use crate::apply_patch::ApplyPatchTool;
    use crate::webfetch::WebFetchTool;

    let mut registry = ToolRegistry::new();

    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(BashTool::new()),
        Box::new(ReadTool::new()),
        Box::new(WriteTool::new()),
        Box::new(EditTool::new()),
        Box::new(GrepTool::new()),
        Box::new(GlobTool::new()),
        Box::new(ApplyPatchTool::new()),
        Box::new(WebFetchTool::new()),
    ];

    for tool in tools {
        let id = tool.id().to_string();
        registry
            .register(tool)
            .unwrap_or_else(|e| panic!("Built-in tool '{id}' has invalid schema: {e}"));
    }

    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bash::BashTool;
    use crate::read::ReadTool;
    use crate::grep::GrepTool;
    use theo_domain::tool::ToolCategory;

    #[test]
    fn registers_and_retrieves_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(BashTool::new())).unwrap();
        registry.register(Box::new(ReadTool::new())).unwrap();

        assert_eq!(registry.len(), 2);
        assert!(registry.get("bash").is_some());
        assert!(registry.get("read").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn ids_returns_sorted_tool_ids() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(BashTool::new())).unwrap();
        registry.register(Box::new(ReadTool::new())).unwrap();

        let ids = registry.ids();
        assert_eq!(ids, vec!["bash", "read"]);
    }

    #[test]
    fn default_registry_has_builtin_tools() {
        let registry = create_default_registry();
        let ids = registry.ids();

        assert!(ids.contains(&"bash".to_string()));
        assert!(ids.contains(&"read".to_string()));
        assert!(ids.contains(&"write".to_string()));
        assert!(ids.contains(&"edit".to_string()));
        assert!(ids.contains(&"grep".to_string()));
        assert!(ids.contains(&"glob".to_string()));
        assert!(ids.contains(&"apply_patch".to_string()));
        assert!(ids.contains(&"webfetch".to_string()));
    }

    #[test]
    fn empty_registry() {
        let registry = ToolRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.ids().is_empty());
    }

    #[test]
    fn ids_by_category_filters_correctly() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(BashTool::new())).unwrap();
        registry.register(Box::new(ReadTool::new())).unwrap();
        registry.register(Box::new(GrepTool::new())).unwrap();

        let execution = registry.ids_by_category(ToolCategory::Execution);
        assert_eq!(execution, vec!["bash"]);

        let file_ops = registry.ids_by_category(ToolCategory::FileOps);
        assert_eq!(file_ops, vec!["read"]);

        let search = registry.ids_by_category(ToolCategory::Search);
        assert_eq!(search, vec!["grep"]);

        let web = registry.ids_by_category(ToolCategory::Web);
        assert!(web.is_empty());
    }

    #[test]
    fn definitions_returns_sorted_tool_definitions() {
        let registry = create_default_registry();
        let defs = registry.definitions();

        assert_eq!(defs.len(), registry.len());

        // Verify sorted by id
        for i in 1..defs.len() {
            assert!(defs[i - 1].id < defs[i].id);
        }

        // Verify each definition has valid schema
        for def in &defs {
            let json = def.schema.to_json_schema();
            assert_eq!(json["type"], "object");
            assert!(json.get("properties").is_some());
        }
    }

    #[test]
    fn definitions_by_category_filters_correctly() {
        let registry = create_default_registry();

        let file_ops = registry.definitions_by_category(ToolCategory::FileOps);
        assert!(file_ops.iter().all(|d| d.category == ToolCategory::FileOps));
        assert!(file_ops.iter().any(|d| d.id == "read"));
        assert!(file_ops.iter().any(|d| d.id == "write"));
        assert!(file_ops.iter().any(|d| d.id == "edit"));
        assert!(file_ops.iter().any(|d| d.id == "apply_patch"));

        let search = registry.definitions_by_category(ToolCategory::Search);
        assert!(search.iter().all(|d| d.category == ToolCategory::Search));
        assert!(search.iter().any(|d| d.id == "grep"));
        assert!(search.iter().any(|d| d.id == "glob"));

        let execution = registry.definitions_by_category(ToolCategory::Execution);
        assert!(execution.iter().all(|d| d.category == ToolCategory::Execution));
        assert!(execution.iter().any(|d| d.id == "bash"));

        let web = registry.definitions_by_category(ToolCategory::Web);
        assert!(web.iter().all(|d| d.category == ToolCategory::Web));
        assert!(web.iter().any(|d| d.id == "webfetch"));
    }

    #[test]
    fn all_tools_have_valid_schemas() {
        // Contract test: every tool that can be instantiated
        // must produce a valid schema and non-Utility category
        // (unless explicitly Utility)
        use crate::ls::LsTool;
        use crate::lsp::LspTool;
        use crate::websearch::WebSearchTool;
        use crate::codesearch::CodeSearchTool;
        use crate::todo::TodoTool;
        use crate::invalid::InvalidTool;
        use crate::batch::BatchTool;
        use crate::multiedit::MultiEditTool;
        use crate::plan::PlanExitTool;

        use crate::task::{TaskTool, SubagentInfo};
        use crate::skill::{SkillTool, SkillInfo};
        use crate::question::{QuestionTool, QuestionAsker, Question};

        struct NoopAsker;
        #[async_trait::async_trait]
        impl QuestionAsker for NoopAsker {
            async fn ask(&self, _: &[Question]) -> Vec<Vec<String>> { vec![] }
        }

        let all_tools: Vec<Box<dyn Tool>> = vec![
            Box::new(BashTool::new()),
            Box::new(ReadTool::new()),
            Box::new(crate::write::WriteTool::new()),
            Box::new(crate::edit::EditTool::new()),
            Box::new(GrepTool::new()),
            Box::new(crate::glob::GlobTool::new()),
            Box::new(crate::apply_patch::ApplyPatchTool::new()),
            Box::new(crate::webfetch::WebFetchTool::new()),
            Box::new(LsTool::new()),
            Box::new(LspTool::new()),
            Box::new(WebSearchTool::new()),
            Box::new(CodeSearchTool::new()),
            Box::new(TodoTool::new()),
            Box::new(InvalidTool::new()),
            Box::new(BatchTool::new()),
            Box::new(MultiEditTool::new()),
            Box::new(PlanExitTool::new()),
            Box::new(TaskTool::new(vec![SubagentInfo {
                name: "test".to_string(),
                description: "test agent".to_string(),
            }])),
            Box::new(SkillTool::new(vec![SkillInfo {
                name: "test".to_string(),
                description: "test skill".to_string(),
                dir: std::path::PathBuf::from("/tmp"),
            }])),
            Box::new(QuestionTool::new(Box::new(NoopAsker))),
        ];

        for tool in &all_tools {
            let id = tool.id();
            let schema = tool.schema();

            // Schema must validate
            assert!(
                schema.validate().is_ok(),
                "Tool '{id}' has invalid schema: {:?}",
                schema.validate().err()
            );

            // JSON schema must have correct structure
            let json = schema.to_json_schema();
            assert_eq!(json["type"], "object", "Tool '{id}' schema type must be 'object'");
            assert!(
                json.get("properties").is_some(),
                "Tool '{id}' schema must have 'properties'"
            );

            // Description must not be empty
            assert!(!tool.description().is_empty(), "Tool '{id}' has empty description");

            // Category must be a valid variant
            let _category = tool.category(); // Just verify it doesn't panic
        }
    }

    #[test]
    fn register_rejects_invalid_schema() {
        use theo_domain::tool::{PermissionCollector, ToolContext, ToolOutput, ToolParam, ToolSchema};

        struct BadTool;

        #[async_trait::async_trait]
        impl Tool for BadTool {
            fn id(&self) -> &str { "bad" }
            fn description(&self) -> &str { "A tool with invalid schema" }
            fn schema(&self) -> ToolSchema {
                ToolSchema {
                    params: vec![ToolParam {
                        name: "x".to_string(),
                        param_type: "invalid_type".to_string(),
                        description: "bad param".to_string(),
                        required: false,
                    }],
                }
            }
            async fn execute(
                &self, _: serde_json::Value, _: &ToolContext, _: &mut PermissionCollector,
            ) -> Result<ToolOutput, ToolError> {
                unreachable!()
            }
        }

        let mut registry = ToolRegistry::new();
        let result = registry.register(Box::new(BadTool));
        assert!(result.is_err());
        assert!(registry.is_empty());
    }
}
