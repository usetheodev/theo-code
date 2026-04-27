#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExposure {
    DefaultRegistry,
    MetaTool,
    ExperimentalModule,
    InternalModule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Implemented,
    Partial,
    Stub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolManifestEntry {
    pub id: &'static str,
    pub exposure: ToolExposure,
    pub status: ToolStatus,
    pub owner: &'static str,
    pub notes: &'static str,
}

pub const TOOL_MANIFEST: &[ToolManifestEntry] = &[
    ToolManifestEntry {
        id: "apply_patch",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in file patch tool in the default registry.",
    },
    ToolManifestEntry {
        id: "bash",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in shell tool; sandboxed when an executor is available.",
    },
    ToolManifestEntry {
        id: "batch",
        exposure: ToolExposure::MetaTool,
        status: ToolStatus::Implemented,
        owner: "theo-agent-runtime",
        notes: "Meta-tool injected by tool_bridge, not registered in create_default_registry().",
    },
    ToolManifestEntry {
        id: "codebase_context",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "On-demand codebase structure and context map.",
    },
    ToolManifestEntry {
        id: "codesearch",
        exposure: ToolExposure::ExperimentalModule,
        status: ToolStatus::Stub,
        owner: "theo-tooling",
        notes: "Module exists but execute() returns not implemented and it is not in the default registry.",
    },
    ToolManifestEntry {
        id: "done",
        exposure: ToolExposure::MetaTool,
        status: ToolStatus::Implemented,
        owner: "theo-agent-runtime",
        notes: "Completion meta-tool injected by tool_bridge.",
    },
    ToolManifestEntry {
        id: "edit",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in file edit tool in the default registry.",
    },
    ToolManifestEntry {
        id: "env_info",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in environment inspection tool in the default registry.",
    },
    ToolManifestEntry {
        id: "git_commit",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in git tool in the default registry.",
    },
    ToolManifestEntry {
        id: "git_diff",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in git tool in the default registry.",
    },
    ToolManifestEntry {
        id: "git_log",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in git tool in the default registry.",
    },
    ToolManifestEntry {
        id: "git_status",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in git tool in the default registry.",
    },
    ToolManifestEntry {
        id: "glob",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in glob search in the default registry.",
    },
    ToolManifestEntry {
        id: "grep",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in grep search in the default registry.",
    },
    ToolManifestEntry {
        id: "http_get",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in HTTP tool in the default registry.",
    },
    ToolManifestEntry {
        id: "http_post",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in HTTP tool in the default registry.",
    },
    ToolManifestEntry {
        id: "invalid",
        exposure: ToolExposure::InternalModule,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Internal placeholder/error helper module; not in the default registry.",
    },
    ToolManifestEntry {
        id: "ls",
        exposure: ToolExposure::ExperimentalModule,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Module exists but is not currently in the default registry.",
    },
    ToolManifestEntry {
        id: "lsp",
        exposure: ToolExposure::ExperimentalModule,
        status: ToolStatus::Stub,
        owner: "theo-tooling",
        notes: "Legacy stub. Replaced by `lsp_definition` (and forthcoming lsp_references / lsp_hover / lsp_rename) in the project-aware registry constructor.",
    },
    ToolManifestEntry {
        id: "lsp_definition",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T3.1 — LSP `textDocument/definition` via LspSessionManager. Spawns + initialises native servers (rust-analyzer, pyright, gopls, tsserver, clangd). Wired by create_default_registry_with_project; missing servers surface as actionable execution errors.",
    },
    ToolManifestEntry {
        id: "lsp_references",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T3.1 — LSP `textDocument/references` via LspSessionManager. Optional `include_declaration` flag. Dedupes overlapping ranges. Shares the spawned servers with lsp_definition.",
    },
    ToolManifestEntry {
        id: "lsp_hover",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T3.1 — LSP `textDocument/hover` via LspSessionManager. Normalises MarkupContent / MarkedString / MarkedString[] into a flat doc string. Cheaper than reading whole files when the agent only needs a signature.",
    },
    ToolManifestEntry {
        id: "lsp_rename",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T3.1 — LSP `textDocument/rename` PREVIEW via LspSessionManager. Returns the WorkspaceEdit (file → range → newText) for agent review. Does NOT write files — agent applies via `edit`/`apply_patch` after inspecting. Supports both LSP 3.16+ documentChanges and legacy changes shapes.",
    },
    ToolManifestEntry {
        id: "debug_launch",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T13.1 — DAP `launch` via DapSessionManager. Spawns the right adapter for `language` (lldb-vscode, debugpy, dlv dap, js-debug-adapter, java-debug-server). Caller-keyed session_id is reused across debug_*. stop_on_entry defaults to true so set_breakpoint sees the program before execution.",
    },
    ToolManifestEntry {
        id: "debug_set_breakpoint",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T13.1 — DAP `setBreakpoints`. REPLACES the breakpoint set in a file (DAP semantics — pass empty array to clear). Validates 1-based lines (rejects 0 — common bug). Reports per-line verified flag.",
    },
    ToolManifestEntry {
        id: "debug_continue",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T13.1 — DAP `continue`. Optional `thread_id` to resume one thread; default resumes all. Reports `all_threads_continued` from the response.",
    },
    ToolManifestEntry {
        id: "debug_step",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T13.1 — DAP `next`/`stepIn`/`stepOut` via `kind` arg (`over`/`in`/`out`). thread_id REQUIRED (unlike continue) — get from the most recent `stopped` event.",
    },
    ToolManifestEntry {
        id: "debug_eval",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T13.1 — DAP `evaluate` for inspecting expressions in the debuggee context. Optional `frame_id` (from debug_stack_trace) and `context` (`watch`/`repl`/`hover`). Returns result + type + variablesReference for drill-down.",
    },
    ToolManifestEntry {
        id: "debug_stack_trace",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T13.1 — DAP `stackTrace`. Returns frames {id, name, source.path, line, column}. Use frame.id as `frame_id` in debug_eval / future debug_scopes. Optional `start_frame` + `levels` for paginating deep stacks.",
    },
    ToolManifestEntry {
        id: "debug_variables",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T13.1 — DAP `variables`. Drills into a variablesReference (from debug_eval / future debug_scopes). Each child carries its OWN variables_reference for recursive descent. Rejects reference == 0 with explanation (scalar values are non-drillable). Optional `start`/`count`/`filter` for paginating large containers.",
    },
    ToolManifestEntry {
        id: "browser_open",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T2.1 — browser_open via BrowserSessionManager + Playwright sidecar. Lazy-spawn on first call. Returns final_url + title. Default registry uses a sentinel script path; create_default_registry_with_project resolves the real script via $THEO_BROWSER_SIDECAR or in-repo fallback.",
    },
    ToolManifestEntry {
        id: "browser_click",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T2.1 — browser_click. Playwright selector engine (CSS / text= / role=). Requires open page (browser_open first).",
    },
    ToolManifestEntry {
        id: "browser_screenshot",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T2.1 — browser_screenshot. PNG (default) or JPEG; full_page toggle. Returns base64 image in metadata for vision-block propagation.",
    },
    ToolManifestEntry {
        id: "browser_type",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T2.1 — browser_type via Playwright's `page.fill`. Replaces field value (NOT append). Empty text clears the field — accepted as the standard idiom.",
    },
    ToolManifestEntry {
        id: "browser_eval",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T2.1 — browser_eval runs JS in page context, returns JSON-serialized value. Useful for extracting hydrated React state, localStorage, etc. DOM nodes / functions return as null.",
    },
    ToolManifestEntry {
        id: "browser_wait_for_selector",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T2.1 — browser_wait_for_selector. Default 5000ms timeout; clamped at 60000ms (1 min) so a stuck page can't hang the agent indefinitely.",
    },
    ToolManifestEntry {
        id: "browser_close",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T2.1 — browser_close. Idempotent: closing without an active session returns was_active=false.",
    },
    ToolManifestEntry {
        id: "debug_terminate",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T13.1 — drop the cached DapClient (kills adapter via kill_on_drop). Idempotent: terminating an unknown session_id returns success with `was_active=false`. Pair with `debug_launch` to avoid leaking adapter processes.",
    },
    ToolManifestEntry {
        id: "memory",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in persistent memory tool in the default registry.",
    },
    ToolManifestEntry {
        id: "multiedit",
        exposure: ToolExposure::ExperimentalModule,
        status: ToolStatus::Stub,
        owner: "theo-tooling",
        notes: "Module exists but execute() returns not implemented and it is not in the default registry.",
    },
    ToolManifestEntry {
        id: "plan_advance_phase",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "SOTA planning — advance current phase to next.",
    },
    ToolManifestEntry {
        id: "plan_create",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "SOTA planning — author a schema-validated Plan and persist to .theo/plans/plan.json.",
    },
    ToolManifestEntry {
        id: "plan_exit",
        exposure: ToolExposure::ExperimentalModule,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Experimental module; not currently in the default registry.",
    },
    ToolManifestEntry {
        id: "plan_log",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "SOTA planning — append finding/error/decision/requirement/resource entries.",
    },
    ToolManifestEntry {
        id: "plan_next_task",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "SOTA planning — return next actionable task via topological sort.",
    },
    ToolManifestEntry {
        id: "plan_replan",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T6.1 — apply a typed PlanPatch (AddTask/RemoveTask/EditTask/ReorderDeps/SkipTask) atomically.",
    },
    ToolManifestEntry {
        id: "gen_property_test",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T5.1 — generate proptest scaffolding for a Rust function (no execution).",
    },
    ToolManifestEntry {
        id: "gen_mutation_test",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T5.2 — invoke cargo-mutants and report surviving mutations (subprocess).",
    },
    ToolManifestEntry {
        id: "read_image",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T1.2 — load PNG/JPEG/WebP/GIF as base64 vision block; magic-byte MIME detection; 20 MiB cap.",
    },
    ToolManifestEntry {
        id: "docs_search",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "T15.1 — in-memory inverted index + DocSource trait; ships StaticDocSource (test corpus) and MarkdownDirSource (offline filesystem-backed scanner for .md docs); crates.io/MDN/npm network fetchers still deferred.",
    },
    ToolManifestEntry {
        id: "plan_summary",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "SOTA planning — render Plan::to_markdown() for prompt injection / UI.",
    },
    ToolManifestEntry {
        id: "plan_update_task",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "SOTA planning — change a task's status and outcome.",
    },
    ToolManifestEntry {
        id: "question",
        exposure: ToolExposure::ExperimentalModule,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Module exists but is not currently in the default registry.",
    },
    ToolManifestEntry {
        id: "read",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in file read tool in the default registry.",
    },
    ToolManifestEntry {
        id: "reflect",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in cognitive tool in the default registry.",
    },
    ToolManifestEntry {
        id: "skill",
        exposure: ToolExposure::MetaTool,
        status: ToolStatus::Implemented,
        owner: "theo-agent-runtime",
        notes: "Meta-tool injected by tool_bridge; skill module exists separately but is not in the default registry.",
    },
    ToolManifestEntry {
        id: "subagent",
        exposure: ToolExposure::MetaTool,
        status: ToolStatus::Implemented,
        owner: "theo-agent-runtime",
        notes: "Delegation meta-tool injected by tool_bridge.",
    },
    ToolManifestEntry {
        id: "subagent_parallel",
        exposure: ToolExposure::MetaTool,
        status: ToolStatus::Implemented,
        owner: "theo-agent-runtime",
        notes: "Parallel delegation meta-tool injected by tool_bridge.",
    },
    ToolManifestEntry {
        id: "task",
        exposure: ToolExposure::ExperimentalModule,
        status: ToolStatus::Partial,
        owner: "theo-tooling",
        notes: "Module exists but execute() only returns placeholder output; not in the default registry.",
    },
    ToolManifestEntry {
        id: "think",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in cognitive tool in the default registry.",
    },
    ToolManifestEntry {
        id: "task_create",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in task tracking tool in the default registry.",
    },
    ToolManifestEntry {
        id: "task_update",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in task tracking tool in the default registry.",
    },
    ToolManifestEntry {
        id: "webfetch",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in URL fetch tool in the default registry.",
    },
    ToolManifestEntry {
        id: "websearch",
        exposure: ToolExposure::ExperimentalModule,
        status: ToolStatus::Stub,
        owner: "theo-tooling",
        notes: "Module exists but execute() returns not implemented and it is not in the default registry.",
    },
    ToolManifestEntry {
        id: "wiki_tool",
        exposure: ToolExposure::InternalModule,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Internal module; not currently in the default registry.",
    },
    ToolManifestEntry {
        id: "write",
        exposure: ToolExposure::DefaultRegistry,
        status: ToolStatus::Implemented,
        owner: "theo-tooling",
        notes: "Built-in file write tool in the default registry.",
    },
];

#[must_use]
pub fn tool_manifest() -> &'static [ToolManifestEntry] {
    TOOL_MANIFEST
}

#[cfg(test)]
mod tests {
    use super::{ToolExposure, ToolStatus, tool_manifest};
    use crate::registry::create_default_registry;
    use std::collections::BTreeSet;

    #[test]
    fn manifest_matches_default_registry_ids() {
        let registry = create_default_registry();
        let registry_ids = registry.ids().into_iter().collect::<BTreeSet<_>>();
        let manifest_ids = tool_manifest()
            .iter()
            .filter(|entry| entry.exposure == ToolExposure::DefaultRegistry)
            .map(|entry| entry.id.to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(registry_ids, manifest_ids);
    }

    #[test]
    fn manifest_has_unique_ids() {
        let ids = tool_manifest()
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        let unique = ids.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), unique.len());
    }

    #[test]
    fn manifest_marks_known_stubs() {
        for id in ["websearch", "codesearch", "lsp", "multiedit"] {
            let entry = tool_manifest()
                .iter()
                .find(|entry| entry.id == id)
                .expect("stub should exist in manifest");
            assert_eq!(entry.status, ToolStatus::Stub);
        }
    }
}
