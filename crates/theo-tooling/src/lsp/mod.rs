//! T3.1 — Agent-callable LSP tool family + protocol primitives.
//!
//! Wraps `LspSessionManager` so the agent can invoke `lsp_definition`,
//! `lsp_references`, `lsp_hover`, `lsp_rename`, `lsp_status` against the
//! project's native language servers (rust-analyzer, pyright, gopls,
//! etc.). 5 tools, one per file. Pre-2026-04-28 the family lived in a
//! single 974-LOC `tool.rs`; the per-file split was T1.3 of
//! `docs/plans/god-files-2026-07-23-plan.md` (ADR D2).

pub mod client;
pub mod discovery;
pub mod operations;
pub mod protocol;
pub mod session_manager;

pub(crate) mod tool_common;

mod definition;
mod hover;
mod references;
mod rename;
mod status;

pub use definition::LspDefinitionTool;
pub use hover::LspHoverTool;
pub use references::LspReferencesTool;
pub use rename::LspRenameTool;
pub use status::LspStatusTool;

pub use client::{LspClient, LspClientError};
pub use discovery::{DiscoveredServer, discover, discover_with_path};
pub use protocol::{
    InboundMessage, JsonRpcErrorObj, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
    LspProtocolError, RequestIdGen, encode_frame, encode_message, try_decode_frame,
};
pub use session_manager::{LspSessionError, LspSessionManager};

// ── Legacy umbrella stub kept for the all_tools_have_valid_schemas
// ── contract test in registry/mod.rs. Production agents call the
// ── per-operation tools (LspDefinitionTool, LspReferencesTool, etc.).

use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

pub struct LspTool;

impl Default for LspTool {
    fn default() -> Self {
        Self::new()
    }
}

impl LspTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for LspTool {
    fn id(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Language Server Protocol operations (experimental)"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "operation".to_string(),
                    param_type: "string".to_string(),
                    description: "LSP operation: goToDefinition, references, hover".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "filePath".to_string(),
                    param_type: "string".to_string(),
                    description: "Path to the source file".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "line".to_string(),
                    param_type: "integer".to_string(),
                    description: "Line number (0-based)".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "character".to_string(),
                    param_type: "integer".to_string(),
                    description: "Character offset (0-based)".to_string(),
                    required: true,
                },
            ],
            input_examples: Vec::new(),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        Err(ToolError::Execution(
            "LSP tool not yet implemented".to_string(),
        ))
    }
}

// Sibling tests split per-tool (T3.7 of code-hygiene-5x5).
#[cfg(test)]
#[path = "lsp_test_helpers.rs"]
mod lsp_test_helpers;
#[cfg(test)]
#[path = "lsp_status_tests.rs"]
mod lsp_status_tests;
#[cfg(test)]
#[path = "lsp_definition_tests.rs"]
mod lsp_definition_tests;
#[cfg(test)]
#[path = "lsp_references_tests.rs"]
mod lsp_references_tests;
#[cfg(test)]
#[path = "lsp_hover_tests.rs"]
mod lsp_hover_tests;
#[cfg(test)]
#[path = "lsp_rename_tests.rs"]
mod lsp_rename_tests;
#[cfg(test)]
#[path = "lsp_common_tests.rs"]
mod lsp_common_tests;
