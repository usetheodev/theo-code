mod execute_meta;
mod execute_regular;
mod meta_schemas;

use theo_domain::tool::ToolContext;
use theo_infra_llm::types::{Message, ToolCall, ToolDefinition};
use theo_tooling::registry::ToolRegistry;

/// Generate OpenAI-compatible tool definitions from the registry.
///
/// Each tool declares its own schema via the `Tool::schema()` method.
/// The registry validates schemas at registration time.
///
/// Deferred tools (those with `should_defer() == true`) are excluded —
/// the agent discovers them by calling the `tool_search` meta-tool.
/// Anthropic principle 12 (minimize context overhead).
pub fn registry_to_definitions(registry: &ToolRegistry) -> Vec<ToolDefinition> {
    let mut defs: Vec<ToolDefinition> = registry
        .visible_definitions()
        .into_iter()
        .map(|def| {
            let parameters = def
                .llm_schema_override
                .clone()
                .unwrap_or_else(|| def.schema.to_json_schema());
            ToolDefinition::new(def.id, &def.description, parameters)
        })
        .collect();

    // Meta-tools exposed to the main agent. Ordering matters for tests
    // that assert `defs.len() == visible + 8`.
    defs.push(meta_schemas::tool_search());
    defs.push(meta_schemas::batch_execute());
    defs.push(meta_schemas::done());
    defs.push(meta_schemas::skill());
    // The unified `delegate_task` schema confused weaker tool-callers
    // like Codex, so we split it into two single-purpose tools with
    // fixed required-shape schemas. The legacy unified variant stays
    // for backward-compat.
    defs.push(meta_schemas::delegate_task_single());
    defs.push(meta_schemas::delegate_task_parallel());
    defs.push(meta_schemas::delegate_task_legacy());
    defs.push(meta_schemas::batch());

    defs
}

/// Generate tool definitions for sub-agents.
///
/// Sub-agents receive ONLY registry tools + `done` + `batch`. They do NOT
/// receive delegation meta-tools (`delegate_task`, `skill`). This prevents
/// recursive spawning — the gold standard pattern used by Claude Code,
/// OpenCode, and OpenDev (arxiv 2603.05344).
pub fn registry_to_definitions_for_subagent(registry: &ToolRegistry) -> Vec<ToolDefinition> {
    let mut defs: Vec<ToolDefinition> = registry
        .definitions()
        .into_iter()
        .map(|def| {
            let parameters = def
                .llm_schema_override
                .clone()
                .unwrap_or_else(|| def.schema.to_json_schema());
            ToolDefinition::new(def.id, &def.description, parameters)
        })
        .collect();

    // `done` is CRITICAL for sub-agents — it's how they signal completion.
    // Without it, sub-agents loop until max_iterations or timeout.
    defs.push(meta_schemas::done());
    // No delegate_task / skill — sub-agents cannot delegate. But they CAN
    // use batch for efficiency (it's not delegation).
    defs.push(meta_schemas::batch_for_subagent());

    defs
}

/// Execute a tool call and return a Message with the result.
pub async fn execute_tool_call(
    registry: &ToolRegistry,
    call: &ToolCall,
    ctx: &ToolContext,
) -> (Message, bool) {
    let (msg, ok, _meta) = execute_tool_call_with_metadata(registry, call, ctx).await;
    (msg, ok)
}

/// T1.2 / T0.1 — Execute a tool call AND surface the tool's metadata
/// (when present) so callers can wire side-band content like vision
/// blocks through `vision_propagation::build_image_followup`.
///
/// Meta-tools (`batch_execute`, `tool_search`) don't expose metadata
/// today, so the third tuple slot is always `None` for them.
///
/// Existing callers can keep using [`execute_tool_call`] which discards
/// the metadata for back-compat.
pub async fn execute_tool_call_with_metadata(
    registry: &ToolRegistry,
    call: &ToolCall,
    ctx: &ToolContext,
) -> (Message, bool, Option<serde_json::Value>) {
    let name = &call.function.name;

    let args = match call.parse_arguments() {
        Ok(args) => args,
        Err(e) => {
            let error_msg = format!("Failed to parse arguments: {e}");
            return (
                Message::tool_result(&call.id, name, &error_msg),
                false,
                None,
            );
        }
    };

    // Meta-tools are dispatched inline (not in the registry) because they
    // need direct registry access. Anthropic "Programmatic Tool Calling"
    // (batch_execute) + deferred-tool discovery (tool_search).
    match name.as_str() {
        "batch_execute" => {
            let (m, ok) = execute_meta::handle_batch_execute(registry, call, ctx, &args).await;
            (m, ok, None)
        }
        "tool_search" => {
            let (m, ok) = execute_meta::handle_tool_search(registry, call, &args);
            (m, ok, None)
        }
        _ => execute_regular::execute_regular_tool_with_metadata(registry, call, ctx, args).await,
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
