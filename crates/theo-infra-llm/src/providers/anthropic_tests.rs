//! Sibling test body of `anthropic.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `anthropic.rs` via `#[path = "anthropic_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use super::*;

    #[test]
    fn test_from_anthropic_request_basic() {
        let body = serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 1024,
            "system": [{"type": "text", "text": "You are helpful."}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]},
            ],
        });

        let req = from_request(&body);
        assert_eq!(req.model, "claude-sonnet-4-20250514");
        assert_eq!(req.messages.len(), 2); // system + user
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[1].role, Role::User);
    }

    #[test]
    fn test_from_anthropic_response_with_tool_use() {
        let resp = serde_json::json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-20250514",
            "content": [
                {"type": "text", "text": "Let me read that."},
                {"type": "tool_use", "id": "toolu_1", "name": "read", "input": {"path": "main.py"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        });

        let common = from_response(&resp);
        assert_eq!(common.id, "chatcmpl_123");
        let choice = &common.choices[0];
        assert_eq!(choice.message.content.as_deref(), Some("Let me read that."));
        assert_eq!(choice.finish_reason.as_deref(), Some("tool_calls"));
        let tc = choice.message.tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "read");
    }

    #[test]
    fn test_roundtrip_request() {
        let common = CommonRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: Some(1024),
            temperature: Some(0.5),
            top_p: None,
            stop: None,
            messages: vec![
                CommonMessage {
                    role: Role::System,
                    content: Some(Content::Text("Be helpful".to_string())),
                    tool_call_id: None,
                    tool_calls: None,
                    name: None,
                },
                CommonMessage {
                    role: Role::User,
                    content: Some(Content::Text("Hello".to_string())),
                    tool_call_id: None,
                    tool_calls: None,
                    name: None,
                },
            ],
            stream: Some(false),
            tools: None,
            tool_choice: None,
        };

        let anthropic = to_request(&common);
        let back = from_request(&anthropic);

        assert_eq!(back.messages.len(), 2);
        assert_eq!(back.messages[0].role, Role::System);
        assert_eq!(back.messages[1].role, Role::User);
    }

    #[test]
    fn test_from_anthropic_chunk_text_delta() {
        let chunk = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}";
        let result = from_chunk(chunk).unwrap();
        assert_eq!(result.choices[0].delta.content.as_deref(), Some("Hello"));
    }

    #[test]
    fn test_from_anthropic_chunk_tool_start() {
        let chunk = "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"read\"}}";
        let result = from_chunk(chunk).unwrap();
        let tc = result.choices[0].delta.tool_calls.as_ref().unwrap();
        assert_eq!(
            tc[0].function.as_ref().unwrap().name.as_deref(),
            Some("read")
        );
    }

    #[test]
    fn test_normalize_usage() {
        let usage = serde_json::json!({
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_read_input_tokens": 80,
            "cache_creation": {
                "ephemeral_5m_input_tokens": 20,
                "ephemeral_1h_input_tokens": 0
            }
        });
        let info = normalize_usage(&usage);
        assert_eq!(info.input_tokens, 100);
        assert_eq!(info.output_tokens, 50);
        assert_eq!(info.cache_read_tokens, Some(80));
        assert_eq!(info.cache_write_5m_tokens, Some(20));
    }
