//! Built-in `AgentSpec` definitions — single source of truth for the 4 default sub-agents.
//!
//! These specs replace the hardcoded `SubAgentRole` enum's match arms.
//! Each spec carries its own system prompt, capability set, max iterations, and timeout.
//!
//! Track A — Phase 1.

use std::collections::BTreeSet;

use theo_domain::agent_spec::{AgentSpec, AgentSpecSource};
use theo_domain::capability::{AllowedTools, CapabilitySet};

/// Returns all four built-in agent specs in canonical order:
/// `explorer`, `implementer`, `verifier`, `reviewer`.
///
/// The order is preserved by the registry (IndexMap) and matters for
/// the deterministic `delegate_task` tool description.
pub fn all_builtins() -> Vec<AgentSpec> {
    vec![explorer(), implementer(), verifier(), reviewer()]
}

/// Read-only research and analysis. Cannot edit files.
pub fn explorer() -> AgentSpec {
    AgentSpec {
        name: "explorer".to_string(),
        description:
            "Explores and understands code without modifying it. Use for context gathering, \
             dependency analysis, and locating relevant files."
                .to_string(),
        system_prompt:
            "You are a code explorer sub-agent. Your job is to READ and UNDERSTAND code, \
             never edit it.\n\
             - Use read, grep, glob, bash(ls/find) to explore the codebase.\n\
             - Use think to organize your findings.\n\
             - Use memory to save important facts.\n\
             - Report your findings clearly and concisely.\n\
             - NEVER use edit, write, or apply_patch.\n\n\
             When you call `done`, structure your summary as:\n\n\
             RESULT: <one-line summary of what was accomplished>\n\
             FILES: <comma-separated list of files examined, or \"none\">\n\
             CONFIDENCE: <HIGH | MEDIUM | LOW>\n\
             DETAILS:\n\
             <detailed findings, one per line>"
                .to_string(),
        capability_set: CapabilitySet::read_only(),
        model_override: None,
        max_iterations: 30,
        timeout_secs: 300,
        source: AgentSpecSource::Builtin,
        output_format: None,
        output_format_strict: None,
        mcp_servers: Vec::new(),
        isolation: None,
        isolation_base_branch: None,
    }
}

/// Full implementation capability. Can read, write, edit, run commands.
pub fn implementer() -> AgentSpec {
    AgentSpec {
        name: "implementer".to_string(),
        description:
            "Makes code changes — creates files, edits, refactors. Use when concrete \
             modifications are needed."
                .to_string(),
        system_prompt:
            "You are an implementer sub-agent. Your job is to MAKE CODE CHANGES.\n\
             - Read files to understand context, then make targeted edits.\n\
             - Use think to plan your approach before editing.\n\
             - Validate your changes with bash (cargo check, tests).\n\
             - Call done when the implementation is complete.\n\n\
             When you call `done`, structure your summary as:\n\n\
             RESULT: <one-line summary of what was changed>\n\
             FILES: <comma-separated list of files modified, or \"none\">\n\
             CONFIDENCE: <HIGH | MEDIUM | LOW>\n\
             DETAILS:\n\
             <changes per file, one per line>"
                .to_string(),
        capability_set: CapabilitySet::unrestricted(),
        model_override: None,
        max_iterations: 100,
        timeout_secs: 600,
        source: AgentSpecSource::Builtin,
        output_format: None,
        output_format_strict: None,
        mcp_servers: Vec::new(),
        isolation: None,
        isolation_base_branch: None,
    }
}

/// Validation and testing. Can read and run commands, but not edit.
pub fn verifier() -> AgentSpec {
    let mut denied = BTreeSet::new();
    denied.insert("edit".to_string());
    denied.insert("write".to_string());
    denied.insert("apply_patch".to_string());

    AgentSpec {
        name: "verifier".to_string(),
        description:
            "Runs tests, validates correctness, executes cargo commands. Cannot edit files."
                .to_string(),
        system_prompt:
            "You are a verifier sub-agent. Your job is to VALIDATE code, never edit it.\n\
             - Run tests: cargo test, cargo check, cargo clippy.\n\
             - Read code to verify correctness.\n\
             - Use reflect to assess quality and confidence.\n\
             - Report issues found clearly.\n\
             - NEVER use edit, write, or apply_patch.\n\n\
             When you call `done`, structure your summary as:\n\n\
             RESULT: <PASS | FAIL | PARTIAL — one-line verdict>\n\
             FILES: <comma-separated list of files inspected, or \"none\">\n\
             CONFIDENCE: <HIGH | MEDIUM | LOW>\n\
             DETAILS:\n\
             <test results, errors, warnings, one per line>"
                .to_string(),
        capability_set: CapabilitySet {
            allowed_tools: AllowedTools::All,
            denied_tools: denied,
            allowed_categories: BTreeSet::new(),
            max_file_size_bytes: u64::MAX,
            allowed_paths: Vec::new(),
            network_access: false,
        },
        model_override: None,
        max_iterations: 20,
        timeout_secs: 600,
        source: AgentSpecSource::Builtin,
        output_format: None,
        output_format_strict: None,
        mcp_servers: Vec::new(),
        isolation: None,
        isolation_base_branch: None,
    }
}

/// Code review and quality analysis. Read-only with reasoning tools.
pub fn reviewer() -> AgentSpec {
    AgentSpec {
        name: "reviewer".to_string(),
        description:
            "Reviews code for bugs, anti-patterns, and improvements. Read-only analysis."
                .to_string(),
        system_prompt:
            "You are a code reviewer sub-agent. Your job is to ANALYZE quality.\n\
             - Read code carefully for bugs, anti-patterns, and improvements.\n\
             - Use think to structure your review.\n\
             - Use reflect to assess overall code quality.\n\
             - Report findings with severity (critical/major/minor/suggestion).\n\
             - NEVER use edit, write, or apply_patch.\n\n\
             When you call `done`, structure your summary as:\n\n\
             RESULT: <one-line verdict — overall code quality>\n\
             FILES: <comma-separated list of files reviewed>\n\
             CONFIDENCE: <HIGH | MEDIUM | LOW>\n\
             DETAILS:\n\
             <findings with [SEVERITY] file:line — message, one per line>"
                .to_string(),
        capability_set: CapabilitySet::read_only(),
        model_override: None,
        max_iterations: 20,
        timeout_secs: 300,
        source: AgentSpecSource::Builtin,
        output_format: None,
        output_format_strict: None,
        mcp_servers: Vec::new(),
        isolation: None,
        isolation_base_branch: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_domain::tool::ToolCategory;

    #[test]
    fn all_builtins_returns_4_specs() {
        let specs = all_builtins();
        assert_eq!(specs.len(), 4);
    }

    #[test]
    fn all_builtins_canonical_order() {
        let specs = all_builtins();
        let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["explorer", "implementer", "verifier", "reviewer"]);
    }

    #[test]
    fn all_builtins_have_builtin_source() {
        for spec in all_builtins() {
            assert_eq!(spec.source, AgentSpecSource::Builtin);
        }
    }

    #[test]
    fn all_builtins_have_non_empty_descriptions() {
        for spec in all_builtins() {
            assert!(!spec.description.is_empty());
            assert!(!spec.system_prompt.is_empty());
        }
    }

    #[test]
    fn builtin_explorer_has_read_only_capabilities() {
        let spec = explorer();
        assert!(
            !spec
                .capability_set
                .can_use_tool("edit", ToolCategory::FileOps)
        );
        assert!(
            !spec
                .capability_set
                .can_use_tool("write", ToolCategory::FileOps)
        );
        assert!(
            !spec
                .capability_set
                .can_use_tool("bash", ToolCategory::Execution)
        );
        assert!(
            spec.capability_set
                .can_use_tool("read", ToolCategory::FileOps)
        );
        assert!(
            spec.capability_set
                .can_use_tool("grep", ToolCategory::Search)
        );
    }

    #[test]
    fn builtin_implementer_has_write_capabilities() {
        let spec = implementer();
        assert!(
            spec.capability_set
                .can_use_tool("edit", ToolCategory::FileOps)
        );
        assert!(
            spec.capability_set
                .can_use_tool("write", ToolCategory::FileOps)
        );
        assert!(
            spec.capability_set
                .can_use_tool("bash", ToolCategory::Execution)
        );
        assert!(spec.capability_set.network_access);
    }

    #[test]
    fn builtin_verifier_cannot_edit_but_can_bash() {
        let spec = verifier();
        assert!(
            !spec
                .capability_set
                .can_use_tool("edit", ToolCategory::FileOps)
        );
        assert!(
            !spec
                .capability_set
                .can_use_tool("write", ToolCategory::FileOps)
        );
        assert!(
            !spec
                .capability_set
                .can_use_tool("apply_patch", ToolCategory::FileOps)
        );
        // Verifier needs bash to run cargo test
        assert!(
            spec.capability_set
                .can_use_tool("bash", ToolCategory::Execution)
        );
        assert!(
            spec.capability_set
                .can_use_tool("read", ToolCategory::FileOps)
        );
    }

    #[test]
    fn builtin_reviewer_is_read_only() {
        let spec = reviewer();
        assert!(
            !spec
                .capability_set
                .can_use_tool("edit", ToolCategory::FileOps)
        );
        assert!(
            !spec
                .capability_set
                .can_use_tool("bash", ToolCategory::Execution)
        );
        assert!(
            spec.capability_set
                .can_use_tool("read", ToolCategory::FileOps)
        );
    }

    #[test]
    fn builtin_explorer_iteration_limit_is_30() {
        assert_eq!(explorer().max_iterations, 30);
    }

    #[test]
    fn builtin_implementer_iteration_limit_is_100() {
        assert_eq!(implementer().max_iterations, 100);
    }

    #[test]
    fn builtin_verifier_iteration_limit_is_20() {
        assert_eq!(verifier().max_iterations, 20);
    }

    #[test]
    fn builtin_reviewer_iteration_limit_is_20() {
        assert_eq!(reviewer().max_iterations, 20);
    }

    #[test]
    fn all_builtins_have_output_protocol_in_prompt() {
        for spec in all_builtins() {
            assert!(
                spec.system_prompt.contains("RESULT:"),
                "{} prompt missing RESULT: marker",
                spec.name
            );
            assert!(
                spec.system_prompt.contains("CONFIDENCE:"),
                "{} prompt missing CONFIDENCE: marker",
                spec.name
            );
        }
    }
}
