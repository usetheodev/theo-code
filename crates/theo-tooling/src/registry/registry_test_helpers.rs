//! Shared test fixtures for registry_*_tests.rs sibling files (T3.7 split).
#![cfg(test)]
#![allow(unused_imports)]

use async_trait::async_trait;

use super::*;

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool as DomainTool, ToolCategory, ToolContext, ToolOutput as DomainOutput,
};

pub(super) struct DeferredStub {
    pub id: &'static str,
    pub hint: &'static str,
}

#[async_trait]
impl DomainTool for DeferredStub {
    fn id(&self) -> &str {
        self.id
    }
    fn description(&self) -> &str {
        "deferred test tool"
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some(self.hint)
    }
    async fn execute(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolContext,
        _perm: &mut PermissionCollector,
    ) -> Result<DomainOutput, ToolError> {
        unreachable!()
    }
}
