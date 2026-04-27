// Sibling test body of `lsp/tool.rs` re-attached via
// `#[cfg(test)] #[path = "tool_tests.rs"] mod tests;`. The inner
// attribute below is redundant for the compiler (the `mod` decl
// already cfg-gates this file) but signals to scripts/check-unwrap.sh
// and scripts/check-panic.sh that every line is test-only — so the
// production-only filter excludes the entire file from violation
// counts. Only test code lives here.
#![cfg(test)]

use super::*;
use std::collections::HashMap;
use theo_domain::session::{MessageId, SessionId};

fn make_ctx(project_dir: PathBuf) -> ToolContext {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolContext {
        session_id: SessionId::new("ses_test"),
        message_id: MessageId::new(""),
        call_id: "call_test".into(),
        agent: "build".into(),
        abort: rx,
        project_dir,
        graph_context: None,
        stdout_tx: None,
    }
}

fn empty_manager() -> Arc<LspSessionManager> {
    Arc::new(LspSessionManager::from_catalogue(HashMap::new()))
}

// ── lsp_status ────────────────────────────────────────────────

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
