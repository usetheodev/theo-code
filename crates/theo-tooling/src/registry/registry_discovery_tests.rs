//! Sibling test body of `registry/mod.rs` — split per-area (T3.7 of code-hygiene-5x5).

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::*;
use super::registry_test_helpers::DeferredStub;
use crate::bash::BashTool;
use crate::grep::GrepTool;
use crate::read::ReadTool;
use theo_domain::tool::{PermissionCollector, ToolCategory, ToolContext};

#[tokio::test]
async fn discovery_tool_family_lsp_browser_share_zero_arg_search_contract() {
    let registry = create_default_registry();
    let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));
    for id in ["lsp_status", "browser_status"] {
        let tool = registry
            .get(id)
            .unwrap_or_else(|| panic!("`{id}` missing from default registry"));
        // (1) Zero-arg + at least one example so the LLM sees an
        //     invocation in the JSON Schema.
        let schema = tool.schema();
        assert!(schema.params.is_empty(), "`{id}` must take zero args");
        assert!(
            !schema.input_examples.is_empty(),
            "`{id}` must declare at least one input example"
        );
        schema
            .validate()
            .unwrap_or_else(|e| panic!("`{id}` schema invalid: {e}"));
        // (2) Search category — these are read-only discovery tools,
        //     not file-ops or network.
        assert_eq!(
            tool.category(),
            ToolCategory::Search,
            "`{id}` must declare ToolCategory::Search"
        );
        // (3) Default registry stub MUST execute successfully (no
        //     ToolError) so the agent always gets actionable
        //     output even when the underlying sidecar isn't
        //     installed.
        let mut perms = PermissionCollector::new();
        let out = tool
            .execute(serde_json::json!({}), &ctx, &mut perms)
            .await
            .unwrap_or_else(|e| panic!("`{id}` execute({{}}) failed: {e:?}"));
        // (4) Metadata `type` discriminator MUST equal the tool id
        //     so JSONL trajectory consumers can filter on a stable
        //     key.
        assert_eq!(
            out.metadata["type"],
            serde_json::json!(id),
            "`{id}` metadata.type must equal the tool id"
        );
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

