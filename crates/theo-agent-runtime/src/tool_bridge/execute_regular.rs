//! Regular (non-meta) tool execution — the path taken when the call name
//! does NOT match `batch_execute` / `tool_search`. Delegates to
//! `Tool::execute`, applies per-tool truncation + optional `llm_suffix`,
//! and on error lets the tool coach the agent via `format_validation_error`.
//!
//! Fase 4 (REMEDIATION_PLAN T4.6). Extracted from `tool_bridge.rs`.

use theo_domain::tool::{PermissionCollector, ToolContext};
use theo_infra_llm::types::{Message, ToolCall};
use theo_tooling::registry::ToolRegistry;

const DEFAULT_TRUNCATION_CAP: usize = 8000;

/// Back-compat shim — discards metadata. Existing callers that don't
/// need to inspect tool metadata stay on this signature.
pub(super) async fn execute_regular_tool(
    registry: &ToolRegistry,
    call: &ToolCall,
    ctx: &ToolContext,
    args: serde_json::Value,
) -> (Message, bool) {
    let (msg, ok, _meta) =
        execute_regular_tool_with_metadata(registry, call, ctx, args).await;
    (msg, ok)
}

/// Execute a regular (non-meta) tool call. Returns the tool_result
/// `Message`, success flag, AND the tool's `output.metadata` JSON when
/// the call succeeded. The metadata is the channel for sideband data
/// like `image_block` (T1.2) — callers that wire vision propagation
/// (T0.1) consume it via `vision_propagation::build_image_followup`.
///
/// On error or unknown tool, returns `None` for the metadata slot.
pub(super) async fn execute_regular_tool_with_metadata(
    registry: &ToolRegistry,
    call: &ToolCall,
    ctx: &ToolContext,
    args: serde_json::Value,
) -> (Message, bool, Option<serde_json::Value>) {
    let name = &call.function.name;

    let Some(tool) = registry.get(name) else {
        let error_msg = format!(
            "Unknown tool: {name}. Available tools: {}",
            registry.ids().join(", ")
        );
        return (
            Message::tool_result(&call.id, name, &error_msg),
            false,
            None,
        );
    };

    let mut permissions = PermissionCollector::new();
    // Clone args so we can still pass them to `format_validation_error`
    // after `execute` consumes its owned copy.
    let args_for_error = args.clone();
    match tool.execute(args, ctx, &mut permissions).await {
        Ok(output) => {
            let metadata = if output.metadata.is_null() {
                None
            } else {
                Some(output.metadata.clone())
            };
            let body = apply_truncation(tool.truncation_rule(), &output.output);
            let result = match output.llm_suffix.as_deref() {
                Some(suffix) if !suffix.is_empty() => format!("{body}\n\n{suffix}"),
                _ => body,
            };
            (Message::tool_result(&call.id, name, result), true, metadata)
        }
        Err(e) => {
            // Let the tool coach the agent on how to fix the call: named
            // parameter, expected type, concrete example. Anthropic
            // principle 8 (actionable errors).
            let coached = tool.format_validation_error(&e, &args_for_error);
            let error_msg = match coached {
                Some(guidance) => format!("Tool error: {e}\n\n{guidance}"),
                None => format!("Tool error: {e}"),
            };
            (Message::tool_result(&call.id, name, error_msg), false, None)
        }
    }
}

/// Apply a per-tool `TruncationRule` when present, falling back to a
/// legacy global `DEFAULT_TRUNCATION_CAP`-char cap when absent. Returns an
/// owned String to keep the caller simple.
fn apply_truncation(
    rule: Option<theo_domain::tool::TruncationRule>,
    output: &str,
) -> String {
    if let Some(rule) = rule {
        return rule.apply(output).unwrap_or_else(|| output.to_string());
    }
    if output.len() > DEFAULT_TRUNCATION_CAP {
        return format!(
            "{}...\n[truncated, {} chars total]",
            &output[..DEFAULT_TRUNCATION_CAP],
            output.len()
        );
    }
    output.to_string()
}
