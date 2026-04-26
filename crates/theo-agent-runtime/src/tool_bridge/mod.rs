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
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use theo_domain::error::ToolError;
    use theo_domain::tool::{PermissionCollector, Tool, ToolOutput};
    use theo_infra_llm::types::FunctionCall;
    use theo_tooling::registry::{ToolRegistry, create_default_registry};

    #[test]
    fn test_registry_to_definitions() {
        let registry = create_default_registry();
        let defs = registry_to_definitions(&registry);

        // Meta-tools injected by registry_to_definitions: tool_search,
        // batch_execute, done, skill, delegate_task_single,
        // delegate_task_parallel, delegate_task (legacy), batch (+8).
        // Deferred tools in the default registry are filtered out.
        assert_eq!(defs.len(), registry.len() + 8);

        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(names.contains(&"read"));
        assert!(names.contains(&"bash"));
        assert!(names.contains(&"edit"));
        assert!(names.contains(&"done")); // meta-tool
        assert!(names.contains(&"delegate_task")); // legacy
        assert!(names.contains(&"delegate_task_single"));
        assert!(names.contains(&"delegate_task_parallel"));
        assert!(!names.contains(&"subagent"), "legacy 'subagent' removed");
        assert!(
            !names.contains(&"subagent_parallel"),
            "legacy 'subagent_parallel' removed"
        );
    }

    #[test]
    fn delegate_task_single_schema_marks_agent_and_objective_required() {
        let registry = create_default_registry();
        let defs = registry_to_definitions(&registry);
        let single = defs
            .iter()
            .find(|d| d.function.name == "delegate_task_single")
            .expect("delegate_task_single must be present");
        let required = single
            .function
            .parameters
            .get("required")
            .and_then(|v| v.as_array())
            .expect("required array must exist");
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"agent"));
        assert!(names.contains(&"objective"));
    }

    #[test]
    fn delegate_task_parallel_schema_marks_tasks_required() {
        let registry = create_default_registry();
        let defs = registry_to_definitions(&registry);
        let parallel = defs
            .iter()
            .find(|d| d.function.name == "delegate_task_parallel")
            .expect("delegate_task_parallel must be present");
        let required = parallel
            .function
            .parameters
            .get("required")
            .and_then(|v| v.as_array())
            .expect("required array must exist");
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(names, vec!["tasks"]);
    }

    #[test]
    fn subagent_tool_defs_exclude_recursive_tools() {
        let registry = create_default_registry();
        let defs = registry_to_definitions_for_subagent(&registry);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();

        // Must NOT contain delegation meta-tools
        assert!(
            !names.contains(&"delegate_task"),
            "sub-agents must not see 'delegate_task' tool"
        );
        assert!(
            !names.contains(&"skill"),
            "sub-agents must not see 'skill' tool"
        );
        // follow-up: also exclude the split variants
        assert!(
            !names.contains(&"delegate_task_single"),
            "sub-agents must not see 'delegate_task_single' tool"
        );
        assert!(
            !names.contains(&"delegate_task_parallel"),
            "sub-agents must not see 'delegate_task_parallel' tool"
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

    // ----- T1.2 / T0.1 — execute_tool_call_with_metadata -----

    /// Tool that emits a vision metadata block — proves the metadata
    /// pathway plumbs end-to-end from `Tool::execute` to the new
    /// `execute_tool_call_with_metadata` return slot.
    struct VisionMetaTool;

    #[async_trait]
    impl Tool for VisionMetaTool {
        fn id(&self) -> &str {
            "vision_meta"
        }
        fn description(&self) -> &str {
            "test tool that emits a vision metadata block"
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
            _perm: &mut PermissionCollector,
        ) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::new("ok", "image attached")
                .with_metadata(serde_json::json!({
                    "type": "vision_meta",
                    "image_block": {
                        "type": "image_url",
                        "image_url": {"url": "https://e.x/test.png"}
                    }
                })))
        }
    }

    fn vision_call() -> ToolCall {
        ToolCall {
            id: "call-vis".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "vision_meta".to_string(),
                arguments: "{}".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn execute_tool_call_with_metadata_returns_tool_metadata() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisionMetaTool)).unwrap();

        let (msg, ok, meta) =
            execute_tool_call_with_metadata(&registry, &vision_call(), &test_ctx()).await;
        assert!(ok);
        assert!(msg.content.is_some());
        let m = meta.expect("metadata returned");
        assert_eq!(m["type"], "vision_meta");
        assert_eq!(m["image_block"]["type"], "image_url");
    }

    #[tokio::test]
    async fn execute_tool_call_with_metadata_propagates_to_image_followup() {
        // E2E: tool emits image_block → vision_propagation builds a
        // user-role follow-up Message with content_blocks. Proves the
        // T1.2 ↔ T0.1 ↔ vision_propagation pipeline composes.
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisionMetaTool)).unwrap();

        let (_msg, _ok, meta) =
            execute_tool_call_with_metadata(&registry, &vision_call(), &test_ctx()).await;
        let metadata = meta.expect("metadata present");
        let followup = crate::vision_propagation::build_image_followup(&metadata, "vision_meta")
            .expect("followup produced");
        assert!(followup.has_image());
        let blocks = followup.content_blocks.as_ref().unwrap();
        // text pointer + image
        assert_eq!(blocks.len(), 2);
    }

    #[tokio::test]
    async fn execute_tool_call_with_metadata_returns_none_for_unknown_tool() {
        let registry = ToolRegistry::new(); // empty — `unknown_tool` not registered
        let call = ToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "unknown_tool".into(),
                arguments: "{}".into(),
            },
        };
        let (msg, ok, meta) =
            execute_tool_call_with_metadata(&registry, &call, &test_ctx()).await;
        assert!(!ok);
        assert!(meta.is_none());
        assert!(msg.content.unwrap().contains("Unknown tool"));
    }

    #[tokio::test]
    async fn execute_tool_call_with_metadata_returns_none_for_meta_tools() {
        let registry = ToolRegistry::new();
        let call = ToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "tool_search".into(),
                arguments: "{\"query\":\"x\"}".into(),
            },
        };
        let (_msg, _ok, meta) =
            execute_tool_call_with_metadata(&registry, &call, &test_ctx()).await;
        // Meta-tools don't expose metadata yet — preserves the existing contract.
        assert!(meta.is_none());
    }

    #[tokio::test]
    async fn execute_tool_call_with_metadata_invalid_args_returns_none() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisionMetaTool)).unwrap();
        let call = ToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "vision_meta".into(),
                arguments: "not valid json".into(), // parse fails
            },
        };
        let (_msg, ok, meta) =
            execute_tool_call_with_metadata(&registry, &call, &test_ctx()).await;
        assert!(!ok);
        assert!(meta.is_none());
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
