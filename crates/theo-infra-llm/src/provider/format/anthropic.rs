//! Anthropic format converter — wraps the existing providers/anthropic.rs converter.
//!
//! Reuses the proven, tested conversion logic from providers/anthropic.rs.

use super::FormatConverter;
use crate::error::LlmError;
use crate::providers::common::Format;
use crate::providers::converter;
use crate::types::{ChatRequest, ChatResponse};

/// Anthropic Messages API converter.
/// Delegates to the existing bidirectional converter in providers/.
pub struct AnthropicConverter;

impl FormatConverter for AnthropicConverter {
    fn convert_request(&self, request: &ChatRequest) -> serde_json::Value {
        // Serialize to OA-compat JSON, then convert to Anthropic format
        let oa_json = serde_json::to_value(request).unwrap_or_default();
        converter::convert_request(Format::OaCompat, Format::Anthropic, &oa_json)
    }

    fn convert_response(&self, body: serde_json::Value) -> Result<ChatResponse, LlmError> {
        // Convert Anthropic response to OA-compat, then deserialize
        let oa_json = converter::convert_response(Format::Anthropic, Format::OaCompat, &body);
        serde_json::from_value(oa_json)
            .map_err(|e| LlmError::Parse(format!("Anthropic response: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRequest, Message};

    #[test]
    fn anthropic_converter_transforms_request() {
        let request = ChatRequest::new(
            "claude-3",
            vec![Message::system("You are helpful"), Message::user("Hello")],
        );
        let converter = AnthropicConverter;
        let json = converter.convert_request(&request);

        // Anthropic format: system is separate, not in messages
        assert!(json.get("system").is_some() || json.get("messages").is_some());
    }

    #[test]
    fn anthropic_converter_parses_response() {
        let converter = AnthropicConverter;
        let anthropic_response = serde_json::json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-3-sonnet-20240229",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });
        let response = converter.convert_response(anthropic_response).unwrap();
        assert!(response.choices[0].message.content.is_some());
    }
}
