//! `delegate_task` / `delegate_task_single` / `delegate_task_parallel`
//! meta-tool handler.
//!
//! Fase 4 (REMEDIATION_PLAN T4.2). Extracted from `run_engine/mod.rs`.
//! The split `_single` / `_parallel` variants are accepted because
//! weaker tool-callers (e.g., Codex) handle fixed-shape schemas better;
//! this handler normalizes them back to the unified shape expected by
//! `AgentRunEngine::handle_delegate_task`.

use theo_infra_llm::types::{Message, ToolCall};

use crate::run_engine::AgentRunEngine;

impl AgentRunEngine {
    /// Dispatch a delegate_task-family tool call. Pushes the tool
    /// result into `messages` and always returns `Continue`.
    pub(in crate::run_engine) async fn dispatch_delegate_task(
        &mut self,
        call: &ToolCall,
        messages: &mut Vec<Message>,
    ) {
        let name = call.function.name.as_str();
        let raw_args = call.parse_arguments().unwrap_or_default();
        // Normalize split variants to the unified `{agent, objective,
        // context}` / `{parallel: [...]}` shape.
        let args = match name {
            "delegate_task_single" => raw_args,
            "delegate_task_parallel" => {
                let tasks = raw_args
                    .get("tasks")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                serde_json::json!({"parallel": tasks})
            }
            _ => raw_args,
        };
        let result_msg = self.handle_delegate_task(args).await;
        messages.push(Message::tool_result(&call.id, name, &result_msg));
    }
}
