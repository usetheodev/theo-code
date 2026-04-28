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

use crate::client::{McpAnyClient, McpClient};
use crate::error::McpError;
use crate::protocol::McpTool;
use crate::registry::McpRegistry;

/// Default per-server timeout for `tools/list`.
pub const DEFAULT_PER_SERVER_TIMEOUT: Duration = Duration::from_secs(5);

/// Phase 33 (mcp-http-and-discover-flake): operator-tunable global default.
///
/// Reads `THEO_MCP_DISCOVER_TIMEOUT_SECS` (a positive integer of seconds).
/// Falls back to `DEFAULT_PER_SERVER_TIMEOUT` (5s) when:
/// - the env var is unset,
/// - the value cannot be parsed as `u64`,
/// - the value is `0` (would cause instant timeout — guard against
///   accidental self-foot-shooting).
///
/// Hierarchy used by `discover_one`:
///   per-server `cfg.timeout_ms()`  →  caller's `per_server_timeout`
/// where the caller typically passes the result of this function. CLI
/// flag (Phase 34) and per-server config (Phase 33) override it.
pub fn effective_default_timeout() -> Duration {
    std::env::var("THEO_MCP_DISCOVER_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_PER_SERVER_TIMEOUT)
}

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

    /// Discovers tools from servers in `registry` whose name appears in
    /// `allowlist`. Empty allowlist returns an empty report.
    ///
    /// Plan §17 signature variant: returns the same `DiscoveryReport` shape
    /// as `discover_all` but applies the allowlist filter before spawning
    /// any clients. Cached entries are reused; failed discoveries are
    /// recorded in `failed`.
    pub async fn discover_filtered(
        &self,
        registry: &McpRegistry,
        allowlist: &[String],
        per_server_timeout: Duration,
    ) -> DiscoveryReport {
        if allowlist.is_empty() {
            return DiscoveryReport::default();
        }
        let filtered = registry.filtered(allowlist);
        self.discover_all(&filtered, per_server_timeout).await
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
    // Phase 33: per-server `timeout_ms` in the config overrides the
    // caller's default. Allows `npx`-based servers to declare 30_000
    // (30s) without inflating the default for HTTP servers.
    let effective = cfg
        .timeout_ms()
        .map(Duration::from_millis)
        .unwrap_or(per_server_timeout);
    // Phase 37 (mcp-http-and-discover-flake): route via McpAnyClient so
    // both stdio and HTTP servers in the registry get discovered. The
    // dispatcher is transport-agnostic.
    let work = async move {
        let mut client = McpAnyClient::from_config(cfg).await?;
        client.list_tools().await
    };
    match timeout(effective, work).await {
        Ok(Ok(tools)) => Ok(tools),
        Ok(Err(McpError::InvalidConfig(msg))) => {
            Err(format!("invalid config for '{}': {}", name, msg))
        }
        Ok(Err(other)) => Err(format!("{}: {}", name, other)),
        Err(_) => Err(format!(
            "{}: timed out after {}s",
            name,
            effective.as_secs()
        )),
    }
}

/// Convenience: shareable handle to the cache.
pub fn shared_cache() -> Arc<DiscoveryCache> {
    Arc::new(DiscoveryCache::new())
}


#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
