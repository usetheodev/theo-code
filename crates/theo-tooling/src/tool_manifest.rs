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
        notes: "Experimental module; execute() returns not implemented and it is not in the default registry.",
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
