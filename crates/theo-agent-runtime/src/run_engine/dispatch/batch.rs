//! `batch` meta-tool handler — execute up to MAX_BATCH_SIZE sub-calls
//! in parallel within a single LLM turn.
//!
//! Fase 4 (REMEDIATION_PLAN T4.2). Extracted from `run_engine/mod.rs`.
//! Blocked tools (batch itself, done, subagent, subagent_parallel,
//! skill) receive immediate error results; Plan-mode also blocks
//! write-class tools. Non-blocked sub-calls run via `join_all`,
//! preserving original request order in the aggregated output.

use theo_domain::event::{DomainEvent, EventType};
use theo_domain::session::{MessageId, SessionId};
use theo_domain::tool::ToolContext;
use theo_infra_llm::types::{Message, ToolCall};

use crate::run_engine::AgentRunEngine;
use crate::run_engine_helpers::truncate_batch_args;
use crate::tool_bridge;

const BLOCKED_IN_BATCH: &[&str] =
    &["batch", "done", "subagent", "subagent_parallel", "skill"];

impl AgentRunEngine {
    /// Dispatch a `batch` tool call. Pushes the aggregated tool result
    /// (one line per sub-call) into `messages`.
    pub(in crate::run_engine) async fn dispatch_batch(
        &mut self,
        call: &ToolCall,
        abort_rx: &tokio::sync::watch::Receiver<bool>,
        messages: &mut Vec<Message>,
    ) {
        use crate::constants::MAX_BATCH_SIZE as MAX_BATCH;

        let args = call.parse_arguments().unwrap_or_default();
        let Some(calls) = args.get("calls").and_then(|v| v.as_array()) else {
            messages.push(Message::tool_result(
                &call.id,
                "batch",
                "Error: 'calls' array is required. Example: batch(calls: [{tool: \"read\", args: {filePath: \"a.rs\"}}])",
            ));
            return;
        };

        let total = calls.len().min(MAX_BATCH);
        let registry = self.registry.clone();
        let mut futures = Vec::new();
        let mut blocked_results: Vec<(usize, String, String)> = Vec::new();

        for (i, batch_call) in calls.iter().take(MAX_BATCH).enumerate() {
            let tool_name = batch_call
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            let tool_args = batch_call
                .get("args")
                .cloned()
                .unwrap_or(serde_json::json!({}));

            if BLOCKED_IN_BATCH.contains(&tool_name.as_str()) {
                blocked_results.push((
                    i,
                    tool_name.clone(),
                    format!("cannot use '{}' inside batch", tool_name),
                ));
                continue;
            }

            // Plan mode guard: block write-class tools inside a batch.
            if self.config.mode == crate::config::AgentMode::Plan
                && matches!(tool_name.as_str(), "edit" | "write" | "apply_patch")
            {
                blocked_results.push((
                    i,
                    tool_name.clone(),
                    "BLOCKED by Plan mode guard — no source edits in batch during planning".to_string(),
                ));
                continue;
            }

            let reg = registry.clone();
            let batch_tool_call = theo_infra_llm::types::ToolCall::new(
                format!("batch_{}_{}", call.id, i),
                &tool_name,
                tool_args.to_string(),
            );
            let batch_ctx = ToolContext {
                session_id: SessionId::new("batch"),
                message_id: MessageId::new(format!("batch_{}", i)),
                call_id: batch_tool_call.id.clone(),
                agent: "main".to_string(),
                abort: abort_rx.clone(),
                project_dir: self.project_dir.clone(),
                graph_context: self.graph_context.clone(),
                stdout_tx: None,
            };

            futures.push(async move {
                let (msg, success) = tool_bridge::execute_tool_call(
                    &reg,
                    &batch_tool_call,
                    &batch_ctx,
                )
                .await;
                (i, tool_name, tool_args, msg, success)
            });
        }

        // Execute all non-blocked calls in parallel — join_all preserves order.
        let results = futures::future::join_all(futures).await;

        // Combine blocked + executed results, sorted by index.
        let mut all_results: Vec<(usize, String, String, bool)> = Vec::new();
        for (i, name, err) in blocked_results {
            all_results.push((i, name, format!("error — {}", err), false));
        }
        for (i, tool_name, tool_args, msg, success) in results {
            let output = msg.content.unwrap_or_default();
            let status = if success { "ok" } else { "error" };
            let preview = theo_domain::prompt_sanitizer::char_boundary_truncate(
                &output,
                crate::constants::TOOL_PREVIEW_BYTES,
            );
            all_results.push((
                i,
                tool_name.clone(),
                format!(
                    "{}({}): {} — {}",
                    tool_name,
                    truncate_batch_args(&tool_args),
                    status,
                    preview
                ),
                success,
            ));

            // Budget + metrics accounting.
            self.budget_enforcer.record_tool_call();
            self.metrics.record_tool_call(&tool_name, 0, success);

            // Track edits in the context loop state.
            if success
                && matches!(tool_name.as_str(), "edit" | "write" | "apply_patch")
            {
                let file = tool_args
                    .get("filePath")
                    .and_then(|p| p.as_str())
                    .unwrap_or("");
                if !file.is_empty() {
                    self.context_loop_state
                        .record_edit_attempt(file, true, None);
                }
            }
        }

        // Sort by original index for deterministic output.
        all_results.sort_by_key(|(i, _, _, _)| *i);

        let mut batch_output = String::new();
        for (i, _name, display, _success) in &all_results {
            batch_output.push_str(&format!("[{}/{}] {}\n", i + 1, total, display));
        }
        if calls.len() > MAX_BATCH {
            batch_output.push_str(&format!(
                "\n⚠ {} calls exceeded max batch size of {}. Only first {} executed.\n",
                calls.len(), MAX_BATCH, MAX_BATCH
            ));
        }

        // Publish batch completion event with OTel payload.
        let mut batch_span = crate::observability::otel::tool_call_span("batch");
        batch_span.set(
            crate::observability::otel::ATTR_THEO_TOOL_CALL_ID,
            call.id.as_str(),
        );
        batch_span.set(crate::observability::otel::ATTR_THEO_TOOL_STATUS, "Succeeded");
        batch_span.set(crate::observability::otel::ATTR_THEO_TOOL_DURATION_MS, 0u64);
        self.event_bus.publish(DomainEvent::new(
            EventType::ToolCallCompleted,
            call.id.as_str(),
            serde_json::json!({
                "tool_name": "batch",
                "success": true,
                "input": { "count": total },
                "output_preview": format!("Batch: {total} calls executed"),
                "duration_ms": 0,
                "otel": batch_span.to_json(),
            }),
        ));

        messages.push(Message::tool_result(&call.id, "batch", &batch_output));
    }
}
