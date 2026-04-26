use serde::{Deserialize, Serialize};

/// Message role in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

// ---------------------------------------------------------------------------
// Multimodal content (T0.1 / D1) — additive support for image blocks.
//
// Implementation note: the SOTA Tier1+Tier2 plan (D1) called for replacing
// `Message.content: Option<String>` with `Option<Vec<ContentBlock>>`. Doing
// so would have a 178+ call-site blast radius (ad-hoc `.content.as_deref()`
// pattern is widespread in `theo-agent-runtime`). To deliver the *value*
// of D1 (vision-capable messages) with minimal risk, we use an **additive**
// approach: keep `content: Option<String>` as the canonical text view AND
// add `content_blocks: Option<Vec<ContentBlock>>` as the multimodal channel.
//
// - When a Message is created via `Message::user("hi")` etc., `content` holds
//   the text and `content_blocks` is None.
// - When created via `Message::with_image(...)`, both are populated.
// - Provider adapters check `content_blocks` first; fall back to `content`.
// - Legacy callers reading `.content` keep working.
// ---------------------------------------------------------------------------

/// A single content block. Mirrors Anthropic/OpenAI content array semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContentBlock {
    /// Plain text content.
    Text { text: String },
    /// External image URL — preferred for OpenAI vision.
    ImageUrl {
        image_url: ImageUrlBlock,
    },
    /// Inline base64-encoded image — preferred for Anthropic vision.
    ImageBase64 {
        source: ImageSource,
    },
}

/// External image URL block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageUrlBlock {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<ImageDetail>,
}

/// OpenAI vision detail hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ImageDetail {
    Low,
    High,
    Auto,
}

/// Inline image source (Anthropic shape).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageSource {
    /// Always `"base64"` for inline images.
    #[serde(rename = "type")]
    pub source_type: String,
    /// MIME type, e.g. `image/png`, `image/jpeg`, `image/webp`.
    pub media_type: String,
    /// Base64-encoded image bytes (no data: prefix).
    pub data: String,
}

impl ContentBlock {
    /// Convenience builder for a text block.
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text { text: s.into() }
    }

    /// Convenience builder for an image_url block (OpenAI vision).
    pub fn image_url(url: impl Into<String>) -> Self {
        Self::ImageUrl {
            image_url: ImageUrlBlock {
                url: url.into(),
                detail: None,
            },
        }
    }

    /// Convenience builder for an image_url block with a detail hint.
    pub fn image_url_with_detail(url: impl Into<String>, detail: ImageDetail) -> Self {
        Self::ImageUrl {
            image_url: ImageUrlBlock {
                url: url.into(),
                detail: Some(detail),
            },
        }
    }

    /// Convenience builder for a base64 image block (Anthropic vision).
    ///
    /// `media_type` is e.g. `image/png`. `data` must be base64 (no `data:` prefix).
    pub fn image_base64(media_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self::ImageBase64 {
            source: ImageSource {
                source_type: "base64".to_string(),
                media_type: media_type.into(),
                data: data.into(),
            },
        }
    }

    /// Returns the text content if this is a Text block, else None.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Returns true if this is a vision block (image of any kind).
    pub fn is_image(&self) -> bool {
        matches!(self, ContentBlock::ImageUrl { .. } | ContentBlock::ImageBase64 { .. })
    }
}

/// A single message in the conversation history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Option<String>,

    /// Optional multimodal content blocks (T0.1 / D1). When present,
    /// vision-capable providers (Anthropic, OpenAI) use this in preference
    /// to `content`. Legacy/text-only providers fall back to `content`.
    /// `None` means classical text-only message — `content` is authoritative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_blocks: Option<Vec<ContentBlock>>,

    /// Tool calls requested by the assistant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,

    /// ID of the tool call this message responds to (role=tool).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// Name of the tool (used when role=tool).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Some(content.into()),
            content_blocks: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(content.into()),
            content_blocks: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: Some(content.into()),
            content_blocks: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant_with_tool_calls(content: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content,
            content_blocks: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
            content_blocks: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: Some(name.into()),
        }
    }

    /// T0.1 / D1 — Build a multimodal user message with text + image blocks.
    ///
    /// Both `content` (text-only view) and `content_blocks` (full multimodal)
    /// are populated so legacy text-only providers still see the prompt.
    pub fn user_with_blocks(blocks: Vec<ContentBlock>) -> Self {
        let text_only = blocks
            .iter()
            .filter_map(ContentBlock::as_text)
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            role: Role::User,
            content: if text_only.is_empty() { None } else { Some(text_only) },
            content_blocks: Some(blocks),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    /// T0.1 / D1 — Convenience: build a user message with prompt text + a
    /// single image (URL form, OpenAI-friendly).
    pub fn user_with_image_url(text: impl Into<String>, image_url: impl Into<String>) -> Self {
        Self::user_with_blocks(vec![
            ContentBlock::text(text),
            ContentBlock::image_url(image_url),
        ])
    }

    /// T0.1 / D1 — Convenience: build a user message with prompt text + a
    /// base64 image (Anthropic-friendly).
    pub fn user_with_image_base64(
        text: impl Into<String>,
        media_type: impl Into<String>,
        data: impl Into<String>,
    ) -> Self {
        Self::user_with_blocks(vec![
            ContentBlock::text(text),
            ContentBlock::image_base64(media_type, data),
        ])
    }

    /// Returns true if this message carries any image content blocks.
    /// Provider adapters use this to decide whether to emit array-shape
    /// content (vision providers) or fall back to text-only.
    pub fn has_image(&self) -> bool {
        self.content_blocks
            .as_ref()
            .map(|blocks| blocks.iter().any(ContentBlock::is_image))
            .unwrap_or(false)
    }

    /// Returns the canonical text view of the message body.
    /// Prefers concatenated text from `content_blocks` when present;
    /// falls back to `content`.
    pub fn text_view(&self) -> Option<String> {
        if let Some(blocks) = &self.content_blocks {
            let text = blocks
                .iter()
                .filter_map(ContentBlock::as_text)
                .collect::<Vec<_>>()
                .join("\n");
            if !text.is_empty() {
                return Some(text);
            }
        }
        self.content.clone()
    }
}

/// A tool call requested by the assistant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// The function name and arguments of a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

impl ToolCall {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: name.into(),
                arguments: arguments.into(),
            },
        }
    }

    /// Parse the arguments JSON string into a serde_json::Value.
    pub fn parse_arguments(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_str(&self.function.arguments)
    }
}

/// Definition of a tool available to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

/// Function schema for a tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

/// Request body for chat completions.
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,

    /// Reasoning effort level: "low", "medium", "high".
    /// Supported by OpenAI GPT models. Ignored by providers that don't support it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            tools: None,
            tool_choice: None,
            max_tokens: None,
            temperature: None,
            stream: None,
            reasoning_effort: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tool_choice = Some("auto".to_string());
        self.tools = Some(tools);
        self
    }

    /// Phase 29 follow-up (sota-gaps-followup) — closes gap #7.
    /// Override the `tool_choice` field with an arbitrary string so the
    /// caller can force the model to invoke a tool:
    ///
    /// - `"auto"` (default) — model decides
    /// - `"required"` — model MUST call a tool
    /// - `"none"` — model MUST NOT call a tool
    ///
    /// Per-tool forcing (`{"type":"function","function":{"name":"X"}}`)
    /// requires switching the field to a JSON value, which is a future
    /// breaking change; for now we cover the OpenAI/Anthropic-compatible
    /// string variants which fix gap #7 (Codex bypassing delegate_task).
    pub fn with_tool_choice(mut self, choice: impl Into<String>) -> Self {
        self.tool_choice = Some(choice.into());
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_reasoning_effort(mut self, effort: impl Into<String>) -> Self {
        self.reasoning_effort = Some(effort.into());
        self
    }
}

/// Response from chat completions.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub id: Option<String>,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

/// A single choice in the response.
#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: ChoiceMessage,
    pub finish_reason: Option<String>,
}

/// The message part of a choice.
#[derive(Debug, Clone, Deserialize)]
pub struct ChoiceMessage {
    pub role: Role,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// Token usage statistics.
#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl ChatResponse {
    /// Extract the first choice's message content.
    pub fn content(&self) -> Option<&str> {
        self.choices.first()?.message.content.as_deref()
    }

    /// Extract tool calls from the first choice.
    pub fn tool_calls(&self) -> &[ToolCall] {
        self.choices
            .first()
            .and_then(|c| c.message.tool_calls.as_deref())
            .unwrap_or(&[])
    }

    /// Get the finish reason of the first choice.
    pub fn finish_reason(&self) -> Option<&str> {
        self.choices.first()?.finish_reason.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_tool() -> ToolDefinition {
        ToolDefinition::new("foo", "desc", serde_json::json!({"type": "object"}))
    }

    // ----- T0.1 / D1: ContentBlock + multimodal Message helpers -----

    #[test]
    fn t01_message_text_helper_produces_string_content() {
        let m = Message::user("hi");
        assert_eq!(m.content.as_deref(), Some("hi"));
        assert!(m.content_blocks.is_none());
        assert!(!m.has_image());
    }

    #[test]
    fn t01_message_with_image_url_produces_two_blocks() {
        let m = Message::user_with_image_url("describe", "https://e.x/img.png");
        let blocks = m.content_blocks.as_ref().expect("blocks set");
        assert_eq!(blocks.len(), 2);
        assert!(matches!(blocks[0], ContentBlock::Text { .. }));
        assert!(matches!(blocks[1], ContentBlock::ImageUrl { .. }));
        assert!(m.has_image());
        assert_eq!(m.content.as_deref(), Some("describe")); // text fallback present
    }

    #[test]
    fn t01_message_with_image_base64_produces_blocks() {
        let m = Message::user_with_image_base64("see", "image/png", "AAAA");
        let blocks = m.content_blocks.as_ref().unwrap();
        assert_eq!(blocks.len(), 2);
        match &blocks[1] {
            ContentBlock::ImageBase64 { source } => {
                assert_eq!(source.source_type, "base64");
                assert_eq!(source.media_type, "image/png");
                assert_eq!(source.data, "AAAA");
            }
            _ => panic!("expected ImageBase64"),
        }
        assert!(m.has_image());
    }

    #[test]
    fn t01_content_block_serde_text_roundtrip() {
        let b = ContentBlock::text("hello");
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(b, back);
    }

    #[test]
    fn t01_content_block_serde_image_url_roundtrip() {
        let b = ContentBlock::image_url("https://x/y.png");
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"type\":\"image_url\""));
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(b, back);
    }

    #[test]
    fn t01_content_block_serde_image_base64_roundtrip() {
        let b = ContentBlock::image_base64("image/jpeg", "ZGF0YQ==");
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"type\":\"image_base64\""));
        assert!(json.contains("\"media_type\":\"image/jpeg\""));
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(b, back);
    }

    #[test]
    fn t01_legacy_v1_message_loads_without_content_blocks() {
        // Wire format from before T0.1 — `content_blocks` field absent.
        let json = r#"{"role":"user","content":"hello"}"#;
        let m: Message = serde_json::from_str(json).unwrap();
        assert_eq!(m.content.as_deref(), Some("hello"));
        assert!(m.content_blocks.is_none());
    }

    #[test]
    fn t01_message_with_blocks_serde_roundtrip() {
        let m = Message::user_with_image_url("see", "https://x/y.png");
        let json = serde_json::to_string(&m).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
        assert!(back.has_image());
    }

    #[test]
    fn t01_text_view_prefers_blocks_when_present() {
        let m = Message::user_with_blocks(vec![
            ContentBlock::text("hello"),
            ContentBlock::image_url("https://x/y.png"),
            ContentBlock::text("world"),
        ]);
        assert_eq!(m.text_view().as_deref(), Some("hello\nworld"));
    }

    #[test]
    fn t01_text_view_falls_back_to_content_when_no_blocks() {
        let m = Message::system("only text");
        assert_eq!(m.text_view().as_deref(), Some("only text"));
    }

    #[test]
    fn t01_content_block_is_image_predicate() {
        assert!(!ContentBlock::text("a").is_image());
        assert!(ContentBlock::image_url("u").is_image());
        assert!(ContentBlock::image_base64("image/png", "AA").is_image());
    }

    #[test]
    fn t01_user_with_blocks_filters_empty_text_to_none_in_content() {
        // Only image blocks, no text → content is None.
        let m = Message::user_with_blocks(vec![ContentBlock::image_url("u")]);
        assert!(m.content.is_none());
        assert!(m.has_image());
    }

    #[test]
    fn t01_image_detail_roundtrip() {
        let b = ContentBlock::image_url_with_detail("u", ImageDetail::High);
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"detail\":\"high\""));
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(b, back);
    }

    #[test]
    fn with_tools_defaults_tool_choice_to_auto() {
        let req = ChatRequest::new("m", vec![]).with_tools(vec![dummy_tool()]);
        assert_eq!(req.tool_choice.as_deref(), Some("auto"));
    }

    #[test]
    fn with_tool_choice_overrides_to_required() {
        let req = ChatRequest::new("m", vec![])
            .with_tools(vec![dummy_tool()])
            .with_tool_choice("required");
        assert_eq!(req.tool_choice.as_deref(), Some("required"));
    }

    #[test]
    fn with_tool_choice_overrides_to_none() {
        let req = ChatRequest::new("m", vec![])
            .with_tools(vec![dummy_tool()])
            .with_tool_choice("none");
        assert_eq!(req.tool_choice.as_deref(), Some("none"));
    }

    #[test]
    fn with_tool_choice_serializes_in_request_json() {
        let req = ChatRequest::new("m", vec![])
            .with_tools(vec![dummy_tool()])
            .with_tool_choice("required");
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"tool_choice\":\"required\""));
    }
}
