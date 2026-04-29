//! Sibling test body of `registry/mod.rs` — split per-area (T3.7 of code-hygiene-5x5).

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::*;
use crate::bash::BashTool;
use crate::grep::GrepTool;
use crate::read::ReadTool;
use theo_domain::tool::{PermissionCollector, ToolCategory, ToolContext};

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
fn default_registry_tool_id_snapshot_is_pinned() {
    let registry = create_default_registry();
    let mut got: Vec<String> = registry.ids();
    got.sort();
    let expected: Vec<&str> = vec![
        "apply_patch",
        "bash",
        "browser_click",
        "browser_close",
        "browser_eval",
        "browser_open",
        "browser_screenshot",
        "browser_status",
        "browser_type",
        "browser_wait_for_selector",
        "codebase_context",
        "docs_search",
        "edit",
        "env_info",
        "gen_mutation_test",
        "gen_property_test",
        "git_commit",
        "git_diff",
        "git_log",
        "git_status",
        "glob",
        "grep",
        "http_get",
        "http_post",
        "lsp_definition",
        "lsp_hover",
        "lsp_references",
        "lsp_rename",
        "lsp_status",
        "memory",
        "plan_advance_phase",
        "plan_create",
        "plan_failure_status",
        "plan_log",
        "plan_next_task",
        "plan_replan",
        "plan_summary",
        "plan_update_task",
        "read",
        "read_image",
        "reflect",
        "screenshot",
        "task_create",
        "task_update",
        "think",
        "webfetch",
        "write",
    ];
    let expected: Vec<String> = expected.into_iter().map(String::from).collect();
    let added: Vec<&String> = got.iter().filter(|id| !expected.contains(id)).collect();
    let removed: Vec<&String> = expected.iter().filter(|id| !got.contains(id)).collect();
    assert!(
        added.is_empty() && removed.is_empty(),
        "default-registry tool id snapshot drifted.\n  \
         added (in registry, not in snapshot — update the snapshot \
         AND tool_manifest.rs): {:?}\n  \
         removed (in snapshot, not in registry — rename, deletion, \
         or wiring regression): {:?}\n  \
         got:      {:?}\n  \
         expected: {:?}",
        added,
        removed,
        got,
        expected
    );
    assert_eq!(
        got.len(),
        expected.len(),
        "snapshot count mismatch: registry has {} ids, snapshot lists {}",
        got.len(),
        expected.len()
    );
}

/// Guard: every SOTA-introduced default-registry tool carries an
/// LLM-friendly description with a concrete `Example: <tool>(...)`
/// invocation, sized for the token budget. Sidecar-backed tools
/// (browser / LSP / DAP / OS-CLI wrappers) must additionally name
/// a fallback alternative for environments where the sidecar
/// isn't installed — `fall back` / `fallback` is the SOTA
/// convention; the original top-5 use `instead`. Self-contained
/// tools (pure file load, pure templating, in-memory index)
/// don't have a sidecar to fall back from, so the fallback
/// contract is targeted, not blanket.
///
/// Locks the description-quality contract that the LLM sees when
/// the JSON Schema is rendered. A future change that silently
/// drops the steering language or the example would make the
/// agent retry doomed calls without an off-ramp.

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

