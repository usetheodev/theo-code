use crate::error::ToolError;
use crate::graph_context::GraphContextProvider;
use crate::permission::PermissionRequest;
use crate::session::{MessageId, SessionId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// Result of a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub title: String,
    pub output: String,
    pub metadata: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<FileAttachment>>,
}

/// Partial result emitted during tool execution.
/// Enables real-time display of long-running operations via callbacks.
/// Pi-mono ref: `packages/agent/src/types.ts:288-289`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialToolResult {
    /// Partial content to display.
    pub content: String,
    /// Optional progress indicator (0.0 to 1.0).
    pub progress: Option<f32>,
}

/// File attachment from tool output (images, PDFs, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAttachment {
    #[serde(rename = "type")]
    pub file_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    pub url: String,
}

// ── Tool Schema & Category ──────────────────────────────────────────

/// Category of a tool — used for filtering and building minimal tool sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    /// File read/write/edit operations
    FileOps,
    /// Search and navigation (grep, glob, codesearch)
    Search,
    /// Shell command execution
    Execution,
    /// Web access (fetch, search)
    Web,
    /// Agent orchestration (task, skill, question)
    Orchestration,
    /// Utilities (todo, invalid, plan)
    Utility,
}

/// A single parameter in a tool's JSON Schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParam {
    /// Parameter name (e.g. "filePath")
    pub name: String,
    /// JSON Schema type (e.g. "string", "integer", "boolean")
    #[serde(rename = "type")]
    pub param_type: String,
    /// Human-readable description for the LLM
    pub description: String,
    /// Whether this parameter is required
    pub required: bool,
}

/// Schema describing a tool's input parameters.
///
/// Designed to be converted to an OpenAI-compatible JSON Schema
/// for LLM tool definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub params: Vec<ToolParam>,
}

impl ToolSchema {
    /// Create a new empty schema (for tools with no parameters).
    pub fn new() -> Self {
        Self { params: vec![] }
    }

    /// Convert to a JSON Schema object suitable for LLM tool definitions.
    pub fn to_json_schema(&self) -> serde_json::Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &self.params {
            let mut prop = serde_json::Map::new();
            prop.insert(
                "type".to_string(),
                serde_json::Value::String(param.param_type.clone()),
            );
            prop.insert(
                "description".to_string(),
                serde_json::Value::String(param.description.clone()),
            );
            // Arrays require "items" schema for OpenAI API compatibility
            if param.param_type == "array" {
                prop.insert("items".to_string(), serde_json::json!({"type": "object"}));
            }
            if param.required {
                required.push(serde_json::Value::String(param.name.clone()));
            }
            properties.insert(param.name.clone(), serde_json::Value::Object(prop));
        }

        let mut schema = serde_json::Map::new();
        schema.insert(
            "type".to_string(),
            serde_json::Value::String("object".to_string()),
        );
        schema.insert(
            "properties".to_string(),
            serde_json::Value::Object(properties),
        );
        if !required.is_empty() {
            schema.insert("required".to_string(), serde_json::Value::Array(required));
        }

        serde_json::Value::Object(schema)
    }

    /// Validate that the schema is well-formed.
    pub fn validate(&self) -> Result<(), String> {
        for param in &self.params {
            match param.param_type.as_str() {
                "string" | "integer" | "number" | "boolean" | "array" | "object" => {}
                other => {
                    return Err(format!(
                        "Invalid param type '{}' for '{}'",
                        other, param.name
                    ));
                }
            }
            if param.name.is_empty() {
                return Err("Parameter name cannot be empty".to_string());
            }
            if param.description.is_empty() {
                return Err(format!(
                    "Parameter '{}' must have a description",
                    param.name
                ));
            }
        }
        Ok(())
    }
}

impl Default for ToolSchema {
    fn default() -> Self {
        Self::new()
    }
}

/// Complete definition of a tool for LLM consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub id: String,
    pub description: String,
    pub category: ToolCategory,
    pub schema: ToolSchema,
}

// ── Tool Context ────────────────────────────────────────────────────

/// Context provided to tools during execution
#[derive(Clone)]
pub struct ToolContext {
    pub session_id: SessionId,
    pub message_id: MessageId,
    pub call_id: String,
    pub agent: String,
    pub abort: tokio::sync::watch::Receiver<bool>,
    pub project_dir: PathBuf,
    /// Code intelligence provider (injected by RunEngine if available).
    pub graph_context: Option<Arc<dyn GraphContextProvider>>,
    /// Optional channel for streaming stdout lines during tool execution.
    /// If Some, tools that support streaming (e.g., BashTool) send lines here
    /// for live display in the TUI. If None, tools execute normally.
    /// The Tool trait signature is NOT affected — this is a lateral channel.
    pub stdout_tx: Option<tokio::sync::mpsc::Sender<String>>,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("session_id", &self.session_id)
            .field("call_id", &self.call_id)
            .field("project_dir", &self.project_dir)
            .field("graph_context", &self.graph_context.as_ref().map(|_| "..."))
            .field("stdout_tx", &self.stdout_tx.as_ref().map(|_| "Some(...)"))
            .finish()
    }
}

impl ToolContext {
    pub fn test_context(project_dir: PathBuf) -> Self {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        Self {
            session_id: SessionId::new("ses_test"),
            message_id: MessageId::new(""),
            call_id: String::new(),
            agent: "build".to_string(),
            abort: rx,
            project_dir,
            graph_context: None,
            stdout_tx: None,
        }
    }
}

/// Permission requests collected during tool execution
#[derive(Debug, Default)]
pub struct PermissionCollector {
    pub requests: Vec<PermissionRequest>,
}

impl PermissionCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, request: PermissionRequest) {
        self.requests.push(request);
    }

    pub fn find_by_type(
        &self,
        permission_type: &crate::permission::PermissionType,
    ) -> Option<&PermissionRequest> {
        self.requests
            .iter()
            .find(|r| r.permission == *permission_type)
    }
}

/// Core trait for all tools
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique identifier for the tool
    fn id(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// JSON Schema for this tool's input parameters.
    /// Default returns an empty schema (no parameters).
    fn schema(&self) -> ToolSchema {
        ToolSchema::new()
    }

    /// Category this tool belongs to.
    /// Default is Utility.
    fn category(&self) -> ToolCategory {
        ToolCategory::Utility
    }

    /// Build a complete ToolDefinition from this tool's metadata.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            id: self.id().to_string(),
            description: self.description().to_string(),
            category: self.category(),
            schema: self.schema(),
        }
    }

    /// Prepare (normalize/migrate) raw arguments before schema validation.
    ///
    /// Override this to accept legacy parameter names or apply argument
    /// transformations.  The default implementation returns args unchanged.
    ///
    /// **Pi-mono ref:** `packages/agent/src/types.ts:298-299` (prepareArguments)
    fn prepare_arguments(&self, args: serde_json::Value) -> serde_json::Value {
        args
    }

    /// Whether this tool supports streaming partial results during execution.
    /// Tools that return true may emit `PartialToolResult` via the runtime callback.
    /// The default returns false (no streaming support).
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Execute the tool with given arguments and context
    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
        permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError>;
}

/// Validate that a JSON value has a required string field
pub fn require_string(args: &serde_json::Value, field: &str) -> Result<String, ToolError> {
    args.get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ToolError::InvalidArgs(format!("Missing required field: {field}")))
}

/// Validate that a JSON value has an optional string field
pub fn optional_string(args: &serde_json::Value, field: &str) -> Option<String> {
    args.get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Validate that a JSON value has an optional integer field
pub fn optional_u64(args: &serde_json::Value, field: &str) -> Option<u64> {
    args.get(field).and_then(|v| v.as_u64())
}

/// Validate that a JSON value has an optional boolean field
pub fn optional_bool(args: &serde_json::Value, field: &str) -> Option<bool> {
    args.get(field).and_then(|v| v.as_bool())
}

#[cfg(test)]
mod tests {
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
        assert_eq!(optional_bool(&args, "replaceAll").unwrap(), true);
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
            ],
        };
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
            }],
        };
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
            }],
        };
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
            }],
        };
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
            }],
        };
        assert!(schema.validate().is_ok());
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

    #[test]
    fn tool_definition_contains_all_fields() {
        let def = ToolDefinition {
            id: "read".to_string(),
            description: "Read a file".to_string(),
            category: ToolCategory::FileOps,
            schema: ToolSchema::new(),
        };
        let json = serde_json::to_value(&def).unwrap();
        assert_eq!(json["id"], "read");
        assert_eq!(json["category"], "file_ops");
    }
}
