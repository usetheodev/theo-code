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
fn t31lsptool_position_schema_lists_required_params() {
    let schema = position_schema(Vec::new());
    let names: Vec<_> = schema.params.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"file_path"));
    assert!(names.contains(&"line"));
    assert!(names.contains(&"character"));
    // All three are required.
    for p in &schema.params {
        assert!(p.required, "{} should be required", p.name);
    }
}

#[tokio::test]
async fn t31lsptool_missing_file_path_returns_invalid_args() {
    let t = LspDefinitionTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(json!({"line": 1, "character": 1}), &ctx, &mut perms)
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t31lsptool_missing_line_returns_invalid_args() {
    let t = LspDefinitionTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"file_path": "/tmp/x.rs", "character": 1}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t31lsptool_missing_character_returns_invalid_args() {
    let t = LspDefinitionTool::new(empty_manager());
    let ctx = make_ctx(PathBuf::from("/tmp"));
    let mut perms = PermissionCollector::new();
    let err = t
        .execute(
            json!({"file_path": "/tmp/x.rs", "line": 1}),
            &ctx,
            &mut perms,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn t31lsptool_extensionless_file_returns_invalid_args() {
    let t = LspDefinitionTool::new(empty_manager());
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
    match err {
        ToolError::InvalidArgs(msg) => assert!(msg.contains("no extension")),
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn t31lsptool_unknown_extension_returns_actionable_execution_error() {
    // Empty manager has no servers — any known extension also
    // hits NoServerForExtension. Verify the user-facing message
    // is actionable (mentions installing a server or fallback).
    let t = LspDefinitionTool::new(empty_manager());
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
        ToolError::Execution(msg) => {
            assert!(msg.contains("no LSP server installed"));
            assert!(msg.contains("`.rs`"));
            // Actionable: tell the agent what to do instead.
            assert!(msg.contains("rust-analyzer") || msg.contains("grep"));
        }
        other => panic!("expected Execution error, got {other:?}"),
    }
}

#[test]
fn t31lsptool_lang_id_for_known_extensions() {
    assert_eq!(lang_id_for_extension(Path::new("/x.rs")), "rust");
    assert_eq!(lang_id_for_extension(Path::new("/x.py")), "python");
    assert_eq!(lang_id_for_extension(Path::new("/x.ts")), "typescript");
    assert_eq!(lang_id_for_extension(Path::new("/x.tsx")), "typescript");
    assert_eq!(lang_id_for_extension(Path::new("/x.js")), "javascript");
    assert_eq!(lang_id_for_extension(Path::new("/x.go")), "go");
    assert_eq!(lang_id_for_extension(Path::new("/x.cpp")), "cpp");
    assert_eq!(lang_id_for_extension(Path::new("/x.java")), "java");
    assert_eq!(lang_id_for_extension(Path::new("/x.rb")), "ruby");
}

#[test]
fn t31lsptool_lang_id_for_unknown_extension_falls_back_to_plaintext() {
    assert_eq!(lang_id_for_extension(Path::new("/x.xyz")), "plaintext");
    assert_eq!(lang_id_for_extension(Path::new("/no_ext")), "plaintext");
}

