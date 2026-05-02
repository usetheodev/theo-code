//! Shared test fixtures for lsp/*_tests.rs sibling files (T3.7 split).
#![cfg(test)]
#![allow(unused_imports)]

use super::*;
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

pub(super) fn make_ctx(project_dir: PathBuf) -> ToolContext {
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

pub(super) fn empty_manager() -> Arc<LspSessionManager> {
    Arc::new(LspSessionManager::from_catalogue(HashMap::new()))
}

// ── lsp_status ────────────────────────────────────────────────

