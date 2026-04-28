//! Sibling test body of `tool.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `tool.rs` via `#[path = "tool_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use super::*;

    #[test]
    fn require_string_returns_value_when_present() {
        let args = serde_json::json!({"filePath": "/tmp/test.txt"});
        let result = require_string(&args, "filePath");
        assert_eq!(result.unwrap(), "/tmp/test.txt");
    }

    #[test]
    fn require_string_returns_error_when_missing() {
        let args = serde_json::json!({});
        let result = require_string(&args, "filePath");
        assert!(result.is_err());
    }

    #[test]
    fn optional_string_returns_none_when_missing() {
        let args = serde_json::json!({});
        assert!(optional_string(&args, "path").is_none());
    }

    #[test]
    fn optional_string_returns_value_when_present() {
        let args = serde_json::json!({"path": "/tmp"});
        assert_eq!(optional_string(&args, "path").unwrap(), "/tmp");
    }

    #[test]
    fn optional_u64_returns_value() {
        let args = serde_json::json!({"limit": 10});
        assert_eq!(optional_u64(&args, "limit").unwrap(), 10);
    }

    #[test]
    fn optional_bool_returns_value() {
        let args = serde_json::json!({"replaceAll": true});
        assert!(optional_bool(&args, "replaceAll").unwrap());
    }

    // ── ToolSchema tests ────────────────────────────────────────

    #[test]
    fn empty_schema_produces_valid_json() {
        let schema = ToolSchema::new();
        let json = schema.to_json_schema();
        assert_eq!(json["type"], "object");
        assert!(json["properties"].as_object().unwrap().is_empty());
        assert!(json.get("required").is_none());
    }

    #[test]
    fn schema_with_params_produces_correct_json() {
        let schema = ToolSchema {
            params: vec![
                ToolParam {
                    name: "filePath".to_string(),
                    param_type: "string".to_string(),
                    description: "Path to the file".to_string(),
                    required: true,
                },
                ToolParam {
                    name: "limit".to_string(),
                    param_type: "integer".to_string(),
                    description: "Max lines".to_string(),
                    required: false,
                },
            ], input_examples: Vec::new(), };
        let json = schema.to_json_schema();

        assert_eq!(json["type"], "object");
        assert_eq!(json["properties"]["filePath"]["type"], "string");
        assert_eq!(json["properties"]["limit"]["type"], "integer");

        let required = json["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "filePath");
    }

    #[test]
    fn schema_validate_rejects_invalid_type() {
        let schema = ToolSchema {
            params: vec![ToolParam {
                name: "x".to_string(),
                param_type: "invalid_type".to_string(),
                description: "desc".to_string(),
                required: false,
            }], input_examples: Vec::new(), };
        assert!(schema.validate().is_err());
    }

    #[test]
    fn schema_validate_rejects_empty_name() {
        let schema = ToolSchema {
            params: vec![ToolParam {
                name: "".to_string(),
                param_type: "string".to_string(),
                description: "desc".to_string(),
                required: false,
            }], input_examples: Vec::new(), };
        assert!(schema.validate().is_err());
    }

    #[test]
    fn schema_validate_rejects_empty_description() {
        let schema = ToolSchema {
            params: vec![ToolParam {
                name: "x".to_string(),
                param_type: "string".to_string(),
                description: "".to_string(),
                required: false,
            }], input_examples: Vec::new(), };
        assert!(schema.validate().is_err());
    }

    #[test]
    fn schema_validate_accepts_valid_schema() {
        let schema = ToolSchema {
            params: vec![ToolParam {
                name: "command".to_string(),
                param_type: "string".to_string(),
                description: "The command to run".to_string(),
                required: true,
            }], input_examples: Vec::new(), };
        assert!(schema.validate().is_ok());
    }

    // ── input_examples tests ─────────────────────────────────────

    #[test]
    fn schema_without_examples_omits_examples_key() {
        let schema = ToolSchema::new();
        let json = schema.to_json_schema();
        assert!(
            json.get("examples").is_none(),
            "empty examples list must not appear in JSON Schema"
        );
    }

    #[test]
    fn schema_with_examples_emits_examples_array() {
        let schema = ToolSchema {
            params: vec![ToolParam {
                name: "pattern".to_string(),
                param_type: "string".to_string(),
                description: "Regex".to_string(),
                required: true,
            }],
            input_examples: vec![
                serde_json::json!({"pattern": "fn main"}),
                serde_json::json!({"pattern": "use serde"}),
            ],
        };
        let json = schema.to_json_schema();
        let examples = json["examples"].as_array().expect("examples is array");
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0]["pattern"], "fn main");
    }

    #[test]
    fn schema_with_examples_builder_produces_same_json() {
        let schema = ToolSchema::new()
            .with_examples(vec![serde_json::json!({"pattern": "fn"})]);
        let json = schema.to_json_schema();
        assert_eq!(json["examples"][0]["pattern"], "fn");
    }

    #[test]
    fn schema_deserializes_without_input_examples_field() {
        let json = r#"{"params":[]}"#;
        let schema: ToolSchema = serde_json::from_str(json).unwrap();
        assert!(schema.input_examples.is_empty());
    }

    #[test]
    fn tool_category_serializes_to_snake_case() {
        let json = serde_json::to_string(&ToolCategory::FileOps).unwrap();
        assert_eq!(json, "\"file_ops\"");
    }

    // ── prepare_arguments tests ──────────────────────────────────

    struct IdentityTool;

    #[async_trait]
    impl Tool for IdentityTool {
        fn id(&self) -> &str {
            "identity"
        }
        fn description(&self) -> &str {
            "tool with default prepare_arguments"
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
            _perm: &mut PermissionCollector,
        ) -> Result<ToolOutput, ToolError> {
            unreachable!()
        }
    }

    struct MigratingTool;

    #[async_trait]
    impl Tool for MigratingTool {
        fn id(&self) -> &str {
            "migrating"
        }
        fn description(&self) -> &str {
            "tool that normalizes legacy arg names"
        }
        fn prepare_arguments(&self, mut args: serde_json::Value) -> serde_json::Value {
            // Accept legacy "filePath" as alias for "file_path"
            if let Some(v) = args.get("filePath").cloned() {
                if args.get("file_path").is_none() {
                    args["file_path"] = v;
                }
                if let Some(obj) = args.as_object_mut() {
                    obj.remove("filePath");
                }
            }
            args
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
            _perm: &mut PermissionCollector,
        ) -> Result<ToolOutput, ToolError> {
            unreachable!()
        }
    }

    #[test]
    fn prepare_arguments_default_is_identity() {
        let tool = IdentityTool;
        let args = serde_json::json!({"file_path": "/tmp/a.rs", "content": "hello"});
        let prepared = tool.prepare_arguments(args.clone());
        assert_eq!(prepared, args);
    }

    #[test]
    fn prepare_arguments_migrates_legacy_field_name() {
        let tool = MigratingTool;
        let args = serde_json::json!({"filePath": "/tmp/a.rs"});
        let prepared = tool.prepare_arguments(args);
        assert_eq!(prepared["file_path"], "/tmp/a.rs");
        assert!(prepared.get("filePath").is_none());
    }

    #[test]
    fn prepare_arguments_preserves_canonical_field_over_legacy() {
        let tool = MigratingTool;
        let args = serde_json::json!({"filePath": "/old", "file_path": "/new"});
        let prepared = tool.prepare_arguments(args);
        assert_eq!(prepared["file_path"], "/new");
    }

    // ── PartialToolResult tests ──────────────────────────────────

    #[test]
    fn partial_tool_result_serde_roundtrip_with_progress() {
        let partial = PartialToolResult {
            content: "Processing file 3/10...".to_string(),
            progress: Some(0.3),
        };
        let json = serde_json::to_string(&partial).unwrap();
        let back: PartialToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.content, "Processing file 3/10...");
        assert_eq!(back.progress, Some(0.3));
    }

    #[test]
    fn partial_tool_result_serde_roundtrip_without_progress() {
        let partial = PartialToolResult {
            content: "Searching...".to_string(),
            progress: None,
        };
        let json = serde_json::to_string(&partial).unwrap();
        let back: PartialToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.content, "Searching...");
        assert!(back.progress.is_none());
    }

    // ── supports_streaming tests ────────────────────────────────

    #[test]
    fn supports_streaming_default_returns_false() {
        let tool = IdentityTool;
        assert!(!tool.supports_streaming());
    }

    // ── should_defer / search_hint tests ─────────────────────────

    #[test]
    fn should_defer_default_is_false() {
        let tool = IdentityTool;
        assert!(!tool.should_defer());
    }

    #[test]
    fn search_hint_default_is_none() {
        let tool = IdentityTool;
        assert!(tool.search_hint().is_none());
    }

    struct DeferredTool;

    #[async_trait]
    impl Tool for DeferredTool {
        fn id(&self) -> &str {
            "deferred"
        }
        fn description(&self) -> &str {
            "rarely-used tool, only surfaced via tool_search"
        }
        fn should_defer(&self) -> bool {
            true
        }
        fn search_hint(&self) -> Option<&str> {
            Some("wiki knowledge base lookup")
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
            _perm: &mut PermissionCollector,
        ) -> Result<ToolOutput, ToolError> {
            unreachable!()
        }
    }

    #[test]
    fn deferred_tool_overrides_defaults() {
        let tool = DeferredTool;
        assert!(tool.should_defer());
        assert_eq!(tool.search_hint(), Some("wiki knowledge base lookup"));
    }

    // ── TruncationRule tests ─────────────────────────────────────

    #[test]
    fn truncation_rule_returns_none_when_input_fits() {
        let rule = TruncationRule {
            max_chars: 100,
            strategy: TruncationStrategy::Head,
        };
        assert!(rule.apply("short").is_none());
    }

    #[test]
    fn truncation_rule_head_keeps_prefix() {
        let rule = TruncationRule {
            max_chars: 5,
            strategy: TruncationStrategy::Head,
        };
        let out = rule.apply("abcdefghij").unwrap();
        assert!(out.starts_with("abcde"));
        assert!(out.contains("[truncated"));
    }

    #[test]
    fn truncation_rule_tail_keeps_suffix() {
        let rule = TruncationRule {
            max_chars: 5,
            strategy: TruncationStrategy::Tail,
        };
        let out = rule.apply("abcdefghij").unwrap();
        assert!(out.ends_with("fghij"));
        assert!(out.contains("[truncated"));
    }

    #[test]
    fn truncation_rule_headtail_keeps_both_ends() {
        let rule = TruncationRule {
            max_chars: 6,
            strategy: TruncationStrategy::HeadTail { head: 3, tail: 3 },
        };
        let out = rule.apply("abcdefghij").unwrap();
        assert!(out.starts_with("abc"));
        assert!(out.ends_with("hij"));
        assert!(!out.contains("defg"));
    }

    #[test]
    fn tool_truncation_rule_default_is_none() {
        let tool = IdentityTool;
        assert!(tool.truncation_rule().is_none());
    }

    // ── format_validation_error tests ────────────────────────────

    struct CoachingTool;

    #[async_trait]
    impl Tool for CoachingTool {
        fn id(&self) -> &str {
            "coaching"
        }
        fn description(&self) -> &str {
            "tool that coaches on validation errors"
        }
        fn format_validation_error(
            &self,
            error: &crate::error::ToolError,
            _args: &serde_json::Value,
        ) -> Option<String> {
            let msg = error.to_string();
            if msg.contains("filePath") {
                Some(
                    "Missing `filePath`. Provide an absolute or project-relative path, \
                     e.g. coaching({filePath: 'src/lib.rs'})."
                        .to_string(),
                )
            } else {
                None
            }
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
            _perm: &mut PermissionCollector,
        ) -> Result<ToolOutput, ToolError> {
            unreachable!()
        }
    }

    #[test]
    fn format_validation_error_default_returns_none() {
        let tool = IdentityTool;
        let err = ToolError::InvalidArgs("Missing required field: filePath".to_string());
        assert!(
            tool.format_validation_error(&err, &serde_json::Value::Null)
                .is_none()
        );
    }

    #[test]
    fn format_validation_error_override_receives_error_and_args() {
        let tool = CoachingTool;
        let err = ToolError::InvalidArgs("Missing required field: filePath".to_string());
        let args = serde_json::json!({});
        let coached = tool.format_validation_error(&err, &args).unwrap();
        assert!(coached.contains("filePath"));
        assert!(coached.contains("Example") || coached.contains("e.g."));
    }

    #[test]
    fn format_validation_error_override_declines_unrecognized_errors() {
        let tool = CoachingTool;
        let err = ToolError::InvalidArgs("Missing required field: other".to_string());
        assert!(
            tool.format_validation_error(&err, &serde_json::Value::Null)
                .is_none(),
            "overrides should only coach on errors they recognize"
        );
    }

    // ── llm_suffix / ToolOutput builder tests ────────────────────

    #[test]
    fn tool_output_new_leaves_suffix_none() {
        let out = ToolOutput::new("title", "body");
        assert_eq!(out.title, "title");
        assert_eq!(out.output, "body");
        assert!(out.llm_suffix.is_none());
    }

    #[test]
    fn tool_output_with_llm_suffix_sets_field() {
        let out = ToolOutput::new("title", "body")
            .with_llm_suffix("Try grep with a narrower pattern.");
        assert_eq!(
            out.llm_suffix.as_deref(),
            Some("Try grep with a narrower pattern.")
        );
    }

    #[test]
    fn tool_output_model_text_appends_suffix() {
        let out =
            ToolOutput::new("t", "line1\nline2").with_llm_suffix("Use read_file with offset.");
        assert_eq!(
            out.model_text(),
            "line1\nline2\n\nUse read_file with offset."
        );
    }

    #[test]
    fn tool_output_model_text_without_suffix_is_output() {
        let out = ToolOutput::new("t", "hello");
        assert_eq!(out.model_text(), "hello");
    }

    #[test]
    fn tool_output_llm_suffix_skipped_when_none_in_serde() {
        let out = ToolOutput::new("t", "o");
        let json = serde_json::to_value(&out).unwrap();
        assert!(
            json.get("llm_suffix").is_none(),
            "serde should omit llm_suffix when None"
        );
    }

    #[test]
    fn tool_output_llm_suffix_roundtrips_through_serde() {
        let out = ToolOutput::new("t", "o").with_llm_suffix("coach");
        let json = serde_json::to_string(&out).unwrap();
        let back: ToolOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(back.llm_suffix.as_deref(), Some("coach"));
    }

    #[test]
    fn tool_output_default_deserializes_without_llm_suffix_field() {
        let json = r#"{"title":"t","output":"o","metadata":null}"#;
        let out: ToolOutput = serde_json::from_str(json).unwrap();
        assert!(out.llm_suffix.is_none());
    }

    #[test]
    fn tool_definition_contains_all_fields() {
        let def = ToolDefinition {
            id: "read".to_string(),
            description: "Read a file".to_string(),
            category: ToolCategory::FileOps,
            schema: ToolSchema::new(),
            llm_schema_override: None,
        };
        let json = serde_json::to_value(&def).unwrap();
        assert_eq!(json["id"], "read");
        assert_eq!(json["category"], "file_ops");
    }
