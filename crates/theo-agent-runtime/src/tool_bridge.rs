use theo_domain::tool::{PermissionCollector, ToolContext};
use theo_infra_llm::types::{Message, ToolCall, ToolDefinition};
use theo_tooling::registry::ToolRegistry;

/// Generate OpenAI-compatible tool definitions from the registry.
///
/// Each tool declares its own schema via the `Tool::schema()` method.
/// The registry validates schemas at registration time.
pub fn registry_to_definitions(registry: &ToolRegistry) -> Vec<ToolDefinition> {
    let mut defs: Vec<ToolDefinition> = registry
        .definitions()
        .into_iter()
        .map(|def| ToolDefinition::new(def.id, &def.description, def.schema.to_json_schema()))
        .collect();

    // Add the `done` meta-tool (not in the registry)
    defs.push(ToolDefinition::new(
        "done",
        "Call when the task is complete. Requires a summary of what was accomplished.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "Brief summary of what was accomplished"
                }
            },
            "required": ["summary"]
        }),
    ));

    // Add the `subagent` meta-tool for delegation
    defs.push(ToolDefinition::new(
        "subagent",
        "Delegate work to a specialized sub-agent. Use for independent sub-problems. Roles: explorer (read-only research), implementer (make code changes), verifier (run tests/checks), reviewer (code review).",
        serde_json::json!({
            "type": "object",
            "properties": {
                "role": {
                    "type": "string",
                    "description": "Sub-agent role: 'explorer', 'implementer', 'verifier', or 'reviewer'"
                },
                "objective": {
                    "type": "string",
                    "description": "What the sub-agent should accomplish"
                }
            },
            "required": ["role", "objective"]
        }),
    ));

    // Add the `skill` meta-tool for invoking packaged capabilities
    defs.push(ToolDefinition::new(
        "skill",
        "Invoke a specialized skill workflow. Skills provide expert instructions for common tasks like commit, test, review, build, explain. Use when the task matches a skill's trigger.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill name to invoke (e.g., 'commit', 'test', 'review', 'build', 'explain')"
                }
            },
            "required": ["name"]
        }),
    ));

    // Add the `subagent_parallel` meta-tool for concurrent delegation
    defs.push(ToolDefinition::new(
        "subagent_parallel",
        "Run multiple sub-agents in parallel. All execute concurrently and results are combined. Use when tasks are independent.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "agents": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "role": {
                                "type": "string",
                                "description": "Sub-agent role: 'explorer', 'implementer', 'verifier', or 'reviewer'"
                            },
                            "objective": {
                                "type": "string",
                                "description": "What this sub-agent should accomplish"
                            }
                        },
                        "required": ["role", "objective"]
                    },
                    "description": "Array of sub-agents to run in parallel"
                }
            },
            "required": ["agents"]
        }),
    ));

    defs
}

/// Generate tool definitions for sub-agents.
///
/// Sub-agents receive ONLY registry tools + `done`. They do NOT receive
/// delegation meta-tools (`subagent`, `subagent_parallel`, `skill`).
/// This prevents recursive spawning — the gold standard pattern used by
/// Claude Code, OpenCode, and OpenDev (arxiv 2603.05344).
pub fn registry_to_definitions_for_subagent(registry: &ToolRegistry) -> Vec<ToolDefinition> {
    let mut defs: Vec<ToolDefinition> = registry
        .definitions()
        .into_iter()
        .map(|def| ToolDefinition::new(def.id, &def.description, def.schema.to_json_schema()))
        .collect();

    // `done` is CRITICAL for sub-agents — it's how they signal completion.
    // Without it, sub-agents loop until max_iterations or timeout.
    defs.push(ToolDefinition::new(
        "done",
        "Call when the task is complete. Requires a summary of what was accomplished.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "Brief summary of what was accomplished"
                }
            },
            "required": ["summary"]
        }),
    ));

    // No subagent, subagent_parallel, or skill — sub-agents cannot delegate.
    defs
}

/// Execute a tool call and return a Message with the result.
pub async fn execute_tool_call(
    registry: &ToolRegistry,
    call: &ToolCall,
    ctx: &ToolContext,
) -> (Message, bool) {
    let name = &call.function.name;

    let args = match call.parse_arguments() {
        Ok(args) => args,
        Err(e) => {
            let error_msg = format!("Failed to parse arguments: {e}");
            return (
                Message::tool_result(&call.id, name, &error_msg),
                false,
            );
        }
    };

    let Some(tool) = registry.get(name) else {
        let error_msg = format!("Unknown tool: {name}. Available tools: {}", registry.ids().join(", "));
        return (
            Message::tool_result(&call.id, name, &error_msg),
            false,
        );
    };

    let mut permissions = PermissionCollector::new();
    match tool.execute(args, ctx, &mut permissions).await {
        Ok(output) => {
            let result = if output.output.len() > 8000 {
                format!("{}...\n[truncated, {} chars total]", &output.output[..8000], output.output.len())
            } else {
                output.output
            };
            (Message::tool_result(&call.id, name, result), true)
        }
        Err(e) => {
            let error_msg = format!("Tool error: {e}");
            (Message::tool_result(&call.id, name, error_msg), false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_tooling::registry::create_default_registry;

    #[test]
    fn test_registry_to_definitions() {
        let registry = create_default_registry();
        let defs = registry_to_definitions(&registry);

        // Should have all registry tools + meta-tools
        assert_eq!(defs.len(), registry.len() + 4); // +4 for `done` + `subagent` + `skill` + `subagent_parallel`

        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(names.contains(&"read"));
        assert!(names.contains(&"bash"));
        assert!(names.contains(&"edit"));
        assert!(names.contains(&"done")); // meta-tool
    }

    #[test]
    fn subagent_tool_defs_exclude_recursive_tools() {
        let registry = create_default_registry();
        let defs = registry_to_definitions_for_subagent(&registry);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();

        // Must NOT contain delegation meta-tools
        assert!(!names.contains(&"subagent"), "sub-agents must not see 'subagent' tool");
        assert!(!names.contains(&"subagent_parallel"), "sub-agents must not see 'subagent_parallel' tool");
        assert!(!names.contains(&"skill"), "sub-agents must not see 'skill' tool");
    }

    #[test]
    fn subagent_tool_defs_include_done() {
        let registry = create_default_registry();
        let defs = registry_to_definitions_for_subagent(&registry);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();

        // done MUST be present — it's how sub-agents signal completion
        assert!(names.contains(&"done"), "sub-agents must have 'done' tool");
    }

    #[test]
    fn subagent_tool_defs_count_is_registry_plus_one() {
        let registry = create_default_registry();
        let defs = registry_to_definitions_for_subagent(&registry);

        // Sub-agents get registry tools + done only (+1)
        assert_eq!(defs.len(), registry.len() + 1, "sub-agent defs = registry + done");
    }

    #[test]
    fn subagent_tool_defs_preserve_all_regular_tools() {
        let registry = create_default_registry();
        let defs = registry_to_definitions_for_subagent(&registry);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();

        // Regular tools must still be available
        assert!(names.contains(&"read"), "sub-agents must have 'read'");
        assert!(names.contains(&"bash"), "sub-agents must have 'bash'");
        assert!(names.contains(&"edit"), "sub-agents must have 'edit'");
        assert!(names.contains(&"grep"), "sub-agents must have 'grep'");
    }

    #[test]
    fn test_all_tool_schemas_produce_valid_json() {
        let registry = create_default_registry();
        for id in registry.ids() {
            let tool = registry.get(&id).unwrap();
            let schema = tool.schema();
            let json = schema.to_json_schema();
            assert_eq!(json["type"], "object", "Tool {id} schema missing 'type'");
            assert!(json.get("properties").is_some(), "Tool {id} schema missing 'properties'");
        }
    }

    #[test]
    fn test_schemas_match_tool_implementations() {
        // Verify that tool schemas declare the same required params
        // that the tool's execute() actually reads
        let registry = create_default_registry();

        // read: requires filePath
        let read = registry.get("read").unwrap();
        let schema = read.schema().to_json_schema();
        let required: Vec<String> = schema["required"]
            .as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(required.contains(&"filePath".to_string()));

        // edit: requires filePath, oldString, newString (NOT oldText/newText)
        let edit = registry.get("edit").unwrap();
        let schema = edit.schema().to_json_schema();
        let required: Vec<String> = schema["required"]
            .as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(required.contains(&"filePath".to_string()));
        assert!(required.contains(&"oldString".to_string()));
        assert!(required.contains(&"newString".to_string()));
        // Must NOT contain the old wrong names
        assert!(!required.contains(&"oldText".to_string()));
        assert!(!required.contains(&"newText".to_string()));

        // apply_patch: requires patchText (NOT patch)
        let patch = registry.get("apply_patch").unwrap();
        let schema = patch.schema().to_json_schema();
        let required: Vec<String> = schema["required"]
            .as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        assert!(required.contains(&"patchText".to_string()));
        assert!(!required.contains(&"patch".to_string()));
    }

    #[tokio::test]
    async fn test_execute_unknown_tool() {
        let registry = create_default_registry();
        let call = ToolCall::new("call_1", "nonexistent_tool", "{}");
        let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));
        let (msg, success) = execute_tool_call(&registry, &call, &ctx).await;
        assert!(!success);
        assert!(msg.content.unwrap().contains("Unknown tool"));
    }

    #[tokio::test]
    async fn test_execute_invalid_args() {
        let registry = create_default_registry();
        let call = ToolCall::new("call_1", "read", "not valid json");
        let ctx = ToolContext::test_context(std::path::PathBuf::from("/tmp"));
        let (msg, success) = execute_tool_call(&registry, &call, &ctx).await;
        assert!(!success);
        assert!(msg.content.unwrap().contains("Failed to parse"));
    }
}
