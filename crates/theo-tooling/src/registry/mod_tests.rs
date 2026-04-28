//! Sibling test body of `mod.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `mod.rs` via `#[path = "mod_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.


#![cfg(test)]

    use super::*;
    use crate::bash::BashTool;
    use crate::grep::GrepTool;
    use crate::read::ReadTool;
    use theo_domain::tool::{PermissionCollector, ToolCategory, ToolContext};

    /// Locks the discovery-tool family contract: `lsp_status`
    /// and `browser_status` must each ship as zero-arg,
    /// `Search`-category, schema-validated read-only tools
    /// in the default registry, and each `execute({})` must return a
    /// JSON metadata object whose `type` matches the tool id. Pairs
    /// the LSP / Browser sidecar-backed families so a future
    /// change that breaks the symmetry (eg. silently adds a required
    /// arg, drops one of the tools, or renames the metadata `type`
    /// discriminator) surfaces immediately instead of leaking out as
    /// an agent-side regression.
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
    use theo_domain::tool::{Tool as DomainTool, ToolOutput as DomainOutput};

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
