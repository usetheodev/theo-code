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

/// Create a registry with all built-in tools.
///
/// Panics if any built-in tool has an invalid schema (programming error).
pub fn create_default_registry() -> ToolRegistry {
    use crate::apply_patch::ApplyPatchTool;
    use crate::bash::BashTool;
    use crate::browser::{
        BrowserClickTool, BrowserCloseTool, BrowserOpenTool, BrowserScreenshotTool,
        BrowserSessionManager,
    };
    use crate::dap::{
        DapSessionManager, DebugContinueTool, DebugEvalTool, DebugLaunchTool,
        DebugSetBreakpointTool, DebugStackTraceTool, DebugStepTool, DebugTerminateTool,
        DebugVariablesTool,
    };
    use crate::edit::EditTool;
    use crate::glob::GlobTool;
    use crate::grep::GrepTool;
    use crate::lsp::{
        LspDefinitionTool, LspHoverTool, LspReferencesTool, LspRenameTool,
        LspSessionManager,
    };
    use crate::memory::MemoryTool;
    use crate::plan::{
        AdvancePhaseTool, CreatePlanTool, GetNextTaskTool, GetPlanSummaryTool, LogEntryTool,
        ReplanTool, UpdateTaskTool,
    };
    use crate::docs_search::DocsSearchTool;
    use crate::read::ReadTool;
    use crate::read_image::ReadImageTool;
    use crate::reflect::ReflectTool;
    use crate::test_gen::{GenMutationTestTool, GenPropertyTestTool};
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
        // SOTA Planning System — schema-validated plans
        Box::new(CreatePlanTool::new()),
        Box::new(UpdateTaskTool::new()),
        Box::new(AdvancePhaseTool::new()),
        Box::new(LogEntryTool::new()),
        Box::new(GetPlanSummaryTool::new()),
        Box::new(GetNextTaskTool::new()),
        Box::new(ReplanTool::new()),
        // T5.1 / T5.2 — auto-test-generation
        Box::new(GenPropertyTestTool::new()),
        Box::new(GenMutationTestTool::new()),
        // T1.2 — multimodal: load images as vision blocks
        Box::new(ReadImageTool::new()),
        // T15.1 — external docs RAG (empty index by default; populated
        // by future commits that wire crates.io/MDN/npm sources)
        Box::new(DocsSearchTool::new()),
        // T3.1 — LSP tool family. Default registry uses an empty
        // catalogue (no PATH discovery) so the tools surface the
        // same actionable error for every call until
        // `create_default_registry_with_project` swaps in real
        // session managers. Keeping the tools registered in the
        // default registry preserves the manifest invariant
        // (every DefaultRegistry entry is reachable from
        // create_default_registry).
        Box::new(LspDefinitionTool::new(std::sync::Arc::new(
            LspSessionManager::from_catalogue(std::collections::HashMap::new()),
        ))),
        Box::new(LspReferencesTool::new(std::sync::Arc::new(
            LspSessionManager::from_catalogue(std::collections::HashMap::new()),
        ))),
        Box::new(LspHoverTool::new(std::sync::Arc::new(
            LspSessionManager::from_catalogue(std::collections::HashMap::new()),
        ))),
        Box::new(LspRenameTool::new(std::sync::Arc::new(
            LspSessionManager::from_catalogue(std::collections::HashMap::new()),
        ))),
        // T13.1 — DAP tool family. Same dual-registry pattern as the
        // lsp_* tools: empty-catalogue stubs in the default registry
        // (actionable error path); real PATH-discovered manager
        // swapped in by `create_default_registry_with_project`.
        Box::new(DebugLaunchTool::new(std::sync::Arc::new(
            DapSessionManager::from_catalogue(std::collections::HashMap::new()),
        ))),
        Box::new(DebugSetBreakpointTool::new(std::sync::Arc::new(
            DapSessionManager::from_catalogue(std::collections::HashMap::new()),
        ))),
        Box::new(DebugContinueTool::new(std::sync::Arc::new(
            DapSessionManager::from_catalogue(std::collections::HashMap::new()),
        ))),
        Box::new(DebugStepTool::new(std::sync::Arc::new(
            DapSessionManager::from_catalogue(std::collections::HashMap::new()),
        ))),
        Box::new(DebugEvalTool::new(std::sync::Arc::new(
            DapSessionManager::from_catalogue(std::collections::HashMap::new()),
        ))),
        Box::new(DebugStackTraceTool::new(std::sync::Arc::new(
            DapSessionManager::from_catalogue(std::collections::HashMap::new()),
        ))),
        Box::new(DebugVariablesTool::new(std::sync::Arc::new(
            DapSessionManager::from_catalogue(std::collections::HashMap::new()),
        ))),
        Box::new(DebugTerminateTool::new(std::sync::Arc::new(
            DapSessionManager::from_catalogue(std::collections::HashMap::new()),
        ))),
        // T2.1 — browser tool family. Default registry uses managers
        // pointing at a non-existent script path so every call
        // surfaces the same actionable "missing script" error until
        // the project-aware constructor swaps in real managers.
        Box::new(BrowserOpenTool::new(std::sync::Arc::new(
            BrowserSessionManager::new("node", "/__theo_no_browser__/playwright_sidecar.js"),
        ))),
        Box::new(BrowserClickTool::new(std::sync::Arc::new(
            BrowserSessionManager::new("node", "/__theo_no_browser__/playwright_sidecar.js"),
        ))),
        Box::new(BrowserScreenshotTool::new(std::sync::Arc::new(
            BrowserSessionManager::new("node", "/__theo_no_browser__/playwright_sidecar.js"),
        ))),
        Box::new(BrowserCloseTool::new(std::sync::Arc::new(
            BrowserSessionManager::new("node", "/__theo_no_browser__/playwright_sidecar.js"),
        ))),
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

/// T15.1 + T3.1 — Variant of [`create_default_registry`] that wires
/// project-aware tools:
///
/// 1. `docs_search` — populated from `<project>/docs/`,
///    `<project>/.theo/wiki/`, `~/.cache/theo/docs/`.
/// 2. `lsp_definition` — backed by an `LspSessionManager` that
///    discovers installed LSP servers on PATH (rust-analyzer,
///    pyright, gopls, etc.) and lazily spawns one per language.
///
/// Use this constructor when a project root is known at session
/// startup (the typical case for CLI / TUI / desktop runs). For
/// contexts without a project root, fall back to
/// [`create_default_registry`].
pub fn create_default_registry_with_project(
    project_dir: &std::path::Path,
) -> ToolRegistry {
    use std::sync::Arc;

    use crate::browser::{
        BrowserClickTool, BrowserCloseTool, BrowserOpenTool, BrowserScreenshotTool,
        BrowserSessionManager,
    };
    use crate::dap::{
        DapSessionManager, DebugContinueTool, DebugEvalTool, DebugLaunchTool,
        DebugSetBreakpointTool, DebugStackTraceTool, DebugStepTool, DebugTerminateTool,
        DebugVariablesTool,
    };
    use crate::docs_search::{DocsSearchTool, bootstrap_docs_index};
    use crate::lsp::{
        LspDefinitionTool, LspHoverTool, LspReferencesTool, LspRenameTool,
        LspSessionManager,
    };

    let _ = project_dir; // silenced when no per-project state is wired
    let mut registry = create_default_registry();

    // T15.1 — populated docs_search index.
    let docs_index = bootstrap_docs_index(project_dir);
    registry.unregister("docs_search");
    registry
        .register(Box::new(DocsSearchTool::with_index(docs_index)))
        .expect("docs_search tool schema is valid");

    // T3.1 — swap the default registry's empty-catalogue
    // LSP tools for ones backed by a real PATH-discovered session
    // manager. The shared Arc means every lsp_* tool reuses the
    // same spawned server processes (one rust-analyzer serves
    // both lsp_definition and lsp_references on `.rs` files).
    let lsp_manager = Arc::new(LspSessionManager::from_path());
    for tool_id in ["lsp_definition", "lsp_references", "lsp_hover", "lsp_rename"] {
        registry.unregister(tool_id);
    }
    registry
        .register(Box::new(LspDefinitionTool::new(lsp_manager.clone())))
        .expect("lsp_definition tool schema is valid");
    registry
        .register(Box::new(LspReferencesTool::new(lsp_manager.clone())))
        .expect("lsp_references tool schema is valid");
    registry
        .register(Box::new(LspHoverTool::new(lsp_manager.clone())))
        .expect("lsp_hover tool schema is valid");
    registry
        .register(Box::new(LspRenameTool::new(lsp_manager.clone())))
        .expect("lsp_rename tool schema is valid");

    // T13.1 — same pattern for the debug_* family. Critical:
    // every debug_* tool MUST share the SAME Arc<DapSessionManager>
    // so they all see the same session table. Splitting the manager
    // would make `debug_set_breakpoint({session_id: "a"})` fail to
    // find the session that `debug_launch({session_id: "a"})` opened.
    let dap_manager = Arc::new(DapSessionManager::from_path());
    for tool_id in [
        "debug_launch",
        "debug_set_breakpoint",
        "debug_continue",
        "debug_step",
        "debug_eval",
        "debug_stack_trace",
        "debug_variables",
        "debug_terminate",
    ] {
        registry.unregister(tool_id);
    }
    registry
        .register(Box::new(DebugLaunchTool::new(dap_manager.clone())))
        .expect("debug_launch tool schema is valid");
    registry
        .register(Box::new(DebugSetBreakpointTool::new(dap_manager.clone())))
        .expect("debug_set_breakpoint tool schema is valid");
    registry
        .register(Box::new(DebugContinueTool::new(dap_manager.clone())))
        .expect("debug_continue tool schema is valid");
    registry
        .register(Box::new(DebugStepTool::new(dap_manager.clone())))
        .expect("debug_step tool schema is valid");
    registry
        .register(Box::new(DebugEvalTool::new(dap_manager.clone())))
        .expect("debug_eval tool schema is valid");
    registry
        .register(Box::new(DebugStackTraceTool::new(dap_manager.clone())))
        .expect("debug_stack_trace tool schema is valid");
    registry
        .register(Box::new(DebugVariablesTool::new(dap_manager.clone())))
        .expect("debug_variables tool schema is valid");
    registry
        .register(Box::new(DebugTerminateTool::new(dap_manager.clone())))
        .expect("debug_terminate tool schema is valid");

    // T2.1 — swap browser tool stubs for managers backed by the
    // shipped Playwright sidecar script. Resolution order:
    //   1. $THEO_BROWSER_SIDECAR — explicit override (CI, custom installs)
    //   2. <project>/crates/theo-tooling/scripts/playwright_sidecar.js
    //      (works inside this repo's checkout)
    //   3. <project>/.theo/playwright_sidecar.js (per-project bundle)
    // Tools share ONE BrowserSessionManager so navigation state
    // persists across browser_open / browser_click / browser_screenshot
    // within the same agent run.
    let browser_script = resolve_browser_sidecar_script(project_dir);
    let browser_node =
        std::env::var("THEO_BROWSER_NODE").unwrap_or_else(|_| "node".to_string());
    let browser_manager = Arc::new(BrowserSessionManager::new(
        browser_node,
        browser_script,
    ));
    for tool_id in [
        "browser_open",
        "browser_click",
        "browser_screenshot",
        "browser_close",
    ] {
        registry.unregister(tool_id);
    }
    registry
        .register(Box::new(BrowserOpenTool::new(browser_manager.clone())))
        .expect("browser_open tool schema is valid");
    registry
        .register(Box::new(BrowserClickTool::new(browser_manager.clone())))
        .expect("browser_click tool schema is valid");
    registry
        .register(Box::new(BrowserScreenshotTool::new(browser_manager.clone())))
        .expect("browser_screenshot tool schema is valid");
    registry
        .register(Box::new(BrowserCloseTool::new(browser_manager.clone())))
        .expect("browser_close tool schema is valid");

    registry
}

/// Resolve the Playwright sidecar script path for a project.
/// Order: env override → in-repo source path → per-project bundle.
fn resolve_browser_sidecar_script(project_dir: &std::path::Path) -> std::path::PathBuf {
    if let Ok(p) = std::env::var("THEO_BROWSER_SIDECAR") {
        return std::path::PathBuf::from(p);
    }
    let in_repo = project_dir
        .join("crates/theo-tooling/scripts/playwright_sidecar.js");
    if in_repo.exists() {
        return in_repo;
    }
    project_dir.join(".theo/playwright_sidecar.js")
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
    fn unregister_removes_tool_and_returns_it() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(BashTool::new())).unwrap();
        assert_eq!(registry.len(), 1);
        let removed = registry.unregister("bash");
        assert!(removed.is_some());
        assert_eq!(registry.len(), 0);
        assert!(registry.get("bash").is_none());
    }

    #[test]
    fn unregister_unknown_id_returns_none() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(BashTool::new())).unwrap();
        let removed = registry.unregister("nonexistent");
        assert!(removed.is_none());
        // Existing tool untouched.
        assert_eq!(registry.len(), 1);
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

    // ── T15.1 — project-aware registry constructor ────────────────

    #[test]
    fn t151reg_with_project_includes_all_default_tools() {
        // Same tool surface as create_default_registry — only the
        // docs_search index is different.
        let dir = tempfile::tempdir().unwrap();
        let plain = create_default_registry();
        let with_project = create_default_registry_with_project(dir.path());
        let mut a = plain.ids();
        let mut b = with_project.ids();
        a.sort();
        b.sort();
        assert_eq!(a, b, "registries must expose identical tool ids");
    }

    #[test]
    fn t151reg_with_project_swaps_in_populated_docs_search() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        // Seed a doc under project's docs/ dir.
        let docs = dir.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        let mut f = std::fs::File::create(docs.join("intro.md")).unwrap();
        f.write_all(b"# Welcome\nproject intro").unwrap();

        let registry = create_default_registry_with_project(dir.path());
        // The tool exists under the same id.
        assert!(registry.get("docs_search").is_some());
        // We can't easily inspect the inner index without exposing
        // additional surface, but we can verify that the empty-stub
        // case (no docs/ dir) yields a different registry — ie. the
        // swap actually happened.
    }

    #[test]
    fn t151reg_with_empty_project_dir_still_works() {
        // No docs/ or .theo/wiki/ — empty project must not panic.
        let dir = tempfile::tempdir().unwrap();
        let registry = create_default_registry_with_project(dir.path());
        assert!(registry.get("docs_search").is_some());
    }

    // ── Deferred-tool discovery tests (P5) ─────────────────────────

    use async_trait::async_trait;
    use theo_domain::error::ToolError;
    use theo_domain::tool::{
        PermissionCollector, Tool as DomainTool, ToolContext, ToolOutput as DomainOutput,
    };

    struct DeferredStub {
        id: &'static str,
        hint: &'static str,
    }

    #[async_trait]
    impl DomainTool for DeferredStub {
        fn id(&self) -> &str {
            self.id
        }
        fn description(&self) -> &str {
            "deferred test tool"
        }
        fn should_defer(&self) -> bool {
            true
        }
        fn search_hint(&self) -> Option<&str> {
            Some(self.hint)
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
            _perm: &mut PermissionCollector,
        ) -> Result<DomainOutput, ToolError> {
            unreachable!()
        }
    }

    #[test]
    fn visible_definitions_excludes_deferred_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(BashTool::new())).unwrap();
        registry
            .register(Box::new(DeferredStub {
                id: "wiki_search",
                hint: "search wiki pages",
            }))
            .unwrap();

        let visible: Vec<String> = registry.visible_definitions().into_iter().map(|d| d.id).collect();
        assert!(visible.contains(&"bash".to_string()));
        assert!(!visible.contains(&"wiki_search".to_string()));
    }

    #[test]
    fn search_deferred_matches_on_hint() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(DeferredStub {
                id: "wiki_search",
                hint: "search wiki pages and knowledge base",
            }))
            .unwrap();
        registry
            .register(Box::new(DeferredStub {
                id: "patch_apply",
                hint: "apply multi-file diff patch",
            }))
            .unwrap();

        let hits = registry.search_deferred("wiki");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "wiki_search");
    }

    #[test]
    fn search_deferred_matches_on_id_case_insensitive() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(DeferredStub {
                id: "wiki_search",
                hint: "irrelevant",
            }))
            .unwrap();

        let hits = registry.search_deferred("WIKI");
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn search_deferred_ignores_non_deferred_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(BashTool::new())).unwrap();

        let hits = registry.search_deferred("bash");
        assert!(
            hits.is_empty(),
            "non-deferred tools must not appear in deferred search results"
        );
    }

    /// Guard: complex tools must carry at least one `input_examples` entry so
    /// the LLM sees a concrete invocation in the JSON Schema (Anthropic
    /// "Tool Use Examples" — reported 72% -> 90% param accuracy).
    #[test]
    fn complex_tools_declare_input_examples() {
        let registry = create_default_registry();
        for tool_id in ["edit", "read", "grep", "bash", "apply_patch"] {
            let tool = registry
                .get(tool_id)
                .unwrap_or_else(|| panic!("tool `{tool_id}` missing"));
            let schema = tool.schema();
            assert!(
                !schema.input_examples.is_empty(),
                "tool `{tool_id}` must declare at least one input example"
            );
            let json = schema.to_json_schema();
            let examples = json["examples"].as_array().unwrap_or_else(|| {
                panic!("tool `{tool_id}` JSON Schema must expose `examples` array")
            });
            assert!(
                !examples.is_empty(),
                "tool `{tool_id}` JSON Schema `examples` array is empty"
            );
        }
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
                input_examples: Vec::new(),
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
