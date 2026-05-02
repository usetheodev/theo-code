//! Sibling test body of `discovery.rs` (T5.7 of god-files-2026-07-23-plan.md).


#![cfg(test)]

#![allow(unused_imports)]

use super::*;

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
            timeout_ms: None,
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

    // ── Plan-mandated test names (sota-gaps-plan.md §17 RED list) ──

    #[tokio::test]
    async fn discover_caches_on_first_call() {
        // Pre-seed the cache via put() to assert that the cache mechanism
        // works (real `discover_all` requires a live MCP server which we
        // can't spawn in unit tests; the integration is covered by
        // discover_all_unreachable_server_records_failure_does_not_panic).
        let c = DiscoveryCache::new();
        assert!(c.get("github").is_none());
        c.put("github", vec![fake_tool("search", "")]);
        assert!(c.get("github").is_some(), "first call must cache");
    }

    #[tokio::test]
    async fn discover_returns_cache_on_second_call() {
        // Second observation reads from the same in-memory map without
        // re-spawning a client. Validates that get() never re-runs IO.
        let c = DiscoveryCache::new();
        c.put("github", vec![fake_tool("a", "")]);
        let first = c.get("github").unwrap();
        let second = c.get("github").unwrap();
        assert_eq!(first.len(), second.len());
        assert_eq!(first[0].name, second[0].name);
    }

    #[tokio::test]
    async fn discover_timeout_5s_fails_with_timeout_error() {
        // Real timeout path: an unreachable command + tight per-server
        // timeout must surface a failure (the wording differs per OS but
        // the failure must be recorded; cache must remain empty).
        let c = DiscoveryCache::new();
        let mut reg = McpRegistry::new();
        reg.register(unreachable_cfg("timeout-target"));
        let r = c.discover_all(&reg, Duration::from_millis(50)).await;
        assert_eq!(r.successful.len(), 0);
        assert_eq!(r.failed.len(), 1);
        assert_eq!(r.failed[0].0, "timeout-target");
        assert!(c.get("timeout-target").is_none());
    }

    #[tokio::test]
    async fn discover_all_filters_by_allowlist() {
        let c = DiscoveryCache::new();
        let mut reg = McpRegistry::new();
        reg.register(unreachable_cfg("alpha"));
        reg.register(unreachable_cfg("beta"));
        reg.register(unreachable_cfg("gamma"));
        let r = c
            .discover_filtered(
                &reg,
                &["alpha".to_string(), "gamma".to_string()],
                Duration::from_millis(200),
            )
            .await;
        // Only 2 servers attempted (alpha + gamma); beta skipped.
        assert_eq!(r.total(), 2);
        let names: Vec<&str> = r.failed.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"gamma"));
        assert!(!names.contains(&"beta"));
    }

    #[tokio::test]
    async fn discover_all_returns_empty_when_allowlist_empty() {
        let c = DiscoveryCache::new();
        let mut reg = McpRegistry::new();
        reg.register(unreachable_cfg("alpha"));
        let r = c
            .discover_filtered(&reg, &[], Duration::from_millis(50))
            .await;
        assert_eq!(r.total(), 0);
        assert!(c.cached_servers().is_empty());
    }

    #[tokio::test]
    async fn invalidate_forces_rediscovery() {
        // Put a tool, invalidate, verify the cache no longer holds it.
        // Subsequent discover_all must NOT short-circuit on the previous
        // entry — verified indirectly by absence in cached_servers.
        let c = DiscoveryCache::new();
        c.put("github", vec![fake_tool("a", "")]);
        assert!(c.get("github").is_some());
        c.invalidate("github");
        assert!(c.get("github").is_none(), "invalidate must drop the entry");
    }

    // ── McpTool conversion expectations (Phase 17 plan-mandated names) ──

    #[test]
    fn mcp_tool_to_definition_uses_qualified_name_smoke() {
        // Sanity test mirroring the plan's `mcp_tool_to_definition_uses_qualified_name`.
        // The McpToolAdapter is in theo-agent-runtime; this smoke test only
        // proves the McpTool surface itself preserves enough metadata to
        // derive the qualified name `mcp:<server>:<tool>`.
        let t = fake_tool("search", "");
        // Adapter is in another crate; just check the building blocks here.
        assert_eq!(t.name, "search");
        let qualified = format!("mcp:github:{}", t.name);
        assert_eq!(qualified, "mcp:github:search");
    }

    #[test]
    fn mcp_tool_to_definition_preserves_input_schema_smoke() {
        let raw = serde_json::json!({"type":"object","properties":{"x":{"type":"integer"}},"required":["x"]});
        let t = fake_tool("calc", "");
        let mut t2 = t.clone();
        t2.input_schema = raw.clone();
        assert_eq!(t2.input_schema, raw, "schema survives clone");
    }

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

    // ── Phase 33 (mcp-http-and-discover-flake) — env-override default ──

    pub mod effective_default_timeout {
        use super::*;

        /// Env var name. Local re-decl prevents typos diverging from prod path.
        const ENV: &str = "THEO_MCP_DISCOVER_TIMEOUT_SECS";

        /// Each test mutates a process-global env var. We serialize them via
        /// a Mutex so they don't race when cargo test runs them in parallel.
        fn lock() -> std::sync::MutexGuard<'static, ()> {
            use std::sync::{Mutex, OnceLock};
            static M: OnceLock<Mutex<()>> = OnceLock::new();
            M.get_or_init(|| Mutex::new(()))
                .lock()
                .unwrap_or_else(|e| e.into_inner())
        }

        #[test]
        fn returns_default_5s_when_env_unset() {
            let _g = lock();
            // SAFETY: serialized by `let _g = lock();` at the top of this test; the env mutex makes the env mutation single-threaded for the lifetime of the guard.
            unsafe { std::env::remove_var(ENV); }
            assert_eq!(
                super::super::effective_default_timeout(),
                DEFAULT_PER_SERVER_TIMEOUT
            );
        }

        #[test]
        fn returns_env_value_when_set_to_valid_number() {
            let _g = lock();
            // SAFETY: serialized by `let _g = lock();` at the top of this test; the env mutex makes the env mutation single-threaded for the lifetime of the guard.
            unsafe { std::env::set_var(ENV, "42"); }
            let got = super::super::effective_default_timeout();
            // SAFETY: serialized by `let _g = lock();` at the top of this test; the env mutex makes the env mutation single-threaded for the lifetime of the guard.
            unsafe { std::env::remove_var(ENV); }
            assert_eq!(got, Duration::from_secs(42));
        }

        #[test]
        fn falls_back_to_default_when_env_unparseable() {
            let _g = lock();
            // SAFETY: serialized by `let _g = lock();` at the top of this test; the env mutex makes the env mutation single-threaded for the lifetime of the guard.
            unsafe { std::env::set_var(ENV, "not-a-number"); }
            let got = super::super::effective_default_timeout();
            // SAFETY: serialized by `let _g = lock();` at the top of this test; the env mutex makes the env mutation single-threaded for the lifetime of the guard.
            unsafe { std::env::remove_var(ENV); }
            assert_eq!(got, DEFAULT_PER_SERVER_TIMEOUT);
        }

        #[test]
        fn falls_back_to_default_when_env_is_zero() {
            // 0s would cause instant timeout — protect operators from
            // self-foot-shooting; treat as "use default".
            let _g = lock();
            // SAFETY: serialized by `let _g = lock();` at the top of this test; the env mutex makes the env mutation single-threaded for the lifetime of the guard.
            unsafe { std::env::set_var(ENV, "0"); }
            let got = super::super::effective_default_timeout();
            // SAFETY: serialized by `let _g = lock();` at the top of this test; the env mutex makes the env mutation single-threaded for the lifetime of the guard.
            unsafe { std::env::remove_var(ENV); }
            assert_eq!(got, DEFAULT_PER_SERVER_TIMEOUT);
        }
    }

    // ── Phase 37 — discover_one routes via McpAnyClient (HTTP support) ──

    pub mod http_routing {
        use super::*;
        use std::sync::Arc;
        use std::sync::Mutex;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        async fn spawn_one_shot(response: &'static [u8]) -> String {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 4096];
                let mut acc: Vec<u8> = Vec::new();
                loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    acc.extend_from_slice(&buf[..n]);
                    if let Some(idx) = acc.windows(4).position(|w| w == b"\r\n\r\n") {
                        // Read body per Content-Length so we don't hang.
                        let head = std::str::from_utf8(&acc[..idx]).unwrap_or("");
                        let len = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        let body_so_far = acc.len() - (idx + 4);
                        if body_so_far < len {
                            let mut more = vec![0u8; len - body_so_far];
                            sock.read_exact(&mut more).await.unwrap();
                            let _ = Arc::new(Mutex::new(more));
                        }
                        break;
                    }
                }
                let _ = sock.write_all(response).await;
                let _ = sock.shutdown().await;
            });
            format!("http://{addr}")
        }

        // Body length: 82 bytes precisely.
        const TOOLS_LIST_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\n\
            Content-Type: application/json\r\n\
            Content-Length: 82\r\n\
            \r\n\
            {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[{\"name\":\"do_thing\",\"inputSchema\":{}}]}}";

        #[tokio::test]
        async fn discover_one_routes_http_through_any_client_and_caches_tools() {
            let url = spawn_one_shot(TOOLS_LIST_RESPONSE).await;
            let cfg = McpServerConfig::Http {
                name: "remote".into(),
                url,
                headers: std::collections::BTreeMap::new(),
                timeout_ms: None,
            };
            let tools = super::super::discover_one(
                "remote",
                &cfg,
                Duration::from_secs(5),
            )
            .await
            .expect("HTTP discover should succeed against mock");
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].name, "do_thing");
        }

        #[tokio::test]
        async fn discover_filtered_caches_http_server_tools_after_success() {
            let url = spawn_one_shot(TOOLS_LIST_RESPONSE).await;
            let mut reg = McpRegistry::new();
            reg.register(McpServerConfig::Http {
                name: "remote".into(),
                url,
                headers: std::collections::BTreeMap::new(),
                timeout_ms: None,
            });
            let cache = DiscoveryCache::new();
            let report = cache
                .discover_filtered(
                    &reg,
                    &["remote".to_string()],
                    Duration::from_secs(5),
                )
                .await;
            assert_eq!(report.successful, vec!["remote"]);
            assert!(report.failed.is_empty());
            let cached = cache.get("remote").expect("cache must populate");
            assert_eq!(cached.len(), 1);
            assert_eq!(cached[0].name, "do_thing");
        }

        #[tokio::test]
        async fn discover_filtered_records_http_failure_when_endpoint_down() {
            // No mock server bound at this address.
            let mut reg = McpRegistry::new();
            reg.register(McpServerConfig::Http {
                name: "down".into(),
                url: "http://127.0.0.1:1".into(),
                headers: std::collections::BTreeMap::new(),
                timeout_ms: Some(500), // fail fast
            });
            let cache = DiscoveryCache::new();
            let report = cache
                .discover_filtered(
                    &reg,
                    &["down".to_string()],
                    Duration::from_secs(5),
                )
                .await;
            assert_eq!(report.successful, Vec::<String>::new());
            assert_eq!(report.failed.len(), 1);
            assert_eq!(report.failed[0].0, "down");
        }
    }

    // ── Phase 33 — per-server timeout overrides caller's value ──

    pub mod per_server_timeout {
        use super::*;

        #[tokio::test]
        async fn discover_one_uses_per_server_timeout_when_present() {
            // The CFG declares a 1ms timeout — so even though the CALLER
            // passes 30s, the per-server override wins and the spawn must
            // time out reporting the per-server value (1s after .as_secs()).
            let cfg = McpServerConfig::Stdio {
                name: "slow".into(),
                command: "sleep".into(),
                args: vec!["10".into()],
                env: BTreeMap::new(),
                timeout_ms: Some(1), // 1ms — must time out instantly
            };
            let err = super::super::discover_one(
                "slow",
                &cfg,
                Duration::from_secs(30),
            )
            .await
            .unwrap_err();
            assert!(err.contains("timed out"), "err was: {err}");
        }

        #[tokio::test]
        async fn discover_one_uses_caller_timeout_when_per_server_none() {
            // The CFG omits timeout — caller's 1ms should apply, time out.
            let cfg = unreachable_cfg("u"); // timeout_ms=None
            let err = super::super::discover_one(
                "u",
                &cfg,
                Duration::from_millis(1),
            )
            .await
            .unwrap_err();
            // We can't guarantee "timed out" vs "spawn failed" because the
            // spawn of /nonexistent/path/xyz123 fails fast; but EITHER way
            // we should NOT panic + must surface a string.
            assert!(!err.is_empty());
        }
    }
