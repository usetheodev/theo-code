//! T3.1 — Agent-callable LSP tool family.
//!
//! Wraps `LspSessionManager` so the agent can invoke `lsp_definition`,
//! `lsp_references`, `lsp_hover`, `lsp_rename` against the project's
//! native language servers (rust-analyzer, pyright, gopls, etc.).
//!
//! The tools share one `Arc<LspSessionManager>` constructed by the
//! registry on session start. Each tool reads the file's extension to
//! select a server, lazily spawns + initialises it on first use, and
//! caches the client across subsequent calls.
//!
//! Failure modes the agent will see:
//!   - "no LSP server installed for `.xyz`" → install a server and
//!     re-run, OR use grep/glob/codesearch as a fallback.
//!   - "file `<x>` has no extension" → not routable to LSP.
//!   - LSP server returned an error → surfaced verbatim with the
//!     server name so the agent can decide whether to retry or fall
//!     back to a different tool.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

use crate::lsp::client::LspClient;
use crate::lsp::operations;
use crate::lsp::protocol::JsonRpcResponse;
use crate::lsp::session_manager::{LspSessionError, LspSessionManager};

/// Common args every LSP positional tool accepts.
struct PositionArgs {
    file_path: PathBuf,
    line: u32,
    character: u32,
}

impl PositionArgs {
    fn parse(args: &Value) -> Result<Self, ToolError> {
        let file_path = args
            .get("file_path")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidArgs("missing string `file_path`".into())
            })?;
        let line = args
            .get("line")
            .and_then(Value::as_u64)
            .ok_or_else(|| ToolError::InvalidArgs("missing integer `line`".into()))?;
        let character = args
            .get("character")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ToolError::InvalidArgs("missing integer `character`".into())
            })?;
        Ok(Self {
            file_path: PathBuf::from(file_path),
            line: line as u32,
            character: character as u32,
        })
    }
}

fn position_schema(extra: Vec<ToolParam>) -> ToolSchema {
    let mut params = vec![
        ToolParam {
            name: "file_path".into(),
            param_type: "string".into(),
            description:
                "Absolute path to the source file. The extension drives \
                 server selection (`.rs` → rust-analyzer, `.py` → pyright, etc.)."
                    .into(),
            required: true,
        },
        ToolParam {
            name: "line".into(),
            param_type: "integer".into(),
            description: "Zero-based line number of the symbol of interest.".into(),
            required: true,
        },
        ToolParam {
            name: "character".into(),
            param_type: "integer".into(),
            description: "Zero-based UTF-16 character offset within the line.".into(),
            required: true,
        },
    ];
    params.extend(extra);
    ToolSchema {
        params,
        input_examples: vec![json!({
            "file_path": "/abs/path/src/lib.rs",
            "line": 42,
            "character": 12,
        })],
    }
}

/// Resolve the file's extension OR surface a typed `ToolError`.
fn extension_or_error(path: &Path) -> Result<&str, ToolError> {
    path.extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "file `{}` has no extension; LSP routing requires one",
                path.display()
            ))
        })
}

/// Map a `LspSessionError` to a `ToolError` with an actionable message.
fn map_session_error(err: LspSessionError) -> ToolError {
    match err {
        LspSessionError::NoServerForExtension { ext } => ToolError::Execution(format!(
            "no LSP server installed for `.{ext}` files. Install one (e.g. \
             rust-analyzer for `.rs`, pyright for `.py`) or fall back to \
             grep/codesearch."
        )),
        LspSessionError::MissingExtension { path } => {
            ToolError::InvalidArgs(format!("file `{path}` has no extension"))
        }
        LspSessionError::InitializeFailed(msg) => ToolError::Execution(format!(
            "LSP `initialize` failed: {msg}"
        )),
        LspSessionError::Client(e) => ToolError::Execution(format!("LSP client error: {e}")),
    }
}

/// Send the open notification + the request, return the typed response.
async fn open_and_request(
    client: &LspClient,
    file_path: &Path,
    method: &'static str,
    params: Value,
) -> Result<JsonRpcResponse, ToolError> {
    // Read the file so we can send `textDocument/didOpen` with the
    // current contents. LSP servers refuse to answer position queries
    // on documents they haven't seen.
    let text = std::fs::read_to_string(file_path).map_err(|e| {
        ToolError::Execution(format!(
            "could not read `{}`: {e}",
            file_path.display()
        ))
    })?;
    let uri = operations::path_to_uri(&file_path.to_string_lossy());
    let language_id = lang_id_for_extension(file_path);

    let did_open_params = json!({
        "textDocument": {
            "uri": uri,
            "languageId": language_id,
            "version": 1,
            "text": text,
        }
    });
    client
        .notify("textDocument/didOpen", Some(did_open_params))
        .await
        .map_err(|e| ToolError::Execution(format!("didOpen failed: {e}")))?;

    let resp = client
        .request(method, Some(params))
        .await
        .map_err(|e| ToolError::Execution(format!("{method} failed: {e}")))?;
    if let Some(err) = resp.error.as_ref() {
        return Err(ToolError::Execution(format!(
            "LSP server returned error: code={}, message={}",
            err.code, err.message
        )));
    }
    Ok(resp)
}

/// Map common file extensions to LSP `languageId` strings. The
/// language id MUST match the LSP spec (`rust`, `python`, `typescript`,
/// not `rs` / `py` / `ts`) — servers reject unknown ids on didOpen.
fn lang_id_for_extension(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => "rust",
        "py" | "pyi" => "python",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "go" => "go",
        "c" => "c",
        "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" | "hxx" => "cpp",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "rb" => "ruby",
        "lua" => "lua",
        _ => "plaintext",
    }
}

// ---------------------------------------------------------------------------
// `lsp_definition`
// ---------------------------------------------------------------------------

/// `lsp_definition` — find the source location where a symbol is defined.
pub struct LspDefinitionTool {
    manager: Arc<LspSessionManager>,
}

impl LspDefinitionTool {
    pub fn new(manager: Arc<LspSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for LspDefinitionTool {
    fn id(&self) -> &str {
        "lsp_definition"
    }

    fn description(&self) -> &str {
        "T3.1 — Jump to the definition of the symbol at file_path:line:character. \
         Uses the project's installed LSP server (rust-analyzer, pyright, gopls, \
         tsserver, clangd, etc.) — accuracy beats grep/codesearch for navigation. \
         Returns one or more {uri, range} entries the agent can read with `read`. \
         Falls back gracefully when no server is installed for the file's extension. \
         Example: lsp_definition({file_path: \"/abs/src/lib.rs\", line: 42, character: 12})."
    }

    fn schema(&self) -> ToolSchema {
        position_schema(Vec::new())
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let pos = PositionArgs::parse(&args)?;
        let _ = extension_or_error(&pos.file_path)?;

        let client = self
            .manager
            .ensure_client_for(&pos.file_path, &ctx.project_dir)
            .await
            .map_err(map_session_error)?;

        let uri = operations::path_to_uri(&pos.file_path.to_string_lossy());
        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": pos.line, "character": pos.character},
        });
        let resp = open_and_request(
            client.as_ref(),
            &pos.file_path,
            "textDocument/definition",
            params,
        )
        .await?;
        Ok(format_definition_output(&resp))
    }
}

fn format_definition_output(resp: &JsonRpcResponse) -> ToolOutput {
    let result = resp.result.as_ref();
    // LSP `definition` returns Location | Location[] | LocationLink[] | null.
    // Normalise to a Vec<{uri, range}> for the metadata; render lines for the
    // text output.
    let entries = collect_locations(result);
    if entries.is_empty() {
        return ToolOutput::new(
            "lsp_definition: no definition found",
            "The LSP server returned no locations for the requested position.",
        )
        .with_metadata(json!({
            "type": "lsp_definition",
            "matched": 0,
            "results": [],
        }));
    }
    let mut out = format!("lsp_definition: {} location(s)\n\n", entries.len());
    for (i, e) in entries.iter().enumerate() {
        out.push_str(&format!(
            "{rank}. {uri}:{line}:{character}\n",
            rank = i + 1,
            uri = e.uri,
            line = e.line,
            character = e.character,
        ));
    }
    let meta_results: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "uri": e.uri,
                "line": e.line,
                "character": e.character,
            })
        })
        .collect();
    ToolOutput::new(format!("lsp_definition: {} hit(s)", entries.len()), out)
        .with_metadata(json!({
            "type": "lsp_definition",
            "matched": entries.len(),
            "results": meta_results,
        }))
}

#[derive(Debug, Clone, PartialEq)]
struct LocationEntry {
    uri: String,
    line: u64,
    character: u64,
}

fn collect_locations(result: Option<&Value>) -> Vec<LocationEntry> {
    let Some(v) = result else { return Vec::new() };
    if v.is_null() {
        return Vec::new();
    }
    if v.is_array() {
        return v
            .as_array()
            .unwrap()
            .iter()
            .filter_map(extract_location)
            .collect();
    }
    extract_location(v).into_iter().collect()
}

/// Pull `(uri, line, character)` from either a `Location` or a
/// `LocationLink`. LocationLink uses `targetUri` + `targetRange`.
fn extract_location(v: &Value) -> Option<LocationEntry> {
    let (uri, range) = if let Some(uri) = v.get("uri").and_then(Value::as_str) {
        (uri.to_string(), v.get("range")?)
    } else if let Some(uri) = v.get("targetUri").and_then(Value::as_str) {
        (uri.to_string(), v.get("targetRange")?)
    } else {
        return None;
    };
    let start = range.get("start")?;
    let line = start.get("line")?.as_u64()?;
    let character = start.get("character")?.as_u64()?;
    Some(LocationEntry { uri, line, character })
}

#[cfg(test)]
mod tests {
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
}
