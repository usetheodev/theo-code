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

    // Add the `batch` meta-tool for parallel execution (CodeAct-inspired)
    defs.push(ToolDefinition::new(
        "batch",
        "Execute multiple tool calls in a single turn. Use for independent operations like reading multiple files. Max 25 calls. Cannot include batch/done/subagent/skill inside.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "calls": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {
                                "type": "string",
                                "description": "Tool name (e.g., 'read', 'grep', 'glob', 'bash')"
                            },
                            "args": {
                                "type": "object",
                                "description": "Arguments for the tool"
                            }
                        },
                        "required": ["tool", "args"]
                    },
                    "description": "Array of tool calls to execute (max 25)"
                }
            },
            "required": ["calls"]
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
    // But sub-agents CAN use batch for efficiency (it's not delegation).
    defs.push(ToolDefinition::new(
        "batch",
        "Execute multiple tool calls in a single turn. Use for independent operations like reading multiple files. Max 25 calls.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "calls": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": { "type": "string" },
                            "args": { "type": "object" }
                        },
                        "required": ["tool", "args"]
                    }
                }
            },
            "required": ["calls"]
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
            return (Message::tool_result(&call.id, name, &error_msg), false);
        }
    };

    let Some(tool) = registry.get(name) else {
        let error_msg = format!(
            "Unknown tool: {name}. Available tools: {}",
            registry.ids().join(", ")
        );
        return (Message::tool_result(&call.id, name, &error_msg), false);
    };

    let mut permissions = PermissionCollector::new();
    match tool.execute(args, ctx, &mut permissions).await {
        Ok(output) => {
            // Truncate the body independently of the model-only suffix so that
            // coaching text (opendev `with_llm_suffix` pattern) is never cut off.
            let body = if output.output.len() > 8000 {
                format!(
                    "{}...\n[truncated, {} chars total]",
                    &output.output[..8000],
                    output.output.len()
                )
            } else {
                output.output.clone()
            };
            let result = match output.llm_suffix.as_deref() {
                Some(suffix) if !suffix.is_empty() => format!("{body}\n\n{suffix}"),
                _ => body,
            };
            (Message::tool_result(&call.id, name, result), true)
        }
        Err(e) => {
            // Give the tool a chance to coach the agent on how to fix the
            // call: named parameter, expected type, concrete example.
            // Anthropic principle 8 (actionable errors).
            let coached = tool.format_validation_error(&e, &args);
            let error_msg = match coached {
                Some(guidance) => format!("Tool error: {e}\n\n{guidance}"),
                None => format!("Tool error: {e}"),
            };
            (Message::tool_result(&call.id, name, error_msg), false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use theo_domain::error::ToolError;
    use theo_domain::tool::{Tool, ToolOutput};
    use theo_infra_llm::types::FunctionCall;
    use theo_tooling::registry::{ToolRegistry, create_default_registry};

    #[test]
    fn test_registry_to_definitions() {
        let registry = create_default_registry();
        let defs = registry_to_definitions(&registry);

        // Should have all registry tools + meta-tools
        assert_eq!(defs.len(), registry.len() + 5); // +5 for `done` + `subagent` + `skill` + `subagent_parallel` + `batch`

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
        assert!(
            !names.contains(&"subagent"),
            "sub-agents must not see 'subagent' tool"
        );
        assert!(
            !names.contains(&"subagent_parallel"),
            "sub-agents must not see 'subagent_parallel' tool"
        );
        assert!(
            !names.contains(&"skill"),
            "sub-agents must not see 'skill' tool"
        );
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

        // Sub-agents get registry tools + done + batch (+2)
        assert_eq!(
            defs.len(),
            registry.len() + 2,
            "sub-agent defs = registry + done + batch"
        );
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

    struct SuffixCoachTool {
        suffix: Option<String>,
        body: String,
    }

    #[async_trait]
    impl Tool for SuffixCoachTool {
        fn id(&self) -> &str {
            "suffix_coach"
        }
        fn description(&self) -> &str {
            "test-only tool that emits body + llm_suffix"
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
            _perm: &mut PermissionCollector,
        ) -> Result<ToolOutput, ToolError> {
            let mut out = ToolOutput::new("coach", self.body.clone());
            if let Some(s) = &self.suffix {
                out = out.with_llm_suffix(s.clone());
            }
            Ok(out)
        }
    }

    fn suffix_call() -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "suffix_coach".to_string(),
                arguments: "{}".to_string(),
            },
        }
    }

    fn test_ctx() -> ToolContext {
        ToolContext::test_context(PathBuf::from("."))
    }

    #[tokio::test]
    async fn execute_tool_call_appends_llm_suffix_to_result() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(SuffixCoachTool {
                body: "main output".to_string(),
                suffix: Some("Try grep with a narrower pattern.".to_string()),
            }))
            .unwrap();

        let (msg, ok) = execute_tool_call(&registry, &suffix_call(), &test_ctx()).await;

        assert!(ok);
        let content = msg.content.expect("tool_result has content");
        assert!(content.contains("main output"), "body must be present");
        assert!(
            content.contains("Try grep with a narrower pattern."),
            "llm_suffix must be appended for the model: got `{content}`"
        );
    }

    struct CoachingErrorTool;

    #[async_trait]
    impl Tool for CoachingErrorTool {
        fn id(&self) -> &str {
            "coaching_error"
        }
        fn description(&self) -> &str {
            "test tool that fails validation with coaching"
        }
        fn format_validation_error(
            &self,
            _error: &ToolError,
            _args: &serde_json::Value,
        ) -> Option<String> {
            Some(
                "Missing `filePath`. Example: coaching_error({filePath: 'src/lib.rs'}).".to_string(),
            )
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
            _perm: &mut PermissionCollector,
        ) -> Result<ToolOutput, ToolError> {
            Err(ToolError::InvalidArgs(
                "Missing required field: filePath".to_string(),
            ))
        }
    }

    #[tokio::test]
    async fn execute_tool_call_appends_validation_coaching_to_error() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(CoachingErrorTool)).unwrap();

        let call = ToolCall {
            id: "c-err".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "coaching_error".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let (msg, ok) = execute_tool_call(&registry, &call, &test_ctx()).await;

        assert!(!ok, "coached error is still a failure");
        let content = msg.content.expect("tool_result has content");
        assert!(content.contains("Missing required field: filePath"));
        assert!(
            content.contains("Example: coaching_error({filePath:"),
            "override guidance must be appended: got `{content}`"
        );
    }

    #[tokio::test]
    async fn execute_tool_call_without_suffix_emits_body_only() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(SuffixCoachTool {
                body: "just the body".to_string(),
                suffix: None,
            }))
            .unwrap();

        let (msg, ok) = execute_tool_call(&registry, &suffix_call(), &test_ctx()).await;

        assert!(ok);
        let content = msg.content.expect("tool_result has content");
        assert_eq!(content, "just the body");
    }

    #[test]
    fn test_all_tool_schemas_produce_valid_json() {
        let registry = create_default_registry();
        for id in registry.ids() {
            let tool = registry.get(&id).unwrap();
            let schema = tool.schema();
            let json = schema.to_json_schema();
            assert_eq!(json["type"], "object", "Tool {id} schema missing 'type'");
            assert!(
                json.get("properties").is_some(),
                "Tool {id} schema missing 'properties'"
            );
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
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(required.contains(&"filePath".to_string()));

        // edit: requires filePath, oldString, newString (NOT oldText/newText)
        let edit = registry.get("edit").unwrap();
        let schema = edit.schema().to_json_schema();
        let required: Vec<String> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
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
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
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
