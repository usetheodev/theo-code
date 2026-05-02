use crate::error::ToolError;
use crate::graph_context::GraphContextProvider;
use crate::permission::PermissionRequest;
use crate::session::{MessageId, SessionId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// Result of a tool execution.
///
/// The optional `llm_suffix` field carries text appended to the output only
/// when the result is serialized for the model (hidden from the user UI).
/// Tools use it to coach the agent on retries or follow-up actions — see
/// Anthropic "Writing tools for agents" principle 8 (actionable errors)
/// and opendev `ToolResult::with_llm_suffix` (traits.rs:128-176).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub title: String,
    pub output: String,
    pub metadata: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<FileAttachment>>,
    /// Model-only trailing text (hidden from UI). Used for retry hints and
    /// truncation guidance. `None` by default; populate via `with_llm_suffix`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_suffix: Option<String>,
}

impl Default for ToolOutput {
    fn default() -> Self {
        Self {
            title: String::new(),
            output: String::new(),
            metadata: serde_json::Value::Null,
            attachments: None,
            llm_suffix: None,
        }
    }
}

impl ToolOutput {
    /// Create a minimal `ToolOutput` with title and textual output.
    pub fn new(title: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            output: output.into(),
            ..Self::default()
        }
    }

    /// Attach structured metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Attach files (images, PDFs) for downstream rendering.
    #[must_use]
    pub fn with_attachments(mut self, attachments: Vec<FileAttachment>) -> Self {
        self.attachments = Some(attachments);
        self
    }

    /// Attach a trailing suffix visible only to the model.
    /// Used to coach retries, document truncation, and name follow-up tools.
    #[must_use]
    pub fn with_llm_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.llm_suffix = Some(suffix.into());
        self
    }

    /// Render for the model: `output` followed by a blank line and
    /// `llm_suffix` when present. Users see only `output` (via `title`/UI).
    #[must_use]
    pub fn model_text(&self) -> String {
        match &self.llm_suffix {
            Some(suffix) if !suffix.is_empty() => format!("{}\n\n{}", self.output, suffix),
            _ => self.output.clone(),
        }
    }
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

// ── Truncation Rule ─────────────────────────────────────────────────

/// Strategy the sanitizer uses when a tool output exceeds `max_chars`.
///
/// Ref: opendev `TruncationStrategy` (traits.rs:534-542, sanitizer.rs:27-53).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TruncationStrategy {
    /// Keep the first `max_chars` characters (best for reads, start of files).
    Head,
    /// Keep the last `max_chars` characters (best for shells, error traces).
    Tail,
    /// Keep the first `head` and last `tail` characters, joined by an elision marker.
    HeadTail { head: usize, tail: usize },
}

/// Per-tool rule the sanitizer consults before appending output to the LLM
/// message stream. Tools return `Option<TruncationRule>` from `truncation_rule()`;
/// `None` disables sanitizer-level truncation (the tool still owns its own limits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TruncationRule {
    pub max_chars: usize,
    pub strategy: TruncationStrategy,
}

impl TruncationRule {
    /// Apply the rule to `input`, returning `None` if `input` already fits.
    /// The returned string includes an inline `[truncated: N of M chars]`
    /// marker so the agent can recognise the shortening.
    pub fn apply(&self, input: &str) -> Option<String> {
        let total = input.chars().count();
        if total <= self.max_chars {
            return None;
        }
        let marker = format!("\n[truncated: showing bounded window of {total} chars]\n");
        let out = match self.strategy {
            TruncationStrategy::Head => {
                let head: String = input.chars().take(self.max_chars).collect();
                format!("{head}{marker}")
            }
            TruncationStrategy::Tail => {
                let skip = total.saturating_sub(self.max_chars);
                let tail: String = input.chars().skip(skip).collect();
                format!("{marker}{tail}")
            }
            TruncationStrategy::HeadTail { head, tail } => {
                let head_s: String = input.chars().take(head).collect();
                let tail_s: String = input.chars().skip(total.saturating_sub(tail)).collect();
                format!("{head_s}{marker}{tail_s}")
            }
        };
        Some(out)
    }
}

// ── Tool Schema & Category ──────────────────────────────────────────

/// Category of a tool — used for filtering and building minimal tool sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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
    /// Third-party plugins loaded from `.theo/plugins/` or `~/.config/theo/plugins/`.
    /// Always subject to the capability gate regardless of the global
    /// `CapabilitySet::unrestricted()` default — plugins must be opted-in
    /// via `allowed_categories` or `allowed_tools` (T1.3).
    Plugin,
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
/// Designed to be converted to an OpenAI/Anthropic-compatible JSON Schema
/// for LLM tool definitions. The optional `input_examples` field is emitted
/// as a top-level `examples: [...]` array — matches Anthropic's "Tool Use
/// Examples" surface and coaches the LLM on how to fill correlated or
/// nested parameters (reported 72% -> 90% accuracy on complex schemas).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub params: Vec<ToolParam>,
    /// Concrete example invocations — each value is a full arguments object
    /// the LLM can copy-paste. Omitted from the JSON Schema when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_examples: Vec<serde_json::Value>,
}

impl ToolSchema {
    /// Create a new empty schema (for tools with no parameters).
    pub fn new() -> Self {
        Self {
            params: vec![],
            input_examples: Vec::new(),
        }
    }

    /// Attach one or more example invocations.
    /// Each example must be a JSON object whose keys correspond to `params`.
    #[must_use]
    pub fn with_examples(mut self, examples: Vec<serde_json::Value>) -> Self {
        self.input_examples = examples;
        self
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
        if !self.input_examples.is_empty() {
            schema.insert(
                "examples".to_string(),
                serde_json::Value::Array(self.input_examples.clone()),
            );
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
    /// Phase 17 (sota-gaps): when present, this raw JSON Schema replaces
    /// `schema.to_json_schema()` in the LLM tool list. Used by MCP-bridged
    /// tools whose schema cannot be represented as `ToolParam`s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_schema_override: Option<serde_json::Value>,
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
            llm_schema_override: self.llm_schema_override(),
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

    /// If `true`, this tool is omitted from the default tool definitions
    /// shown to the agent — the agent discovers it by calling the
    /// `tool_search` meta-tool with a keyword that matches `search_hint`.
    ///
    /// Use sparingly: a deferred tool costs one extra round-trip to surface,
    /// so defer only tools that are rarely needed AND have expensive schemas.
    /// Default `false` (tool is always visible). Anthropic principle 12
    /// (minimize context overhead). Ref: opendev `should_defer`
    /// (traits.rs:547-575).
    fn should_defer(&self) -> bool {
        false
    }

    /// Short keyword phrase used by `tool_search` to match deferred tools.
    /// Should contain the verbs an agent would use when describing the task
    /// (e.g. "apply multi-file patch diff", "fetch web url contents").
    /// Returning `None` means the tool is only matched by its id.
    fn search_hint(&self) -> Option<&str> {
        None
    }

    /// Override the JSON Schema serialised to the LLM tool list.
    ///
    /// The default implementation returns `None`, which means
    /// `tool_bridge::registry_to_definitions` falls back to
    /// `self.schema().to_json_schema()`. Tools whose argument shape is
    /// declared elsewhere (e.g. an MCP server's `inputSchema`) override
    /// this to inject the raw schema verbatim — preserving nested types,
    /// enums, and oneOf/anyOf clauses that `ToolSchema::ToolParam` cannot
    /// express.
    ///
    /// Phase 17 (sota-gaps): used by `subagent::mcp_tools::McpToolAdapter`
    /// so MCP servers' tools enter the LLM `tools` array with their full
    /// fidelity instead of relying on a textual hint.
    fn llm_schema_override(&self) -> Option<serde_json::Value> {
        None
    }

    /// Per-tool truncation rule enforced by the agent-runtime sanitizer.
    ///
    /// Return `Some(TruncationRule)` to cap the output length for this tool —
    /// the sanitizer applies the rule AFTER `execute` returns and BEFORE the
    /// message reaches the LLM. Tools that already truncate internally (e.g.
    /// `bash` via `theo_domain::truncate`) can return `None`.
    ///
    /// The `llm_suffix` is applied after truncation, so coaching is never
    /// cut off. Anthropic principles 10 (truncate with guidance) and 12
    /// (minimize context overhead). Ref: opendev `BaseTool::truncation_rule`
    /// (traits.rs:534-542) and `ToolResultSanitizer` (sanitizer.rs:27-53).
    fn truncation_rule(&self) -> Option<TruncationRule> {
        None
    }

    /// Coach the agent when argument validation fails.
    ///
    /// Return `Some(msg)` to replace the raw `ToolError::InvalidArgs` /
    /// `ToolError::Validation` string with an onboarding-style message that
    /// names the offending parameter, shows the expected type, and gives a
    /// concrete example. Return `None` (default) to keep the raw error.
    ///
    /// The default is `None` — opt-in, so unmigrated tools are unaffected.
    ///
    /// Anthropic "Writing tools for agents" principle 8 (actionable errors).
    /// Ref: opendev `BaseTool::format_validation_error` (traits.rs:444-447).
    fn format_validation_error(
        &self,
        _error: &crate::error::ToolError,
        _args: &serde_json::Value,
    ) -> Option<String> {
        None
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
#[path = "tool_tests.rs"]
mod tests;
