use theo_domain::tool::{PermissionCollector, ToolContext};
use theo_infra_llm::types::{Message, ToolCall, ToolDefinition};
use theo_tooling::registry::ToolRegistry;

/// Generate OpenAI-compatible tool definitions from the registry.
///
/// Since the Tool trait doesn't expose JSON schemas, we maintain
/// a mapping of tool_id → schema here.
pub fn registry_to_definitions(registry: &ToolRegistry) -> Vec<ToolDefinition> {
    let mut defs = Vec::new();

    for id in registry.ids() {
        if let Some(tool) = registry.get(&id) {
            let schema = tool_schema(&id);
            defs.push(ToolDefinition::new(id, tool.description(), schema));
        }
    }

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

/// Return the JSON schema for a tool's parameters.
fn tool_schema(tool_id: &str) -> serde_json::Value {
    match tool_id {
        "read" => serde_json::json!({
            "type": "object",
            "properties": {
                "filePath": {
                    "type": "string",
                    "description": "Absolute or relative path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (0-based)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read"
                }
            },
            "required": ["filePath"]
        }),
        "write" => serde_json::json!({
            "type": "object",
            "properties": {
                "filePath": {
                    "type": "string",
                    "description": "Absolute or relative path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "The complete content to write to the file"
                }
            },
            "required": ["filePath", "content"]
        }),
        "edit" => serde_json::json!({
            "type": "object",
            "properties": {
                "filePath": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "oldText": {
                    "type": "string",
                    "description": "Exact text to find and replace (must be unique in the file)"
                },
                "newText": {
                    "type": "string",
                    "description": "Replacement text"
                },
                "replaceAll": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default false)"
                }
            },
            "required": ["filePath", "oldText", "newText"]
        }),
        "bash" => serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default 120000)"
                }
            },
            "required": ["command"]
        }),
        "grep" => serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search in"
                },
                "include": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g. '*.py')"
                }
            },
            "required": ["pattern"]
        }),
        "glob" => serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files (e.g. 'src/**/*.rs')"
                },
                "path": {
                    "type": "string",
                    "description": "Base directory to search in"
                }
            },
            "required": ["pattern"]
        }),
        "apply_patch" => serde_json::json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "Unified diff patch to apply"
                }
            },
            "required": ["patch"]
        }),
        "webfetch" => serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                }
            },
            "required": ["url"]
        }),
        _ => serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": true
        }),
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

        // Should have all registry tools + done
        assert!(defs.len() > 1);

        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(names.contains(&"read"));
        assert!(names.contains(&"bash"));
        assert!(names.contains(&"edit"));
        assert!(names.contains(&"done")); // meta-tool
    }

    #[test]
    fn test_tool_schema_completeness() {
        let registry = create_default_registry();
        for id in registry.ids() {
            let schema = tool_schema(&id);
            assert!(schema.get("type").is_some(), "Tool {id} schema missing 'type'");
            assert!(schema.get("properties").is_some(), "Tool {id} schema missing 'properties'");
        }
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
