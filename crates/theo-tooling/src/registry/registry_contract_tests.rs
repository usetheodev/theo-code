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

/// Locks the schema-vs-input_examples consistency for every
/// default-registry tool: each declared `input_examples[i]`
/// (when present) MUST be a JSON object that includes every
/// REQUIRED param the tool declares. Otherwise the JSON Schema
/// rendered to the LLM advertises an example invocation that
/// the tool itself would reject as `InvalidArgs` — a silent
/// quality bug that wastes turns when the LLM copies the
/// example.
///
/// Same lesson as commits 86165f8 / 3c0f73c (CLI surface
/// invokability): a CONTENT audit (does the example exist?)
/// doesn't substitute for a STRUCTURAL audit (does the example
/// satisfy the declared contract?). The `complex_tools_declare_
/// input_examples` test only checks the array is non-empty;
/// this test checks the array's CONTENTS are well-formed.

#[test]
fn every_tool_input_example_satisfies_declared_required_params() {
    let registry = create_default_registry();
    let mut violations: Vec<String> = Vec::new();
    for tool_id in registry.ids() {
        let tool = registry
            .get(&tool_id)
            .unwrap_or_else(|| panic!("tool `{tool_id}` missing"));
        let schema = tool.schema();
        let required: Vec<&str> = schema
            .params
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name.as_str())
            .collect();
        for (i, example) in schema.input_examples.iter().enumerate() {
            let obj = match example.as_object() {
                Some(o) => o,
                None => {
                    violations.push(format!(
                        "tool `{tool_id}` input_examples[{i}] is not a \
                         JSON object: {example:?}"
                    ));
                    continue;
                }
            };
            for req in &required {
                if !obj.contains_key(*req) {
                    violations.push(format!(
                        "tool `{tool_id}` input_examples[{i}] missing \
                         required param `{req}` (declared in schema). \
                         The LLM would copy this example and get \
                         InvalidArgs back. Update the example or \
                         relax the param to optional."
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "schema-vs-examples consistency violations:\n  {}",
        violations.join("\n  ")
    );
}

/// Snapshot guard: pins the EXACT set of default-registry tool ids
/// by name. Catches silent renames and silent removals that the
/// looser `manifest_matches_default_registry_ids` test misses (it
/// passes whenever the manifest and registry are in lockstep, even
/// if both were renamed in the same edit). The agent's wire format
/// (state/transcripts/JSONL) is keyed by tool id, so a rename
/// breaks every saved session — pinning the names by snapshot
/// makes such a change a visible decision instead of a quiet edit.
///
/// To intentionally add/remove a tool: update this list AND the
/// manifest entry in `tool_manifest.rs` AND the registry vec in
/// `create_default_registry`. The friction is the point.

#[test]
fn sota_tools_have_steering_descriptions_with_concrete_examples() {
    let registry = create_default_registry();
    // Tuple: (tool_id, sidecar_backed). Sidecar-backed tools wrap
    // an external process (Playwright / LSP / DAP / xdotool /
    // cargo-mutants / screencapture-style CLI) and MUST name a
    // fallback. Self-contained tools (`read_image`, plan tools,
    // `gen_property_test`, `docs_search`) load files / mutate
    // JSON / serve an in-memory index — no sidecar, no fallback
    // required.
    // Tuple: (tool_id, needs_fallback_wording).
    //
    // Tools that REQUIRE fallback wording in the description are
    // those the agent reads as a *decision point* — either:
    //   - A discovery tool (`*_status`) — it's the documented
    //     entry point for "should I even try this family?"
    //   - A standalone sidecar wrapper without a discovery tool
    //     (`screenshot`, `gen_mutation_test`)
    //     — the agent has nowhere else to learn the fallback.
    //
    // Operation tools inside a discovery-backed family
    // (`browser_open` / `lsp_definition` etc.)
    // delegate fallback-naming to their family's `*_status` tool
    // and to the actionable error returned by `map_session_error`
    // when the sidecar is missing — repeating the fallback in
    // every operation tool's description would burn token budget
    // for zero agent-decision benefit. Self-contained tools
    // (pure file load, JSON manipulation, in-memory index)
    // have no sidecar to fall back from.
    let sota_tools: &[(&str, bool)] = &[
        // Phase 1 — multimodal
        ("screenshot", true),  // standalone sidecar wrapper (screencapture/gnome-screenshot/import)
        ("read_image", false), // pure filesystem load — no sidecar
        // Phase 2 — browser automation (Playwright sidecar)
        ("browser_status", true), // discovery entry point
        ("browser_open", false),  // operation; delegates to browser_status
        ("browser_click", false),
        ("browser_type", false),
        ("browser_eval", false),
        ("browser_wait_for_selector", false),
        ("browser_screenshot", false),
        ("browser_close", false),
        // Phase 3 — LSP (rust-analyzer / pyright / gopls / ...)
        ("lsp_status", true), // discovery entry point
        ("lsp_definition", false), // operation; delegates to lsp_status
        ("lsp_references", false),
        ("lsp_hover", false),
        ("lsp_rename", false),
        // Phase 5 — auto-test-gen
        ("gen_property_test", false), // pure templating, no exec
        ("gen_mutation_test", true),  // standalone wrapper around cargo-mutants binary
        // Phase 6 — adaptive replanning (pure JSON manipulation)
        ("plan_failure_status", false),
        ("plan_replan", false),
        // Phase 15 — external docs RAG (in-memory index)
        ("docs_search", false),
    ];
    let mut missing: Vec<&str> = Vec::new();
    for &(tool_id, needs_fallback_wording) in sota_tools {
        let Some(tool) = registry.get(tool_id) else {
            missing.push(tool_id);
            continue;
        };
        let desc = tool.description();
        let lower = desc.to_lowercase();

        assert!(
            desc.len() >= 100,
            "description for `{tool_id}` is too short ({} chars) — \
             SOTA tools must explain when to use them",
            desc.len()
        );
        assert!(
            desc.len() <= 1500,
            "description for `{tool_id}` is too long ({} chars) — \
             keep under 1500 to preserve token budget",
            desc.len()
        );
        // Concrete invocation. The convention across all SOTA tools
        // is `Example: <tool_id>(...)` or `Examples: <tool_id>(...)`
        // when several variants are demonstrated. Lowercased lookup
        // accepts both `example:` and `examples:`.
        assert!(
            lower.contains("example:") || lower.contains("examples:"),
            "description for `{tool_id}` must include a concrete \
             `Example: <tool_id>(...)` invocation (plural \
             `Examples:` is also accepted)"
        );
        assert!(
            desc.contains(tool_id),
            "the `Example:` block in `{tool_id}` must reference the \
             tool id itself so the LLM sees a callable invocation"
        );
        // Discovery-entry-point tools (`*_status`) and standalone
        // sidecar wrappers (`screenshot`, `gen_mutation_test`)
        // MUST name a fallback in the
        // description — those are decision points the agent reads
        // before committing to a tool family. Operation tools
        // inside a discovery-backed family delegate fallback
        // naming to their `*_status` sibling (and to the
        // actionable error the dispatch layer returns when the
        // sidecar is missing) so the description token budget
        // stays focused on the operation itself.
        if needs_fallback_wording {
            assert!(
                lower.contains("fall back")
                    || lower.contains("fallback")
                    || lower.contains("instead"),
                "description for `{tool_id}` must name a fallback \
                 (`fall back to ...` is the SOTA convention; \
                 `instead` is the legacy convention) so the agent \
                 has an off-ramp when the underlying sidecar / \
                 binary is unavailable"
            );
        }
    }
    assert!(
        missing.is_empty(),
        "SOTA tool ids missing from default registry — \
         `manifest_matches_default_registry_ids` should have caught \
         this first; check tool_manifest.rs and registry/mod.rs:\n{:?}",
        missing
    );
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

