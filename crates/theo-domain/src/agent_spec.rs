//! Sub-agent specification — pure value type for the dynamic sub-agent system.
//!
//! Replaces the hardcoded `SubAgentRole` enum with a declarative spec that
//! supports three sources: built-in (code), custom (markdown files in
//! `.theo/agents/` or `~/.theo/agents/`), and on-demand (LLM-created at runtime).
//!
//! Lives in `theo-domain` (zero deps). Loading from markdown frontmatter,
//! registry, and runtime semantics live in `theo-agent-runtime`.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::capability::CapabilitySet;
use crate::routing::SubAgentRoleId;

/// Where an `AgentSpec` came from.
///
/// Drives ordering (project > global > built-in) and security policies:
/// - `OnDemand`: read-only by default (S1), session-cap enforced (A5)
/// - `Project`: requires user approval via `.theo/.agents-approved` (S3/G1)
/// - `Global`: same as project but path is `~/.theo/agents/`
/// - `Builtin`: trusted, no approval needed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentSpecSource {
    /// Hardcoded in the runtime crate (theo-agent-runtime/src/subagent/builtins.rs).
    Builtin,
    /// Loaded from `<project_dir>/.theo/agents/*.md`.
    Project,
    /// Loaded from `~/.theo/agents/*.md` (or `$THEO_HOME/agents/`).
    Global,
    /// Created dynamically by the LLM via `delegate_task` with an unknown name.
    OnDemand,
}

impl AgentSpecSource {
    /// Stable string label for telemetry, events, and logging.
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentSpecSource::Builtin => "builtin",
            AgentSpecSource::Project => "project",
            AgentSpecSource::Global => "global",
            AgentSpecSource::OnDemand => "on_demand",
        }
    }
}

impl std::fmt::Display for AgentSpecSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Specification for a sub-agent.
///
/// Pure value type — no I/O, no async, no runtime deps. Loaded from markdown
/// frontmatter (custom) or constructed in code (builtin/on-demand).
///
/// Numeric type convention (A1):
/// - Frontmatter is parsed as `u32` (compatibility, portability).
/// - Conversion to `usize`/`u64` happens in the parser before constructing this type.
/// - Justification: `u32` is sufficient (max_iterations < 10k, timeout < 24h)
///   and avoids platform-dependent serialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSpec {
    /// Unique ID (e.g. "explorer", "my-reviewer"). Used as the lookup key in
    /// the registry and as the value of `delegate_task.agent`.
    pub name: String,
    /// Human-readable description, surfaced in the `delegate_task` tool schema
    /// so the LLM knows when to delegate to this agent.
    pub description: String,
    /// System prompt — body of the markdown file (custom) or the hardcoded
    /// prompt (built-in / on-demand). Sent to the LLM as the agent's role.
    pub system_prompt: String,
    /// Tool restrictions. For built-ins this is the curated capability set;
    /// for custom agents overriding a built-in, this is the *intersection*
    /// with the built-in's set (S2).
    pub capability_set: CapabilitySet,
    /// Optional model override. `None` inherits the parent's model.
    pub model_override: Option<String>,
    /// Iteration cap for the agent loop.
    pub max_iterations: usize,
    /// Wall-clock timeout in seconds.
    pub timeout_secs: u64,
    /// Origin of the spec (Builtin / Project / Global / OnDemand).
    pub source: AgentSpecSource,
}

impl AgentSpec {
    /// Bridge to model routing: derives a `SubAgentRoleId` from this spec's
    /// name. Enables per-agent model routing via `RoutingPhase::Subagent { role }`.
    pub fn role_id(&self) -> SubAgentRoleId {
        SubAgentRoleId(Cow::Owned(self.name.clone()))
    }

    /// Create an on-demand agent with **restricted** defaults (S1).
    ///
    /// On-demand agents are created by the LLM at runtime when it calls
    /// `delegate_task` with a name that's not in the registry. To prevent
    /// the LLM from escalating capabilities by inventing arbitrary agents,
    /// on-demand specs:
    /// - Use `CapabilitySet::read_only()` (no edit/write/bash).
    /// - Cap `max_iterations` at 10 (cost guard).
    /// - Cap `timeout_secs` at 120 (read-only is fast).
    pub fn on_demand(name: &str, objective: &str) -> Self {
        Self {
            name: name.to_string(),
            description: format!("On-demand agent: {}", objective),
            system_prompt: format!(
                "You are an on-demand sub-agent. Your objective:\n{}\n\n\
                 You have READ-ONLY access. Analyze and report findings.\n\
                 NEVER attempt to edit, write, or execute commands.",
                objective
            ),
            capability_set: CapabilitySet::read_only(),
            model_override: None,
            max_iterations: 10,
            timeout_secs: 120,
            source: AgentSpecSource::OnDemand,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::AllowedTools;
    use crate::tool::ToolCategory;

    fn fixture_spec(name: &str, source: AgentSpecSource) -> AgentSpec {
        AgentSpec {
            name: name.to_string(),
            description: "test fixture".to_string(),
            system_prompt: "you are a test agent".to_string(),
            capability_set: CapabilitySet::unrestricted(),
            model_override: None,
            max_iterations: 30,
            timeout_secs: 300,
            source,
        }
    }

    #[test]
    fn agent_spec_on_demand_is_read_only() {
        let spec = AgentSpec::on_demand("temp", "scan src/");
        // Read-only: cannot bash/edit/write
        assert!(
            !spec
                .capability_set
                .can_use_tool("bash", ToolCategory::Execution)
        );
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
        // But can read
        assert!(
            spec.capability_set
                .can_use_tool("read", ToolCategory::FileOps)
        );
        assert!(
            spec.capability_set
                .can_use_tool("grep", ToolCategory::Search)
        );
        // No network
        assert!(!spec.capability_set.network_access);
    }

    #[test]
    fn agent_spec_on_demand_max_iterations_capped_at_10() {
        let spec = AgentSpec::on_demand("temp", "scan");
        assert_eq!(spec.max_iterations, 10);
    }

    #[test]
    fn agent_spec_on_demand_timeout_120s() {
        let spec = AgentSpec::on_demand("temp", "scan");
        assert_eq!(spec.timeout_secs, 120);
    }

    #[test]
    fn agent_spec_on_demand_source_is_on_demand() {
        let spec = AgentSpec::on_demand("temp", "scan");
        assert_eq!(spec.source, AgentSpecSource::OnDemand);
    }

    #[test]
    fn agent_spec_on_demand_uses_provided_name() {
        let spec = AgentSpec::on_demand("ad-hoc-scanner", "scan");
        assert_eq!(spec.name, "ad-hoc-scanner");
    }

    #[test]
    fn agent_spec_on_demand_includes_objective_in_prompt() {
        let spec = AgentSpec::on_demand("x", "find OWASP issues");
        assert!(spec.system_prompt.contains("find OWASP issues"));
        assert!(
            spec.system_prompt.contains("READ-ONLY"),
            "on-demand prompt must explicitly state read-only"
        );
    }

    #[test]
    fn agent_spec_on_demand_uses_allowed_tools_all_with_denied() {
        let spec = AgentSpec::on_demand("x", "y");
        // Read-only uses AllowedTools::All + denied for write/exec
        assert_eq!(spec.capability_set.allowed_tools, AllowedTools::All);
        assert!(spec.capability_set.denied_tools.contains("bash"));
    }

    #[test]
    fn agent_spec_role_id_returns_correct_id() {
        let spec = fixture_spec("explorer", AgentSpecSource::Builtin);
        assert_eq!(spec.role_id().as_str(), "explorer");

        let spec = fixture_spec("custom-name", AgentSpecSource::Project);
        assert_eq!(spec.role_id().as_str(), "custom-name");
    }

    #[test]
    fn agent_spec_source_serde_roundtrip() {
        for source in [
            AgentSpecSource::Builtin,
            AgentSpecSource::Project,
            AgentSpecSource::Global,
            AgentSpecSource::OnDemand,
        ] {
            let json = serde_json::to_string(&source).unwrap();
            let back: AgentSpecSource = serde_json::from_str(&json).unwrap();
            assert_eq!(back, source);
        }
    }

    #[test]
    fn agent_spec_source_as_str_is_stable() {
        assert_eq!(AgentSpecSource::Builtin.as_str(), "builtin");
        assert_eq!(AgentSpecSource::Project.as_str(), "project");
        assert_eq!(AgentSpecSource::Global.as_str(), "global");
        assert_eq!(AgentSpecSource::OnDemand.as_str(), "on_demand");
    }

    #[test]
    fn agent_spec_source_display_matches_as_str() {
        assert_eq!(format!("{}", AgentSpecSource::Builtin), "builtin");
        assert_eq!(format!("{}", AgentSpecSource::OnDemand), "on_demand");
    }

    #[test]
    fn agent_spec_serde_roundtrip() {
        let spec = AgentSpec::on_demand("test", "an objective");
        let json = serde_json::to_string(&spec).unwrap();
        let back: AgentSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, spec.name);
        assert_eq!(back.description, spec.description);
        assert_eq!(back.source, spec.source);
        assert_eq!(back.max_iterations, spec.max_iterations);
        assert_eq!(back.timeout_secs, spec.timeout_secs);
    }
}
