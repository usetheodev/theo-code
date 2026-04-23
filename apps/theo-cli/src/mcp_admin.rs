//! `theo mcp` admin commands — Phase 21 (sota-gaps-followup).
//!
//! Operator surface for the MCP discovery cache:
//! - `theo mcp discover [server]` — populate cache (all servers if omitted)
//! - `theo mcp invalidate <server>` — drop a single server's cache entry
//! - `theo mcp clear-all` — reset the cache entirely
//! - `theo mcp list` — show cached servers + tool counts
//!
//! The cache is process-local; this CLI exists so an operator can warm it
//! manually before the first sub-agent spawn (avoiding the 5s discovery
//! latency on the user's first delegate_task) and trigger a refresh after
//! upgrading an MCP server to a new version (gap #9).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use clap::Subcommand;

use theo_application::facade::mcp::{
    DEFAULT_PER_SERVER_TIMEOUT, DiscoveryCache, McpRegistry, McpServerConfig,
};

#[derive(Subcommand)]
pub enum McpCmd {
    /// Discover tools from one or all configured MCP servers.
    Discover {
        /// Server name (omit to discover every server in the registry).
        server: Option<String>,
    },
    /// Drop the cache entry for a specific server.
    Invalidate {
        /// Server name.
        server: String,
    },
    /// Empty the entire MCP discovery cache.
    ClearAll,
    /// List cached servers and how many tools each one exposes.
    List,
}

/// Construct an MCP registry from `.theo/mcp.toml` in `project_dir`.
/// Schema (minimal):
/// ```toml
/// [[server]]
/// name = "github"
/// command = "npx"
/// args = ["-y", "@modelcontextprotocol/server-github"]
/// ```
/// Returns an empty registry when the file is absent (loose contract:
/// missing config is not an error, the cache stays empty).
pub fn load_registry_from_project(project_dir: &Path) -> McpRegistry {
    let path = project_dir.join(".theo").join("mcp.toml");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return McpRegistry::new(),
    };
    parse_registry_toml(&content).unwrap_or_default()
}

fn parse_registry_toml(content: &str) -> anyhow::Result<McpRegistry> {
    use serde::Deserialize;
    use std::collections::BTreeMap;

    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        server: Vec<RawServer>,
    }
    #[derive(Deserialize)]
    struct RawServer {
        name: String,
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    }
    let f: File = toml::from_str(content)?;
    let mut reg = McpRegistry::new();
    for raw in f.server {
        reg.register(McpServerConfig::Stdio {
            name: raw.name,
            command: raw.command,
            args: raw.args,
            env: raw.env,
        });
    }
    Ok(reg)
}

pub async fn handle_mcp(
    cmd: McpCmd,
    project_dir: &Path,
    cache: Arc<DiscoveryCache>,
) -> anyhow::Result<()> {
    match cmd {
        McpCmd::Discover { server } => {
            let registry = load_registry_from_project(project_dir);
            if registry.is_empty() {
                println!(
                    "No MCP servers configured at {}. Create the file with [[server]] entries.",
                    project_dir.join(".theo").join("mcp.toml").display()
                );
                return Ok(());
            }
            let allowlist: Vec<String> = match &server {
                Some(name) => {
                    if registry.get(name).is_none() {
                        return Err(anyhow::anyhow!(
                            "unknown MCP server '{}'. Configured: {:?}",
                            name,
                            registry.names()
                        ));
                    }
                    vec![name.clone()]
                }
                None => registry.names().into_iter().map(String::from).collect(),
            };
            let report = cache
                .discover_filtered(
                    &registry,
                    &allowlist,
                    DEFAULT_PER_SERVER_TIMEOUT.max(Duration::from_secs(5)),
                )
                .await;
            for ok in &report.successful {
                let n = cache.get(ok).map(|t| t.len()).unwrap_or(0);
                println!("✓ {} ({} tools)", ok, n);
            }
            for (name, reason) in &report.failed {
                println!("✗ {}: {}", name, reason);
            }
            println!(
                "Discover finished: {} successful, {} failed.",
                report.successful.len(),
                report.failed.len()
            );
        }
        McpCmd::Invalidate { server } => {
            if cache.invalidate(&server) {
                println!("✓ Invalidated cache for '{}'.", server);
            } else {
                println!("Server '{}' not in cache; nothing to drop.", server);
            }
        }
        McpCmd::ClearAll => {
            cache.clear_all();
            println!("✓ Cleared the entire MCP discovery cache.");
        }
        McpCmd::List => {
            let names = cache.cached_servers();
            if names.is_empty() {
                println!(
                    "No MCP servers cached. Run `theo mcp discover` to populate."
                );
                return Ok(());
            }
            println!("{:<28} {:>6}", "SERVER", "TOOLS");
            for name in names {
                let n = cache.get(&name).map(|t| t.len()).unwrap_or(0);
                println!("{:<28} {:>6}", name, n);
            }
            println!("Total tools cached: {}", cache.total_tools());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use theo_application::facade::mcp::McpTool;

    fn fixture_project_with_mcp_toml(content: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        let theo = dir.path().join(".theo");
        std::fs::create_dir_all(&theo).unwrap();
        std::fs::write(theo.join("mcp.toml"), content).unwrap();
        dir
    }

    // ── load_registry_from_project ──

    #[test]
    fn load_registry_returns_empty_when_file_absent() {
        let dir = TempDir::new().unwrap();
        let reg = load_registry_from_project(dir.path());
        assert!(reg.is_empty());
    }

    #[test]
    fn load_registry_parses_a_single_server_entry() {
        let dir = fixture_project_with_mcp_toml(
            r#"
            [[server]]
            name = "github"
            command = "npx"
            args = ["-y", "@modelcontextprotocol/server-github"]
            "#,
        );
        let reg = load_registry_from_project(dir.path());
        assert_eq!(reg.len(), 1);
        assert!(reg.get("github").is_some());
    }

    #[test]
    fn load_registry_parses_multiple_servers() {
        let dir = fixture_project_with_mcp_toml(
            r#"
            [[server]]
            name = "github"
            command = "echo"

            [[server]]
            name = "filesystem"
            command = "echo"
            args = ["/tmp"]
            "#,
        );
        let reg = load_registry_from_project(dir.path());
        assert_eq!(reg.len(), 2);
        assert!(reg.get("github").is_some());
        assert!(reg.get("filesystem").is_some());
    }

    #[test]
    fn load_registry_returns_empty_for_malformed_toml() {
        let dir = fixture_project_with_mcp_toml("not valid [toml");
        let reg = load_registry_from_project(dir.path());
        assert!(reg.is_empty());
    }

    // ── handle_mcp ──

    #[tokio::test]
    async fn cmd_mcp_discover_unknown_server_returns_err() {
        let dir = fixture_project_with_mcp_toml(
            r#"
            [[server]]
            name = "alpha"
            command = "echo"
            "#,
        );
        let cache = Arc::new(DiscoveryCache::new());
        let res = handle_mcp(
            McpCmd::Discover {
                server: Some("missing".into()),
            },
            dir.path(),
            cache,
        )
        .await;
        assert!(res.is_err());
        let err = format!("{}", res.unwrap_err());
        assert!(err.contains("unknown MCP server"));
        assert!(err.contains("missing"));
    }

    #[tokio::test]
    async fn cmd_mcp_discover_no_config_prints_guidance() {
        let dir = TempDir::new().unwrap();
        let cache = Arc::new(DiscoveryCache::new());
        let res = handle_mcp(McpCmd::Discover { server: None }, dir.path(), cache)
            .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn cmd_mcp_discover_known_server_attempts_population() {
        // Configured but unreachable command → discover runs + report.failed
        // contains the entry. cache stays empty (fail-soft).
        let dir = fixture_project_with_mcp_toml(
            r#"
            [[server]]
            name = "alpha"
            command = "/nonexistent/never-spawned"
            "#,
        );
        let cache = Arc::new(DiscoveryCache::new());
        let res = handle_mcp(
            McpCmd::Discover {
                server: Some("alpha".into()),
            },
            dir.path(),
            cache.clone(),
        )
        .await;
        assert!(res.is_ok());
        // Unreachable → not cached, but no error returned (operator-friendly).
        assert!(cache.get("alpha").is_none());
    }

    #[tokio::test]
    async fn cmd_mcp_invalidate_drops_entry() {
        let dir = TempDir::new().unwrap();
        let cache = Arc::new(DiscoveryCache::new());
        cache.put("github", vec![]);
        assert!(cache.get("github").is_some());

        let res = handle_mcp(
            McpCmd::Invalidate {
                server: "github".into(),
            },
            dir.path(),
            cache.clone(),
        )
        .await;
        assert!(res.is_ok());
        assert!(cache.get("github").is_none());
    }

    #[tokio::test]
    async fn cmd_mcp_invalidate_returns_ok_for_unknown_server() {
        let dir = TempDir::new().unwrap();
        let cache = Arc::new(DiscoveryCache::new());
        let res = handle_mcp(
            McpCmd::Invalidate {
                server: "ghost".into(),
            },
            dir.path(),
            cache,
        )
        .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn cmd_mcp_clear_all_empties_cache() {
        let dir = TempDir::new().unwrap();
        let cache = Arc::new(DiscoveryCache::new());
        cache.put("a", vec![]);
        cache.put("b", vec![]);
        assert_eq!(cache.cached_servers().len(), 2);
        let res = handle_mcp(McpCmd::ClearAll, dir.path(), cache.clone()).await;
        assert!(res.is_ok());
        assert!(cache.cached_servers().is_empty());
    }

    #[tokio::test]
    async fn cmd_mcp_list_empty_returns_ok() {
        let dir = TempDir::new().unwrap();
        let cache = Arc::new(DiscoveryCache::new());
        let res = handle_mcp(McpCmd::List, dir.path(), cache).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn cmd_mcp_list_shows_cached_servers_and_tool_counts() {
        let dir = TempDir::new().unwrap();
        let cache = Arc::new(DiscoveryCache::new());
        cache.put(
            "github",
            vec![McpTool {
                name: "search".into(),
                description: None,
                input_schema: serde_json::json!({}),
            }],
        );
        let res = handle_mcp(McpCmd::List, dir.path(), cache).await;
        assert!(res.is_ok());
    }
}
