//! Phase 22 (sota-gaps-followup) — real MCP server integration test.
//!
//! Spawns `npx @modelcontextprotocol/server-filesystem <tmp>` as a child
//! stdio process and exercises the full protocol path: handshake →
//! `tools/list` → `tools/call`. Gated by `MCP_REAL_TEST=1` env AND `npx`
//! presence in $PATH so the unit suite stays hermetic by default.
//!
//! Run with:
//!
//!     MCP_REAL_TEST=1 cargo test -p theo-infra-mcp --test real_server -- --ignored

use std::collections::BTreeMap;
use std::sync::Arc;

use tempfile::TempDir;

use theo_infra_mcp::{
    DiscoveryCache, DEFAULT_PER_SERVER_TIMEOUT, McpDispatcher, McpRegistry,
    McpServerConfig,
};

/// Skip when `MCP_REAL_TEST` is unset or `npx` is missing. Returns
/// `Some((registry, tmpdir))` when ready to run.
fn maybe_setup() -> Option<(McpRegistry, TempDir)> {
    if std::env::var("MCP_REAL_TEST").map(|v| v == "0").unwrap_or(true) {
        eprintln!("real_server: skipped (set MCP_REAL_TEST=1 to enable)");
        return None;
    }
    if which_npx().is_none() {
        eprintln!("real_server: skipped (npx not found in PATH)");
        return None;
    }
    let tmp = TempDir::new().ok()?;
    // Seed a known file so read_file has something to return.
    std::fs::write(tmp.path().join("hello.txt"), "real-mcp-greeting").ok()?;

    let mut reg = McpRegistry::new();
    reg.register(McpServerConfig::Stdio {
        name: "fs".into(),
        command: "npx".into(),
        args: vec![
            "-y".into(),
            "@modelcontextprotocol/server-filesystem".into(),
            tmp.path().to_string_lossy().to_string(),
        ],
        env: BTreeMap::new(),
        timeout_ms: None,
    });
    Some((reg, tmp))
}

fn which_npx() -> Option<std::path::PathBuf> {
    // Lightweight `which`: walk PATH manually (avoids extra dep).
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("npx");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[tokio::test]
#[ignore = "requires MCP_REAL_TEST=1 and npx in PATH"]
async fn real_mcp_filesystem_server_lists_expected_tools() {
    let Some((reg, _tmp)) = maybe_setup() else { return };
    let cache = DiscoveryCache::new();
    let report = cache
        .discover_filtered(&reg, &["fs".to_string()], DEFAULT_PER_SERVER_TIMEOUT)
        .await;
    assert!(
        report.successful.contains(&"fs".to_string()),
        "discover_filtered must succeed; report={:?}",
        report
    );
    let tools = cache.get("fs").expect("cache must have entry");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    // The filesystem server exposes at least these tools (per its README).
    for expected in ["read_file", "list_directory"] {
        assert!(
            names.iter().any(|n| *n == expected),
            "expected '{}' in {:?}",
            expected,
            names
        );
    }
}

#[tokio::test]
#[ignore = "requires MCP_REAL_TEST=1 and npx in PATH"]
async fn real_mcp_filesystem_server_calls_read_file() {
    let Some((reg, tmp)) = maybe_setup() else { return };
    let dispatcher = McpDispatcher::new(Arc::new(reg));
    let outcome = dispatcher
        .dispatch(
            "mcp:fs:read_file",
            serde_json::json!({
                "path": tmp.path().join("hello.txt").to_string_lossy(),
            }),
        )
        .await
        .expect("dispatch must succeed");
    assert!(
        outcome.text.contains("real-mcp-greeting"),
        "tool/call result must echo file contents; got: {}",
        outcome.text
    );
    assert!(!outcome.is_error, "is_error must be false on success");
}

#[tokio::test]
#[ignore = "requires MCP_REAL_TEST=1 and npx in PATH"]
async fn real_mcp_filesystem_server_handles_invalid_args() {
    let Some((reg, _tmp)) = maybe_setup() else { return };
    let dispatcher = McpDispatcher::new(Arc::new(reg));
    let result = dispatcher
        .dispatch(
            "mcp:fs:read_file",
            serde_json::json!({
                "path": "/nonexistent/path/that/should/not/exist/xyz",
            }),
        )
        .await;
    // Either: server returns Ok with is_error=true OR Err — both are
    // acceptable signals that the invalid input was rejected.
    match result {
        Ok(outcome) => {
            assert!(
                outcome.is_error || outcome.text.to_lowercase().contains("error")
                    || outcome.text.to_lowercase().contains("not found")
                    || outcome.text.to_lowercase().contains("no such"),
                "expected error indicator; got: {}",
                outcome.text
            );
        }
        Err(_) => { /* RPC error is acceptable */ }
    }
}
