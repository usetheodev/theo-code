//! Sibling test body of `openai.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `openai.rs` via `#[path = "openai_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    #![allow(unused_imports)]
    use super::*;
    use crate::providers::openai::*;
    use crate::providers::common::*;
    use serde_json::{Value, json};

    #[test]
    fn test_from_openai_responses_request() {
        let body = serde_json::json!({
            "model": "gpt-4o",
            "input": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": [{"type": "input_text", "text": "Hello"}]},
                {"type": "function_call", "id": "fc_1", "name": "read", "arguments": "{\"path\":\"a.py\"}"},
                {"type": "function_call_output", "call_id": "fc_1", "output": "file contents"},
            ],
            "max_output_tokens": 1024,
        });

        let req = from_request(&body);
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 4);
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[1].role, Role::User);
        assert_eq!(req.messages[2].role, Role::Assistant);
        assert_eq!(req.messages[3].role, Role::Tool);
        assert_eq!(req.max_tokens, Some(1024));
    }

    #[test]
    fn test_from_openai_responses_response() {
        let resp = serde_json::json!({
            "id": "resp_123",
            "object": "response",
            "model": "gpt-4o",
            "output": [
                {"type": "message", "content": [{"type": "output_text", "text": "Hello!"}]},
                {"type": "function_call", "id": "fc_1", "name": "read", "arguments": "{\"path\":\"a.py\"}"},
            ],
            "stop_reason": "tool_call",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        });

        let common = from_response(&resp);
        assert_eq!(common.id, "chatcmpl_123");
        assert_eq!(common.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(
            common.choices[0].finish_reason.as_deref(),
            Some("tool_calls")
        );
        assert_eq!(
            common.choices[0].message.tool_calls.as_ref().unwrap().len(),
            1
        );
    }

    #[test]
    fn test_from_openai_chunk_text() {
        let chunk = "event: response.output_text.delta\ndata: {\"delta\":\"Hello\",\"response\":{\"id\":\"r1\",\"model\":\"gpt-4o\"}}";
        let result = from_chunk(chunk).unwrap();
        assert_eq!(result.choices[0].delta.content.as_deref(), Some("Hello"));
    }

    #[test]
    fn test_from_openai_chunk_tool_start() {
        let chunk = "event: response.output_item.added\ndata: {\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"name\":\"read\"}}";
        let result = from_chunk(chunk).unwrap();
        let tc = result.choices[0].delta.tool_calls.as_ref().unwrap();
        assert_eq!(
            tc[0].function.as_ref().unwrap().name.as_deref(),
            Some("read")
        );
    }
