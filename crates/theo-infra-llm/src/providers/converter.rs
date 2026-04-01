use super::common::*;
use super::{anthropic, openai, openai_compatible};
use serde_json::Value;

/// Convert a request body from one format to another.
///
/// If `from == to`, returns the body unchanged.
pub fn convert_request(from: Format, to: Format, body: &Value) -> Value {
    if from == to {
        return body.clone();
    }

    // Parse into common format
    let common = match from {
        Format::Anthropic => anthropic::from_request(body),
        Format::OpenAI => openai::from_request(body),
        Format::OaCompat => openai_compatible::from_request(body),
    };

    // Convert to target format
    match to {
        Format::Anthropic => anthropic::to_request(&common),
        Format::OpenAI => openai::to_request(&common),
        Format::OaCompat => openai_compatible::to_request(&common),
    }
}

/// Convert a response from one format to another.
///
/// If `from == to`, returns the response unchanged.
pub fn convert_response(from: Format, to: Format, resp: &Value) -> Value {
    if from == to {
        return resp.clone();
    }

    let common = match from {
        Format::Anthropic => anthropic::from_response(resp),
        Format::OpenAI => openai::from_response(resp),
        Format::OaCompat => openai_compatible::from_response(resp),
    };

    match to {
        Format::Anthropic => anthropic::to_response(&common),
        Format::OpenAI => openai::to_response(&common),
        Format::OaCompat => openai_compatible::to_response(&common),
    }
}

/// Convert a streaming chunk from one format to another.
///
/// Returns `Ok(converted_string)` or `Err(original)` if parsing fails.
pub fn convert_chunk(from: Format, to: Format, chunk: &str) -> Result<String, String> {
    if from == to {
        return Ok(chunk.to_string());
    }

    let common = match from {
        Format::Anthropic => anthropic::from_chunk(chunk)?,
        Format::OpenAI => openai::from_chunk(chunk)?,
        Format::OaCompat => openai_compatible::from_chunk(chunk)?,
    };

    let result = match to {
        Format::Anthropic => anthropic::to_chunk(&common),
        Format::OpenAI => openai::to_chunk(&common),
        Format::OaCompat => openai_compatible::to_chunk(&common),
    };

    Ok(result)
}

/// Normalize usage from any provider into a unified UsageInfo.
pub fn normalize_usage(format: Format, usage: &Value, adjust_cache: bool) -> UsageInfo {
    match format {
        Format::Anthropic => anthropic::normalize_usage(usage),
        Format::OpenAI => openai::normalize_usage(usage),
        Format::OaCompat => openai_compatible::normalize_usage(usage, adjust_cache),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_request_oa_compat_to_anthropic() {
        let body = serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": "Hello"},
            ],
            "max_tokens": 1024,
        });

        let result = convert_request(Format::OaCompat, Format::Anthropic, &body);

        // Anthropic format has system as top-level array
        assert!(result.get("system").is_some());
        assert!(result.get("messages").is_some());
        assert_eq!(result["max_tokens"], 1024);
    }

    #[test]
    fn test_convert_request_anthropic_to_oa_compat() {
        let body = serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 1024,
            "system": [{"type": "text", "text": "Be helpful"}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]},
            ],
        });

        let result = convert_request(Format::Anthropic, Format::OaCompat, &body);

        assert_eq!(result["model"], "claude-sonnet-4-20250514");
        assert!(result["messages"].as_array().unwrap().len() >= 2); // system + user
    }

    #[test]
    fn test_convert_response_anthropic_to_oa_compat() {
        let resp = serde_json::json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-20250514",
            "content": [{"type": "text", "text": "Hello!"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });

        let result = convert_response(Format::Anthropic, Format::OaCompat, &resp);

        assert!(result.get("choices").is_some());
        assert_eq!(result["choices"][0]["message"]["content"], "Hello!");
        assert_eq!(result["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn test_same_format_passthrough() {
        let body = serde_json::json!({"model": "test", "messages": []});
        let result = convert_request(Format::OaCompat, Format::OaCompat, &body);
        assert_eq!(body, result);
    }

    #[test]
    fn test_convert_chunk_anthropic_to_oa_compat() {
        let chunk = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}";
        let result = convert_chunk(Format::Anthropic, Format::OaCompat, chunk).unwrap();
        assert!(result.starts_with("data: "));
        let json: Value = serde_json::from_str(&result[6..]).unwrap();
        assert!(json["choices"][0]["delta"]["content"].as_str().is_some());
    }
}
