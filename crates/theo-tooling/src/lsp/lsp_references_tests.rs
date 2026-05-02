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
fn t31lsptool_references_id_and_category() {
    let t = LspReferencesTool::new(empty_manager());
    assert_eq!(t.id(), "lsp_references");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t31lsptool_references_schema_validates_and_includes_optional_flag() {
    let t = LspReferencesTool::new(empty_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    let names: Vec<_> = schema.params.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"include_declaration"));
    let inc = schema
        .params
        .iter()
        .find(|p| p.name == "include_declaration")
        .unwrap();
    assert!(!inc.required, "include_declaration must be optional");
}

#[tokio::test]
async fn t31lsptool_references_extensionless_file_returns_invalid_args() {
    let t = LspReferencesTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"file_path": "/tmp/Makefile", "line": 0, "character": 0}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t31lsptool_references_unknown_extension_returns_actionable_error() {
    let t = LspReferencesTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"file_path": "/tmp/x.rs", "line": 0, "character": 0}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::Execution(msg) => assert!(msg.contains("no LSP server installed")),
        other => panic!("expected Execution error, got {other:?}"),
    }
}

#[test]
fn t31lsptool_format_references_includes_count_and_dedups() {
    // Same (uri, line, character) twice — must collapse to 1.
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: 1,
        result: Some(json!([
            {"uri":"file:///a","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":3}}},
            {"uri":"file:///a","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":4}}},
            {"uri":"file:///b","range":{"start":{"line":4,"character":5},"end":{"line":4,"character":6}}},
        ])),
        error: None,
    };
    let out = format_references_output(&resp, false);
    assert_eq!(out.metadata["matched"], 2, "duplicate (a,1,2) collapses");
    assert!(out.output.contains("file:///a:1:2"));
    assert!(out.output.contains("file:///b:4:5"));
}

#[test]
fn t31lsptool_format_references_with_declaration_marks_metadata() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: 1,
        result: Some(json!([
            {"uri":"file:///a","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":3}}},
        ])),
        error: None,
    };
    let out = format_references_output(&resp, true);
    assert_eq!(out.metadata["include_declaration"], true);
    assert!(out.output.contains("(including declaration)"));
}

#[test]
fn t31lsptool_format_references_handles_no_results_gracefully() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: 1,
        result: Some(serde_json::Value::Null),
        error: None,
    };
    let out = format_references_output(&resp, false);
    assert!(out.title.contains("no references found"));
    assert_eq!(out.metadata["matched"], 0);
}

// ── lsp_hover ─────────────────────────────────────────────────

