//! Sibling test body of `lsp/tool.rs` — split per-tool (T3.7 of code-hygiene-5x5).

#![cfg(test)]
#![allow(unused_imports)]

use super::*;
use super::lsp_test_helpers::*;
use super::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use serde_json::{Value, json};
use theo_domain::error::ToolError;
use theo_domain::session::{MessageId, SessionId};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput,
};
use crate::lsp::tool_common::*;
use crate::lsp::definition::*;
use crate::lsp::hover::*;
use crate::lsp::references::*;
use crate::lsp::rename::*;
use crate::lsp::status::*;

#[test]
fn t31lsptool_status_id_and_category() {
    let t = LspStatusTool::new(empty_manager());
    assert_eq!(t.id(), "lsp_status");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t31lsptool_status_schema_validates_with_no_args() {
    let t = LspStatusTool::new(empty_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    assert!(schema.params.is_empty());
}

#[tokio::test]
async fn t31lsptool_status_empty_catalogue_returns_zero_extensions() {
    // Default registry uses an empty-catalogue manager; status
    // should list zero extensions and point at grep fallback.
    let t = LspStatusTool::new(empty_manager());
    let mut perms = PermissionCollector::new();
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let out = t.execute(json!({}), &ctx, &mut perms).await.unwrap();
    assert_eq!(out.metadata["supported_extension_count"], 0);
    assert!(
        out.output.contains("No LSP servers"),
        "empty catalogue must surface the install/grep-fallback hint"
    );
    assert!(out.output.contains("grep") || out.output.contains("codesearch"));
}

#[tokio::test]
async fn t31lsptool_status_lists_extensions_alphabetically_with_count() {
    // Inject a fake catalogue with two distinct extensions and
    // verify the response sorts them deterministically + counts
    // them. Extension order matters for status-line UIs that
    // diff frame-to-frame.
    use std::collections::HashMap;
    use std::path::PathBuf;

    let mut catalogue: HashMap<&'static str, crate::lsp::DiscoveredServer> =
        HashMap::new();
    catalogue.insert(
        "rs",
        crate::lsp::DiscoveredServer {
            name: "rust-analyzer",
            command: PathBuf::from("/usr/bin/rust-analyzer"),
            args: vec![],
            file_extensions: &["rs"],
            languages: &["rust"],
        },
    );
    catalogue.insert(
        "py",
        crate::lsp::DiscoveredServer {
            name: "pyright",
            command: PathBuf::from("/usr/bin/pyright-langserver"),
            args: vec!["--stdio"],
            file_extensions: &["py", "pyi"],
            languages: &["python"],
        },
    );
    let manager = Arc::new(LspSessionManager::from_catalogue(catalogue));

    let t = LspStatusTool::new(manager);
    let mut perms = PermissionCollector::new();
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let out = t.execute(json!({}), &ctx, &mut perms).await.unwrap();

    assert_eq!(out.metadata["supported_extension_count"], 2);
    let exts = out.metadata["supported_extensions"].as_array().unwrap();
    assert_eq!(exts.len(), 2);
    // Alphabetical ordering: py before rs.
    assert_eq!(exts[0], "py");
    assert_eq!(exts[1], "rs");
    // Output text mentions both and points the agent at the
    // navigation tools.
    assert!(out.output.contains("py"));
    assert!(out.output.contains("rs"));
    assert!(out.output.contains("lsp_definition"));
}

