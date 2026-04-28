//! Single-purpose slice extracted from `providers/anthropic.rs` (D5 split).

#![allow(unused_imports)]

use super::super::common::*;
use serde_json::Value;

/// Anthropic response conversion.
pub fn from_response(resp: &Value) -> CommonResponse {
    let id = resp
        .get("id")
        .and_then(|i| i.as_str())
        .map(|i| i.replace("msg_", "chatcmpl_"))
        .unwrap_or_default();
    let model = resp
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or_default()
        .to_string();

    let blocks = resp
        .get("content")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    let text: String = blocks
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("");

    let tool_calls: Vec<CommonToolCall> = blocks
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
        .map(|b| {
            let name = b
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or_default()
                .to_string();
            let id = b
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or_default()
                .to_string();
            let arguments = match b.get("input") {
                Some(v) => v.as_str().map_or_else(|| v.to_string(), str::to_owned),
                None => "{}".to_string(),
            };
            CommonToolCall {
                id,
                call_type: "function".to_string(),
                function: CommonFunctionCall { name, arguments },
            }
        })
        .collect();

    let finish_reason = resp.get("stop_reason").and_then(|r| r.as_str()).map(|r| {
        match r {
            "end_turn" => "stop",
            "tool_use" => "tool_calls",
            "max_tokens" => "length",
            "content_filter" => "content_filter",
            _ => r,
        }
        .to_string()
    });

    let usage = resp.get("usage").map(|u| {
        let pt = u
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let ct = u
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let total = match (pt, ct) {
            (Some(p), Some(c)) => Some(p + c),
            _ => None,
        };
        let cached = u
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        CommonUsage {
            prompt_tokens: pt,
            completion_tokens: ct,
            total_tokens: total,
            prompt_tokens_details: cached.map(|c| PromptTokensDetails {
                cached_tokens: Some(c),
            }),
        }
    });

    CommonResponse {
        id,
        object: "chat.completion".to_string(),
        created: now_unix(),
        model,
        choices: vec![CommonChoice {
            index: 0,
            message: CommonChoiceMessage {
                role: "assistant".to_string(),
                content: if text.is_empty() { None } else { Some(text) },
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
            },
            finish_reason,
        }],
        usage,
    }
}

/// Convert a CommonResponse to Anthropic Messages API response format.
pub fn to_response(resp: &CommonResponse) -> Value {
    let choice = resp.choices.first();
    let mut content_blocks: Vec<Value> = Vec::new();

    if let Some(choice) = choice {
        if let Some(text) = &choice.message.content
            && !text.is_empty() {
                content_blocks.push(serde_json::json!({ "type": "text", "text": text }));
            }
        if let Some(tool_calls) = &choice.message.tool_calls {
            for tc in tool_calls {
                let input: Value = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or_else(|_| Value::String(tc.function.arguments.clone()));
                content_blocks.push(serde_json::json!({
                    "type": "tool_use",
                    "id": tc.id,
                    "name": tc.function.name,
                    "input": input,
                }));
            }
        }
    }

    let stop_reason = choice
        .and_then(|c| c.finish_reason.as_deref())
        .map(|r| match r {
            "stop" => "end_turn",
            "tool_calls" => "tool_use",
            "length" => "max_tokens",
            other => other,
        });

    let usage = resp.usage.as_ref().map(|u| {
        let mut usage = serde_json::json!({
            "input_tokens": u.prompt_tokens,
            "output_tokens": u.completion_tokens,
        });
        if let Some(details) = &u.prompt_tokens_details
            && let Some(cached) = details.cached_tokens {
                usage["cache_read_input_tokens"] = serde_json::json!(cached);
            }
        usage
    });

    let mut result = serde_json::json!({
        "id": resp.id,
        "type": "message",
        "role": "assistant",
        "content": if content_blocks.is_empty() { vec![serde_json::json!({"type":"text","text":""})] } else { content_blocks },
        "model": resp.model,
        "stop_reason": stop_reason,
    });

    if let Some(u) = usage {
        result["usage"] = u;
    }

    result
}
