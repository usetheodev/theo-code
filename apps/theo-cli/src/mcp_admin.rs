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

use clap::Subcommand;

use theo_application::facade::mcp::{
    effective_default_timeout, DiscoveryCache, McpRegistry, McpServerConfig,
};

#[derive(Subcommand)]
pub enum McpCmd {
    /// Discover tools from one or all configured MCP servers.
    Discover {
        /// Server name (omit to discover every server in the registry).
        server: Option<String>,
        /// Phase 34 (mcp-http-and-discover-flake) — per-call override
        /// of the discover timeout, in seconds. Hierarchy:
        ///   CLI flag  >  THEO_MCP_DISCOVER_TIMEOUT_SECS env var
        ///             >  per-server `timeout_ms` in mcp.toml
        ///             >  default 5s
        /// Useful in CI when the operator knows `npx` needs ≥30s on
        /// a cold runner (server-filesystem download + node bootstrap).
        #[arg(long)]
        timeout_secs: Option<u64>,
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
    /// Phase 39 (mcp-http-and-discover-flake): tagged enum for the
    /// `transport` discriminator. `transport = "stdio"` (default when
    /// the field is absent) yields the legacy stdio path; `transport =
    /// "http"` activates the HTTP/Streamable client.
    ///
    /// D5 backward-compat: legacy `[[server]]` entries without an
    /// explicit `transport` field still parse as `Stdio` because we
    /// implement a custom `Deserialize` that defaults the tag.
    #[derive(Debug)]
    enum RawServer {
        Stdio {
            name: String,
            command: String,
            args: Vec<String>,
            env: BTreeMap<String, String>,
            timeout_ms: Option<u64>,
        },
        Http {
            name: String,
            url: String,
            headers: BTreeMap<String, String>,
            timeout_ms: Option<u64>,
        },
    }

    impl<'de> Deserialize<'de> for RawServer {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            #[derive(Deserialize)]
            struct All {
                #[serde(default)]
                transport: Option<String>,
                name: String,
                #[serde(default)]
                command: Option<String>,
                #[serde(default)]
                args: Vec<String>,
                #[serde(default)]
                env: BTreeMap<String, String>,
                #[serde(default)]
                url: Option<String>,
                #[serde(default)]
                headers: BTreeMap<String, String>,
                #[serde(default)]
                timeout_ms: Option<u64>,
            }
            let all = All::deserialize(deserializer)?;
            let kind = all.transport.as_deref().unwrap_or("stdio");
            match kind {
                "stdio" => {
                    let command = all.command.ok_or_else(|| {
                        serde::de::Error::custom(format!(
                            "stdio server '{}' is missing required `command` field",
                            all.name
                        ))
                    })?;
                    Ok(RawServer::Stdio {
                        name: all.name,
                        command,
                        args: all.args,
                        env: all.env,
                        timeout_ms: all.timeout_ms,
                    })
                }
                "http" => {
                    let url = all.url.ok_or_else(|| {
                        serde::de::Error::custom(format!(
                            "http server '{}' is missing required `url` field",
                            all.name
                        ))
                    })?;
                    Ok(RawServer::Http {
                        name: all.name,
                        url,
                        headers: all.headers,
                        timeout_ms: all.timeout_ms,
                    })
                }
                other => Err(serde::de::Error::custom(format!(
                    "unknown MCP transport '{}' for server '{}'; \
                     supported: 'stdio' | 'http'",
                    other, all.name
                ))),
            }
        }
    }

    let f: File = toml::from_str(content)?;
    let mut reg = McpRegistry::new();
    for raw in f.server {
        let cfg = match raw {
            RawServer::Stdio {
                name, command, args, env, timeout_ms,
            } => McpServerConfig::Stdio {
                name, command, args, env, timeout_ms,
            },
            RawServer::Http {
                name, url, headers, timeout_ms,
            } => McpServerConfig::Http {
                name, url, headers, timeout_ms,
            },
        };
        reg.register(cfg);
    }
    Ok(reg)
}

pub async fn handle_mcp(
    cmd: McpCmd,
    project_dir: &Path,
    cache: Arc<DiscoveryCache>,
) -> anyhow::Result<()> {
    match cmd {
        McpCmd::Discover { server, timeout_secs } => {
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
            // Phase 34 hierarchy: CLI flag > env > default. Per-server
            // `timeout_ms` in mcp.toml still wins inside discover_one.
            let global_timeout = timeout_secs
                .filter(|n| *n > 0)
                .map(std::time::Duration::from_secs)
                .unwrap_or_else(effective_default_timeout);
            let report = cache
                .discover_filtered(&registry, &allowlist, global_timeout)
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

    // ── Phase 33 (mcp-http-and-discover-flake) — timeout_ms in TOML ──

    #[test]
    fn load_registry_reads_timeout_ms_field_from_toml() {
        let dir = fixture_project_with_mcp_toml(
            r#"
            [[server]]
            name = "fs"
            command = "npx"
            args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
            timeout_ms = 30000
            "#,
        );
        let reg = load_registry_from_project(dir.path());
        let cfg = reg.get("fs").expect("fs server must be registered");
        assert_eq!(
            cfg.timeout_ms(),
            Some(30_000),
            "TOML timeout_ms field must reach the McpServerConfig"
        );
    }

    #[test]
    fn load_registry_defaults_timeout_ms_to_none_when_omitted() {
        let dir = fixture_project_with_mcp_toml(
            r#"
            [[server]]
            name = "github"
            command = "echo"
            "#,
        );
        let reg = load_registry_from_project(dir.path());
        let cfg = reg.get("github").unwrap();
        assert_eq!(cfg.timeout_ms(), None);
    }

    // ── Phase 39 (mcp-http-and-discover-flake) — http transport in TOML ──

    pub mod parse_http {
        use super::*;
        use theo_application::facade::mcp::McpServerConfig;

        #[test]
        fn parser_reads_http_server_with_explicit_transport() {
            let dir = fixture_project_with_mcp_toml(
                r#"
                [[server]]
                transport = "http"
                name = "company-internal"
                url = "https://mcp.example.com/api"
                "#,
            );
            let reg = load_registry_from_project(dir.path());
            let cfg = reg
                .get("company-internal")
                .expect("http server must register");
            assert!(
                matches!(&*cfg, McpServerConfig::Http { .. }),
                "transport=http must yield Http variant"
            );
        }

        #[test]
        fn parser_reads_http_server_headers() {
            let dir = fixture_project_with_mcp_toml(
                r#"
                [[server]]
                transport = "http"
                name = "x"
                url = "http://x"
                headers = { Authorization = "Bearer abc-123" }
                "#,
            );
            let reg = load_registry_from_project(dir.path());
            let cfg = reg.get("x").unwrap();
            match &*cfg {
                McpServerConfig::Http { headers, .. } => {
                    assert_eq!(
                        headers.get("Authorization").map(|s| s.as_str()),
                        Some("Bearer abc-123")
                    );
                }
                _ => panic!("expected Http"),
            }
        }

        #[test]
        fn parser_reads_http_server_timeout_ms() {
            let dir = fixture_project_with_mcp_toml(
                r#"
                [[server]]
                transport = "http"
                name = "x"
                url = "http://x"
                timeout_ms = 8000
                "#,
            );
            let reg = load_registry_from_project(dir.path());
            assert_eq!(reg.get("x").unwrap().timeout_ms(), Some(8000));
        }

        #[test]
        fn parser_defaults_missing_transport_to_stdio_for_backcompat() {
            // D5: legacy mcp.toml without `transport` field still parses.
            let dir = fixture_project_with_mcp_toml(
                r#"
                [[server]]
                name = "legacy"
                command = "echo"
                "#,
            );
            let reg = load_registry_from_project(dir.path());
            let cfg = reg.get("legacy").unwrap();
            assert!(
                matches!(&*cfg, McpServerConfig::Stdio { .. }),
                "missing transport must default to Stdio (D5 backcompat)"
            );
        }

        #[test]
        fn parser_returns_empty_registry_on_unknown_transport() {
            // load_registry_from_project swallows parse errors and yields
            // an empty registry (loose contract — see existing tests).
            // The deserializer error itself is exercised in the next test.
            let dir = fixture_project_with_mcp_toml(
                r#"
                [[server]]
                transport = "websocket"
                name = "x"
                url = "ws://x"
                "#,
            );
            let reg = load_registry_from_project(dir.path());
            assert!(
                reg.is_empty(),
                "unknown transport must surface as empty registry"
            );
        }

        #[test]
        fn parser_reads_mixed_stdio_and_http_servers_in_one_file() {
            let dir = fixture_project_with_mcp_toml(
                r#"
                [[server]]
                name = "github"
                command = "npx"
                args = ["-y", "@modelcontextprotocol/server-github"]

                [[server]]
                transport = "http"
                name = "remote"
                url = "https://remote.example.com"
                "#,
            );
            let reg = load_registry_from_project(dir.path());
            assert_eq!(reg.len(), 2);
            assert!(matches!(
                &*reg.get("github").unwrap(),
                McpServerConfig::Stdio { .. }
            ));
            assert!(matches!(
                &*reg.get("remote").unwrap(),
                McpServerConfig::Http { .. }
            ));
        }

        #[test]
        fn parser_emits_parse_error_when_http_server_missing_url() {
            // Internal: parse_registry_toml returns Err — but the public
            // load_registry_from_project swallows it. Verify by calling
            // the inner parser directly.
            let res = parse_registry_toml(
                r#"
                [[server]]
                transport = "http"
                name = "no-url"
                "#,
            );
            let err = res.expect_err("missing url must be a parse error");
            assert!(
                format!("{err}").contains("url"),
                "error must mention `url`; got {err}"
            );
        }
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
                timeout_secs: None,
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
        let res = handle_mcp(
            McpCmd::Discover { server: None, timeout_secs: None },
            dir.path(),
            cache,
        )
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
                timeout_secs: None,
            },
            dir.path(),
            cache.clone(),
        )
        .await;
        assert!(res.is_ok());
        // Unreachable → not cached, but no error returned (operator-friendly).
        assert!(cache.get("alpha").is_none());
    }

    // ── Phase 34 (mcp-http-and-discover-flake) — --timeout-secs flag ──

    pub mod timeout_flag {
        use super::*;
        use clap::Parser;

        // Wrap McpCmd in a top-level Parser shell to exercise the
        // declarative #[arg(long)] derivation end-to-end.
        #[derive(Parser)]
        struct Wrap {
            #[command(subcommand)]
            cmd: McpCmd,
        }

        #[test]
        fn discover_command_accepts_timeout_secs_flag_long_form() {
            let parsed = Wrap::try_parse_from([
                "x", "discover", "--timeout-secs", "30",
            ]).expect("clap must accept the flag");
            match parsed.cmd {
                McpCmd::Discover { server, timeout_secs } => {
                    assert_eq!(server, None);
                    assert_eq!(timeout_secs, Some(30));
                }
                _ => panic!("expected Discover variant"),
            }
        }

        #[test]
        fn discover_command_accepts_timeout_secs_with_server_arg() {
            let parsed = Wrap::try_parse_from([
                "x", "discover", "fs", "--timeout-secs", "45",
            ]).expect("clap must accept flag + positional");
            match parsed.cmd {
                McpCmd::Discover { server, timeout_secs } => {
                    assert_eq!(server.as_deref(), Some("fs"));
                    assert_eq!(timeout_secs, Some(45));
                }
                _ => panic!(),
            }
        }

        #[test]
        fn discover_command_omitted_flag_defaults_to_none() {
            let parsed = Wrap::try_parse_from(["x", "discover"]).unwrap();
            match parsed.cmd {
                McpCmd::Discover { timeout_secs, .. } => assert_eq!(timeout_secs, None),
                _ => panic!(),
            }
        }

        #[test]
        fn discover_command_rejects_non_numeric_timeout() {
            let res = Wrap::try_parse_from([
                "x", "discover", "--timeout-secs", "not-a-number",
            ]);
            assert!(res.is_err(), "clap must reject non-numeric u64");
        }
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
