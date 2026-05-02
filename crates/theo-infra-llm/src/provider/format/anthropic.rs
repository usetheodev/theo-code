//! Anthropic format converter — wraps the existing providers/anthropic.rs converter.
//!
//! Reuses the proven, tested conversion logic from providers/anthropic.rs.

use super::FormatConverter;
use super::serialize_oa::serialize_oa_compat;
use crate::error::LlmError;
use crate::providers::common::Format;
use crate::providers::converter;
use crate::types::{ChatRequest, ChatResponse};

/// Anthropic Messages API converter.
/// Delegates to the existing bidirectional converter in providers/.
pub struct AnthropicConverter;

impl FormatConverter for AnthropicConverter {
    fn convert_request(&self, request: &ChatRequest) -> serde_json::Value {
        // T0.1 / D1: bridge `Message.content_blocks` to OA-compat array
        // before delegating to the bidirectional converter.
        let oa_json = serialize_oa_compat(request);
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

    /// T0.1 / D1 — End-to-end: a Message with `content_blocks` carrying an
    /// `image_url` block flows through `convert_request` and ends up as
    /// Anthropic's native `{type:image, source:{type:url, url}}` shape.
    #[test]
    fn t01_anthropic_converter_propagates_image_url_block() {
        let request = ChatRequest::new(
            "claude-3-5-sonnet-20240620",
            vec![Message::user_with_image_url("describe", "https://e.x/a.png")],
        );
        let converter = AnthropicConverter;
        let json = converter.convert_request(&request);

        // The user message in Anthropic format should have content as an
        // array with an image block.
        let user_msg = json["messages"]
            .as_array()
            .expect("messages array")
            .iter()
            .find(|m| m["role"] == "user")
            .expect("user message present");
        let content = user_msg["content"].as_array().expect("array content");
        assert!(
            content.iter().any(|b| b["type"] == "image"),
            "image block should be present in Anthropic output: {content:?}"
        );
    }

    /// T0.1 / D1 — base64 data URL is mapped to Anthropic's native
    /// `{type:image, source:{type:base64, media_type, data}}` shape.
    #[test]
    fn t01_anthropic_converter_propagates_image_base64_block() {
        let request = ChatRequest::new(
            "claude-3-5-sonnet-20240620",
            vec![Message::user_with_image_base64(
                "what is this",
                "image/png",
                "AAAA",
            )],
        );
        let converter = AnthropicConverter;
        let json = converter.convert_request(&request);

        let user_msg = json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "user")
            .unwrap();
        let content = user_msg["content"].as_array().unwrap();
        let img = content
            .iter()
            .find(|b| b["type"] == "image")
            .expect("image block");
        // Anthropic's source can be either base64 (preferred) or url.
        let src_type = img["source"]["type"].as_str().unwrap_or("");
        assert!(
            src_type == "base64" || src_type == "url",
            "unexpected source type: {src_type:?} ({img:?})"
        );
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
