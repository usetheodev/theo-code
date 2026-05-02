//! Default tool registry builders.
//!
//! Extracted from `registry/mod.rs` during T1.5 of god-files-2026-07-23-plan.md
//! (ADR D5). This file holds:
//!   - `create_default_registry` — register every built-in tool stub
//!   - `create_default_registry_with_project` — swap stubs for project-aware
//!     LSP/Browser/DocsSearch managers
//!   - browser sidecar resolver / materializer helpers
//!
//! `registry/mod.rs` keeps the `ToolRegistry` struct + impls and
//! `register_plugin_tools`.

use super::ToolRegistry;
use theo_domain::tool::Tool;

/// Create a registry with all built-in tools.
///
/// Panics if any built-in tool has an invalid schema (programming error).
pub fn create_default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();
    tools.push(build_bash_tool_with_sandbox());
    tools.extend(file_ops_tools());
    tools.extend(cognitive_tools());
    tools.extend(plan_tools());
    tools.extend(autotest_tools());
    tools.extend(multimodal_tools());
    tools.extend(docs_tools());
    tools.extend(lsp_default_stub_tools());
    tools.extend(browser_default_stub_tools());
    tools.extend(plugin_tools());
    register_all(&mut registry, tools);
    registry
}

/// Activate sandbox for BashTool — bwrap > landlock > noop cascade.
/// Falls back to no-sandbox if executor creation fails.
fn build_bash_tool_with_sandbox() -> Box<dyn Tool> {
    use crate::bash::BashTool;
    let mut sandbox_config = theo_domain::sandbox::SandboxConfig::default();
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
    sandbox_config.process.allowed_env_vars.extend(vec![
        "CARGO_HOME".to_string(),
        "RUSTUP_HOME".to_string(),
        "RUSTFLAGS".to_string(),
        "CARGO_TARGET_DIR".to_string(),
    ]);
    match crate::sandbox::executor::create_executor(&sandbox_config) {
        Ok(executor) => Box::new(BashTool::with_sandbox(
            std::sync::Arc::from(executor),
            sandbox_config,
        )) as Box<dyn Tool>,
        Err(_) => {
            eprintln!("[theo] Warning: sandbox unavailable — BashTool running without isolation");
            Box::new(BashTool::new()) as Box<dyn Tool>
        }
    }
}

fn file_ops_tools() -> Vec<Box<dyn Tool>> {
    use crate::apply_patch::ApplyPatchTool;
    use crate::edit::EditTool;
    use crate::glob::GlobTool;
    use crate::grep::GrepTool;
    use crate::read::ReadTool;
    use crate::webfetch::WebFetchTool;
    use crate::write::WriteTool;
    vec![
        Box::new(ReadTool::new()),
        Box::new(WriteTool::new()),
        Box::new(EditTool::new()),
        Box::new(GrepTool::new()),
        Box::new(GlobTool::new()),
        Box::new(ApplyPatchTool::new()),
        Box::new(WebFetchTool::new()),
    ]
}

fn cognitive_tools() -> Vec<Box<dyn Tool>> {
    use crate::memory::MemoryTool;
    use crate::reflect::ReflectTool;
    use crate::think::ThinkTool;
    use crate::todo::{TaskCreateTool, TaskUpdateTool};
    vec![
        Box::new(ThinkTool::new()),
        Box::new(ReflectTool::new()),
        Box::new(MemoryTool::new()),
        Box::new(TaskCreateTool::new()),
        Box::new(TaskUpdateTool::new()),
    ]
}

fn plan_tools() -> Vec<Box<dyn Tool>> {
    use crate::plan::{
        AdvancePhaseTool, CreatePlanTool, GetNextTaskTool, GetPlanSummaryTool, LogEntryTool,
        PlanFailureStatusTool, ReplanTool, UpdateTaskTool,
    };
    vec![
        Box::new(CreatePlanTool::new()),
        Box::new(UpdateTaskTool::new()),
        Box::new(AdvancePhaseTool::new()),
        Box::new(LogEntryTool::new()),
        Box::new(GetPlanSummaryTool::new()),
        Box::new(GetNextTaskTool::new()),
        Box::new(ReplanTool::new()),
        Box::new(PlanFailureStatusTool::new()),
    ]
}

fn autotest_tools() -> Vec<Box<dyn Tool>> {
    use crate::test_gen::{GenMutationTestTool, GenPropertyTestTool};
    vec![
        Box::new(GenPropertyTestTool::new()),
        Box::new(GenMutationTestTool::new()),
    ]
}

fn multimodal_tools() -> Vec<Box<dyn Tool>> {
    use crate::read_image::ReadImageTool;
    use crate::screenshot::ScreenshotTool;
    vec![
        Box::new(ReadImageTool::new()),
        Box::new(ScreenshotTool::new()),
    ]
}

fn docs_tools() -> Vec<Box<dyn Tool>> {
    use crate::docs_search::DocsSearchTool;
    vec![Box::new(DocsSearchTool::new())]
}

/// Default LSP stubs — empty catalogue, surfaced as the same actionable
/// error for every call until `create_default_registry_with_project`
/// swaps in real session managers.
fn lsp_default_stub_tools() -> Vec<Box<dyn Tool>> {
    use crate::lsp::{
        LspDefinitionTool, LspHoverTool, LspReferencesTool, LspRenameTool, LspSessionManager,
        LspStatusTool,
    };
    let empty = || {
        std::sync::Arc::new(LspSessionManager::from_catalogue(
            std::collections::HashMap::new(),
        ))
    };
    vec![
        Box::new(LspStatusTool::new(empty())),
        Box::new(LspDefinitionTool::new(empty())),
        Box::new(LspReferencesTool::new(empty())),
        Box::new(LspHoverTool::new(empty())),
        Box::new(LspRenameTool::new(empty())),
    ]
}

/// Default browser stubs — pointed at a non-existent script path so every
/// call surfaces the actionable "missing script" error until the
/// project-aware constructor swaps in real managers.
fn browser_default_stub_tools() -> Vec<Box<dyn Tool>> {
    use crate::browser::{
        BrowserClickTool, BrowserCloseTool, BrowserEvalTool, BrowserOpenTool,
        BrowserScreenshotTool, BrowserSessionManager, BrowserStatusTool, BrowserTypeTool,
        BrowserWaitForSelectorTool,
    };
    let stub = || {
        std::sync::Arc::new(BrowserSessionManager::new(
            "node",
            "/__theo_no_browser__/playwright_sidecar.js",
        ))
    };
    vec![
        Box::new(BrowserStatusTool::new(stub())),
        Box::new(BrowserOpenTool::new(stub())),
        Box::new(BrowserClickTool::new(stub())),
        Box::new(BrowserScreenshotTool::new(stub())),
        Box::new(BrowserTypeTool::new(stub())),
        Box::new(BrowserEvalTool::new(stub())),
        Box::new(BrowserWaitForSelectorTool::new(stub())),
        Box::new(BrowserCloseTool::new(stub())),
    ]
}

fn plugin_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(crate::git::GitStatusTool),
        Box::new(crate::git::GitDiffTool),
        Box::new(crate::git::GitLogTool),
        Box::new(crate::git::GitCommitTool),
        Box::new(crate::env_info::EnvInfoTool),
        Box::new(crate::http_client::HttpGetTool),
        Box::new(crate::http_client::HttpPostTool),
        Box::new(crate::codebase_context::CodebaseContextTool::new()),
    ]
}

fn register_all(registry: &mut ToolRegistry, tools: Vec<Box<dyn Tool>>) {
    for tool in tools {
        let id = tool.id().to_string();
        registry
            .register(tool)
            .unwrap_or_else(|e| panic!("Built-in tool '{id}' has invalid schema: {e}"));
    }
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
        BrowserClickTool, BrowserCloseTool, BrowserEvalTool, BrowserOpenTool,
        BrowserScreenshotTool, BrowserSessionManager, BrowserStatusTool, BrowserTypeTool,
        BrowserWaitForSelectorTool,
    };
    use crate::docs_search::{DocsSearchTool, bootstrap_docs_index};
    use crate::lsp::{
        LspDefinitionTool, LspHoverTool, LspReferencesTool, LspRenameTool,
        LspSessionManager, LspStatusTool,
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
    for tool_id in [
        "lsp_status",
        "lsp_definition",
        "lsp_references",
        "lsp_hover",
        "lsp_rename",
    ] {
        registry.unregister(tool_id);
    }
    registry
        .register(Box::new(LspStatusTool::new(lsp_manager.clone())))
        .expect("lsp_status tool schema is valid");
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
        "browser_status",
        "browser_open",
        "browser_click",
        "browser_type",
        "browser_eval",
        "browser_wait_for_selector",
        "browser_screenshot",
        "browser_close",
    ] {
        registry.unregister(tool_id);
    }
    registry
        .register(Box::new(BrowserStatusTool::new(browser_manager.clone())))
        .expect("browser_status tool schema is valid");
    registry
        .register(Box::new(BrowserOpenTool::new(browser_manager.clone())))
        .expect("browser_open tool schema is valid");
    registry
        .register(Box::new(BrowserClickTool::new(browser_manager.clone())))
        .expect("browser_click tool schema is valid");
    registry
        .register(Box::new(BrowserTypeTool::new(browser_manager.clone())))
        .expect("browser_type tool schema is valid");
    registry
        .register(Box::new(BrowserEvalTool::new(browser_manager.clone())))
        .expect("browser_eval tool schema is valid");
    registry
        .register(Box::new(BrowserWaitForSelectorTool::new(browser_manager.clone())))
        .expect("browser_wait_for_selector tool schema is valid");
    registry
        .register(Box::new(BrowserScreenshotTool::new(browser_manager.clone())))
        .expect("browser_screenshot tool schema is valid");
    registry
        .register(Box::new(BrowserCloseTool::new(browser_manager.clone())))
        .expect("browser_close tool schema is valid");

    registry
}

/// Sidecar script source — embedded at compile time so the binary is
/// self-contained. Materialised to disk by
/// [`resolve_browser_sidecar_script`] on first use.
///
/// Bug 2026-04-27 (dogfood F3): the previous resolver only succeeded
/// when the user ran `theo` from inside the source checkout; external
/// projects had to set `THEO_BROWSER_SIDECAR` or hand-copy the script
/// into `<project>/.theo/playwright_sidecar.js`. Embedding makes the
/// `browser_*` family work out of the box (after the operator runs
/// `npx playwright install chromium` plus the OS deps).
pub const EMBEDDED_BROWSER_SIDECAR: &str =
    include_str!("../../scripts/playwright_sidecar.js");

/// Resolve the Playwright sidecar script path for a project.
///
/// Resolution order (first match wins):
///   1. `THEO_BROWSER_SIDECAR` env var (operator override)
///   2. `<project>/.theo/playwright_sidecar.js` (per-project, persists)
///   3. `<project>/crates/theo-tooling/scripts/playwright_sidecar.js`
///      (developer running theo from the source checkout)
///   4. Materialised copy of the embedded script under
///      `~/.cache/theo/playwright_sidecar.js` (default for end users —
///      written on first call, idempotent)
fn resolve_browser_sidecar_script(project_dir: &std::path::Path) -> std::path::PathBuf {
    if let Ok(p) = std::env::var("THEO_BROWSER_SIDECAR") {
        return std::path::PathBuf::from(p);
    }
    let per_project = project_dir.join(".theo/playwright_sidecar.js");
    if per_project.exists() {
        return per_project;
    }
    let in_repo = project_dir.join("crates/theo-tooling/scripts/playwright_sidecar.js");
    if in_repo.exists() {
        return in_repo;
    }
    materialize_embedded_browser_sidecar().unwrap_or_else(|_| {
        // Materialisation can only fail when the cache directory cannot
        // be created (read-only home, sandbox, etc.). Fall back to the
        // historical per-project path so the operator still gets a clear
        // "missing sidecar" error pointing at a writable location.
        project_dir.join(".theo/playwright_sidecar.js")
    })
}

/// Write the embedded sidecar script to `~/.cache/theo/playwright_sidecar.js`
/// when missing or out-of-date and return its path. Idempotent: rewrites
/// only when the on-disk content differs from the embedded source so
/// upgrades land automatically when the binary is reinstalled.
fn materialize_embedded_browser_sidecar() -> std::io::Result<std::path::PathBuf> {
    let cache_dir = browser_sidecar_cache_dir()?;
    std::fs::create_dir_all(&cache_dir)?;
    let path = cache_dir.join("playwright_sidecar.js");
    let needs_write = match std::fs::read_to_string(&path) {
        Ok(existing) => existing != EMBEDDED_BROWSER_SIDECAR,
        Err(_) => true,
    };
    if needs_write {
        std::fs::write(&path, EMBEDDED_BROWSER_SIDECAR)?;
    }
    Ok(path)
}

/// Cache dir for binary-shipped scripts. Honors `XDG_CACHE_HOME` and
/// falls back to `$HOME/.cache/theo`.
fn browser_sidecar_cache_dir() -> std::io::Result<std::path::PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME")
        && !xdg.is_empty()
    {
        return Ok(std::path::PathBuf::from(xdg).join("theo"));
    }
    let home = std::env::var("HOME").map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "neither XDG_CACHE_HOME nor HOME is set; cannot locate cache dir",
        )
    })?;
    Ok(std::path::PathBuf::from(home).join(".cache/theo"))
}
