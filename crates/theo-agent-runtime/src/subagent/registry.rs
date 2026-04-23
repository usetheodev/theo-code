//! `SubAgentRegistry` — central lookup for all sub-agent specs.
//!
//! Loads from three sources with priority (project > global > built-in) and
//! supports runtime registration for on-demand agents.
//!
//! Uses `IndexMap` to preserve insertion order — guarantees deterministic
//! ordering in `build_tool_description()` regardless of insertion ordering.
//!
//! Track A — Phase 1.

use std::path::Path;

use indexmap::IndexMap;

use theo_domain::agent_spec::{AgentSpec, AgentSpecSource};

/// Warning emitted when a custom agent overrides a builtin or fails to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryWarning {
    pub kind: WarningKind,
    pub message: String,
    pub path: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// A spec file failed to parse and was skipped.
    InvalidSpec,
    /// A custom spec overrode a builtin name (capabilities are intersected).
    BuiltinOverride,
    /// A spec file was unreadable (I/O error).
    ReadError,
}

#[derive(Debug, Clone, Default)]
pub struct SubAgentRegistry {
    specs: IndexMap<String, AgentSpec>,
}

impl SubAgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-loaded with the 4 built-in agents (explorer, implementer, verifier, reviewer).
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();
        for spec in super::builtins::all_builtins() {
            reg.specs.insert(spec.name.clone(), spec);
        }
        reg
    }

    /// Register an agent (used for on-demand or programmatic registration).
    pub fn register(&mut self, spec: AgentSpec) {
        self.specs.insert(spec.name.clone(), spec);
    }

    /// Lookup by name.
    pub fn get(&self, name: &str) -> Option<&AgentSpec> {
        self.specs.get(name)
    }

    /// True if a spec with this name exists.
    pub fn contains(&self, name: &str) -> bool {
        self.specs.contains_key(name)
    }

    /// Number of registered specs.
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// True if no specs are registered.
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// Names of all registered agents (preserves insertion order).
    pub fn names(&self) -> Vec<&str> {
        self.specs.keys().map(|s| s.as_str()).collect()
    }

    /// Iterate over all specs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &AgentSpec> {
        self.specs.values()
    }

    /// Generate the description for the `delegate_task` tool schema.
    /// Deterministic: same registry → same output (IndexMap preserves order).
    pub fn build_tool_description(&self) -> String {
        let agents: Vec<String> = self
            .specs
            .values()
            .map(|s| format!("- {}: {}", s.name, s.description))
            .collect();
        format!(
            "Delegate work to a specialized sub-agent.\n\nAvailable agents:\n{}\n\n\
             Use any name not listed above to create an on-demand read-only agent.",
            agents.join("\n")
        )
    }

    /// Load custom agents from a directory (e.g. `.theo/agents/`).
    /// Implementation arrives in Phase 2 (parser + frontmatter).
    /// Stub for now to preserve the API surface.
    pub fn load_from_dir(&mut self, _dir: &Path, _source: AgentSpecSource) -> Vec<RegistryWarning> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_new_is_empty() {
        let reg = SubAgentRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn registry_with_builtins_has_4_agents() {
        let reg = SubAgentRegistry::with_builtins();
        assert_eq!(reg.len(), 4);
    }

    #[test]
    fn registry_get_returns_some_for_existing() {
        let reg = SubAgentRegistry::with_builtins();
        assert!(reg.get("explorer").is_some());
        assert!(reg.get("implementer").is_some());
        assert!(reg.get("verifier").is_some());
        assert!(reg.get("reviewer").is_some());
    }

    #[test]
    fn registry_get_returns_none_for_missing() {
        let reg = SubAgentRegistry::with_builtins();
        assert!(reg.get("non-existent").is_none());
    }

    #[test]
    fn registry_register_adds_agent() {
        let mut reg = SubAgentRegistry::new();
        let spec = AgentSpec::on_demand("custom", "do x");
        reg.register(spec.clone());
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.get("custom").unwrap().name, "custom");
    }

    #[test]
    fn registry_register_replaces_same_name() {
        let mut reg = SubAgentRegistry::with_builtins();
        // Register a new agent with same name as builtin → replaces it
        let mut spec = AgentSpec::on_demand("explorer", "redefined");
        spec.description = "redefined desc".to_string();
        reg.register(spec);
        assert_eq!(reg.len(), 4); // still 4
        assert_eq!(reg.get("explorer").unwrap().description, "redefined desc");
    }

    #[test]
    fn registry_names_preserves_insertion_order() {
        let reg = SubAgentRegistry::with_builtins();
        assert_eq!(
            reg.names(),
            vec!["explorer", "implementer", "verifier", "reviewer"]
        );
    }

    #[test]
    fn registry_contains_returns_correct_bool() {
        let reg = SubAgentRegistry::with_builtins();
        assert!(reg.contains("explorer"));
        assert!(!reg.contains("missing"));
    }

    #[test]
    fn registry_build_tool_description_is_deterministic() {
        let reg1 = SubAgentRegistry::with_builtins();
        let reg2 = SubAgentRegistry::with_builtins();
        assert_eq!(reg1.build_tool_description(), reg2.build_tool_description());
    }

    #[test]
    fn registry_build_tool_description_lists_all_agents() {
        let reg = SubAgentRegistry::with_builtins();
        let desc = reg.build_tool_description();
        assert!(desc.contains("explorer"));
        assert!(desc.contains("implementer"));
        assert!(desc.contains("verifier"));
        assert!(desc.contains("reviewer"));
        assert!(desc.contains("on-demand"));
    }

    #[test]
    fn registry_iter_returns_specs_in_order() {
        let reg = SubAgentRegistry::with_builtins();
        let names: Vec<&str> = reg.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["explorer", "implementer", "verifier", "reviewer"]);
    }
}
