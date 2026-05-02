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
        let (futures, blocked_results) = self.spawn_batch_subcalls(call, calls, abort_rx);
        let results = futures::future::join_all(futures).await;
        let (all_results, vision_followups) = self.combine_batch_results(blocked_results, results);
        let batch_output = format_batch_output(&all_results, total, calls.len());
        self.publish_batch_completion(call, total);
        messages.push(Message::tool_result(&call.id, "batch", &batch_output));
        // T1.2/T0.1 — image follow-ups are pushed AFTER the combined
        // tool_result so the LLM sees the textual summary first.
        for (tool_name, metadata) in &vision_followups {
            crate::vision_propagation::push_image_followup(messages, metadata, tool_name);
        }
    }

    /// Walk the requested sub-calls; return (parallel_futures,
    /// blocked_synchronously). Every entry preserves its index so the
    /// final output is deterministic regardless of completion order.
    #[allow(clippy::type_complexity)]
    fn spawn_batch_subcalls(
        &self,
        call: &ToolCall,
        calls: &[serde_json::Value],
        abort_rx: &tokio::sync::watch::Receiver<bool>,
    ) -> (
        Vec<
            std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = (
                                usize,
                                String,
                                serde_json::Value,
                                Message,
                                bool,
                                Option<serde_json::Value>,
                            ),
                        > + Send,
                >,
            >,
        >,
        Vec<(usize, String, String)>,
    ) {
        use crate::constants::MAX_BATCH_SIZE as MAX_BATCH;
        let registry = self.llm.registry.clone();
        let mut futures: Vec<
            std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = (
                                usize,
                                String,
                                serde_json::Value,
                                Message,
                                bool,
                                Option<serde_json::Value>,
                            ),
                        > + Send,
                >,
            >,
        > = Vec::new();
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
            if self.config.loop_cfg().mode == crate::config::AgentMode::Plan
                && matches!(tool_name.as_str(), "edit" | "write" | "apply_patch")
            {
                blocked_results.push((
                    i,
                    tool_name.clone(),
                    "BLOCKED by Plan mode guard — no source edits in batch during planning"
                        .to_string(),
                ));
                continue;
            }
            let reg = registry.clone();
            let batch_tool_call = theo_infra_llm::types::ToolCall::new(
                format!("batch_{}_{}", call.id, i),
                &tool_name,
                tool_args.to_string(),
            );
            // T14.1 — partial_progress_tx propagates to the parallel path.
            let batch_ctx = ToolContext {
                session_id: SessionId::new("batch"),
                message_id: MessageId::new(format!("batch_{}", i)),
                call_id: batch_tool_call.id.clone(),
                agent: "main".to_string(),
                abort: abort_rx.clone(),
                project_dir: self.project_dir.clone(),
                graph_context: self.rt.graph_context.clone(),
                stdout_tx: self.rt.partial_progress_tx.clone(),
            };
            futures.push(Box::pin(async move {
                let (msg, success, metadata) = tool_bridge::execute_tool_call_with_metadata(
                    &reg,
                    &batch_tool_call,
                    &batch_ctx,
                )
                .await;
                (i, tool_name, tool_args, msg, success, metadata)
            }));
        }
        (futures, blocked_results)
    }

    /// Merge synchronously-blocked entries with parallel results,
    /// record budget/metrics, capture vision metadata, and sort by
    /// original index.
    #[allow(clippy::type_complexity)]
    fn combine_batch_results(
        &mut self,
        blocked_results: Vec<(usize, String, String)>,
        results: Vec<(
            usize,
            String,
            serde_json::Value,
            Message,
            bool,
            Option<serde_json::Value>,
        )>,
    ) -> (
        Vec<(usize, String, String, bool)>,
        Vec<(String, serde_json::Value)>,
    ) {
        let mut all_results: Vec<(usize, String, String, bool)> = Vec::new();
        let mut vision_followups: Vec<(String, serde_json::Value)> = Vec::new();
        for (i, name, err) in blocked_results {
            all_results.push((i, name, format!("error — {}", err), false));
        }
        for (i, tool_name, tool_args, msg, success, metadata) in results {
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
            self.llm.budget_enforcer.record_tool_call();
            self.obs.metrics.record_tool_call(&tool_name, 0, success);
            if success && matches!(tool_name.as_str(), "edit" | "write" | "apply_patch") {
                let file = tool_args
                    .get("filePath")
                    .and_then(|p| p.as_str())
                    .unwrap_or("");
                if !file.is_empty() {
                    self.rt
                        .context_loop_state
                        .record_edit_attempt(file, true, None);
                }
            }
            if success
                && let Some(m) = metadata
            {
                vision_followups.push((tool_name.clone(), m));
            }
        }
        all_results.sort_by_key(|(i, _, _, _)| *i);
        (all_results, vision_followups)
    }

    fn publish_batch_completion(&self, call: &ToolCall, total: usize) {
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
    }
}

fn format_batch_output(
    all_results: &[(usize, String, String, bool)],
    total: usize,
    requested: usize,
) -> String {
    use crate::constants::MAX_BATCH_SIZE as MAX_BATCH;
    let mut batch_output = String::new();
    for (i, _name, display, _success) in all_results {
        batch_output.push_str(&format!("[{}/{}] {}\n", i + 1, total, display));
    }
    if requested > MAX_BATCH {
        batch_output.push_str(&format!(
            "\n⚠ {} calls exceeded max batch size of {}. Only first {} executed.\n",
            requested, MAX_BATCH, MAX_BATCH
        ));
    }
    batch_output
}
