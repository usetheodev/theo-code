//! Meta-tool execution handlers: `batch_execute` + `tool_search`.
//!
//! Fase 4 (REMEDIATION_PLAN T4.6). Extracted from `tool_bridge.rs`.
//! These handlers are dispatched inside `execute_tool_call` because they
//! need direct registry access (not backed by a `Tool` impl).

use theo_domain::tool::ToolContext;
use theo_infra_llm::types::{FunctionCall, Message, ToolCall};
use theo_tooling::registry::ToolRegistry;

/// Meta-tools that cannot recurse inside `batch_execute`. Keeping them in
/// a single const avoids drift with the outer loop + is independently
/// testable. Sub-agents never receive `batch_execute` itself, so
/// recursion only happens on the main-agent path.
const BATCH_EXECUTE_BLOCKED: &[&str] = &[
    "batch_execute",
    "batch",
    "tool_search",
    "done",
    "delegate_task",
    "skill",
];

/// `batch_execute` — minimum-viable Programmatic Tool Calling. Runs an
/// ordered list of `{tool, args}` serially; early-exits on failure so
/// downstream steps never see stale data. Returns a JSON blob with
/// per-step `ok`/`tool`/`result`.
pub(super) async fn handle_batch_execute(
    registry: &ToolRegistry,
    call: &ToolCall,
    ctx: &ToolContext,
    args: &serde_json::Value,
) -> (Message, bool) {
    let name = &call.function.name;

    let Some(calls) = args.get("calls").and_then(|v| v.as_array()) else {
        return (
            Message::tool_result(
                &call.id,
                name,
                "batch_execute requires a `calls` array. Example: batch_execute({calls: [{tool: 'read', args: {filePath: 'Cargo.toml'}}]}).",
            ),
            false,
        );
    };
    if calls.is_empty() {
        return (
            Message::tool_result(
                &call.id,
                name,
                "batch_execute received an empty `calls` array — nothing to execute.",
            ),
            false,
        );
    }

    let mut results: Vec<serde_json::Value> = Vec::with_capacity(calls.len());
    let mut any_failed = false;
    for (i, step) in calls.iter().enumerate() {
        let tool_name = step
            .get("tool")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let step_args = step
            .get("args")
            .cloned()
            .unwrap_or(serde_json::Value::Object(Default::default()));
        if tool_name.is_empty() || BATCH_EXECUTE_BLOCKED.contains(&tool_name.as_str()) {
            results.push(serde_json::json!({
                "step": i,
                "tool": tool_name,
                "ok": false,
                "error": format!(
                    "cannot run `{tool_name}` inside batch_execute (missing or blocked meta-tool)"
                )
            }));
            any_failed = true;
            break;
        }
        let step_call = ToolCall {
            id: format!("{}_step{i}", call.id),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: tool_name.clone(),
                arguments: step_args.to_string(),
            },
        };
        let (step_msg, ok) =
            Box::pin(super::execute_tool_call(registry, &step_call, ctx)).await;
        let content = step_msg.content.unwrap_or_default();
        results.push(serde_json::json!({
            "step": i,
            "tool": tool_name,
            "ok": ok,
            "result": content,
        }));
        if !ok {
            any_failed = true;
            break;
        }
    }
    let body = serde_json::json!({
        "ok": !any_failed,
        "steps": results,
    });
    (
        Message::tool_result(&call.id, name, body.to_string()),
        !any_failed,
    )
}

/// `tool_search` — keyword lookup over deferred tools. Dispatched here
/// (not in the registry) because it needs direct registry access.
pub(super) fn handle_tool_search(
    registry: &ToolRegistry,
    call: &ToolCall,
    args: &serde_json::Value,
) -> (Message, bool) {
    let name = &call.function.name;
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
    if query.is_empty() {
        return (
            Message::tool_result(
                &call.id,
                name,
                "tool_search requires a non-empty `query`. Example: tool_search({query: 'wiki'}).",
            ),
            false,
        );
    }
    let hits = registry.search_deferred(query);
    let body = if hits.is_empty() {
        format!("No deferred tools matched `{query}`.")
    } else {
        let mut out = format!("Deferred tools matching `{query}`:\n");
        for (id, hint) in &hits {
            out.push_str(&format!("- {id}: {hint}\n"));
        }
        out.push_str("\nInvoke any of these by name with their normal schema.");
        out
    };
    (Message::tool_result(&call.id, name, body), true)
}
