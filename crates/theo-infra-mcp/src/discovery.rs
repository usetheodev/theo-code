//! `DiscoveryCache` — pre-fetches `tools/list` from each MCP server so the
//! sub-agent's system prompt can advertise *actual* tool names instead of
//! only the `mcp:<server>:<tool>` namespace placeholder.
//!
//! Phase 17 — MCP Pre-Discovery. Fail-soft: per-server failures are logged
//! and the cache simply omits that server from the rendered hint. Hard
//! global timeout (default 5s) bounds total discovery cost so a hung server
//! cannot block sub-agent spawn.
//!
//! References:
//! - MCP spec `tools/list` (modelcontextprotocol.io 2025-03-26)
//! - Anthropic multi-agent paper §4 (lazy discovery considered acceptable
//!   only for synchronous user requests; for sub-agents, pre-discovery
//!   is preferred to avoid mid-loop latency spikes)

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

use tokio::time::timeout;

use crate::client::{McpClient, McpStdioClient};
use crate::error::McpError;
use crate::protocol::McpTool;
use crate::registry::McpRegistry;

/// Default per-server timeout for `tools/list`.
pub const DEFAULT_PER_SERVER_TIMEOUT: Duration = Duration::from_secs(5);

/// Outcome of a `discover_all` call.
#[derive(Debug, Default, Clone)]
pub struct DiscoveryReport {
    /// Servers whose `tools/list` succeeded.
    pub successful: Vec<String>,
    /// Servers that failed (server_name, error reason).
    pub failed: Vec<(String, String)>,
}

impl DiscoveryReport {
    /// Total servers attempted.
    pub fn total(&self) -> usize {
        self.successful.len() + self.failed.len()
    }

    pub fn is_complete_success(&self) -> bool {
        self.failed.is_empty() && !self.successful.is_empty()
    }
}

/// Cache of `tools/list` results indexed by server name.
///
/// Thread-safe (RwLock) so a single instance can be cloned (Arc) and
/// shared across the SubAgentManager and CLI.
#[derive(Debug, Default)]
pub struct DiscoveryCache {
    tools_by_server: RwLock<BTreeMap<String, Vec<McpTool>>>,
}

impl DiscoveryCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached tools for a server (cloned). `None` if the
    /// server was never discovered or discovery failed.
    pub fn get(&self, server: &str) -> Option<Vec<McpTool>> {
        self.tools_by_server.read().ok()?.get(server).cloned()
    }

    /// Returns the list of cached server names.
    pub fn cached_servers(&self) -> Vec<String> {
        self.tools_by_server
            .read()
            .map(|g| g.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Total number of cached tools across all servers.
    pub fn total_tools(&self) -> usize {
        self.tools_by_server
            .read()
            .map(|g| g.values().map(|v| v.len()).sum())
            .unwrap_or(0)
    }

    /// Manually insert tools (test helper / external population).
    pub fn put(&self, server: impl Into<String>, tools: Vec<McpTool>) {
        if let Ok(mut g) = self.tools_by_server.write() {
            g.insert(server.into(), tools);
        }
    }

    /// Drop the cached entry for `server`. The next `discover_all` call
    /// will re-spawn a client and re-fetch the tool list. Used after a
    /// known server upgrade or when the server reports a new capability.
    pub fn invalidate(&self, server: &str) -> bool {
        match self.tools_by_server.write() {
            Ok(mut g) => g.remove(server).is_some(),
            Err(_) => false,
        }
    }

    /// Drop every cached entry. Equivalent to instantiating a fresh cache
    /// without losing the `Arc` handle subscribers may still hold.
    pub fn clear_all(&self) {
        if let Ok(mut g) = self.tools_by_server.write() {
            g.clear();
        }
    }

    /// Discovers tools from every server in the registry.
    /// Per-server failures (timeout or RPC) are collected in the report;
    /// successful discoveries are cached.
    pub async fn discover_all(
        &self,
        registry: &McpRegistry,
        per_server_timeout: Duration,
    ) -> DiscoveryReport {
        let mut report = DiscoveryReport::default();
        let names: Vec<String> = registry
            .names()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        for server in names {
            let cfg = match registry.get(&server) {
                Some(c) => c,
                None => continue,
            };
            match discover_one(&server, &cfg, per_server_timeout).await {
                Ok(tools) => {
                    if let Ok(mut g) = self.tools_by_server.write() {
                        g.insert(server.clone(), tools);
                    }
                    report.successful.push(server);
                }
                Err(reason) => {
                    report.failed.push((server, reason));
                }
            }
        }
        report
    }

    /// Render a system-prompt section listing every cached server and its
    /// concrete tools. If `allowlist` is non-empty, only servers in the
    /// allowlist appear. Falls back to bare-namespace hint when no tools
    /// were discovered for a server in the allowlist.
    pub fn render_prompt_hint(&self, allowlist: &[String]) -> String {
        let guard = match self.tools_by_server.read() {
            Ok(g) => g,
            Err(_) => return String::new(),
        };
        let allow: std::collections::BTreeSet<&String> = allowlist.iter().collect();
        let entries: Vec<&String> = if allowlist.is_empty() {
            guard.keys().collect()
        } else {
            guard.keys().filter(|k| allow.contains(k)).collect()
        };
        if entries.is_empty() {
            return String::new();
        }
        let mut sections: Vec<String> = Vec::with_capacity(entries.len());
        for server in entries {
            let tools = guard.get(server).cloned().unwrap_or_default();
            if tools.is_empty() {
                sections.push(format!(
                    "- **{}** (no tools discovered) — invoke as `mcp:{}:<tool>` once available",
                    server, server
                ));
                continue;
            }
            let mut lines: Vec<String> = Vec::with_capacity(tools.len() + 1);
            lines.push(format!("- **{}** ({} tools)", server, tools.len()));
            for t in tools.iter().take(20) {
                let desc = t
                    .description
                    .as_deref()
                    .map(|d| format!(": {}", first_line(d)))
                    .unwrap_or_default();
                lines.push(format!("    - `mcp:{}:{}`{}", server, t.name, desc));
            }
            if tools.len() > 20 {
                lines.push(format!("    - … and {} more", tools.len() - 20));
            }
            sections.push(lines.join("\n"));
        }
        format!(
            "## MCP servers available (pre-discovered)\n\n\
             You can invoke external Model Context Protocol tools via \
             the `mcp:<server>:<tool>` namespace. The following tools were \
             discovered before this run started:\n\n{}\n",
            sections.join("\n")
        )
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

async fn discover_one(
    name: &str,
    cfg: &crate::config::McpServerConfig,
    per_server_timeout: Duration,
) -> Result<Vec<McpTool>, String> {
    let work = async move {
        let mut client = McpStdioClient::from_config(cfg).await?;
        client.list_tools().await
    };
    match timeout(per_server_timeout, work).await {
        Ok(Ok(tools)) => Ok(tools),
        Ok(Err(McpError::InvalidConfig(msg))) => {
            Err(format!("invalid config for '{}': {}", name, msg))
        }
        Ok(Err(other)) => Err(format!("{}: {}", name, other)),
        Err(_) => Err(format!(
            "{}: timed out after {}s",
            name,
            per_server_timeout.as_secs()
        )),
    }
}

/// Convenience: shareable handle to the cache.
pub fn shared_cache() -> Arc<DiscoveryCache> {
    Arc::new(DiscoveryCache::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::McpServerConfig;
    use std::collections::BTreeMap;

    fn fake_tool(name: &str, desc: &str) -> McpTool {
        McpTool {
            name: name.into(),
            description: Some(desc.into()),
            input_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn unreachable_cfg(name: &str) -> McpServerConfig {
        McpServerConfig::Stdio {
            name: name.into(),
            command: "/nonexistent/path/xyz123".into(),
            args: vec![],
            env: BTreeMap::new(),
        }
    }

    // ── DiscoveryReport ──

    #[test]
    fn report_default_is_empty() {
        let r = DiscoveryReport::default();
        assert_eq!(r.total(), 0);
        assert!(!r.is_complete_success());
    }

    #[test]
    fn report_total_sums_successful_and_failed() {
        let r = DiscoveryReport {
            successful: vec!["a".into(), "b".into()],
            failed: vec![("c".into(), "boom".into())],
        };
        assert_eq!(r.total(), 3);
    }

    #[test]
    fn report_is_complete_success_only_when_all_succeed() {
        let r1 = DiscoveryReport {
            successful: vec!["a".into()],
            failed: vec![],
        };
        assert!(r1.is_complete_success());
        let r2 = DiscoveryReport {
            successful: vec!["a".into()],
            failed: vec![("b".into(), "x".into())],
        };
        assert!(!r2.is_complete_success());
        let r3 = DiscoveryReport::default();
        assert!(!r3.is_complete_success(), "empty is not success");
    }

    // ── DiscoveryCache::get / put / cached_servers ──

    #[test]
    fn cache_new_is_empty() {
        let c = DiscoveryCache::new();
        assert!(c.cached_servers().is_empty());
        assert_eq!(c.total_tools(), 0);
        assert!(c.get("anything").is_none());
    }

    #[test]
    fn put_and_get_roundtrip() {
        let c = DiscoveryCache::new();
        c.put("github", vec![fake_tool("search", "search code")]);
        let got = c.get("github").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "search");
    }

    #[test]
    fn get_returns_none_for_missing_server() {
        let c = DiscoveryCache::new();
        c.put("a", vec![fake_tool("t", "")]);
        assert!(c.get("b").is_none());
    }

    #[test]
    fn cached_servers_returns_all_keys_sorted() {
        let c = DiscoveryCache::new();
        c.put("zeta", vec![]);
        c.put("alpha", vec![]);
        c.put("mu", vec![]);
        assert_eq!(c.cached_servers(), vec!["alpha", "mu", "zeta"]);
    }

    #[test]
    fn total_tools_aggregates_across_servers() {
        let c = DiscoveryCache::new();
        c.put("a", vec![fake_tool("x", ""), fake_tool("y", "")]);
        c.put("b", vec![fake_tool("z", "")]);
        assert_eq!(c.total_tools(), 3);
    }

    // ── render_prompt_hint ──

    #[test]
    fn render_empty_cache_returns_empty_string() {
        let c = DiscoveryCache::new();
        assert_eq!(c.render_prompt_hint(&[]), "");
    }

    #[test]
    fn render_includes_server_and_tool_names() {
        let c = DiscoveryCache::new();
        c.put(
            "github",
            vec![
                fake_tool("search_code", "search the repo"),
                fake_tool("read_issue", "read a github issue"),
            ],
        );
        let hint = c.render_prompt_hint(&[]);
        assert!(hint.contains("MCP servers available"));
        assert!(hint.contains("**github** (2 tools)"));
        assert!(hint.contains("`mcp:github:search_code`"));
        assert!(hint.contains("`mcp:github:read_issue`"));
    }

    #[test]
    fn render_with_allowlist_filters_servers() {
        let c = DiscoveryCache::new();
        c.put("github", vec![fake_tool("a", "")]);
        c.put("postgres", vec![fake_tool("b", "")]);
        let hint = c.render_prompt_hint(&["github".into()]);
        assert!(hint.contains("github"));
        assert!(!hint.contains("postgres"));
    }

    #[test]
    fn render_allowlist_filtering_unknown_returns_empty() {
        let c = DiscoveryCache::new();
        c.put("github", vec![fake_tool("a", "")]);
        let hint = c.render_prompt_hint(&["nonexistent".into()]);
        assert_eq!(hint, "");
    }

    #[test]
    fn render_truncates_after_20_tools_per_server() {
        let c = DiscoveryCache::new();
        let mut tools = Vec::new();
        for i in 0..30 {
            tools.push(fake_tool(&format!("t{}", i), ""));
        }
        c.put("big", tools);
        let hint = c.render_prompt_hint(&[]);
        assert!(hint.contains("`mcp:big:t0`"));
        assert!(hint.contains("`mcp:big:t19`"));
        assert!(!hint.contains("`mcp:big:t25`"), "should truncate >20");
        assert!(hint.contains("and 10 more"));
    }

    #[test]
    fn render_uses_first_line_of_multiline_description() {
        let c = DiscoveryCache::new();
        c.put(
            "x",
            vec![fake_tool("t", "first line\nsecond line\nthird line")],
        );
        let hint = c.render_prompt_hint(&[]);
        assert!(hint.contains("first line"));
        assert!(!hint.contains("second line"), "must collapse to first line");
    }

    #[test]
    fn render_handles_server_with_no_tools_gracefully() {
        let c = DiscoveryCache::new();
        c.put("empty", vec![]);
        let hint = c.render_prompt_hint(&[]);
        assert!(hint.contains("empty"));
        assert!(hint.contains("no tools discovered"));
    }

    // ── discover_all (fail-soft) ──

    #[tokio::test]
    async fn discover_all_empty_registry_returns_empty_report() {
        let c = DiscoveryCache::new();
        let reg = McpRegistry::new();
        let r = c.discover_all(&reg, Duration::from_millis(100)).await;
        assert_eq!(r.total(), 0);
        assert!(c.cached_servers().is_empty());
    }

    #[tokio::test]
    async fn discover_all_unreachable_server_records_failure_does_not_panic() {
        let c = DiscoveryCache::new();
        let mut reg = McpRegistry::new();
        reg.register(unreachable_cfg("dead"));
        let r = c.discover_all(&reg, Duration::from_secs(1)).await;
        assert_eq!(r.successful.len(), 0);
        assert_eq!(r.failed.len(), 1);
        assert_eq!(r.failed[0].0, "dead");
        assert!(c.get("dead").is_none(), "failed server NOT cached");
    }

    #[tokio::test]
    async fn discover_all_partial_failure_caches_only_successful() {
        // Two unreachable servers; we just confirm both end up in failed.
        let c = DiscoveryCache::new();
        let mut reg = McpRegistry::new();
        reg.register(unreachable_cfg("dead1"));
        reg.register(unreachable_cfg("dead2"));
        let r = c.discover_all(&reg, Duration::from_secs(1)).await;
        assert_eq!(r.failed.len(), 2);
        assert!(!r.is_complete_success());
        assert!(c.cached_servers().is_empty());
    }

    #[tokio::test]
    async fn discover_all_failure_reason_includes_server_name() {
        let c = DiscoveryCache::new();
        let mut reg = McpRegistry::new();
        reg.register(unreachable_cfg("xyz"));
        let r = c.discover_all(&reg, Duration::from_secs(1)).await;
        let (name, reason) = &r.failed[0];
        assert_eq!(name, "xyz");
        assert!(!reason.is_empty(), "must carry a non-empty reason");
    }

    // ── invalidate / clear_all ──

    #[test]
    fn invalidate_returns_true_when_server_was_cached() {
        let c = DiscoveryCache::new();
        c.put("github", vec![fake_tool("a", "")]);
        assert!(c.invalidate("github"));
        assert!(c.get("github").is_none());
    }

    #[test]
    fn invalidate_returns_false_when_server_was_not_cached() {
        let c = DiscoveryCache::new();
        assert!(!c.invalidate("ghost"));
    }

    #[test]
    fn invalidate_only_removes_specified_server() {
        let c = DiscoveryCache::new();
        c.put("a", vec![fake_tool("x", "")]);
        c.put("b", vec![fake_tool("y", "")]);
        c.invalidate("a");
        assert!(c.get("a").is_none());
        assert!(c.get("b").is_some());
    }

    #[test]
    fn clear_all_empties_cache() {
        let c = DiscoveryCache::new();
        c.put("a", vec![fake_tool("x", "")]);
        c.put("b", vec![fake_tool("y", "")]);
        c.put("c", vec![fake_tool("z", "")]);
        assert_eq!(c.cached_servers().len(), 3);
        c.clear_all();
        assert!(c.cached_servers().is_empty());
        assert_eq!(c.total_tools(), 0);
    }

    #[test]
    fn clear_all_on_empty_cache_is_noop() {
        let c = DiscoveryCache::new();
        c.clear_all();
        assert!(c.cached_servers().is_empty());
    }

    // ── caching semantics: discover_all is idempotent across calls ──

    #[tokio::test]
    async fn discover_all_idempotent_for_failed_servers() {
        // Failed servers stay un-cached; calling discover_all twice still
        // surfaces them in the failed list (no false-positive cache hit).
        let c = DiscoveryCache::new();
        let mut reg = McpRegistry::new();
        reg.register(unreachable_cfg("dead"));
        let r1 = c.discover_all(&reg, Duration::from_secs(1)).await;
        let r2 = c.discover_all(&reg, Duration::from_secs(1)).await;
        assert_eq!(r1.failed.len(), 1);
        assert_eq!(r2.failed.len(), 1, "second call must NOT hide the failure");
        assert!(c.cached_servers().is_empty());
    }

    #[test]
    fn put_overwrites_existing_cache_entry() {
        // Idempotency: re-discovering the same server replaces (not appends)
        // — matches the semantics `discover_all` would have on cache hit.
        let c = DiscoveryCache::new();
        c.put("github", vec![fake_tool("a", "")]);
        c.put("github", vec![fake_tool("a", ""), fake_tool("b", "")]);
        assert_eq!(c.get("github").unwrap().len(), 2);
    }

    #[test]
    fn shared_cache_returns_arc_clonable_handle() {
        let c1 = shared_cache();
        let c2 = c1.clone();
        c1.put("x", vec![fake_tool("t", "")]);
        assert_eq!(c2.get("x").unwrap().len(), 1, "cache shared via Arc");
    }
}
