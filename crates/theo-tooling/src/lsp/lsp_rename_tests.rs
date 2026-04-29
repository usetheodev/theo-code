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
fn t31lsptool_rename_id_and_category() {
    let t = LspRenameTool::new(empty_manager());
    assert_eq!(t.id(), "lsp_rename");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t31lsptool_rename_schema_validates_and_includes_new_name() {
    let t = LspRenameTool::new(empty_manager());
    let schema = t.schema();
    schema.validate().unwrap();
    let names: Vec<_> = schema.params.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"new_name"));
    let nn = schema.params.iter().find(|p| p.name == "new_name").unwrap();
    assert!(nn.required, "new_name must be required");
}

#[tokio::test]
async fn t31lsptool_rename_missing_new_name_returns_invalid_args() {
    let t = LspRenameTool::new(empty_manager());
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
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t31lsptool_rename_empty_new_name_returns_invalid_args() {
    let t = LspRenameTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({
                "file_path": "/tmp/x.rs",
                "line": 0,
                "character": 0,
                "new_name": "   "
            }),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    match err {
        ToolError::InvalidArgs(msg) => assert!(msg.contains("`new_name` is empty")),
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t31lsptool_rename_unknown_extension_returns_actionable_error() {
    let t = LspRenameTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({
                "file_path": "/tmp/x.rs",
                "line": 0,
                "character": 0,
                "new_name": "foo_v2"
            }),
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
fn t31lsptool_collect_rename_edits_handles_legacy_changes_shape() {
    // LSP < 3.16: changes: {uri: TextEdit[]}
    let v = json!({
        "changes": {
            "file:///a": [
                {"range":{"start":{"line":1,"character":2},"end":{"line":1,"character":7}},"newText":"foo"},
                {"range":{"start":{"line":3,"character":0},"end":{"line":3,"character":3}},"newText":"foo"},
            ],
            "file:///b": [
                {"range":{"start":{"line":5,"character":4},"end":{"line":5,"character":9}},"newText":"foo"},
            ],
        }
    });
    let edits = collect_rename_edits(Some(&v));
    assert_eq!(edits.len(), 3);
    // Edits with uri=file:///a are 2; uri=file:///b is 1.
    let a_count = edits.iter().filter(|e| e.uri == "file:///a").count();
    let b_count = edits.iter().filter(|e| e.uri == "file:///b").count();
    assert_eq!(a_count, 2);
    assert_eq!(b_count, 1);
}

#[test]
fn t31lsptool_collect_rename_edits_handles_document_changes_shape() {
    // LSP 3.16+: documentChanges: TextDocumentEdit[]
    let v = json!({
        "documentChanges": [
            {
                "textDocument": {"uri":"file:///c","version":1},
                "edits": [
                    {"range":{"start":{"line":7,"character":0},"end":{"line":7,"character":5}},"newText":"bar"},
                ]
            },
            {
                "textDocument": {"uri":"file:///d","version":1},
                "edits": [
                    {"range":{"start":{"line":9,"character":2},"end":{"line":9,"character":6}},"newText":"bar"},
                    {"range":{"start":{"line":12,"character":0},"end":{"line":12,"character":3}},"newText":"bar"},
                ]
            }
        ]
    });
    let edits = collect_rename_edits(Some(&v));
    assert_eq!(edits.len(), 3);
    // Verify endLine/endChar were captured (we render them in the
    // preview output).
    let c_edit = edits.iter().find(|e| e.uri == "file:///c").unwrap();
    assert_eq!(c_edit.line, 7);
    assert_eq!(c_edit.end_character, 5);
    assert_eq!(c_edit.new_text, "bar");
}

#[test]
fn t31lsptool_collect_rename_edits_skips_resource_ops_in_document_changes() {
    // documentChanges can contain CreateFile/RenameFile/DeleteFile
    // ops alongside text edits. We only render text edits.
    let v = json!({
        "documentChanges": [
            {"kind": "rename", "oldUri": "file:///old", "newUri": "file:///new"},
            {
                "textDocument": {"uri":"file:///e","version":1},
                "edits": [
                    {"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},"newText":"new"},
                ]
            }
        ]
    });
    let edits = collect_rename_edits(Some(&v));
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].uri, "file:///e");
}

#[test]
fn t31lsptool_collect_rename_edits_handles_null_response() {
    let v = serde_json::Value::Null;
    let edits = collect_rename_edits(Some(&v));
    assert!(edits.is_empty());
}

#[test]
fn t31lsptool_collect_rename_edits_handles_missing_response() {
    let edits = collect_rename_edits(None);
    assert!(edits.is_empty());
}

#[test]
fn t31lsptool_format_rename_marks_preview_only_in_metadata() {
    // PREVIEW-ONLY safety invariant: metadata.preview_only must
    // be true so the agent (and any downstream auditor) knows
    // this tool didn't write files.
    let v = json!({
        "changes": {
            "file:///a": [
                {"range":{"start":{"line":1,"character":2},"end":{"line":1,"character":7}},"newText":"foo_v2"},
            ]
        }
    });
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: 1,
        result: Some(v),
        error: None,
    };
    let out = format_rename_output(&resp, "foo_v2");
    assert_eq!(out.metadata["preview_only"], true);
    assert_eq!(out.metadata["new_name"], "foo_v2");
    assert_eq!(out.metadata["matched"], 1);
    assert_eq!(out.metadata["files_affected"], 1);
    assert!(out.output.contains("PREVIEW-ONLY"));
    assert!(out.output.contains("`edit` or `apply_patch`"));
}

#[test]
fn t31lsptool_format_rename_groups_edits_by_file_in_summary() {
    let v = json!({
        "changes": {
            "file:///a": [
                {"range":{"start":{"line":1,"character":2},"end":{"line":1,"character":7}},"newText":"X"},
                {"range":{"start":{"line":3,"character":0},"end":{"line":3,"character":3}},"newText":"X"},
            ],
            "file:///b": [
                {"range":{"start":{"line":5,"character":4},"end":{"line":5,"character":9}},"newText":"X"},
            ]
        }
    });
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: 1,
        result: Some(v),
        error: None,
    };
    let out = format_rename_output(&resp, "X");
    // Per-file summary lines: "  file:///a: 2 edit(s)"
    assert!(out.output.contains("file:///a: 2 edit(s)"));
    assert!(out.output.contains("file:///b: 1 edit(s)"));
    assert_eq!(out.metadata["files_affected"], 2);
    assert_eq!(out.metadata["matched"], 3);
}

#[test]
fn t31lsptool_format_rename_no_edits_path_keeps_preview_only_flag() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: 1,
        result: Some(serde_json::Value::Null),
        error: None,
    };
    let out = format_rename_output(&resp, "anything");
    assert_eq!(out.metadata["matched"], 0);
    assert_eq!(out.metadata["files_affected"], 0);
    assert!(out.title.contains("no edits proposed"));
}

