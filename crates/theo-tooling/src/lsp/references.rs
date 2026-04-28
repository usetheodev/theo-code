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
// `lsp_references`
// ---------------------------------------------------------------------------

/// `lsp_references` — find every reference to the symbol at a position.
pub struct LspReferencesTool {
    manager: Arc<LspSessionManager>,
}

impl LspReferencesTool {
    pub fn new(manager: Arc<LspSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for LspReferencesTool {
    fn id(&self) -> &str {
        "lsp_references"
    }

    fn description(&self) -> &str {
        "T3.1 — List every reference to the symbol at file_path:line:character. \
         Uses the project's installed LSP server. Pass `include_declaration: true` \
         to also include the declaration site (default: false — references only). \
         Beats grep when the symbol name is shared across modules — the LSP \
         server understands scope and returns only true references. Returns a \
         deduplicated list of {uri, line, character} entries. \
         Example: lsp_references({file_path: \"/abs/src/lib.rs\", line: 42, character: 12, include_declaration: true})."
    }

    fn schema(&self) -> ToolSchema {
        position_schema(vec![ToolParam {
            name: "include_declaration".into(),
            param_type: "boolean".into(),
            description:
                "When true, the result also includes the declaration site. Default: false."
                    .into(),
            required: false,
        }])
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
        let include_declaration = args
            .get("include_declaration")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let client = self
            .manager
            .ensure_client_for(&pos.file_path, &ctx.project_dir)
            .await
            .map_err(map_session_error)?;

        let uri = operations::path_to_uri(&pos.file_path.to_string_lossy());
        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": pos.line, "character": pos.character},
            "context": {"includeDeclaration": include_declaration},
        });
        let resp = open_and_request(
            ctx,
            client.as_ref(),
            &pos.file_path,
            "textDocument/references",
            params,
        )
        .await?;
        Ok(format_references_output(&resp, include_declaration))
    }
}

pub fn format_references_output(resp: &JsonRpcResponse, include_declaration: bool) -> ToolOutput {
    let entries = collect_locations(resp.result.as_ref());
    if entries.is_empty() {
        return ToolOutput::new(
            "lsp_references: no references found",
            "The LSP server returned no references for the requested position.",
        )
        .with_metadata(json!({
            "type": "lsp_references",
            "include_declaration": include_declaration,
            "matched": 0,
            "results": [],
        }));
    }
    // Dedup identical (uri, line, character) tuples — some servers
    // emit overlapping references when the call site spans multiple
    // ranges, and the user's view shouldn't be cluttered.
    let mut seen: std::collections::HashSet<(String, u64, u64)> =
        std::collections::HashSet::new();
    let entries: Vec<LocationEntry> = entries
        .into_iter()
        .filter(|e| seen.insert((e.uri.clone(), e.line, e.character)))
        .collect();

    let mut out = format!(
        "lsp_references: {} reference(s){}\n\n",
        entries.len(),
        if include_declaration {
            " (including declaration)"
        } else {
            ""
        }
    );
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
    ToolOutput::new(format!("lsp_references: {} hit(s)", entries.len()), out).with_metadata(
        json!({
            "type": "lsp_references",
            "include_declaration": include_declaration,
            "matched": entries.len(),
            "results": meta_results,
        }),
    )
}

