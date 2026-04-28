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
// `lsp_status`
// ---------------------------------------------------------------------------

/// `lsp_status` — report which LSP servers are discoverable on PATH.
/// Lets the agent decide between LSP-based navigation and a grep
/// fallback BEFORE issuing a doomed `lsp_definition` call against
/// a language with no installed server.
pub struct LspStatusTool {
    manager: Arc<LspSessionManager>,
}

impl LspStatusTool {
    pub fn new(manager: Arc<LspSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for LspStatusTool {
    fn id(&self) -> &str {
        "lsp_status"
    }

    fn description(&self) -> &str {
        "T3.1 — Report which LSP servers are discoverable on PATH and which \
         file extensions they support. Use this BEFORE `lsp_definition` / \
         `lsp_references` / `lsp_hover` / `lsp_rename` to know whether the \
         language even has a server installed; when none is available for \
         your file's extension, fall back to grep / codesearch. Empty \
         result list means no LSP server is installed at all (or PATH is \
         unusual). \
         Example: lsp_status({})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![],
            input_examples: vec![json!({})],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        _args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let mut extensions = self.manager.supported_extensions().await;
        extensions.sort();

        let count = extensions.len();
        let metadata = json!({
            "type": "lsp_status",
            "supported_extension_count": count,
            "supported_extensions": extensions,
        });
        let output = if extensions.is_empty() {
            "No LSP servers discovered on PATH. Install one for the languages \
             you work with (rust-analyzer / pyright / typescript-language-server / \
             gopls / clangd / etc.) or fall back to grep / codesearch for \
             navigation."
                .to_string()
        } else {
            let listed = extensions.join(", ");
            format!(
                "{count} extension(s) routable to an installed LSP server: {listed}.\n\
                 Use lsp_definition / lsp_references / lsp_hover / lsp_rename \
                 against files matching these extensions."
            )
        };
        Ok(ToolOutput::new(
            format!("lsp_status: {count} extension(s) routable"),
            output,
        )
        .with_metadata(metadata))
    }
}

