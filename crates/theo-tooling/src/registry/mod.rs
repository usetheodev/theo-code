use std::collections::HashMap;
use std::path::Path;
use theo_domain::error::ToolError;
use theo_domain::tool::{Tool, ToolCategory, ToolDefinition};

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
        let mut defs: Vec<ToolDefinition> = self.tools.values().map(|t| t.definition()).collect();
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
    use crate::apply_patch::ApplyPatchTool;
    use crate::bash::BashTool;
    use crate::edit::EditTool;
    use crate::glob::GlobTool;
    use crate::grep::GrepTool;
    use crate::memory::MemoryTool;
    use crate::read::ReadTool;
    use crate::reflect::ReflectTool;
    use crate::think::ThinkTool;
    use crate::todo::{TaskCreateTool, TaskUpdateTool};
    use crate::webfetch::WebFetchTool;
    use crate::write::WriteTool;

    let mut registry = ToolRegistry::new();

    // Activate sandbox for BashTool — bwrap > landlock > noop cascade.
    // Allow build tools (cargo, rustc) inside sandbox via read-only mounts.
    let mut sandbox_config = theo_domain::sandbox::SandboxConfig::default();

    // Mount cargo/rustup toolchains as read-only so build tools work inside sandbox
    if let Ok(home) = std::env::var("HOME") {
        let cargo_dir = format!("{home}/.cargo");
        let rustup_dir = format!("{home}/.rustup");
        if std::path::Path::new(&cargo_dir).exists() {
            sandbox_config.filesystem.allowed_read.push(cargo_dir);
        }
        if std::path::Path::new(&rustup_dir).exists() {
            sandbox_config.filesystem.allowed_read.push(rustup_dir);
        }
    }
    // Allow build tool env vars through the sanitizer
    sandbox_config.process.allowed_env_vars.extend(vec![
        "CARGO_HOME".to_string(),
        "RUSTUP_HOME".to_string(),
        "RUSTFLAGS".to_string(),
        "CARGO_TARGET_DIR".to_string(),
    ]);
    let bash_tool = match crate::sandbox::executor::create_executor(&sandbox_config) {
        Ok(executor) => Box::new(BashTool::with_sandbox(
            std::sync::Arc::from(executor),
            sandbox_config,
        )) as Box<dyn Tool>,
        Err(_) => {
            eprintln!("[theo] Warning: sandbox unavailable — BashTool running without isolation");
            Box::new(BashTool::new()) as Box<dyn Tool>
        }
    };

    let tools: Vec<Box<dyn Tool>> = vec![
        bash_tool,
        Box::new(ReadTool::new()),
        Box::new(WriteTool::new()),
        Box::new(EditTool::new()),
        Box::new(GrepTool::new()),
        Box::new(GlobTool::new()),
        Box::new(ApplyPatchTool::new()),
        Box::new(WebFetchTool::new()),
        // Cognitive tools
        Box::new(ThinkTool::new()),
        Box::new(ReflectTool::new()),
        Box::new(MemoryTool::new()),
        Box::new(TaskCreateTool::new()),
        Box::new(TaskUpdateTool::new()),
        // Builtin plugins — typed operations
        Box::new(crate::git::GitStatusTool),
        Box::new(crate::git::GitDiffTool),
        Box::new(crate::git::GitLogTool),
        Box::new(crate::git::GitCommitTool),
        Box::new(crate::env_info::EnvInfoTool),
        Box::new(crate::http_client::HttpGetTool),
        Box::new(crate::http_client::HttpPostTool),
        // Code intelligence — on-demand codebase structure map
        Box::new(crate::codebase_context::CodebaseContextTool::new()),
    ];

    for tool in tools {
        let id = tool.id().to_string();
        registry
            .register(tool)
            .unwrap_or_else(|e| panic!("Built-in tool '{id}' has invalid schema: {e}"));
    }

    registry
}

/// Load plugin tools into an existing registry.
/// Called after create_default_registry() with discovered plugins.
pub fn register_plugin_tools(
    registry: &mut ToolRegistry,
    plugin_tools: Vec<(
        String,
        String,
        std::path::PathBuf,
        Vec<theo_domain::tool::ToolParam>,
    )>,
) {
    use crate::shell_tool::ShellTool;

    for (name, description, script_path, params) in plugin_tools {
        let tool = Box::new(ShellTool::new(
            name.clone(),
            description,
            script_path,
            params,
        ));
        match registry.register(tool) {
            Ok(()) => eprintln!("[theo] Plugin tool registered: {name}"),
            Err(e) => eprintln!("[theo] Warning: plugin tool '{name}' failed to register: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bash::BashTool;
    use crate::grep::GrepTool;
    use crate::read::ReadTool;
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

    /// Guard: the top-5 tools must have onboarding-style descriptions with
    /// NOT-usage rules and at least one concrete example.
    /// Anthropic "Writing tools for agents", principles 3 and 11.
    /// fff-mcp `server.rs:388-502` models the decision-tree format.
    #[test]
    fn top_tools_have_decision_tree_descriptions() {
        let registry = create_default_registry();
        for tool_id in ["read", "grep", "glob", "bash", "edit"] {
            let tool = registry
                .get(tool_id)
                .unwrap_or_else(|| panic!("tool `{tool_id}` missing from default registry"));
            let desc = tool.description();

            assert!(
                desc.len() >= 200,
                "description for `{tool_id}` is too short ({} chars) — \
                 onboarding-style descriptions should explain when to use and when NOT to use the tool",
                desc.len()
            );
            assert!(
                desc.len() <= 1200,
                "description for `{tool_id}` is too long ({} chars) — keep under 1200 to preserve token budget",
                desc.len()
            );
            assert!(
                desc.contains("instead"),
                "description for `{tool_id}` must steer the model away from overlapping tools \
                 (use the word `instead` to name an alternative)"
            );
            assert!(
                desc.to_lowercase().contains("example"),
                "description for `{tool_id}` must include at least one concrete `Example: ...` usage"
            );
        }
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
        assert!(
            execution
                .iter()
                .all(|d| d.category == ToolCategory::Execution)
        );
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
        use crate::batch::BatchTool;
        use crate::codesearch::CodeSearchTool;
        use crate::invalid::InvalidTool;
        use crate::ls::LsTool;
        use crate::lsp::LspTool;
        use crate::multiedit::MultiEditTool;
        use crate::plan::PlanExitTool;
        use crate::todo::{TaskCreateTool, TaskUpdateTool};
        use crate::websearch::WebSearchTool;

        use crate::question::{Question, QuestionAsker, QuestionTool};
        use crate::skill::{SkillInfo, SkillTool};
        use crate::task::{SubagentInfo, TaskTool};

        struct NoopAsker;
        #[async_trait::async_trait]
        impl QuestionAsker for NoopAsker {
            async fn ask(&self, _: &[Question]) -> Vec<Vec<String>> {
                vec![]
            }
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
            Box::new(TaskCreateTool::new()),
            Box::new(TaskUpdateTool::new()),
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
            assert_eq!(
                json["type"], "object",
                "Tool '{id}' schema type must be 'object'"
            );
            assert!(
                json.get("properties").is_some(),
                "Tool '{id}' schema must have 'properties'"
            );

            // Description must not be empty
            assert!(
                !tool.description().is_empty(),
                "Tool '{id}' has empty description"
            );

            // Category must be a valid variant
            let _category = tool.category(); // Just verify it doesn't panic
        }
    }

    #[test]
    fn register_rejects_invalid_schema() {
        use theo_domain::tool::{
            PermissionCollector, ToolContext, ToolOutput, ToolParam, ToolSchema,
        };

        struct BadTool;

        #[async_trait::async_trait]
        impl Tool for BadTool {
            fn id(&self) -> &str {
                "bad"
            }
            fn description(&self) -> &str {
                "A tool with invalid schema"
            }
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
                &self,
                _: serde_json::Value,
                _: &ToolContext,
                _: &mut PermissionCollector,
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
