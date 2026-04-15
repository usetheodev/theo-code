//! Codex (OpenAI Responses API) format converter.
//!
//! Wraps the existing codex.rs conversion logic.

use super::FormatConverter;
use crate::error::LlmError;
use crate::types::{ChatRequest, ChatResponse};

/// Codex Responses API converter.
/// Delegates to existing codex::to_codex_body / codex::from_codex_stream.
pub struct CodexConverter;

impl FormatConverter for CodexConverter {
    fn convert_request(&self, request: &ChatRequest) -> serde_json::Value {
        crate::codex::to_codex_body(request)
    }

    fn convert_response(&self, body: serde_json::Value) -> Result<ChatResponse, LlmError> {
        // Codex responses come as SSE stream text, not JSON.
        // For non-streaming, the body should be the full SSE text.
        let text = if let Some(s) = body.as_str() {
            s.to_string()
        } else {
            serde_json::to_string(&body).unwrap_or_default()
        };
        crate::codex::from_codex_stream(&text)
            .ok_or_else(|| LlmError::Parse("failed to parse Codex response".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRequest, Message};

    #[test]
    fn codex_converter_transforms_request() {
        let request = ChatRequest::new(
            "gpt-4",
            vec![Message::system("You are helpful"), Message::user("Hello")],
        );
        let converter = CodexConverter;
        let json = converter.convert_request(&request);

        // Codex format: system becomes "instructions"
        assert!(json.get("instructions").is_some() || json.get("input").is_some());
    }
}
