//! `McpRegistry` — Map<server_name, McpServerConfig> compartilhado entre
//! callers (CLI / pipeline / SubAgentManager).
//!
//! A AgentSpec declara `mcp_servers: Vec<String>` (allowlist). Quando
//! presente, o spawn_with_spec consulta o McpRegistry para enriquecer o
//! system prompt do sub-agent com a descrição dos servers MCP disponíveis,
//! e (futuramente) faz discovery + dispatch via `mcp:server:tool`.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::config::McpServerConfig;

#[derive(Debug, Clone, Default)]
pub struct McpRegistry {
    by_name: BTreeMap<String, Arc<McpServerConfig>>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, config: McpServerConfig) {
        let name = config.name().to_string();
        self.by_name.insert(name, Arc::new(config));
    }

    pub fn get(&self, name: &str) -> Option<Arc<McpServerConfig>> {
        self.by_name.get(name).cloned()
    }

    pub fn names(&self) -> Vec<&str> {
        self.by_name.keys().map(|s| s.as_str()).collect()
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Filter the registry to only the servers in `allowlist` (per
    /// `AgentSpec.mcp_servers`). Returns a new sub-registry.
    pub fn filtered(&self, allowlist: &[String]) -> McpRegistry {
        let mut out = McpRegistry::new();
        for name in allowlist {
            if let Some(cfg) = self.by_name.get(name) {
                out.by_name.insert(name.clone(), cfg.clone());
            }
        }
        out
    }

    /// Render a system-prompt section advertising the available MCP servers
    /// to the sub-agent. Returns empty string if registry is empty.
    pub fn render_prompt_hint(&self) -> String {
        if self.by_name.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = self
            .by_name
            .iter()
            .map(|(n, _)| format!("- {}: invoke tools as `mcp:{}:<tool>`", n, n))
            .collect();
        format!(
            "## MCP servers available\n\n\
             You can invoke external Model Context Protocol servers via the \
             `mcp:<server>:<tool>` namespace. The following servers are accessible:\n\n{}\n",
            lines.join("\n")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn stdio_cfg(name: &str) -> McpServerConfig {
        McpServerConfig::Stdio {
            name: name.to_string(),
            command: "echo".to_string(),
            args: vec![],
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn new_is_empty() {
        let r = McpRegistry::new();
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn register_adds_by_name() {
        let mut r = McpRegistry::new();
        r.register(stdio_cfg("github"));
        r.register(stdio_cfg("postgres"));
        assert_eq!(r.len(), 2);
        assert!(r.get("github").is_some());
        assert!(r.get("postgres").is_some());
        assert!(r.get("missing").is_none());
    }

    #[test]
    fn filtered_returns_only_allowlist() {
        let mut r = McpRegistry::new();
        r.register(stdio_cfg("github"));
        r.register(stdio_cfg("postgres"));
        r.register(stdio_cfg("slack"));

        let allow = vec!["github".to_string(), "slack".to_string()];
        let f = r.filtered(&allow);
        assert_eq!(f.len(), 2);
        assert!(f.get("github").is_some());
        assert!(f.get("slack").is_some());
        assert!(f.get("postgres").is_none());
    }

    #[test]
    fn filtered_ignores_unknown_names() {
        let mut r = McpRegistry::new();
        r.register(stdio_cfg("a"));
        let f = r.filtered(&["a".to_string(), "nonexistent".to_string()]);
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn render_prompt_hint_lists_servers() {
        let mut r = McpRegistry::new();
        r.register(stdio_cfg("github"));
        r.register(stdio_cfg("postgres"));
        let hint = r.render_prompt_hint();
        assert!(hint.contains("MCP servers available"));
        assert!(hint.contains("mcp:github:<tool>"));
        assert!(hint.contains("mcp:postgres:<tool>"));
    }

    #[test]
    fn render_prompt_hint_empty_registry_returns_empty() {
        let r = McpRegistry::new();
        assert_eq!(r.render_prompt_hint(), "");
    }

    #[test]
    fn names_deterministic_order() {
        let mut r = McpRegistry::new();
        r.register(stdio_cfg("zeta"));
        r.register(stdio_cfg("alpha"));
        r.register(stdio_cfg("mu"));
        // BTreeMap → sorted
        assert_eq!(r.names(), vec!["alpha", "mu", "zeta"]);
    }
}
