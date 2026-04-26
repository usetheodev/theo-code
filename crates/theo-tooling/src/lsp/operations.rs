//! T3.1 — LSP operation builders.
//!
//! Each function constructs a `JsonRpcRequest` for the corresponding
//! LSP method. Pure JSON construction — no IO, testable in isolation.
//! The future `client.rs` sends these requests and parses the
//! responses; this module is the wire-format dictionary.
//!
//! Methods covered:
//! - `initialize` — first call, advertises client capabilities
//! - `textDocument/didOpen` — notification
//! - `textDocument/didChange` — notification
//! - `textDocument/rename` — request
//! - `textDocument/references` — request
//! - `textDocument/definition` — request
//! - `textDocument/hover` — request
//! - `textDocument/codeAction` — request
//! - `shutdown` / `exit` — lifecycle
//!
//! See <https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/>.

use serde_json::{Value, json};

use crate::lsp::protocol::{JsonRpcNotification, JsonRpcRequest};

/// LSP `Position` — zero-based line + UTF-16 character offset.
pub fn position(line: u32, character: u32) -> Value {
    json!({"line": line, "character": character})
}

/// LSP `TextDocumentIdentifier` — `{ "uri": "file:///abs/path" }`.
pub fn text_document(uri: &str) -> Value {
    json!({"uri": uri})
}

/// Convert an absolute filesystem path to an LSP `file://` URI.
/// Caller is responsible for passing a canonical path.
pub fn path_to_uri(absolute_path: &str) -> String {
    if absolute_path.starts_with("file://") {
        return absolute_path.to_string();
    }
    if absolute_path.starts_with('/') {
        format!("file://{absolute_path}")
    } else {
        // Windows-style — caller is responsible for forward-slashing
        // before calling. Best effort: prefix with `file:///` so the
        // LSP server can parse `file:///C:/...`.
        format!("file:///{}", absolute_path.replace('\\', "/"))
    }
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// `initialize` — first request after spawn. Advertises client
/// capabilities; servers reply with their own.
///
/// Minimal capabilities set: most servers function without advertising
/// the full capability matrix.
pub fn initialize(seq: u64, root_uri: &str, process_id: Option<u32>) -> JsonRpcRequest {
    let pid = process_id
        .map(|p| Value::from(p as u64))
        .unwrap_or(Value::Null);
    JsonRpcRequest::new(
        seq,
        "initialize",
        Some(json!({
            "processId": pid,
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "synchronization": {"didSave": true},
                    "rename": {"dynamicRegistration": false},
                    "references": {"dynamicRegistration": false},
                    "definition": {"dynamicRegistration": false},
                    "hover": {"dynamicRegistration": false},
                    "codeAction": {"dynamicRegistration": false},
                },
                "workspace": {
                    "workspaceFolders": true,
                }
            },
            "workspaceFolders": [
                {"uri": root_uri, "name": "workspace"}
            ]
        })),
    )
}

/// `initialized` — notification sent after the `initialize` reply.
/// No params.
pub fn initialized() -> JsonRpcNotification {
    JsonRpcNotification::new("initialized", Some(json!({})))
}

/// `shutdown` — request to prepare the server for exit.
pub fn shutdown(seq: u64) -> JsonRpcRequest {
    JsonRpcRequest::new(seq, "shutdown", None)
}

/// `exit` — final notification. Server terminates after this.
pub fn exit() -> JsonRpcNotification {
    JsonRpcNotification::new("exit", None)
}

// ---------------------------------------------------------------------------
// Text document sync (notifications — no response)
// ---------------------------------------------------------------------------

/// `textDocument/didOpen` — notify the server that the client has
/// opened a document and is now its source of truth.
pub fn did_open(uri: &str, language_id: &str, version: i64, text: &str) -> JsonRpcNotification {
    JsonRpcNotification::new(
        "textDocument/didOpen",
        Some(json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": version,
                "text": text,
            }
        })),
    )
}

/// `textDocument/didChange` — full-text replacement (simplest mode;
/// servers may also accept incremental ranges, deferred).
pub fn did_change_full(uri: &str, version: i64, full_text: &str) -> JsonRpcNotification {
    JsonRpcNotification::new(
        "textDocument/didChange",
        Some(json!({
            "textDocument": {"uri": uri, "version": version},
            "contentChanges": [{"text": full_text}]
        })),
    )
}

/// `textDocument/didClose` — notify the server we're done editing.
pub fn did_close(uri: &str) -> JsonRpcNotification {
    JsonRpcNotification::new(
        "textDocument/didClose",
        Some(json!({"textDocument": text_document(uri)})),
    )
}

// ---------------------------------------------------------------------------
// Operations (requests — produce responses)
// ---------------------------------------------------------------------------

/// `textDocument/rename` — rename a symbol across the workspace. The
/// server returns a `WorkspaceEdit`.
pub fn rename(seq: u64, uri: &str, line: u32, character: u32, new_name: &str) -> JsonRpcRequest {
    JsonRpcRequest::new(
        seq,
        "textDocument/rename",
        Some(json!({
            "textDocument": text_document(uri),
            "position": position(line, character),
            "newName": new_name,
        })),
    )
}

/// `textDocument/references` — find every reference to the symbol at
/// the given position. `include_declaration` controls whether the
/// declaration itself is part of the result.
pub fn references(
    seq: u64,
    uri: &str,
    line: u32,
    character: u32,
    include_declaration: bool,
) -> JsonRpcRequest {
    JsonRpcRequest::new(
        seq,
        "textDocument/references",
        Some(json!({
            "textDocument": text_document(uri),
            "position": position(line, character),
            "context": {"includeDeclaration": include_declaration},
        })),
    )
}

/// `textDocument/definition` — go-to-definition.
pub fn definition(seq: u64, uri: &str, line: u32, character: u32) -> JsonRpcRequest {
    JsonRpcRequest::new(
        seq,
        "textDocument/definition",
        Some(json!({
            "textDocument": text_document(uri),
            "position": position(line, character),
        })),
    )
}

/// `textDocument/hover` — hover info at position.
pub fn hover(seq: u64, uri: &str, line: u32, character: u32) -> JsonRpcRequest {
    JsonRpcRequest::new(
        seq,
        "textDocument/hover",
        Some(json!({
            "textDocument": text_document(uri),
            "position": position(line, character),
        })),
    )
}

/// `textDocument/codeAction` — list available code actions
/// (refactorings, quick fixes) for a range.
pub fn code_action(
    seq: u64,
    uri: &str,
    start_line: u32,
    start_char: u32,
    end_line: u32,
    end_char: u32,
) -> JsonRpcRequest {
    JsonRpcRequest::new(
        seq,
        "textDocument/codeAction",
        Some(json!({
            "textDocument": text_document(uri),
            "range": {
                "start": position(start_line, start_char),
                "end": position(end_line, end_char),
            },
            "context": {"diagnostics": []},
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- URI helpers ----

    #[test]
    fn t31ops_path_to_uri_unix_absolute() {
        assert_eq!(path_to_uri("/abs/path"), "file:///abs/path");
    }

    #[test]
    fn t31ops_path_to_uri_already_uri_passes_through() {
        assert_eq!(
            path_to_uri("file:///already/uri.rs"),
            "file:///already/uri.rs"
        );
    }

    #[test]
    fn t31ops_path_to_uri_windows_uses_forward_slashes() {
        let u = path_to_uri("C:\\Users\\x\\file.rs");
        assert!(u.starts_with("file:///"));
        assert!(!u.contains('\\'));
    }

    #[test]
    fn t31ops_position_shape() {
        let p = position(10, 5);
        assert_eq!(p["line"], 10);
        assert_eq!(p["character"], 5);
    }

    #[test]
    fn t31ops_text_document_carries_uri() {
        let d = text_document("file:///x");
        assert_eq!(d["uri"], "file:///x");
    }

    // ---- Lifecycle ----

    #[test]
    fn t31ops_initialize_includes_root_uri_and_process_id() {
        let req = initialize(1, "file:///workspace", Some(12345));
        assert_eq!(req.method, "initialize");
        let p = req.params.unwrap();
        assert_eq!(p["rootUri"], "file:///workspace");
        assert_eq!(p["processId"], 12345);
        assert!(p["capabilities"]["textDocument"]["rename"].is_object());
    }

    #[test]
    fn t31ops_initialize_without_process_id_uses_null() {
        let req = initialize(1, "file:///x", None);
        let p = req.params.unwrap();
        assert!(p["processId"].is_null());
    }

    #[test]
    fn t31ops_initialize_workspace_folders_match_root_uri() {
        let req = initialize(1, "file:///root", None);
        let p = req.params.unwrap();
        let wf = p["workspaceFolders"].as_array().unwrap();
        assert_eq!(wf[0]["uri"], "file:///root");
    }

    #[test]
    fn t31ops_initialized_is_notification_with_empty_params() {
        let n = initialized();
        assert_eq!(n.method, "initialized");
        assert_eq!(n.params.unwrap(), json!({}));
    }

    #[test]
    fn t31ops_shutdown_request_has_no_params() {
        let req = shutdown(99);
        assert_eq!(req.method, "shutdown");
        assert_eq!(req.id, 99);
        assert!(req.params.is_none());
    }

    #[test]
    fn t31ops_exit_is_notification_with_no_params() {
        let n = exit();
        assert_eq!(n.method, "exit");
        assert!(n.params.is_none());
    }

    // ---- Document sync ----

    #[test]
    fn t31ops_did_open_carries_full_text_and_metadata() {
        let n = did_open("file:///foo.rs", "rust", 1, "fn main() {}");
        let p = n.params.unwrap();
        assert_eq!(p["textDocument"]["uri"], "file:///foo.rs");
        assert_eq!(p["textDocument"]["languageId"], "rust");
        assert_eq!(p["textDocument"]["version"], 1);
        assert_eq!(p["textDocument"]["text"], "fn main() {}");
    }

    #[test]
    fn t31ops_did_change_full_replacement_form() {
        let n = did_change_full("file:///x", 2, "new text");
        let p = n.params.unwrap();
        assert_eq!(p["textDocument"]["version"], 2);
        let cc = p["contentChanges"].as_array().unwrap();
        assert_eq!(cc.len(), 1);
        assert_eq!(cc[0]["text"], "new text");
    }

    #[test]
    fn t31ops_did_close_minimal_params() {
        let n = did_close("file:///x");
        let p = n.params.unwrap();
        assert_eq!(p["textDocument"]["uri"], "file:///x");
    }

    // ---- Operations ----

    #[test]
    fn t31ops_rename_includes_position_and_new_name() {
        let req = rename(7, "file:///x", 10, 5, "new_fn");
        assert_eq!(req.method, "textDocument/rename");
        assert_eq!(req.id, 7);
        let p = req.params.unwrap();
        assert_eq!(p["position"]["line"], 10);
        assert_eq!(p["position"]["character"], 5);
        assert_eq!(p["newName"], "new_fn");
    }

    #[test]
    fn t31ops_references_include_declaration_flag() {
        let req = references(1, "file:///x", 0, 0, true);
        let p = req.params.unwrap();
        assert_eq!(p["context"]["includeDeclaration"], true);
    }

    #[test]
    fn t31ops_references_exclude_declaration_flag() {
        let req = references(1, "file:///x", 0, 0, false);
        let p = req.params.unwrap();
        assert_eq!(p["context"]["includeDeclaration"], false);
    }

    #[test]
    fn t31ops_definition_minimal_request() {
        let req = definition(1, "file:///x", 10, 5);
        assert_eq!(req.method, "textDocument/definition");
        let p = req.params.unwrap();
        assert_eq!(p["position"]["line"], 10);
    }

    #[test]
    fn t31ops_hover_minimal_request() {
        let req = hover(1, "file:///x", 0, 0);
        assert_eq!(req.method, "textDocument/hover");
    }

    #[test]
    fn t31ops_code_action_includes_range_and_empty_diagnostics() {
        let req = code_action(1, "file:///x", 0, 0, 5, 10);
        assert_eq!(req.method, "textDocument/codeAction");
        let p = req.params.unwrap();
        assert_eq!(p["range"]["start"]["line"], 0);
        assert_eq!(p["range"]["end"]["line"], 5);
        assert_eq!(p["range"]["end"]["character"], 10);
        assert!(p["context"]["diagnostics"].is_array());
    }

    #[test]
    fn t31ops_request_serializes_with_jsonrpc_version() {
        // Smoke test that the JsonRpcRequest types we return serialize
        // with `jsonrpc: "2.0"` for wire compatibility.
        let req = rename(1, "file:///x", 0, 0, "y");
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
    }
}
