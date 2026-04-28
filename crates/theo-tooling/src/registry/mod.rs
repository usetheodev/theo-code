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

    /// Remove a tool by id; returns the removed tool when present.
    /// Used by `create_default_registry_with_project` to swap the
    /// empty `docs_search` stub for one with a populated index.
    pub fn unregister(&mut self, id: &str) -> Option<Box<dyn Tool>> {
        self.tools.remove(id)
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

    /// Generate ToolDefinitions for tools that are NOT deferred.
    ///
    /// Deferred tools (those with `should_defer() == true`) are hidden from
    /// the default system prompt and must be discovered via `tool_search`.
    /// Anthropic principle 12; ref: opendev `should_defer` (traits.rs:547-575).
    pub fn visible_definitions(&self) -> Vec<ToolDefinition> {
        let mut defs: Vec<ToolDefinition> = self
            .tools
            .values()
            .filter(|t| !t.should_defer())
            .map(|t| t.definition())
            .collect();
        defs.sort_by(|a, b| a.id.cmp(&b.id));
        defs
    }

    /// Search deferred tools whose id or `search_hint` contains `query`
    /// (case-insensitive). Returns `(id, hint)` pairs sorted by id so the
    /// agent gets a deterministic shortlist from a `tool_search` call.
    pub fn search_deferred(&self, query: &str) -> Vec<(String, String)> {
        let q = query.to_lowercase();
        let mut hits: Vec<(String, String)> = self
            .tools
            .values()
            .filter(|t| t.should_defer())
            .filter_map(|t| {
                let id = t.id().to_string();
                let hint = t.search_hint().unwrap_or("").to_string();
                let id_match = id.to_lowercase().contains(&q);
                let hint_match = !hint.is_empty() && hint.to_lowercase().contains(&q);
                if id_match || hint_match {
                    Some((id, hint))
                } else {
                    None
                }
            })
            .collect();
        hits.sort_by(|a, b| a.0.cmp(&b.0));
        hits
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

mod builders;

pub use builders::{create_default_registry, create_default_registry_with_project};

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
#[path = "mod_tests.rs"]
mod tests;
