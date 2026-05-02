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
            ctx,
            client.as_ref(),
            &pos.file_path,
            "textDocument/definition",
            params,
        )
        .await?;
        Ok(format_definition_output(&resp))
    }
}



pub fn format_definition_output(resp: &JsonRpcResponse) -> ToolOutput {
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
