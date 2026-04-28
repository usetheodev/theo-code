//! Shared helpers for the lsp_* tool family.
//!
//! Extracted from `lsp/tool.rs` during T1.3 of god-files-2026-07-23-plan.md
//! (ADR D2). Each individual tool file imports from here via
//! `use crate::lsp::tool_common::*;`.

#![allow(unused_imports, dead_code)]

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
pub struct PositionArgs {
    pub file_path: PathBuf,
    pub line: u32,
    pub character: u32,
}

impl PositionArgs {
    pub fn parse(args: &Value) -> Result<Self, ToolError> {
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

pub fn position_schema(extra: Vec<ToolParam>) -> ToolSchema {
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
pub fn extension_or_error(path: &Path) -> Result<&str, ToolError> {
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
pub fn map_session_error(err: LspSessionError) -> ToolError {
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
/// T14.1 — emits a partial-progress envelope tagged with `method` so
/// each LSP call (definition / references / hover / rename) shows
/// up as a distinct progress line in the streaming UI. Cold first
/// calls hit the rust-analyzer initialize handshake and can take
/// several seconds; without progress the agent appears frozen.
pub async fn open_and_request(
    ctx: &ToolContext,
    client: &LspClient,
    file_path: &Path,
    method: &'static str,
    params: Value,
) -> Result<JsonRpcResponse, ToolError> {
    crate::partial::emit_progress(
        ctx,
        method,
        format!("Querying LSP server for {}", file_path.display()),
    );

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
pub fn lang_id_for_extension(path: &Path) -> &'static str {
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


#[derive(Debug, Clone, PartialEq)]
pub struct LocationEntry {
    pub uri: String,
    pub line: u64,
    pub character: u64,
}

pub fn collect_locations(result: Option<&Value>) -> Vec<LocationEntry> {
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
pub fn extract_location(v: &Value) -> Option<LocationEntry> {
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

