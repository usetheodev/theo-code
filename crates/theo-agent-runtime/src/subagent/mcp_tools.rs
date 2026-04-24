//! `McpToolAdapter` — wraps a discovered MCP tool as a regular `Tool` so
//! the sub-agent's `ToolRegistry` can route calls through the standard
//! dispatch path.
//!
//! The adapter:
//! - Identifies itself with `mcp:<server>:<tool>` (Claude Code convention
//!   adapted from `mcp__server__tool` to keep our `:` separator).
//! - Returns the MCP server's raw `inputSchema` via `llm_schema_override`
//!   so the LLM sees the actual parameter shape (not a stripped
//!   `{type:object, properties:{}}`).
//! - On execute, delegates to `McpDispatcher::dispatch` which spawns a
//!   transient client, sends `tools/call`, and returns formatted text.
//!
//! References:
//! - MCP spec 2025-03-26 §3.2 (tools/call)
//! - Claude Code MCP integration (tools added to LLM tool array)

use std::sync::Arc;

use async_trait::async_trait;

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolSchema,
};
use theo_infra_mcp::{McpDispatcher, McpTool};

/// Adapter that exposes one discovered MCP tool through Theo's `Tool` trait.
pub struct McpToolAdapter {
    qualified_name: String,
    description: String,
    raw_schema: serde_json::Value,
    dispatcher: Arc<McpDispatcher>,
}

impl McpToolAdapter {
    /// Build the adapter for `tool` discovered from `server`.
    pub fn new(server: &str, tool: &McpTool, dispatcher: Arc<McpDispatcher>) -> Self {
        let qualified_name = format!("mcp:{}:{}", server, tool.name);
        let description = tool.description.clone().unwrap_or_else(|| {
            format!("MCP tool '{}' from server '{}'", tool.name, server)
        });
        Self {
            qualified_name,
            description,
            raw_schema: tool.input_schema.clone(),
            dispatcher,
        }
    }

    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }
}

impl std::fmt::Debug for McpToolAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpToolAdapter")
            .field("qualified_name", &self.qualified_name)
            .field("description_len", &self.description.len())
            .finish()
    }
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn id(&self) -> &str {
        &self.qualified_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> ToolSchema {
        // Empty typed schema — the real schema is exposed via
        // `llm_schema_override` so the LLM sees the MCP server's full spec.
        ToolSchema::new()
    }

    fn category(&self) -> ToolCategory {
        // MCP tools may do anything (read, mutate, network, etc.) — Utility
        // is the safest default. Per-server categorization is a future
        // refinement (would require server metadata extension).
        ToolCategory::Utility
    }

    fn llm_schema_override(&self) -> Option<serde_json::Value> {
        Some(self.raw_schema.clone())
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        match self.dispatcher.dispatch(&self.qualified_name, args).await {
            Ok(outcome) => {
                let title = if outcome.is_error {
                    format!("MCP error from {}", self.qualified_name)
                } else {
                    format!("MCP result from {}", self.qualified_name)
                };
                let mut out = ToolOutput::new(title, outcome.text);
                if outcome.is_error {
                    out = out.with_llm_suffix(
                        "MCP server marked the response as an error — \
                         inspect the message and adjust arguments before retry.",
                    );
                }
                Ok(out)
            }
            Err(e) => Err(ToolError::Execution(format!(
                "MCP dispatch failed for {}: {}",
                self.qualified_name, e
            ))),
        }
    }
}

/// Build adapters from a discovery cache for a sub-agent's `mcp_servers`
/// allowlist. Caller registers them into the sub-agent's `ToolRegistry`.
///
/// Returns `(adapter, name)` pairs so the caller can short-circuit on
/// duplicate IDs without losing context.
pub fn build_adapters_for_spec(
    cache: &theo_infra_mcp::DiscoveryCache,
    allowlist: &[String],
    dispatcher: Arc<McpDispatcher>,
) -> Vec<McpToolAdapter> {
    let mut out = Vec::new();
    for server in allowlist {
        let tools = match cache.get(server) {
            Some(t) => t,
            None => continue,
        };
        for tool in &tools {
            out.push(McpToolAdapter::new(server, tool, dispatcher.clone()));
        }
    }
    out
}

/// Plan §17 line 745: convert a discovered `McpTool` into a domain
/// `ToolDefinition` directly (bypasses the trait machinery).
///
/// Useful for tests that want to inspect the `mcp:<server>:<tool>` naming
/// + raw inputSchema preservation without spawning the registry. The
/// runtime path always goes through `McpToolAdapter`, but this helper
/// exists so the plan-mandated tests
/// `mcp_tool_to_definition_uses_qualified_name` and
/// `mcp_tool_to_definition_preserves_input_schema` can exercise the
/// conversion in isolation.
pub fn mcp_tool_to_definition(
    server: &str,
    tool: &theo_infra_mcp::McpTool,
) -> theo_domain::tool::ToolDefinition {
    theo_domain::tool::ToolDefinition {
        id: format!("mcp:{}:{}", server, tool.name),
        description: tool
            .description
            .clone()
            .unwrap_or_else(|| format!("MCP tool {} from {}", tool.name, server)),
        category: theo_domain::tool::ToolCategory::Utility,
        schema: theo_domain::tool::ToolSchema::new(),
        llm_schema_override: Some(tool.input_schema.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use theo_infra_mcp::{DiscoveryCache, McpRegistry, McpServerConfig};

    fn fake_tool(name: &str, schema: serde_json::Value) -> McpTool {
        McpTool {
            name: name.into(),
            description: Some(format!("Tool {}", name)),
            input_schema: schema,
        }
    }

    fn dispatcher() -> Arc<McpDispatcher> {
        Arc::new(McpDispatcher::new(Arc::new(McpRegistry::new())))
    }

    // ── Identification + schema override ──

    #[test]
    fn adapter_id_uses_qualified_name() {
        let t = fake_tool("search", serde_json::json!({"type": "object"}));
        let a = McpToolAdapter::new("github", &t, dispatcher());
        assert_eq!(a.id(), "mcp:github:search");
        assert_eq!(a.qualified_name(), "mcp:github:search");
    }

    #[test]
    fn adapter_description_falls_back_when_mcp_tool_lacks_one() {
        let t = McpTool {
            name: "raw".into(),
            description: None,
            input_schema: serde_json::json!({}),
        };
        let a = McpToolAdapter::new("srv", &t, dispatcher());
        assert!(a.description().contains("raw"));
        assert!(a.description().contains("srv"));
    }

    #[test]
    fn adapter_uses_provided_description_when_present() {
        let t = fake_tool("x", serde_json::json!({}));
        let a = McpToolAdapter::new("srv", &t, dispatcher());
        assert_eq!(a.description(), "Tool x");
    }

    #[test]
    fn adapter_llm_schema_override_returns_raw_input_schema() {
        let raw = serde_json::json!({
            "type": "object",
            "properties": { "q": { "type": "string", "description": "query" } },
            "required": ["q"]
        });
        let t = fake_tool("search", raw.clone());
        let a = McpToolAdapter::new("srv", &t, dispatcher());
        assert_eq!(a.llm_schema_override(), Some(raw));
    }

    #[test]
    fn adapter_typed_schema_is_empty_so_validate_passes() {
        let t = fake_tool("x", serde_json::json!({}));
        let a = McpToolAdapter::new("srv", &t, dispatcher());
        assert!(a.schema().validate().is_ok());
    }

    #[test]
    fn adapter_definition_includes_override() {
        let raw = serde_json::json!({"type":"object","properties":{"a":{"type":"integer","description":"d"}}});
        let t = fake_tool("calc", raw.clone());
        let a = McpToolAdapter::new("math", &t, dispatcher());
        let def = a.definition();
        assert_eq!(def.id, "mcp:math:calc");
        assert_eq!(def.llm_schema_override, Some(raw));
    }

    #[test]
    fn adapter_default_category_is_utility() {
        let t = fake_tool("x", serde_json::json!({}));
        let a = McpToolAdapter::new("srv", &t, dispatcher());
        assert_eq!(a.category(), ToolCategory::Utility);
    }

    // ── Execute path ──

    #[tokio::test]
    async fn execute_with_unknown_server_returns_execution_failed() {
        // Dispatcher with empty registry → InvalidConfig → bubble up as
        // ExecutionFailed.
        let t = fake_tool("noop", serde_json::json!({}));
        let a = McpToolAdapter::new("ghost", &t, dispatcher());
        let ctx = ToolContext::test_context(std::env::temp_dir());
        let mut perms = PermissionCollector::new();
        let res = a.execute(serde_json::json!({}), &ctx, &mut perms).await;
        assert!(res.is_err());
        let err = format!("{}", res.unwrap_err());
        assert!(err.contains("MCP dispatch failed"));
        assert!(err.contains("ghost"));
    }

    // ── build_adapters_for_spec ──

    fn cache_with(server: &str, tools: Vec<McpTool>) -> DiscoveryCache {
        let c = DiscoveryCache::new();
        c.put(server, tools);
        c
    }

    #[test]
    fn build_adapters_returns_empty_for_unknown_server() {
        let cache = DiscoveryCache::new();
        let adapters = build_adapters_for_spec(
            &cache,
            &["github".to_string()],
            dispatcher(),
        );
        assert!(adapters.is_empty());
    }

    #[test]
    fn build_adapters_returns_one_per_discovered_tool() {
        let cache = cache_with(
            "github",
            vec![
                fake_tool("a", serde_json::json!({})),
                fake_tool("b", serde_json::json!({})),
                fake_tool("c", serde_json::json!({})),
            ],
        );
        let adapters = build_adapters_for_spec(
            &cache,
            &["github".to_string()],
            dispatcher(),
        );
        assert_eq!(adapters.len(), 3);
        let ids: Vec<&str> = adapters.iter().map(|a| a.id()).collect();
        assert!(ids.contains(&"mcp:github:a"));
        assert!(ids.contains(&"mcp:github:c"));
    }

    #[test]
    fn build_adapters_filters_by_allowlist() {
        let cache = DiscoveryCache::new();
        cache.put("alpha", vec![fake_tool("x", serde_json::json!({}))]);
        cache.put("beta", vec![fake_tool("y", serde_json::json!({}))]);
        cache.put("gamma", vec![fake_tool("z", serde_json::json!({}))]);
        let adapters = build_adapters_for_spec(
            &cache,
            &["alpha".to_string(), "gamma".to_string()],
            dispatcher(),
        );
        assert_eq!(adapters.len(), 2);
        let ids: Vec<&str> = adapters.iter().map(|a| a.id()).collect();
        assert!(ids.contains(&"mcp:alpha:x"));
        assert!(ids.contains(&"mcp:gamma:z"));
        assert!(!ids.iter().any(|s| s.contains("beta")));
    }

    #[test]
    fn build_adapters_skips_servers_without_cached_tools() {
        // Only `github` in cache; allowlist asks for github + slack
        let cache = cache_with(
            "github",
            vec![fake_tool("search", serde_json::json!({}))],
        );
        let adapters = build_adapters_for_spec(
            &cache,
            &["github".to_string(), "slack".to_string()],
            dispatcher(),
        );
        assert_eq!(adapters.len(), 1);
        assert_eq!(adapters[0].id(), "mcp:github:search");
    }

    // ── Plan §17 line 795-796 mandated test names ──

    #[test]
    fn mcp_tool_to_definition_uses_qualified_name() {
        let raw = serde_json::json!({"type":"object"});
        let t = fake_tool("search_repos", raw);
        let def = super::mcp_tool_to_definition("github", &t);
        assert_eq!(def.id, "mcp:github:search_repos");
    }

    #[test]
    fn mcp_tool_to_definition_preserves_input_schema() {
        let raw = serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "search term"},
                "limit": {"type": "integer", "description": "max results"}
            },
            "required": ["query"]
        });
        let t = fake_tool("search", raw.clone());
        let def = super::mcp_tool_to_definition("github", &t);
        assert_eq!(def.llm_schema_override, Some(raw.clone()));
        // The empty typed schema is intentional — the override wins in
        // `tool_bridge::registry_to_definitions`.
        assert!(def.schema.params.is_empty());
    }

    #[test]
    fn mcp_tool_to_definition_falls_back_when_description_absent() {
        let t = McpTool {
            name: "raw".into(),
            description: None,
            input_schema: serde_json::json!({}),
        };
        let def = super::mcp_tool_to_definition("srv", &t);
        assert!(def.description.contains("raw"));
        assert!(def.description.contains("srv"));
    }

    // ── Dispatcher integration: real spawn fails for unreachable command ──

    #[tokio::test]
    async fn execute_with_unreachable_server_command_returns_execution_failed() {
        let mut reg = McpRegistry::new();
        reg.register(McpServerConfig::Stdio {
            name: "dead".into(),
            command: "/nonexistent/cmd/xyz".into(),
            args: vec![],
            env: BTreeMap::new(),
            timeout_ms: None,
        });
        let dispatcher = Arc::new(McpDispatcher::new(Arc::new(reg)));
        let t = fake_tool("foo", serde_json::json!({}));
        let a = McpToolAdapter::new("dead", &t, dispatcher);
        let ctx = ToolContext::test_context(std::env::temp_dir());
        let mut perms = PermissionCollector::new();
        let res = a.execute(serde_json::json!({}), &ctx, &mut perms).await;
        assert!(res.is_err());
    }
}
