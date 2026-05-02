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
fn t31lsptool_definition_id_and_category() {
    let t = LspDefinitionTool::new(empty_manager());
    assert_eq!(t.id(), "lsp_definition");
    assert_eq!(t.category(), ToolCategory::Search);
}

#[test]
fn t31lsptool_definition_schema_validates() {
    let t = LspDefinitionTool::new(empty_manager());
    t.schema().validate().unwrap();
}

#[test]
fn t31lsptool_extract_location_handles_location_shape() {
    let v = json!({
        "uri": "file:///abs/x.rs",
        "range": {
            "start": {"line": 10, "character": 4},
            "end":   {"line": 10, "character": 9},
        }
    });
    let loc = extract_location(&v).unwrap();
    assert_eq!(loc.uri, "file:///abs/x.rs");
    assert_eq!(loc.line, 10);
    assert_eq!(loc.character, 4);
}

#[test]
fn t31lsptool_extract_location_handles_location_link_shape() {
    // LSP 3.14+: `LocationLink` uses `targetUri` + `targetRange`
    // instead of `uri` + `range`. Easy to miss.
    let v = json!({
        "originSelectionRange": {"start":{"line":1,"character":2},"end":{"line":1,"character":3}},
        "targetUri": "file:///abs/y.rs",
        "targetRange": {"start":{"line":20,"character":0},"end":{"line":25,"character":5}},
        "targetSelectionRange": {"start":{"line":20,"character":0},"end":{"line":20,"character":3}},
    });
    let loc = extract_location(&v).unwrap();
    assert_eq!(loc.uri, "file:///abs/y.rs");
    assert_eq!(loc.line, 20);
    assert_eq!(loc.character, 0);
}

#[test]
fn t31lsptool_extract_location_returns_none_for_unknown_shape() {
    let v = json!({"random": "shape"});
    assert!(extract_location(&v).is_none());
}

#[test]
fn t31lsptool_collect_locations_handles_array_response() {
    let v = json!([
        {"uri":"file:///a","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":3}}},
        {"uri":"file:///b","range":{"start":{"line":4,"character":5},"end":{"line":4,"character":6}}},
    ]);
    let locs = collect_locations(Some(&v));
    assert_eq!(locs.len(), 2);
    assert_eq!(locs[0].uri, "file:///a");
    assert_eq!(locs[1].uri, "file:///b");
}

#[test]
fn t31lsptool_collect_locations_handles_single_location_response() {
    let v = json!({
        "uri":"file:///a",
        "range":{"start":{"line":1,"character":2},"end":{"line":1,"character":3}}
    });
    let locs = collect_locations(Some(&v));
    assert_eq!(locs.len(), 1);
}

#[test]
fn t31lsptool_collect_locations_handles_null_response() {
    let v = serde_json::Value::Null;
    let locs = collect_locations(Some(&v));
    assert!(locs.is_empty());
}

#[test]
fn t31lsptool_collect_locations_handles_missing_result() {
    let locs = collect_locations(None);
    assert!(locs.is_empty());
}

#[test]
fn t31lsptool_format_output_includes_count_and_uris() {
    // Build a fake response with two locations.
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: 1,
        result: Some(json!([
            {"uri":"file:///a","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":3}}},
            {"uri":"file:///b","range":{"start":{"line":4,"character":5},"end":{"line":4,"character":6}}},
        ])),
        error: None,
    };
    let out = format_definition_output(&resp);
    assert!(out.output.contains("2 location(s)"));
    assert!(out.output.contains("file:///a:1:2"));
    assert!(out.output.contains("file:///b:4:5"));
    assert_eq!(out.metadata["matched"], 2);
}

#[test]
fn t31lsptool_format_output_handles_no_locations_gracefully() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: 1,
        result: Some(serde_json::Value::Null),
        error: None,
    };
    let out = format_definition_output(&resp);
    assert!(out.title.contains("no definition found"));
    assert_eq!(out.metadata["matched"], 0);
}

// ── lsp_references ────────────────────────────────────────────

