// Shared test fixtures for dap/*_tests.rs sibling files (T3.1 split).
#![cfg(test)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::dap::DapSessionManager;
use theo_domain::session::{MessageId, SessionId};
use theo_domain::tool::ToolContext;

pub(crate) fn make_ctx(project_dir: PathBuf) -> ToolContext {
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

pub(crate) fn empty_manager() -> Arc<DapSessionManager> {
    Arc::new(DapSessionManager::from_catalogue(HashMap::new()))
}
