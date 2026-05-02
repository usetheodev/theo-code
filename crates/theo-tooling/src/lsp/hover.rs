//! Single-tool slice extracted from `lsp/tool.rs` (T1.3 of god-files-2026-07-23-plan.md, ADR D2).

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

use crate::lsp::tool_common::*;

// ---------------------------------------------------------------------------
// `lsp_hover`
// ---------------------------------------------------------------------------

/// `lsp_hover` — return the LSP server's documentation for a symbol.
pub struct LspHoverTool {
    manager: Arc<LspSessionManager>,
}

impl LspHoverTool {
    pub fn new(manager: Arc<LspSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for LspHoverTool {
    fn id(&self) -> &str {
        "lsp_hover"
    }

    fn description(&self) -> &str {
        "T3.1 — Show the LSP server's hover documentation for the symbol at \
         file_path:line:character. Includes type signature, doc comments, and \
         (in some servers) examples. Cheaper than reading whole files when you \
         only need to know what a function takes / returns. \
         Example: lsp_hover({file_path: \"/abs/src/lib.rs\", line: 42, character: 12})."
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
            ctx,
            client.as_ref(),
            &pos.file_path,
            "textDocument/hover",
            params,
        )
        .await?;
        Ok(format_hover_output(&resp))
    }
}


pub fn format_hover_output(resp: &JsonRpcResponse) -> ToolOutput {
    let result = resp.result.as_ref();
    let body = result.and_then(extract_hover_text).unwrap_or_default();
    if body.is_empty() {
        return ToolOutput::new(
            "lsp_hover: no documentation",
            "The LSP server returned no hover content for the requested position.",
        )
        .with_metadata(json!({
            "type": "lsp_hover",
            "matched": 0,
            "contents": "",
        }));
    }
    let preview = body.lines().next().unwrap_or("");
    ToolOutput::new(format!("lsp_hover: {preview}"), body.clone()).with_metadata(json!({
        "type": "lsp_hover",
        "matched": 1,
        "contents": body,
    }))
}

/// Pull the displayable text out of a hover result. LSP `Hover.contents`
/// is `MarkupContent | MarkedString | MarkedString[]`. We flatten to
/// a newline-joined string. Unknown shapes return None.
pub fn extract_hover_text(v: &Value) -> Option<String> {
    let contents = v.get("contents")?;
    if contents.is_null() {
        return None;
    }
    Some(flatten_contents(contents))
}

pub fn flatten_contents(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    // MarkedString { language, value }
    if let Some(value) = v.get("value").and_then(Value::as_str) {
        return value.to_string();
    }
    if let Some(arr) = v.as_array() {
        let parts: Vec<String> = arr.iter().map(flatten_contents).collect();
        return parts.join("\n\n");
    }
    String::new()
}
