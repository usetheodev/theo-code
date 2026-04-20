//! BatchTool — execute multiple tool calls in a single LLM turn.
//!
//! Inspired by CodeAct (arxiv:2402.01030) and OpenCode's BatchTool.
//! The LLM calls batch(calls: [...]) and the RunEngine executes all
//! calls sequentially without LLM round-trips between them.
//!
//! This is a meta-tool: execution happens in RunEngine, not here.
//! This module provides the schema definition only.

use async_trait::async_trait;
use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

pub struct BatchTool;

impl BatchTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for BatchTool {
    fn id(&self) -> &str {
        "batch"
    }

    fn description(&self) -> &str {
        "Execute multiple tool calls in a single turn for efficiency. Use when you need to perform independent operations like reading multiple files, running multiple searches, etc. Max 25 calls per batch."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "calls".to_string(),
                param_type: "array".to_string(),
                description: "Array of tool calls. Each item: {\"tool\": \"tool_name\", \"args\": {tool_arguments}}. Max 25 calls. Cannot include batch, done, subagent, subagent_parallel, or skill.".to_string(),
                required: true,
            }],
        input_examples: Vec::new(),
    }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Orchestration
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        // Meta-tool: execution handled by RunEngine intercept, not here.
        // If we reach here, the RunEngine didn't intercept — return error.
        Err(ToolError::Execution(
            "batch is a meta-tool handled by the RunEngine. This should not be called directly."
                .to_string(),
        ))
    }
}
