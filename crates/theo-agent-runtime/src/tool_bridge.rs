use theo_domain::tool::{PermissionCollector, ToolContext};
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
        .map(|def| ToolDefinition::new(def.id, &def.description, def.schema.to_json_schema()))
        .collect();

    // Add the `tool_search` meta-tool — lets the model discover deferred
    // tools by keyword when it needs capability beyond the default set.
    // Ref: opendev `search_hint` + registry discovery (traits.rs:547-575).
    defs.push(ToolDefinition::new(
        "tool_search",
        concat!(
            "Search for deferred (rarely-used) tools by keyword. Returns a list of `(id, hint)` \
             pairs the agent can invoke by name. Use this only when none of the visible tools ",
            "fit the task. Example: tool_search({query: 'wiki lookup'})."
        ),
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keyword to match against deferred tool ids and search hints"
                }
            },
            "required": ["query"]
        }),
    ));

    // Add the `batch_execute` meta-tool — minimum-viable programmatic tool
    // calling. The agent supplies an ordered list of {tool, args}, each
    // executed serially. Early-exits on the first failure so downstream
    // steps don't see stale data. This is NOT a full code interpreter; it
    // is the deterministic core of Anthropic's "Programmatic Tool Calling"
    // that unlocks for-loop-over-inputs patterns without a sandbox.
    defs.push(ToolDefinition::new(
        "batch_execute",
        concat!(
            "Execute an ordered list of tool calls in one assistant turn. ",
            "Runs each step serially and stops at the first failure. Use this to collapse N round-trips ",
            "(e.g. 'fetch 3 URLs then summarise') into a single LLM generation — cuts ~30-50% of tokens ",
            "on parallelisable workflows. Each step is `{tool: string, args: object}` matching that ",
            "tool's own schema. The aggregated result is returned as a JSON block with per-step ",
            "`ok: bool`, `name`, and `result`/`error` fields. ",
            "Example: batch_execute({calls: [",
            "{tool: 'read', args: {filePath: 'Cargo.toml'}}, ",
            "{tool: 'grep', args: {pattern: 'version', path: '.'}} ]})."
        ),
        serde_json::json!({
            "type": "object",
            "properties": {
                "calls": {
                    "type": "array",
                    "description": "Ordered list of tool invocations to execute serially.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {
                                "type": "string",
                                "description": "Name of a visible tool (not a meta-tool). Cannot be `done`, `subagent`, `subagent_parallel`, `skill`, `tool_search`, or `batch_execute` itself."
                            },
                            "args": {
                                "type": "object",
                                "description": "Arguments for the invoked tool, matching its JSON schema."
                            }
                        },
                        "required": ["tool", "args"]
                    }
                }
            },
            "required": ["calls"]
        }),
    ));

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

    // Meta-tool: `batch_execute` — run a list of tool calls serially and
    // return a combined JSON result. Early-exits on the first failure so
    // downstream steps do not see stale data. Anthropic "Programmatic Tool
    // Calling" (minimum-viable form: no code sandbox, just batched calls).
    if name == "batch_execute" {
        const BLOCKED: &[&str] = &[
            "batch_execute",
            "batch",
            "tool_search",
            "done",
            "subagent",
            "subagent_parallel",
            "skill",
        ];
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
                Message::tool_result(&call.id, name, "batch_execute received an empty `calls` array — nothing to execute."),
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
            if tool_name.is_empty() || BLOCKED.contains(&tool_name.as_str()) {
                results.push(serde_json::json!({
                    "step": i,
                    "tool": tool_name,
                    "ok": false,
                    "error": format!("cannot run `{tool_name}` inside batch_execute (missing or blocked meta-tool)")
                }));
                any_failed = true;
                break;
            }
            let step_call = ToolCall {
                id: format!("{}_step{i}", call.id),
                call_type: "function".to_string(),
                function: theo_infra_llm::types::FunctionCall {
                    name: tool_name.clone(),
                    arguments: step_args.to_string(),
                },
            };
            let (step_msg, ok) =
                Box::pin(execute_tool_call(registry, &step_call, ctx)).await;
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
        return (
            Message::tool_result(&call.id, name, body.to_string()),
            !any_failed,
        );
    }

    // Meta-tool: `tool_search` — keyword lookup over deferred tools.
    // Dispatched here (not in the registry) because it needs direct
    // access to the registry to enumerate deferred entries.
    if name == "tool_search" {
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
        return (Message::tool_result(&call.id, name, body), true);
    }

    let Some(tool) = registry.get(name) else {
        let error_msg = format!(
            "Unknown tool: {name}. Available tools: {}",
            registry.ids().join(", ")
        );
        return (Message::tool_result(&call.id, name, &error_msg), false);
    };

    let mut permissions = PermissionCollector::new();
    // Clone args so we can still pass them to `format_validation_error`
    // after `execute` consumes its owned copy.
    let args_for_error = args.clone();
    match tool.execute(args, ctx, &mut permissions).await {
        Ok(output) => {
            // Per-tool truncation rule (opendev `ToolResultSanitizer` pattern).
            // Falls back to the legacy 8000-char global cap when no rule is set.
            let body = if let Some(rule) = tool.truncation_rule() {
                rule.apply(&output.output).unwrap_or_else(|| output.output.clone())
            } else if output.output.len() > 8000 {
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
            let coached = tool.format_validation_error(&e, &args_for_error);
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

        // Meta-tools injected by registry_to_definitions: tool_search,
        // batch_execute, done, subagent, subagent_parallel, skill, batch
        // (+7). Deferred tools in the default registry are filtered out;
        // none are currently deferred.
        assert_eq!(defs.len(), registry.len() + 7);

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

    struct BigOutputTool;

    #[async_trait]
    impl Tool for BigOutputTool {
        fn id(&self) -> &str {
            "big_output"
        }
        fn description(&self) -> &str {
            "test tool that emits a large payload under a tail-truncation rule"
        }
        fn truncation_rule(&self) -> Option<theo_domain::tool::TruncationRule> {
            Some(theo_domain::tool::TruncationRule {
                max_chars: 50,
                strategy: theo_domain::tool::TruncationStrategy::Tail,
            })
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
            _perm: &mut PermissionCollector,
        ) -> Result<ToolOutput, ToolError> {
            let body = "AAAAAAAAAA".repeat(50); // 500 chars
            Ok(ToolOutput::new("big", body).with_llm_suffix("narrow next call"))
        }
    }

    #[tokio::test]
    async fn execute_tool_call_applies_truncation_rule_before_suffix() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(BigOutputTool)).unwrap();

        let call = ToolCall {
            id: "c-big".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "big_output".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let (msg, ok) = execute_tool_call(&registry, &call, &test_ctx()).await;

        assert!(ok);
        let content = msg.content.expect("tool_result has content");
        // Body must be truncated (original was 500 chars; we capped at 50 + marker).
        assert!(
            content.len() < 500,
            "sanitizer must have truncated the body"
        );
        assert!(
            content.contains("[truncated"),
            "sanitizer must annotate truncation"
        );
        // Suffix must still survive past the truncation.
        assert!(
            content.contains("narrow next call"),
            "llm_suffix must be preserved after truncation"
        );
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

    // ── batch_execute meta-tool tests ────────────────────────────

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn id(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "test tool that echoes a `value` field"
        }
        async fn execute(
            &self,
            args: serde_json::Value,
            _ctx: &ToolContext,
            _perm: &mut PermissionCollector,
        ) -> Result<ToolOutput, ToolError> {
            let v = args.get("value").and_then(|v| v.as_str()).unwrap_or("");
            Ok(ToolOutput::new("echo", v.to_string()))
        }
    }

    struct FailingTool;

    #[async_trait]
    impl Tool for FailingTool {
        fn id(&self) -> &str {
            "failing"
        }
        fn description(&self) -> &str {
            "test tool that always errors"
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
            _perm: &mut PermissionCollector,
        ) -> Result<ToolOutput, ToolError> {
            Err(ToolError::Execution("boom".to_string()))
        }
    }

    fn batch_call(calls: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "batch-1".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "batch_execute".to_string(),
                arguments: serde_json::json!({"calls": calls}).to_string(),
            },
        }
    }

    #[tokio::test]
    async fn batch_execute_runs_calls_in_order_and_returns_all_results() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();

        let call = batch_call(serde_json::json!([
            {"tool": "echo", "args": {"value": "alpha"}},
            {"tool": "echo", "args": {"value": "beta"}},
            {"tool": "echo", "args": {"value": "gamma"}},
        ]));

        let (msg, ok) = execute_tool_call(&registry, &call, &test_ctx()).await;
        assert!(ok);
        let content = msg.content.expect("tool_result has content");
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["ok"], true);
        let steps = parsed["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0]["result"], "alpha");
        assert_eq!(steps[1]["result"], "beta");
        assert_eq!(steps[2]["result"], "gamma");
    }

    #[tokio::test]
    async fn batch_execute_stops_at_first_failure() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        registry.register(Box::new(FailingTool)).unwrap();

        let call = batch_call(serde_json::json!([
            {"tool": "echo", "args": {"value": "first"}},
            {"tool": "failing", "args": {}},
            {"tool": "echo", "args": {"value": "unreachable"}},
        ]));

        let (msg, ok) = execute_tool_call(&registry, &call, &test_ctx()).await;
        assert!(!ok, "batch_execute should report failure when any step fails");
        let content = msg.content.expect("tool_result has content");
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["ok"], false);
        let steps = parsed["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 2, "execution must stop after the failing step");
        assert_eq!(steps[0]["ok"], true);
        assert_eq!(steps[1]["ok"], false);
    }

    #[tokio::test]
    async fn batch_execute_rejects_meta_tools_inside() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();

        let call = batch_call(serde_json::json!([
            {"tool": "batch_execute", "args": {"calls": []}},
        ]));

        let (msg, ok) = execute_tool_call(&registry, &call, &test_ctx()).await;
        assert!(!ok);
        let content = msg.content.expect("tool_result has content");
        assert!(
            content.contains("batch_execute"),
            "error must name the blocked meta-tool: got `{content}`"
        );
    }

    #[tokio::test]
    async fn batch_execute_rejects_missing_calls_array() {
        let registry = ToolRegistry::new();
        let call = ToolCall {
            id: "batch-none".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "batch_execute".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let (msg, ok) = execute_tool_call(&registry, &call, &test_ctx()).await;
        assert!(!ok);
        let content = msg.content.expect("tool_result has content");
        assert!(content.contains("requires a `calls` array"));
    }

    #[tokio::test]
    async fn batch_execute_rejects_empty_calls_array() {
        let registry = ToolRegistry::new();
        let call = batch_call(serde_json::json!([]));
        let (msg, ok) = execute_tool_call(&registry, &call, &test_ctx()).await;
        assert!(!ok);
        let content = msg.content.expect("tool_result has content");
        assert!(content.contains("empty `calls` array"));
    }

    // ── tool_search meta-tool tests ──────────────────────────────

    struct DeferredWikiTool;

    #[async_trait]
    impl Tool for DeferredWikiTool {
        fn id(&self) -> &str {
            "wiki_lookup"
        }
        fn description(&self) -> &str {
            "deferred wiki lookup tool"
        }
        fn should_defer(&self) -> bool {
            true
        }
        fn search_hint(&self) -> Option<&str> {
            Some("wiki knowledge base page lookup")
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
            _perm: &mut PermissionCollector,
        ) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::new("wiki", "page content"))
        }
    }

    #[test]
    fn registry_to_definitions_hides_deferred_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(DeferredWikiTool)).unwrap();

        let defs = registry_to_definitions(&registry);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();

        assert!(
            !names.contains(&"wiki_lookup"),
            "deferred tools must not appear in the default tool definitions"
        );
        assert!(
            names.contains(&"tool_search"),
            "tool_search meta-tool must be injected so the agent can discover deferred tools"
        );
    }

    #[tokio::test]
    async fn tool_search_returns_matching_deferred_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(DeferredWikiTool)).unwrap();

        let call = ToolCall {
            id: "c-search".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "tool_search".to_string(),
                arguments: serde_json::json!({"query": "wiki"}).to_string(),
            },
        };

        let (msg, ok) = execute_tool_call(&registry, &call, &test_ctx()).await;

        assert!(ok);
        let content = msg.content.expect("tool_result has content");
        assert!(content.contains("wiki_lookup"));
        assert!(content.contains("wiki knowledge base page lookup"));
    }

    #[tokio::test]
    async fn tool_search_reports_empty_when_no_deferred_tool_matches() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(DeferredWikiTool)).unwrap();

        let call = ToolCall {
            id: "c-miss".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "tool_search".to_string(),
                arguments: serde_json::json!({"query": "nonexistent"}).to_string(),
            },
        };

        let (msg, ok) = execute_tool_call(&registry, &call, &test_ctx()).await;

        assert!(ok);
        let content = msg.content.expect("tool_result has content");
        assert!(content.contains("No deferred tools matched"));
    }

    #[tokio::test]
    async fn tool_search_rejects_empty_query() {
        let registry = ToolRegistry::new();
        let call = ToolCall {
            id: "c-empty".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "tool_search".to_string(),
                arguments: serde_json::json!({"query": ""}).to_string(),
            },
        };
        let (msg, ok) = execute_tool_call(&registry, &call, &test_ctx()).await;
        assert!(!ok);
        let content = msg.content.expect("tool_result has content");
        assert!(content.contains("non-empty `query`"));
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
