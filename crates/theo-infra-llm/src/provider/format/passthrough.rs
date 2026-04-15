//! OA-compatible passthrough — identity converter (no transformation).
//!
//! Used by the majority of providers that speak standard /v1/chat/completions.

use super::FormatConverter;
use crate::error::LlmError;
use crate::types::{ChatRequest, ChatResponse};

/// Identity converter — request/response pass through unchanged.
pub struct OaPassthrough;

impl FormatConverter for OaPassthrough {
    fn convert_request(&self, request: &ChatRequest) -> serde_json::Value {
        serde_json::to_value(request).unwrap_or_default()
    }

    fn convert_response(&self, body: serde_json::Value) -> Result<ChatResponse, LlmError> {
        serde_json::from_value(body).map_err(|e| LlmError::Parse(format!("OA response parse: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRequest, Message};

    #[test]
    fn passthrough_request_preserves_structure() {
        let request = ChatRequest::new("gpt-4", vec![Message::user("hello")]);
        let converter = OaPassthrough;
        let json = converter.convert_request(&request);
        assert_eq!(json["model"], "gpt-4");
        assert!(json["messages"].is_array());
    }

    #[test]
    fn passthrough_response_parses_valid_json() {
        let converter = OaPassthrough;
        let json = serde_json::json!({
            "id": "chatcmpl-123",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });
        let response = converter.convert_response(json).unwrap();
        assert_eq!(
            response.choices[0].message.content.as_deref(),
            Some("Hello!")
        );
    }

    #[test]
    fn passthrough_response_rejects_invalid_json() {
        let converter = OaPassthrough;
        let json = serde_json::json!({"invalid": true});
        let result = converter.convert_response(json);
        assert!(result.is_err());
    }
}
