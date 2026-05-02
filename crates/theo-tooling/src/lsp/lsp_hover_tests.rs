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
fn t31lsptool_hover_id_and_category() {
    let t = LspHoverTool::new(empty_manager());
    assert_eq!(t.id(), "lsp_hover");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t31lsptool_hover_schema_validates() {
    let t = LspHoverTool::new(empty_manager());
    t.schema().validate().unwrap();
}

#[tokio::test]
async fn t31lsptool_hover_unknown_extension_returns_actionable_error() {
    let t = LspHoverTool::new(empty_manager());
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
fn t31lsptool_extract_hover_text_handles_markup_content() {
    // MarkupContent { kind: "markdown", value: "..." }
    let v = json!({
        "contents": {"kind": "markdown", "value": "fn foo(x: u32) -> bool"}
    });
    let text = extract_hover_text(&v).unwrap();
    assert_eq!(text, "fn foo(x: u32) -> bool");
}

#[test]
fn t31lsptool_extract_hover_text_handles_marked_string_array() {
    // MarkedString[] — older LSP servers (deprecated but real).
    let v = json!({
        "contents": [
            {"language": "rust", "value": "fn foo(x: u32) -> bool"},
            "Returns true when even.",
        ]
    });
    let text = extract_hover_text(&v).unwrap();
    assert!(text.contains("fn foo(x: u32) -> bool"));
    assert!(text.contains("Returns true when even."));
}

#[test]
fn t31lsptool_extract_hover_text_handles_plain_string() {
    // MarkedString as a bare string (legacy).
    let v = json!({"contents": "plain doc string"});
    let text = extract_hover_text(&v).unwrap();
    assert_eq!(text, "plain doc string");
}

#[test]
fn t31lsptool_extract_hover_text_returns_none_for_null() {
    let v = json!({"contents": null});
    assert!(extract_hover_text(&v).is_none());
}

#[test]
fn t31lsptool_extract_hover_text_returns_none_for_missing_contents() {
    let v = json!({"unrelated": "field"});
    assert!(extract_hover_text(&v).is_none());
}

#[test]
fn t31lsptool_format_hover_uses_first_line_as_title() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: 1,
        result: Some(json!({
            "contents": {"kind":"markdown","value":"fn first_line\nbody continues"}
        })),
        error: None,
    };
    let out = format_hover_output(&resp);
    assert!(out.title.contains("fn first_line"));
    assert_eq!(out.metadata["matched"], 1);
    assert_eq!(out.metadata["contents"], "fn first_line\nbody continues");
}

#[test]
fn t31lsptool_format_hover_handles_empty_response_gracefully() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: 1,
        result: Some(serde_json::Value::Null),
        error: None,
    };
    let out = format_hover_output(&resp);
    assert!(out.title.contains("no documentation"));
    assert_eq!(out.metadata["matched"], 0);
}

// ── lsp_rename ────────────────────────────────────────────────

